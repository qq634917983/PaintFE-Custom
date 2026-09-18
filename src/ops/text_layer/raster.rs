pub fn rasterize_single_block(
    block: &mut TextBlock,
    content_generation: u64,
    canvas_w: u32,
    canvas_h: u32,
    target: &mut crate::canvas::TiledImage,
    coverage_buf: &mut Vec<f32>,
    glyph_cache: &mut GlyphPixelCache,
) {
    rasterize_block_multirun(
        block,
        content_generation,
        canvas_w,
        canvas_h,
        target,
        coverage_buf,
        glyph_cache,
    );
}

/// Find the tight AABB of non-transparent pixels in an RGBA buffer.
/// Returns (x, y, w, h, cropped_buf). Returns (0, 0, 0, 0, vec![]) if empty.
pub fn find_tight_bounds_rgba(data: &[u8], w: u32, h: u32) -> (u32, u32, u32, u32, Vec<u8>) {
    let (w_us, h_us) = (w as usize, h as usize);
    let mut min_x = w_us;
    let mut min_y = h_us;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    for y in 0..h_us {
        for x in 0..w_us {
            if data[(y * w_us + x) * 4 + 3] > 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if max_x < min_x || max_y < min_y {
        return (0, 0, 0, 0, Vec::new());
    }
    let tw = max_x - min_x + 1;
    let th = max_y - min_y + 1;
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th {
        let src_off = ((min_y + y) * w_us + min_x) * 4;
        let dst_off = y * tw * 4;
        out[dst_off..dst_off + tw * 4].copy_from_slice(&data[src_off..src_off + tw * 4]);
    }
    (min_x as u32, min_y as u32, tw as u32, th as u32, out)
}

/// Rasterize a single block with multi-run support.
/// Each run can have a different font/size/weight/color. Runs on the same line
/// share a common baseline (derived from the tallest ascent on that line).
fn rasterize_block_multirun(
    block: &mut TextBlock,
    content_generation: u64,
    canvas_w: u32,
    canvas_h: u32,
    target: &mut TiledImage,
    coverage_buf: &mut Vec<f32>,
    glyph_cache: &mut GlyphPixelCache,
) {
    // If there's only one run, or all runs share the same style, use the simple path
    // for maximum compatibility with the existing rasterizer.
    let all_text: String = block.runs.iter().map(|r| r.text.as_str()).collect();
    if all_text.is_empty() {
        return;
    }

    let has_warp = !matches!(block.warp, TextWarp::None);
    let has_glyph_overrides = block.has_glyph_overrides();
    let has_rotation = block.rotation.abs() > 0.001;

    // Precompute rotation pivot (matches UI overlay center)
    let rot_pivot = if has_rotation {
        let layout = compute_block_layout(&*block);
        Some(block_rotation_pivot(&*block, &layout))
    } else {
        None
    };

    // If glyph overrides exist, use per-glyph rasterization path
    if has_glyph_overrides {
        if has_rotation {
            // Per-glyph into temp, then rotate the whole block result
            let mut temp = TiledImage::new(canvas_w, canvas_h);
            rasterize_block_per_glyph(
                &*block,
                canvas_w,
                canvas_h,
                &mut temp,
                coverage_buf,
                glyph_cache,
            );
            let max_font = block
                .runs
                .iter()
                .map(|r| r.style.font_size)
                .fold(0.0f32, f32::max);
            let margin = (max_font * 2.0) as u32 + 20;
            let bx = (block.position[0] as i32 - margin as i32).max(0) as u32;
            let by = (block.position[1] as i32 - margin as i32).max(0) as u32;
            let bx2 = (block.position[0] as u32 + canvas_w / 2 + margin).min(canvas_w);
            let by2 = (block.position[1] as u32 + canvas_h / 2 + margin).min(canvas_h);
            let bw = bx2.saturating_sub(bx);
            let bh = by2.saturating_sub(by);
            if bw > 0 && bh > 0 {
                let region = temp.extract_region_rgba(bx, by, bw, bh);
                if let Some((tx, ty, tw, th, trimmed)) = trim_to_content(&region, bw, bh) {
                    maybe_rotate_and_blit(
                        target,
                        &trimmed,
                        tw,
                        th,
                        bx as i32 + tx as i32,
                        by as i32 + ty as i32,
                        block.rotation,
                        canvas_w,
                        canvas_h,
                        rot_pivot,
                    );
                }
            }
        } else {
            rasterize_block_per_glyph(
                &*block,
                canvas_w,
                canvas_h,
                target,
                coverage_buf,
                glyph_cache,
            );
        }
        return;
    }

    let single_style =
        block.runs.len() <= 1 || block.runs.windows(2).all(|w| w[0].style == w[1].style);

    if single_style {
        // Fast path: check per-block cache for position-only changes
        if let Some(ref cached) = block.cached_raster
            && cached.content_generation == content_generation
            && cached.buf_w > 0
            && cached.buf_h > 0
        {
            let dx = (block.position[0] - cached.origin[0]).round() as i32;
            let dy = (block.position[1] - cached.origin[1]).round() as i32;
            let adj_off_x = cached.off_x + dx;
            let adj_off_y = cached.off_y + dy;
            if has_warp {
                if let Some((warped, ww, wh, wox, woy)) =
                    apply_block_warp(&cached.buf, cached.buf_w, cached.buf_h, &block.warp)
                {
                    maybe_rotate_and_blit(
                        target,
                        &warped,
                        ww,
                        wh,
                        adj_off_x + wox,
                        adj_off_y + woy,
                        block.rotation,
                        canvas_w,
                        canvas_h,
                        rot_pivot,
                    );
                }
            } else {
                maybe_rotate_and_blit(
                    target,
                    &cached.buf,
                    cached.buf_w,
                    cached.buf_h,
                    adj_off_x,
                    adj_off_y,
                    block.rotation,
                    canvas_w,
                    canvas_h,
                    rot_pivot,
                );
            }
            return;
        }

        // Cache miss: full rasterization
        let style = block.runs.first().map(|r| &r.style).unwrap_or_else(|| {
            static DEFAULT: std::sync::OnceLock<TextStyle> = std::sync::OnceLock::new();
            DEFAULT.get_or_init(TextStyle::default)
        });

        let font = match load_font_for_style(style) {
            Some(f) => f,
            None => return,
        };

        let alignment = match block.paragraph.alignment {
            TextAlignment::Left => text::TextAlignment::Left,
            TextAlignment::Center => text::TextAlignment::Center,
            TextAlignment::Right => text::TextAlignment::Right,
        };

        let rasterized = text::rasterize_text(
            &font,
            text::font_cache_key(&style.font_family, style.font_weight, style.italic),
            &all_text,
            style.font_size,
            alignment,
            block.position[0],
            block.position[1],
            style.color,
            true,
            style.font_weight >= 700,
            style.italic,
            style.underline,
            style.strikethrough,
            canvas_w,
            canvas_h,
            coverage_buf,
            glyph_cache,
            block.max_width,
            style.letter_spacing,
            block.paragraph.line_spacing,
            style.width_scale,
            style.height_scale,
        );

        // Cache the rasterized buffer for fast position-only updates
        if rasterized.buf_w > 0 && rasterized.buf_h > 0 {
            block.cached_raster = Some(CachedBlockRaster {
                buf: rasterized.buf.clone(),
                buf_w: rasterized.buf_w,
                buf_h: rasterized.buf_h,
                off_x: rasterized.off_x,
                off_y: rasterized.off_y,
                origin: block.position,
                content_generation,
            });
        }

        if rasterized.buf_w > 0 && rasterized.buf_h > 0 {
            if has_warp {
                if let Some((warped, ww, wh, wox, woy)) = apply_block_warp(
                    &rasterized.buf,
                    rasterized.buf_w,
                    rasterized.buf_h,
                    &block.warp,
                ) {
                    maybe_rotate_and_blit(
                        target,
                        &warped,
                        ww,
                        wh,
                        rasterized.off_x + wox,
                        rasterized.off_y + woy,
                        block.rotation,
                        canvas_w,
                        canvas_h,
                        rot_pivot,
                    );
                }
            } else {
                maybe_rotate_and_blit(
                    target,
                    &rasterized.buf,
                    rasterized.buf_w,
                    rasterized.buf_h,
                    rasterized.off_x,
                    rasterized.off_y,
                    block.rotation,
                    canvas_w,
                    canvas_h,
                    rot_pivot,
                );
            }
        }
        return;
    }

    // Multi-run path: lay out each run segment with its own font/size,
    // sharing a baseline per line (tallest ascent wins).

    // Check per-block cache for position-only changes (multi-run).
    // The cached buffer stores the tight pre-warp/pre-rotation raster.
    if let Some(ref cached) = block.cached_raster
        && cached.content_generation == content_generation
        && cached.buf_w > 0
        && cached.buf_h > 0
    {
        let dx = (block.position[0] - cached.origin[0]).round() as i32;
        let dy = (block.position[1] - cached.origin[1]).round() as i32;
        let adj_off_x = cached.off_x + dx;
        let adj_off_y = cached.off_y + dy;
        if has_warp {
            if let Some((warped, ww, wh, wox, woy)) =
                apply_block_warp(&cached.buf, cached.buf_w, cached.buf_h, &block.warp)
            {
                maybe_rotate_and_blit(
                    target,
                    &warped,
                    ww,
                    wh,
                    adj_off_x + wox,
                    adj_off_y + woy,
                    block.rotation,
                    canvas_w,
                    canvas_h,
                    rot_pivot,
                );
            }
        } else {
            maybe_rotate_and_blit(
                target,
                &cached.buf,
                cached.buf_w,
                cached.buf_h,
                adj_off_x,
                adj_off_y,
                block.rotation,
                canvas_w,
                canvas_h,
                rot_pivot,
            );
        }
        return;
    }

    // Cache miss — full multi-run rasterization, then cache the tight result.
    let rasterize_multirun_and_extract = |block: &TextBlock,
                                          canvas_w: u32,
                                          canvas_h: u32,
                                          coverage_buf: &mut Vec<f32>,
                                          glyph_cache: &mut GlyphPixelCache|
     -> Option<(Vec<u8>, u32, u32, i32, i32)> {
        let mut temp = TiledImage::new(canvas_w, canvas_h);
        rasterize_block_multirun_slow(
            block,
            canvas_w,
            canvas_h,
            &mut temp,
            coverage_buf,
            glyph_cache,
        );
        let max_font = block
            .runs
            .iter()
            .map(|r| r.style.font_size)
            .fold(0.0f32, f32::max);
        let margin = (max_font * 2.0) as u32 + 20;
        let bx = (block.position[0] as i32 - margin as i32).max(0) as u32;
        let by = (block.position[1] as i32 - margin as i32).max(0) as u32;
        let bx2 = (block.position[0] as u32 + canvas_w / 2 + margin).min(canvas_w);
        let by2 = (block.position[1] as u32 + canvas_h / 2 + margin).min(canvas_h);
        let bw = bx2.saturating_sub(bx);
        let bh = by2.saturating_sub(by);
        if bw == 0 || bh == 0 {
            return None;
        }
        let region = temp.extract_region_rgba(bx, by, bw, bh);
        if let Some((tx, ty, tw, th, trimmed)) = trim_to_content(&region, bw, bh) {
            Some((
                trimmed,
                tw,
                th,
                bx as i32 + tx as i32,
                by as i32 + ty as i32,
            ))
        } else {
            None
        }
    };

    if let Some((buf, tw, th, off_x, off_y)) =
        rasterize_multirun_and_extract(&*block, canvas_w, canvas_h, coverage_buf, glyph_cache)
    {
        // Cache the tight buffer for fast position-only updates
        block.cached_raster = Some(CachedBlockRaster {
            buf: buf.clone(),
            buf_w: tw,
            buf_h: th,
            off_x,
            off_y,
            origin: block.position,
            content_generation,
        });

        if has_warp {
            if let Some((warped, ww, wh, wox, woy)) = apply_block_warp(&buf, tw, th, &block.warp) {
                maybe_rotate_and_blit(
                    target,
                    &warped,
                    ww,
                    wh,
                    off_x + wox,
                    off_y + woy,
                    block.rotation,
                    canvas_w,
                    canvas_h,
                    rot_pivot,
                );
            }
        } else {
            maybe_rotate_and_blit(
                target,
                &buf,
                tw,
                th,
                off_x,
                off_y,
                block.rotation,
                canvas_w,
                canvas_h,
                rot_pivot,
            );
        }
    }
}

/// Segment of text within a single run that belongs to a single line.
struct RunSegment {
    run_index: usize,
    text: String,
    style: TextStyle,
    font: FontArc,
    font_cache_key: u64,
    ascent: f32,
    line_height: f32,
    advance: f32,
}

// ---------------------------------------------------------------------------
// Per-Glyph Rasterization (Phase 5 — Batch 9: glyph overrides)
// ---------------------------------------------------------------------------

/// Rasterize a block with per-glyph overrides.
/// Each glyph is rasterized individually and then transformed (offset, rotated, scaled)
/// according to its GlyphOverride entry before compositing into the target.
fn rasterize_block_per_glyph(
    block: &TextBlock,
    canvas_w: u32,
    canvas_h: u32,
    target: &mut TiledImage,
    coverage_buf: &mut Vec<f32>,
    glyph_cache: &mut GlyphPixelCache,
) {
    // Compute glyph positions (same layout as compute_glyph_bounds)
    let bounds = compute_glyph_bounds(block);
    if bounds.is_empty() {
        return;
    }

    // Rasterize each glyph individually
    for gb in &bounds {
        // Find the style for this glyph:
        // Walk runs to determine which run contains this glyph_index
        let (style, font) = match find_glyph_style(block, gb.glyph_index) {
            Some(sf) => sf,
            None => continue,
        };

        // Get override for this glyph
        let ovr = block.get_glyph_override(gb.glyph_index);
        let offset_x = ovr.map_or(0.0, |o| o.position_offset[0]);
        let offset_y = ovr.map_or(0.0, |o| o.position_offset[1]);
        let rotation = ovr.map_or(0.0, |o| o.rotation);
        let scale = ovr.map_or(1.0, |o| o.scale);

        // Compute the glyph's canvas position with offset
        let glyph_origin_x = gb.x + offset_x;
        let glyph_origin_y = gb.y + offset_y;

        // Rasterize this single character
        let ch_str = gb.ch.to_string();
        let rasterized = text::rasterize_text(
            &font,
            text::font_cache_key(&style.font_family, style.font_weight, style.italic),
            &ch_str,
            style.font_size * scale,
            text::TextAlignment::Left,
            glyph_origin_x,
            glyph_origin_y,
            style.color,
            true,
            style.font_weight >= 700,
            style.italic,
            style.underline,
            style.strikethrough,
            canvas_w,
            canvas_h,
            coverage_buf,
            glyph_cache,
            None,
            style.letter_spacing,
            1.0,
            style.width_scale,
            style.height_scale,
        );

        if rasterized.buf_w == 0 || rasterized.buf_h == 0 {
            continue;
        }

        // If rotation is needed, rotate the glyph buffer around its center
        if rotation.abs() > 0.001 {
            let (rotated, rw, rh, rx_off, ry_off) = rotate_glyph_buffer(
                &rasterized.buf,
                rasterized.buf_w,
                rasterized.buf_h,
                rotation,
                None,
            );
            blit_rgba_buffer(
                target,
                &rotated,
                rw,
                rh,
                rasterized.off_x + rx_off,
                rasterized.off_y + ry_off,
                canvas_w,
                canvas_h,
            );
        } else {
            blit_rgba_buffer(
                target,
                &rasterized.buf,
                rasterized.buf_w,
                rasterized.buf_h,
                rasterized.off_x,
                rasterized.off_y,
                canvas_w,
                canvas_h,
            );
        }
    }
}

/// Find the TextStyle and font for a specific glyph index within a block.
/// Walks the runs to determine which run contains the glyph at the given
/// flat index (newlines excluded from the count).
fn find_glyph_style(block: &TextBlock, glyph_index: usize) -> Option<(TextStyle, FontArc)> {
    let mut idx = 0usize;
    for run in &block.runs {
        for ch in run.text.chars() {
            if ch == '\n' {
                continue;
            }
            if idx == glyph_index {
                let font = load_font_for_style(&run.style)?;
                return Some((run.style.clone(), font));
            }
            idx += 1;
        }
    }
    None
}

/// Rotate an RGBA buffer around its center by the given angle (radians).
/// Returns (rotated_buffer, new_width, new_height, x_offset, y_offset)
/// where offsets are adjustments to the original blit position.
fn rotate_glyph_buffer(
    buf: &[u8],
    w: u32,
    h: u32,
    angle: f32,
    pivot: Option<(f32, f32)>,
) -> (Vec<u8>, u32, u32, i32, i32) {
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    let fw = w as f32;
    let fh = h as f32;

    // Rotation pivot: custom or buffer center
    let (cx, cy) = pivot.unwrap_or((fw * 0.5, fh * 0.5));

    // Compute new bounding box for the rotated image
    let corners = [(0.0f32, 0.0f32), (fw, 0.0), (0.0, fh), (fw, fh)];

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    for &(px, py) in &corners {
        let dx = px - cx;
        let dy = py - cy;
        let rx = dx * cos_a - dy * sin_a + cx;
        let ry = dx * sin_a + dy * cos_a + cy;
        min_x = min_x.min(rx);
        min_y = min_y.min(ry);
        max_x = max_x.max(rx);
        max_y = max_y.max(ry);
    }

    let new_w = (max_x - min_x).ceil() as u32 + 1;
    let new_h = (max_y - min_y).ceil() as u32 + 1;
    // The pivot's location in the output buffer = (cx - min_x, cy - min_y)
    let new_cx = cx - min_x.floor();
    let new_cy = cy - min_y.floor();

    let mut out = vec![0u8; (new_w * new_h * 4) as usize];

    // Inverse transform: for each output pixel, find source pixel
    for oy in 0..new_h {
        for ox in 0..new_w {
            let dx = ox as f32 - new_cx;
            let dy = oy as f32 - new_cy;
            // Rotate back (inverse rotation)
            let sx = dx * cos_a + dy * sin_a + cx;
            let sy = -dx * sin_a + dy * cos_a + cy;

            // Bilinear sample from source
            if sx >= 0.0 && sx < fw && sy >= 0.0 && sy < fh {
                let sx0 = sx.floor() as u32;
                let sy0 = sy.floor() as u32;
                let sx1 = (sx0 + 1).min(w - 1);
                let sy1 = (sy0 + 1).min(h - 1);
                let fx = sx - sx0 as f32;
                let fy = sy - sy0 as f32;

                let sample = |x: u32, y: u32| -> [f32; 4] {
                    let i = (y * w + x) as usize * 4;
                    [
                        buf[i] as f32,
                        buf[i + 1] as f32,
                        buf[i + 2] as f32,
                        buf[i + 3] as f32,
                    ]
                };

                let p00 = sample(sx0, sy0);
                let p10 = sample(sx1, sy0);
                let p01 = sample(sx0, sy1);
                let p11 = sample(sx1, sy1);

                let oi = (oy * new_w + ox) as usize * 4;
                for c in 0..4 {
                    let val = p00[c] * (1.0 - fx) * (1.0 - fy)
                        + p10[c] * fx * (1.0 - fy)
                        + p01[c] * (1.0 - fx) * fy
                        + p11[c] * fx * fy;
                    out[oi + c] = val.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    let x_off = (min_x.floor()) as i32;
    let y_off = (min_y.floor()) as i32;

    (out, new_w, new_h, x_off, y_off)
}

/// Multi-run rasterization: each run may have a different font/size.
/// We split by newlines first, determine per-line metrics (max ascent, max line height),
/// then rasterize each segment at the correct baseline.
fn rasterize_block_multirun_slow(
    block: &TextBlock,
    canvas_w: u32,
    canvas_h: u32,
    target: &mut TiledImage,
    coverage_buf: &mut Vec<f32>,
    glyph_cache: &mut GlyphPixelCache,
) {
    // Build segments: split each run by '\n' into per-line pieces
    let mut lines: Vec<Vec<RunSegment>> = vec![Vec::new()];

    for (run_idx, run) in block.runs.iter().enumerate() {
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
                // Start a new line
                lines.push(Vec::new());
            }
            if part.is_empty() {
                // Even empty segments contribute to line metrics
                lines.last_mut().unwrap().push(RunSegment {
                    run_index: run_idx,
                    text: String::new(),
                    style: run.style.clone(),
                    font: font.clone(),
                    font_cache_key: text::font_cache_key(
                        &run.style.font_family,
                        run.style.font_weight,
                        run.style.italic,
                    ),
                    ascent: scaled.ascent(),
                    line_height: scaled.height(),
                    advance: 0.0,
                });
                continue;
            }
            // Compute advance for this segment
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
            lines.last_mut().unwrap().push(RunSegment {
                run_index: run_idx,
                text: part.to_string(),
                style: run.style.clone(),
                font: font.clone(),
                font_cache_key: text::font_cache_key(
                    &run.style.font_family,
                    run.style.font_weight,
                    run.style.italic,
                ),
                ascent: scaled.ascent(),
                line_height: scaled.height(),
                advance,
            });
        }
    }

    // Compute per-line metrics
    let line_metrics: Vec<(f32, f32, f32)> = lines
        .iter()
        .map(|segs| {
            let max_ascent = segs.iter().map(|s| s.ascent).fold(0.0f32, f32::max);
            let max_lh = segs.iter().map(|s| s.line_height).fold(0.0f32, f32::max);
            let total_width: f32 = segs.iter().map(|s| s.advance).sum();
            (total_width, max_ascent, max_lh)
        })
        .collect();

    // Rasterize each segment individually
    let mut y_pos = 0.0f32;
    for (line_idx, segs) in lines.iter().enumerate() {
        let (line_width, max_ascent, max_lh) = line_metrics[line_idx];

        // Alignment offset for this line
        let align_off = match block.paragraph.alignment {
            TextAlignment::Left => 0.0,
            TextAlignment::Center => -line_width * 0.5,
            TextAlignment::Right => -line_width,
        };

        let mut x_cursor = align_off;

        for seg in segs {
            if seg.text.is_empty() {
                continue;
            }

            // This segment's baseline y = block.position[1] + y_pos + max_ascent
            // But since rasterize_text takes origin as baseline-start, we pass
            // (block.position[0] + x_cursor, block.position[1] + y_pos)
            let seg_origin_x = block.position[0] + x_cursor;
            let seg_origin_y = block.position[1] + y_pos + (max_ascent - seg.ascent);

            let rasterized = text::rasterize_text(
                &seg.font,
                seg.font_cache_key,
                &seg.text,
                seg.style.font_size,
                text::TextAlignment::Left, // alignment handled by x_cursor
                seg_origin_x,
                seg_origin_y,
                seg.style.color,
                true,
                seg.style.font_weight >= 700,
                seg.style.italic,
                seg.style.underline,
                seg.style.strikethrough,
                canvas_w,
                canvas_h,
                coverage_buf,
                glyph_cache,
                None, // word wrap handled at segment level
                seg.style.letter_spacing,
                block.paragraph.line_spacing,
                seg.style.width_scale,
                seg.style.height_scale,
            );

            if rasterized.buf_w > 0 && rasterized.buf_h > 0 {
                blit_rgba_buffer(
                    target,
                    &rasterized.buf,
                    rasterized.buf_w,
                    rasterized.buf_h,
                    rasterized.off_x,
                    rasterized.off_y,
                    canvas_w,
                    canvas_h,
                );
            }

            x_cursor += seg.advance;
        }

        y_pos += max_lh * block.paragraph.line_spacing;
    }
}

