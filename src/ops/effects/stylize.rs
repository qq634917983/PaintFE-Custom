pub fn glow(state: &mut CanvasState, layer_idx: usize, radius: f32, intensity: f32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = glow_core(&flat, radius, intensity, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn glow_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    radius: f32,
    intensity: f32,
    original_flat: &RgbaImage,
) {
    let result = glow_core(
        original_flat,
        radius,
        intensity,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn glow_core(
    flat: &RgbaImage,
    radius: f32,
    intensity: f32,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    // Glow = original + blurred * intensity (screen blend)
    let blurred = crate::ops::filters::parallel_gaussian_blur_pub(flat, radius);
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    let src_raw = flat.as_raw();
    let blur_raw = blurred.as_raw();
    let stride = w * 4;
    let mut dst_raw = vec![0u8; w * h * 4];
    let mask_raw = mask.map(|m| m.as_raw().as_slice());
    let mask_w = mask.map_or(0, |m| m.width() as usize);
    let mask_h = mask.map_or(0, |m| m.height() as usize);

    dst_raw
        .par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..w {
                let pi = x * 4;
                if let Some(mr) = mask_raw
                    && x < mask_w
                    && y < mask_h
                    && mr[y * mask_w + x] == 0
                {
                    let si = y * stride + pi;
                    row_out[pi..pi + 4].copy_from_slice(&src_raw[si..si + 4]);
                    continue;
                }
                let si = y * stride + pi;
                for c in 0..3 {
                    let s = src_raw[si + c] as f32 / 255.0;
                    let b = blur_raw[si + c] as f32 / 255.0;
                    // Screen blend: 1 - (1 - s) * (1 - b * intensity)
                    let result = 1.0 - (1.0 - s) * (1.0 - b * intensity);
                    row_out[pi + c] = (result * 255.0).round().clamp(0.0, 255.0) as u8;
                }
                row_out[pi + 3] = src_raw[si + 3]; // preserve alpha
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// --- Sharpen (unsharp mask) ---

pub fn sharpen(state: &mut CanvasState, layer_idx: usize, amount: f32, radius: f32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = sharpen_core(&flat, amount, radius, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn sharpen_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    amount: f32,
    radius: f32,
    original_flat: &RgbaImage,
) {
    let result = sharpen_core(original_flat, amount, radius, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn sharpen_core(
    flat: &RgbaImage,
    amount: f32,
    radius: f32,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    // Unsharp mask: result = original + amount * (original - blurred)
    let blurred = crate::ops::filters::parallel_gaussian_blur_pub(flat, radius);
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    let src_raw = flat.as_raw();
    let blur_raw = blurred.as_raw();
    let stride = w * 4;
    let mut dst_raw = vec![0u8; w * h * 4];
    let mask_raw = mask.map(|m| m.as_raw().as_slice());
    let mask_w = mask.map_or(0, |m| m.width() as usize);
    let mask_h = mask.map_or(0, |m| m.height() as usize);

    dst_raw
        .par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..w {
                let pi = x * 4;
                if let Some(mr) = mask_raw
                    && x < mask_w
                    && y < mask_h
                    && mr[y * mask_w + x] == 0
                {
                    let si = y * stride + pi;
                    row_out[pi..pi + 4].copy_from_slice(&src_raw[si..si + 4]);
                    continue;
                }
                let si = y * stride + pi;
                for c in 0..3 {
                    let s = src_raw[si + c] as f32;
                    let b = blur_raw[si + c] as f32;
                    let v = s + amount * (s - b);
                    row_out[pi + c] = v.round().clamp(0.0, 255.0) as u8;
                }
                row_out[pi + 3] = src_raw[si + 3];
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// --- Vignette ---

pub fn vignette(state: &mut CanvasState, layer_idx: usize, amount: f32, softness: f32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = vignette_core(&flat, amount, softness, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn vignette_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    amount: f32,
    softness: f32,
    original_flat: &RgbaImage,
) {
    let result = vignette_core(
        original_flat,
        amount,
        softness,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn vignette_core(
    flat: &RgbaImage,
    amount: f32,
    softness: f32,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    let w = flat.width() as f32;
    let h = flat.height() as f32;
    let cx = w / 2.0;
    let cy = h / 2.0;
    let max_dist = (cx * cx + cy * cy).sqrt();
    let soft = softness.max(0.01);

    apply_per_pixel(flat, mask, |x, y, r, g, b, a| {
        let dx = x as f32 - cx;
        let dy = y as f32 - cy;
        let dist = (dx * dx + dy * dy).sqrt() / max_dist;
        let vignette_factor = 1.0 - (amount * ((dist / soft).min(1.0)).powf(2.0));
        let vf = vignette_factor.clamp(0.0, 1.0);
        (r * vf, g * vf, b * vf, a)
    })
}

// --- Halftone ---

#[derive(Clone, Copy, PartialEq)]
pub enum HalftoneShape {
    Circle,
    Square,
    Diamond,
    Line,
}

pub fn halftone(
    state: &mut CanvasState,
    layer_idx: usize,
    dot_size: f32,
    angle_deg: f32,
    shape: HalftoneShape,
) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = halftone_core(
        &flat,
        dot_size,
        angle_deg,
        shape,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn halftone_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    dot_size: f32,
    angle_deg: f32,
    shape: HalftoneShape,
    original_flat: &RgbaImage,
) {
    let result = halftone_core(
        original_flat,
        dot_size,
        angle_deg,
        shape,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn halftone_core(
    flat: &RgbaImage,
    dot_size: f32,
    angle_deg: f32,
    shape: HalftoneShape,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    let ds = dot_size.max(2.0);
    let angle = angle_deg.to_radians();
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    apply_per_pixel(flat, mask, |x, y, r, g, b, a| {
        let lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;

        // Rotate coordinate space
        let fx = x as f32 * cos_a + y as f32 * sin_a;
        let fy = -(x as f32) * sin_a + y as f32 * cos_a;

        // Position within the halftone cell
        let cell_x = (fx / ds).fract().abs();
        let cell_y = (fy / ds).fract().abs();
        let cx = cell_x - 0.5;
        let cy = cell_y - 0.5;

        let threshold = match shape {
            HalftoneShape::Circle => (cx * cx + cy * cy).sqrt() * 2.0,
            HalftoneShape::Square => cx.abs().max(cy.abs()) * 2.0,
            HalftoneShape::Diamond => cx.abs() + cy.abs(),
            HalftoneShape::Line => cy.abs() * 2.0,
        };

        let val = if threshold < lum { 255.0 } else { 0.0 };
        (val, val, val, a)
    })
}

// ============================================================================
// RENDER EFFECTS
// ============================================================================

// --- Grid ---

#[derive(Clone, Copy, PartialEq)]
pub enum GridStyle {
    Lines,
    Checkerboard,
}

