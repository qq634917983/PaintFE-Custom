impl PartialEq for TextStyle {
    fn eq(&self, other: &Self) -> bool {
        self.font_family == other.font_family
            && self.font_weight == other.font_weight
            && self.font_size.to_bits() == other.font_size.to_bits()
            && self.italic == other.italic
            && self.underline == other.underline
            && self.strikethrough == other.strikethrough
            && self.color == other.color
            && self.letter_spacing.to_bits() == other.letter_spacing.to_bits()
            && self.baseline_offset.to_bits() == other.baseline_offset.to_bits()
            && self.width_scale.to_bits() == other.width_scale.to_bits()
            && self.height_scale.to_bits() == other.height_scale.to_bits()
    }
}

impl Eq for TextStyle {}

// ---------------------------------------------------------------------------
// TextSelection helpers
// ---------------------------------------------------------------------------

impl TextSelection {
    /// Whether there is an active selection (anchor != cursor).
    pub fn has_selection(&self) -> bool {
        self.anchor != self.cursor
    }

    /// Get the ordered (start, end) flat byte offsets within a block.
    pub fn ordered_flat_offsets(&self, block: &TextBlock) -> (usize, usize) {
        let a = block.run_pos_to_flat_offset(self.anchor);
        let c = block.run_pos_to_flat_offset(self.cursor);
        if a <= c { (a, c) } else { (c, a) }
    }

    /// Collapse selection to cursor position (deselect).
    pub fn collapse_to_cursor(&mut self) {
        self.anchor = self.cursor;
    }
}

impl RunPosition {
    /// Compare two positions using flat byte offsets.
    pub fn cmp_in(&self, other: &RunPosition, block: &TextBlock) -> std::cmp::Ordering {
        let a = block.run_pos_to_flat_offset(*self);
        let b = block.run_pos_to_flat_offset(*other);
        a.cmp(&b)
    }
}

// ---------------------------------------------------------------------------
// Multi-run rasterization (Batch 3)
// ---------------------------------------------------------------------------

/// Compute the rotation pivot for a `TextBlock` in canvas-pixel coordinates.
/// This must match the pivot used by the UI overlay so that the rendered text
/// and the overlay handles rotate around the same point.
fn block_rotation_pivot(block: &TextBlock, layout: &BlockLayout) -> (f32, f32) {
    let display_w = block.max_width.unwrap_or(layout.total_width).max(1.0);
    let display_h = block
        .max_height
        .map(|mh| mh.max(layout.total_height))
        .unwrap_or(layout.total_height)
        .max(1.0);
    (
        block.position[0] + display_w * 0.5,
        block.position[1] + display_h * 0.5,
    )
}

/// Optionally rotate an RGBA buffer by `rotation` radians and then blit the result
/// onto the target `TiledImage`.  If `rotation` is near-zero, blits directly.
///
/// `pivot_canvas` — if `Some((px, py))`, rotation happens around that point
/// (in canvas pixel coordinates); the buffer-local pivot is derived from
/// `(px - off_x, py - off_y)`.  If `None`, the buffer center is used.
fn maybe_rotate_and_blit(
    target: &mut TiledImage,
    buf: &[u8],
    buf_w: u32,
    buf_h: u32,
    off_x: i32,
    off_y: i32,
    rotation: f32,
    canvas_w: u32,
    canvas_h: u32,
    pivot_canvas: Option<(f32, f32)>,
) {
    if rotation.abs() > 0.001 {
        let local_pivot = pivot_canvas.map(|(px, py)| (px - off_x as f32, py - off_y as f32));
        let (rotated, rw, rh, rx_off, ry_off) =
            rotate_glyph_buffer(buf, buf_w, buf_h, rotation, local_pivot);
        blit_rgba_buffer(
            target,
            &rotated,
            rw,
            rh,
            off_x + rx_off,
            off_y + ry_off,
            canvas_w,
            canvas_h,
        );
    } else {
        blit_rgba_buffer(target, buf, buf_w, buf_h, off_x, off_y, canvas_w, canvas_h);
    }
}

