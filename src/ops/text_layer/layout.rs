pub fn compute_block_layout(block: &TextBlock) -> BlockLayout {
    let mut run_infos = Vec::new();
    let mut line_infos: Vec<LineInfo> = Vec::new();

    // Split runs by newlines into per-line segments, same as rasterizer
    struct SegInfo {
        run_index: usize,
        text: String,
        ascent: f32,
        line_height: f32,
        char_advances: Vec<f32>,
    }

    let mut lines: Vec<Vec<SegInfo>> = vec![Vec::new()];

    for (run_idx, run) in block.runs.iter().enumerate() {
        let font = match load_font_for_style(&run.style) {
            Some(f) => f,
            None => {
                // Can't load font — provide dummy metrics
                run_infos.push(RunLayoutInfo {
                    char_advances: vec![0.0; run.text.chars().count() + 1],
                    baseline_y: 0.0,
                    line_height: run.style.font_size,
                    ascent: run.style.font_size * 0.8,
                });
                continue;
            }
        };
        let safe_ws = run.style.width_scale.clamp(0.001, f32::MAX / run.style.font_size.max(1.0));
        let safe_hs = run.style.height_scale.clamp(0.001, f32::MAX / run.style.font_size.max(1.0));
        let scaled = font.as_scaled(ab_glyph::PxScale {
            x: run.style.font_size * safe_ws,
            y: run.style.font_size * safe_hs,
        });
        let ascent = scaled.ascent();
        let lh = scaled.height();

        let parts: Vec<&str> = run.text.split('\n').collect();
        let mut run_advances = vec![0.0f32]; // entry 0 = 0.0

        for (pi, part) in parts.iter().enumerate() {
            if pi > 0 {
                lines.push(Vec::new());
                run_advances.push(*run_advances.last().unwrap()); // newline doesn't advance x
            }
            let mut seg_advances = vec![0.0f32];
            let mut cursor_x = 0.0f32;
            let mut prev_glyph: Option<ab_glyph::GlyphId> = None;
            for ch in part.chars() {
                let gid = font.glyph_id(ch);
                if let Some(prev) = prev_glyph {
                    cursor_x += scaled.kern(prev, gid);
                }
                cursor_x += scaled.h_advance(gid);
                cursor_x += run.style.letter_spacing;
                seg_advances.push(cursor_x);
                prev_glyph = Some(gid);
            }
            // Extend the flat run_advances with this segment's advances (offset from line start)
            for &a in seg_advances.iter().skip(1) {
                run_advances.push(
                    *run_advances.last().unwrap_or(&0.0) + a
                        - seg_advances[seg_advances.len().saturating_sub(1)
                            - (seg_advances.len()
                                - 1
                                - (seg_advances.iter().position(|&x| x == a).unwrap_or(0)))]
                        .min(a),
                );
            }

            lines.last_mut().unwrap().push(SegInfo {
                run_index: run_idx,
                text: part.to_string(),
                ascent,
                line_height: lh,
                char_advances: seg_advances,
            });
        }

        // Simplified: just store flat char advances for this run
        let mut flat_advances = vec![0.0f32];
        let mut cx = 0.0f32;
        let mut prev_g: Option<ab_glyph::GlyphId> = None;
        for ch in run.text.chars() {
            if ch == '\n' {
                flat_advances.push(cx);
                prev_g = None;
                continue;
            }
            let gid = font.glyph_id(ch);
            if let Some(prev) = prev_g {
                cx += scaled.kern(prev, gid);
            }
            cx += scaled.h_advance(gid);
            cx += run.style.letter_spacing;
            flat_advances.push(cx);
            prev_g = Some(gid);
        }

        run_infos.push(RunLayoutInfo {
            char_advances: flat_advances,
            baseline_y: 0.0, // filled in below per-line
            line_height: lh,
            ascent,
        });
    }

    // Compute per-line metrics
    let mut total_width = 0.0f32;
    let mut total_height = 0.0f32;
    let ls = block.paragraph.line_spacing;
    for segs in &lines {
        let max_ascent = segs.iter().map(|s| s.ascent).fold(0.0f32, f32::max);
        let max_lh = segs.iter().map(|s| s.line_height).fold(0.0f32, f32::max);
        let width: f32 = segs
            .iter()
            .map(|s| *s.char_advances.last().unwrap_or(&0.0))
            .sum();
        total_width = total_width.max(width);
        total_height += max_lh * ls;
        line_infos.push(LineInfo {
            width,
            max_ascent,
            max_line_height: max_lh * ls,
        });
    }

    // If max_width is set, adjust layout for word wrapping
    if let Some(mw) = block.max_width {
        total_width = mw;
        // For single-run blocks, recompute height with word wrapping
        if block.runs.len() == 1 {
            let run = &block.runs[0];
            if let Some(font) = load_font_for_style(&run.style) {
                let wrapped_lines: Vec<String> = run
                    .text
                    .split('\n')
                    .flat_map(|line| {
                        text::word_wrap_line(
                            line,
                            &font,
                            run.style.font_size,
                            mw,
                            run.style.letter_spacing,
                            run.style.width_scale,
                            run.style.height_scale,
                        )
                    })
                    .collect();
                let scaled = font.as_scaled(ab_glyph::PxScale {
                    x: run.style.font_size * run.style.width_scale,
                    y: run.style.font_size * run.style.height_scale,
                });
                total_height = wrapped_lines.len().max(1) as f32
                    * scaled.height()
                    * block.paragraph.line_spacing;
            }
        }
    }

    BlockLayout {
        runs: run_infos,
        lines: line_infos,
        total_width,
        total_height,
    }
}

// ---------------------------------------------------------------------------
// Per-Glyph Bounding Boxes (Phase 5 — Batch 9: glyph vertex editing)
// ---------------------------------------------------------------------------

/// Bounding box for a single glyph, in canvas coordinates.
#[derive(Clone, Debug)]
pub struct GlyphBounds {
    /// Index of this glyph in the flat sequence (across all runs in a block).
    pub glyph_index: usize,
    /// The character this glyph represents.
    pub ch: char,
    pub flat_start: usize,
    pub flat_end: usize,
    /// Top-left corner in canvas coords.
    pub x: f32,
    pub y: f32,
    /// Width and height of the glyph's bounding box.
    pub w: f32,
    pub h: f32,
    /// Center point (convenience, = x + w/2, y + h/2).
    pub cx: f32,
    pub cy: f32,
}

/// Compute per-glyph bounding boxes for a text block, in canvas coordinates.
/// Newline characters are excluded from the returned list.
/// Respects alignment and multi-run layout.
pub fn compute_glyph_bounds(block: &TextBlock) -> Vec<GlyphBounds> {
    let mut result = Vec::new();
    let mut glyph_idx = 0usize;

    // Split runs into per-line segments (same logic as rasterize_block_multirun_slow)
    struct GlyphSeg {
        text: String,
        flat_start: usize,
        font: FontArc,
        font_size: f32,
        width_scale: f32,
        height_scale: f32,
        letter_spacing: f32,
        ascent: f32,
        line_height: f32,
        advance: f32,
    }

    let mut lines: Vec<Vec<GlyphSeg>> = vec![Vec::new()];
    let mut flat_offset = 0usize;

    for run in &block.runs {
        let font = match load_font_for_style(&run.style) {
            Some(f) => f,
            None => continue,
        };
        let safe_ws = run.style.width_scale.clamp(0.001, f32::MAX / run.style.font_size.max(1.0));
        let safe_hs = run.style.height_scale.clamp(0.001, f32::MAX / run.style.font_size.max(1.0));
        let scaled = font.as_scaled(ab_glyph::PxScale {
            x: run.style.font_size * safe_ws,
            y: run.style.font_size * safe_hs,
        });
        let parts: Vec<&str> = run.text.split('\n').collect();
        for (pi, part) in parts.iter().enumerate() {
            if pi > 0 {
                lines.push(Vec::new());
                flat_offset += 1;
            }
            let mut advance = 0.0f32;
            let mut prev_glyph: Option<ab_glyph::GlyphId> = None;
            for ch in part.chars() {
                let gid = font.glyph_id(ch);
                if let Some(prev) = prev_glyph {
                    advance += scaled.kern(prev, gid);
                }
                advance += scaled.h_advance(gid);
                advance += run.style.letter_spacing;
                prev_glyph = Some(gid);
            }
            lines.last_mut().unwrap().push(GlyphSeg {
                text: part.to_string(),
                flat_start: flat_offset,
                font: font.clone(),
                font_size: run.style.font_size,
                width_scale: run.style.width_scale,
                height_scale: run.style.height_scale,
                letter_spacing: run.style.letter_spacing,
                ascent: scaled.ascent(),
                line_height: scaled.height(),
                advance,
            });
            flat_offset += part.len();
        }
    }

    // Per-line metrics
    let line_metrics: Vec<(f32, f32, f32)> = lines
        .iter()
        .map(|segs| {
            let max_ascent = segs.iter().map(|s| s.ascent).fold(0.0f32, f32::max);
            let max_lh = segs.iter().map(|s| s.line_height).fold(0.0f32, f32::max);
            let total_width: f32 = segs.iter().map(|s| s.advance).sum();
            (total_width, max_ascent, max_lh)
        })
        .collect();

    let mut y_pos = 0.0f32;
    for (line_idx, segs) in lines.iter().enumerate() {
        let (line_width, max_ascent, max_lh) = line_metrics[line_idx];
        let align_off = match block.paragraph.alignment {
            TextAlignment::Left => 0.0,
            TextAlignment::Center => -line_width * 0.5,
            TextAlignment::Right => -line_width,
        };
        let mut x_cursor = align_off;

        for seg in segs {
            let font = &seg.font;
            let scaled = font.as_scaled(ab_glyph::PxScale {
                x: seg.font_size * seg.width_scale,
                y: seg.font_size * seg.height_scale,
            });
            let baseline_y = block.position[1] + y_pos + max_ascent;
            let seg_ascent = seg.ascent;
            let mut prev_glyph: Option<ab_glyph::GlyphId> = None;
            let mut seg_flat = seg.flat_start;

            for ch in seg.text.chars() {
                let gid = font.glyph_id(ch);
                if let Some(prev) = prev_glyph {
                    x_cursor += scaled.kern(prev, gid);
                }
                let h_advance = scaled.h_advance(gid);
                let flat_start = seg_flat;
                let flat_end = flat_start + ch.len_utf8();

                // Glyph box: use the tight glyph bounds from the font
                let glyph_x = block.position[0] + x_cursor;
                let glyph_y = baseline_y - seg_ascent;
                let glyph_w = h_advance + seg.letter_spacing.max(0.0);
                let glyph_h = seg.line_height;

                result.push(GlyphBounds {
                    glyph_index: glyph_idx,
                    ch,
                    flat_start,
                    flat_end,
                    x: glyph_x,
                    y: glyph_y,
                    w: glyph_w,
                    h: glyph_h,
                    cx: glyph_x + glyph_w * 0.5,
                    cy: glyph_y + glyph_h * 0.5,
                });

                x_cursor += h_advance;
                x_cursor += seg.letter_spacing;
                prev_glyph = Some(gid);
                glyph_idx += 1;
                seg_flat = flat_end;
            }
        }

        y_pos += max_lh;
    }

    result
}

/// Hit-test a point (in canvas coordinates) against all blocks in a TextLayerData.
/// Returns the block index if the point falls within a block's bounding box.
pub fn hit_test_blocks(data: &TextLayerData, x: f32, y: f32) -> Option<usize> {
    for (idx, block) in data.blocks.iter().enumerate() {
        let layout = compute_block_layout(block);
        let bx = block.position[0];
        let by = block.position[1];
        // Use max_width for box width if set, otherwise use natural text width
        let box_w = block.max_width.unwrap_or(layout.total_width);
        let box_h = if let Some(mh) = block.max_height {
            mh.max(layout.total_height)
        } else {
            layout.total_height
        };
        let margin = 10.0;
        if x >= bx - margin
            && x <= bx + box_w + margin
            && y >= by - margin
            && y <= by + box_h + margin
        {
            return Some(idx);
        }
    }
    None
}

