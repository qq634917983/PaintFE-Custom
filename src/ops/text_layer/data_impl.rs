impl TextLayerData {
    /// Returns true if the cached rasterized pixels are stale.
    pub fn needs_rasterize(&self) -> bool {
        self.cache_generation != self.raster_generation
    }

    /// Mark text data as dirty (call after any text content edit).
    pub fn mark_dirty(&mut self) {
        self.cache_generation = self.cache_generation.wrapping_add(1);
        self.text_content_generation = self.text_content_generation.wrapping_add(1);
        self.position_generation = self.position_generation.wrapping_add(1);
    }

    /// Mark only effects as dirty (text content unchanged — can reuse cached text RGBA).
    pub fn mark_effects_dirty(&mut self) {
        self.cache_generation = self.cache_generation.wrapping_add(1);
    }

    /// Mark as dirty due to position-only change (preserves text content cache).
    /// The glyph pixel cache will NOT be cleared since text_content_generation is unchanged.
    pub fn mark_position_dirty(&mut self) {
        self.cache_generation = self.cache_generation.wrapping_add(1);
        self.position_generation = self.position_generation.wrapping_add(1);
    }

    /// Concatenate all text from all blocks into a single flat string
    /// (for simple single-block display).
    pub fn flat_text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            for run in &block.runs {
                out.push_str(&run.text);
            }
        }
        out
    }

    /// Get the primary style (from the first run of the first block).
    pub fn primary_style(&self) -> &TextStyle {
        self.blocks
            .first()
            .and_then(|b| b.runs.first())
            .map(|r| &r.style)
            .unwrap_or_else(|| {
                static DEFAULT: std::sync::OnceLock<TextStyle> = std::sync::OnceLock::new();
                DEFAULT.get_or_init(TextStyle::default)
            })
    }

    /// Create a new block at the given position, returns its ID.
    pub fn add_block(&mut self, x: f32, y: f32) -> u64 {
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.blocks.push(TextBlock {
            id,
            position: [x, y],
            rotation: 0.0,
            runs: vec![TextRun {
                text: String::new(),
                style: self.primary_style().clone(),
            }],
            paragraph: ParagraphStyle::default(),
            max_width: None,
            max_height: None,
            warp: TextWarp::None,
            glyph_overrides: Vec::new(),
            cached_raster: None,
        });
        self.mark_dirty();
        id
    }

    /// Remove empty blocks (blocks whose runs all have empty text).
    /// Returns the number of blocks removed.
    pub fn remove_empty_blocks(&mut self) -> usize {
        let before = self.blocks.len();
        self.blocks
            .retain(|b| b.runs.iter().any(|r| !r.text.is_empty()));
        let removed = before - self.blocks.len();
        if removed > 0 {
            self.mark_dirty();
        }
        removed
    }

    /// Find block index by ID.
    pub fn block_index_by_id(&self, id: u64) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    /// Rasterize all text blocks into a `TiledImage`, applying any active effects.
    pub fn rasterize(
        &mut self,
        canvas_w: u32,
        canvas_h: u32,
        coverage_buf: &mut Vec<f32>,
        glyph_cache: &mut GlyphPixelCache,
    ) -> TiledImage {
        let has_effects = self.effects.has_any();

        // If text content AND positions haven't changed, reuse cached text RGBA.
        let text_rgba_valid = self.cached_text_generation == self.text_content_generation
            && self.cached_position_generation == self.position_generation
            && !self.cached_text_rgba.is_empty()
            && self.cached_text_w == canvas_w
            && self.cached_text_h == canvas_h;

        if has_effects {
            // We need the raw text RGBA (no effects) to apply effects on top.
            if !text_rgba_valid {
                // Rasterize text into a flat RGBA buffer
                let mut text_tiled = TiledImage::new(canvas_w, canvas_h);
                let content_gen = self.text_content_generation;
                for i in 0..self.blocks.len() {
                    rasterize_block_multirun(
                        &mut self.blocks[i],
                        content_gen,
                        canvas_w,
                        canvas_h,
                        &mut text_tiled,
                        coverage_buf,
                        glyph_cache,
                    );
                }
                self.cached_text_rgba = text_tiled.extract_region_rgba(0, 0, canvas_w, canvas_h);
                self.cached_text_generation = self.text_content_generation;
                self.cached_position_generation = self.position_generation;
                self.cached_text_w = canvas_w;
                self.cached_text_h = canvas_h;
            }

            // Compute the maximum padding required by active effects.
            // Shadow: offset + blur_radius + spread.  Outline: width.
            let mut pad = 0.0f32;
            if let Some(ref shadow) = self.effects.shadow {
                pad = pad.max(
                    shadow.offset_x.abs()
                        + shadow.offset_y.abs()
                        + shadow.blur_radius * 3.0
                        + shadow.spread,
                );
            }
            if let Some(ref outline) = self.effects.outline {
                pad = pad.max(outline.width.ceil() + 2.0);
            }
            if let Some(ref inner) = self.effects.inner_shadow {
                pad =
                    pad.max(inner.offset_x.abs() + inner.offset_y.abs() + inner.blur_radius * 3.0);
            }
            let pad_px = (pad.ceil() as u32).max(4);

            // Find tight AABB of non-transparent text pixels
            let (bx, by, bw, bh, _) =
                find_tight_bounds_rgba(&self.cached_text_rgba, canvas_w, canvas_h);

            if bw == 0 || bh == 0 {
                // No visible text — return empty
                return TiledImage::new(canvas_w, canvas_h);
            }

            // Expand bounds by effect padding, clamped to canvas
            let rx = (bx as i32 - pad_px as i32).max(0) as u32;
            let ry = (by as i32 - pad_px as i32).max(0) as u32;
            let rx2 = (bx + bw + pad_px).min(canvas_w);
            let ry2 = (by + bh + pad_px).min(canvas_h);
            let rw = rx2 - rx;
            let rh = ry2 - ry;

            // If the region covers most of the canvas, fall back to full-canvas path
            let region_area = (rw as u64) * (rh as u64);
            let canvas_area = (canvas_w as u64) * (canvas_h as u64);
            if region_area * 2 >= canvas_area {
                let final_rgba =
                    apply_text_effects(&self.cached_text_rgba, canvas_w, canvas_h, &self.effects);
                TiledImage::from_raw_rgba(canvas_w, canvas_h, &final_rgba)
            } else {
                // Extract tight region from cached RGBA
                let cw = canvas_w as usize;
                let mut region_in = vec![0u8; (rw as usize) * (rh as usize) * 4];
                for row in 0..rh as usize {
                    let src_off = ((ry as usize + row) * cw + rx as usize) * 4;
                    let dst_off = row * rw as usize * 4;
                    let len = rw as usize * 4;
                    region_in[dst_off..dst_off + len]
                        .copy_from_slice(&self.cached_text_rgba[src_off..src_off + len]);
                }

                let region_out = apply_text_effects(&region_in, rw, rh, &self.effects);

                // Blit the effects result back into a TiledImage at the correct offset
                let mut result = TiledImage::new(canvas_w, canvas_h);
                result.blit_rgba_at(rx as i32, ry as i32, rw, rh, &region_out);
                result
            }
        } else {
            // No effects — standard rasterization.
            // Skip the full-canvas cache extraction (it's only needed when
            // effects are added later, and will be populated on demand).
            let mut result = TiledImage::new(canvas_w, canvas_h);
            let content_gen = self.text_content_generation;
            for i in 0..self.blocks.len() {
                rasterize_block_multirun(
                    &mut self.blocks[i],
                    content_gen,
                    canvas_w,
                    canvas_h,
                    &mut result,
                    coverage_buf,
                    glyph_cache,
                );
            }
            // Invalidate cached text RGBA — it will be recomputed if effects are
            // added later. This avoids a full-canvas extract_region_rgba every frame.
            self.cached_text_rgba.clear();
            self.cached_text_generation = 0;
            self.cached_position_generation = 0;
            self.cached_text_w = 0;
            self.cached_text_h = 0;
            result
        }
    }
}

// ---------------------------------------------------------------------------
// TextBlock helpers
// ---------------------------------------------------------------------------

