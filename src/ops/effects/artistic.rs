pub fn ink(state: &mut CanvasState, layer_idx: usize, edge_strength: f32, threshold: f32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = ink_core(
        &flat,
        edge_strength,
        threshold,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn ink_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    edge_strength: f32,
    threshold: f32,
    original_flat: &RgbaImage,
) {
    let result = ink_core(
        original_flat,
        edge_strength,
        threshold,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn ink_core(
    flat: &RgbaImage,
    edge_strength: f32,
    threshold: f32,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    // Sobel edge detection â†’ thresholded black/white ink effect.
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    if w == 0 || h == 0 {
        return flat.clone();
    }

    let src_raw = flat.as_raw();
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

                // Compute luminance of surrounding pixels for Sobel.
                let lum = |px: i32, py: i32| -> f32 {
                    let cx = px.clamp(0, w as i32 - 1) as usize;
                    let cy = py.clamp(0, h as i32 - 1) as usize;
                    let si = cy * stride + cx * 4;
                    0.2126 * src_raw[si] as f32
                        + 0.7152 * src_raw[si + 1] as f32
                        + 0.0722 * src_raw[si + 2] as f32
                };

                let ix = x as i32;
                let iy = y as i32;
                let gx = -lum(ix - 1, iy - 1) - 2.0 * lum(ix - 1, iy) - lum(ix - 1, iy + 1)
                    + lum(ix + 1, iy - 1)
                    + 2.0 * lum(ix + 1, iy)
                    + lum(ix + 1, iy + 1);
                let gy = -lum(ix - 1, iy - 1) - 2.0 * lum(ix, iy - 1) - lum(ix + 1, iy - 1)
                    + lum(ix - 1, iy + 1)
                    + 2.0 * lum(ix, iy + 1)
                    + lum(ix + 1, iy + 1);
                let edge = (gx * gx + gy * gy).sqrt() * edge_strength / 100.0;
                let val = if edge > threshold { 0u8 } else { 255u8 };

                let si = y * stride + pi;
                row_out[pi] = val;
                row_out[pi + 1] = val;
                row_out[pi + 2] = val;
                row_out[pi + 3] = src_raw[si + 3];
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// --- Oil Painting ---

pub fn oil_painting(state: &mut CanvasState, layer_idx: usize, radius: u32, levels: u32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = oil_painting_core(&flat, radius, levels, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn oil_painting_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    radius: u32,
    levels: u32,
    original_flat: &RgbaImage,
) {
    let result = oil_painting_core(original_flat, radius, levels, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn oil_painting_core(
    flat: &RgbaImage,
    radius: u32,
    levels: u32,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    if w == 0 || h == 0 {
        return flat.clone();
    }

    let r = radius.clamp(1, 10) as i32;
    let num_levels = levels.clamp(2, 64) as usize;
    let src_raw = flat.as_raw();
    let stride = w * 4;
    let mut dst_raw = vec![0u8; w * h * 4];
    let mask_raw = mask.map(|m| m.as_raw().as_slice());
    let mask_w = mask.map_or(0, |m| m.width() as usize);
    let mask_h = mask.map_or(0, |m| m.height() as usize);

    dst_raw
        .par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            let mut intensity_count = vec![0u32; num_levels];
            let mut sum_r = vec![0u32; num_levels];
            let mut sum_g = vec![0u32; num_levels];
            let mut sum_b = vec![0u32; num_levels];

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

                // Reset bins.
                for i in 0..num_levels {
                    intensity_count[i] = 0;
                    sum_r[i] = 0;
                    sum_g[i] = 0;
                    sum_b[i] = 0;
                }

                for dy in -r..=r {
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    for dx in -r..=r {
                        let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                        let si = sy * stride + sx * 4;
                        let pr = src_raw[si] as u32;
                        let pg = src_raw[si + 1] as u32;
                        let pb = src_raw[si + 2] as u32;
                        let intensity = ((pr + pg + pb) / 3 * num_levels as u32 / 256) as usize;
                        let intensity = intensity.min(num_levels - 1);
                        intensity_count[intensity] += 1;
                        sum_r[intensity] += pr;
                        sum_g[intensity] += pg;
                        sum_b[intensity] += pb;
                    }
                }

                // Find the most common intensity bin.
                let mut max_count = 0u32;
                let mut max_idx = 0usize;
                for (i, &count) in intensity_count.iter().enumerate() {
                    if count > max_count {
                        max_count = count;
                        max_idx = i;
                    }
                }

                if let (Some(avg_r), Some(avg_g), Some(avg_b)) = (
                    sum_r[max_idx].checked_div(max_count),
                    sum_g[max_idx].checked_div(max_count),
                    sum_b[max_idx].checked_div(max_count),
                ) {
                    row_out[pi] = avg_r as u8;
                    row_out[pi + 1] = avg_g as u8;
                    row_out[pi + 2] = avg_b as u8;
                }
                let si = y * stride + pi;
                row_out[pi + 3] = src_raw[si + 3];
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// --- Color Filter ---

#[derive(Clone, Copy, PartialEq)]
pub enum ColorFilterMode {
    Multiply,
    Screen,
    Overlay,
    SoftLight,
}

pub fn color_filter(
    state: &mut CanvasState,
    layer_idx: usize,
    filter_color: [u8; 4],
    intensity: f32,
    mode: ColorFilterMode,
) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = color_filter_core(
        &flat,
        filter_color,
        intensity,
        mode,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn color_filter_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    filter_color: [u8; 4],
    intensity: f32,
    mode: ColorFilterMode,
    original_flat: &RgbaImage,
) {
    let result = color_filter_core(
        original_flat,
        filter_color,
        intensity,
        mode,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn color_filter_core(
    flat: &RgbaImage,
    filter_color: [u8; 4],
    intensity: f32,
    mode: ColorFilterMode,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    let fc = [
        filter_color[0] as f32 / 255.0,
        filter_color[1] as f32 / 255.0,
        filter_color[2] as f32 / 255.0,
    ];

    apply_per_pixel(flat, mask, |_x, _y, r, g, b, a| {
        let rs = r / 255.0;
        let gs = g / 255.0;
        let bs = b / 255.0;
        let blend_fn = |s: f32, f: f32| -> f32 {
            match mode {
                ColorFilterMode::Multiply => s * f,
                ColorFilterMode::Screen => 1.0 - (1.0 - s) * (1.0 - f),
                ColorFilterMode::Overlay => {
                    if s < 0.5 {
                        2.0 * s * f
                    } else {
                        1.0 - 2.0 * (1.0 - s) * (1.0 - f)
                    }
                }
                ColorFilterMode::SoftLight => {
                    if f < 0.5 {
                        s - (1.0 - 2.0 * f) * s * (1.0 - s)
                    } else {
                        s + (2.0 * f - 1.0) * (s.sqrt() - s)
                    }
                }
            }
        };

        let nr = (rs * (1.0 - intensity) + blend_fn(rs, fc[0]) * intensity) * 255.0;
        let ng = (gs * (1.0 - intensity) + blend_fn(gs, fc[1]) * intensity) * 255.0;
        let nb = (bs * (1.0 - intensity) + blend_fn(bs, fc[2]) * intensity) * 255.0;
        (nr, ng, nb, a)
    })
}

// ============================================================================
// RENDER â€” Contours (topographic map lines)
// ============================================================================

