pub fn pixel_drag(
    state: &mut CanvasState,
    layer_idx: usize,
    seed: u32,
    amount: f32,
    distance: u32,
    direction: f32,
) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = pixel_drag_core(
        &flat,
        seed,
        amount,
        distance,
        direction,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn pixel_drag_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    seed: u32,
    amount: f32,
    distance: u32,
    direction: f32,
    original_flat: &RgbaImage,
) {
    let result = pixel_drag_core(
        original_flat,
        seed,
        amount,
        distance,
        direction,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn pixel_drag_core(
    flat: &RgbaImage,
    seed: u32,
    amount: f32,
    distance: u32,
    direction: f32,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    if w == 0 || h == 0 {
        return flat.clone();
    }

    let src_raw = flat.as_raw();
    let stride = w * 4;
    let mut dst_raw = src_raw.to_vec();
    let mask_raw = mask.map(|m| m.as_raw().as_slice());
    let mask_w = mask.map_or(0, |m| m.width() as usize);
    let mask_h = mask.map_or(0, |m| m.height() as usize);
    let dir_rad = direction.to_radians();
    let dx_dir = dir_rad.cos();
    let dy_dir = dir_rad.sin();
    let dist = distance.max(1) as f32;

    // Generate drag bands per row
    dst_raw
        .par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            let row_hash = hash_f32(y as u32, 0, seed);
            if row_hash > amount / 100.0 {
                return; // This row is not affected.
            }
            let drag_dist = (hash_f32(y as u32, 1, seed) * dist) as i32;

            for x in 0..w {
                let pi = x * 4;
                if let Some(mr) = mask_raw
                    && x < mask_w
                    && y < mask_h
                    && mr[y * mask_w + x] == 0
                {
                    continue;
                }
                let sx = (x as f32 - drag_dist as f32 * dx_dir).round() as i32;
                let sy = (y as f32 - drag_dist as f32 * dy_dir).round() as i32;
                let sx = sx.clamp(0, w as i32 - 1) as usize;
                let sy = sy.clamp(0, h as i32 - 1) as usize;
                let si = sy * stride + sx * 4;
                row_out[pi..pi + 4].copy_from_slice(&src_raw[si..si + 4]);
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// --- RGB Displace ---

pub fn rgb_displace(
    state: &mut CanvasState,
    layer_idx: usize,
    r_offset: (i32, i32),
    g_offset: (i32, i32),
    b_offset: (i32, i32),
) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = rgb_displace_core(
        &flat,
        r_offset,
        g_offset,
        b_offset,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn rgb_displace_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    r_offset: (i32, i32),
    g_offset: (i32, i32),
    b_offset: (i32, i32),
    original_flat: &RgbaImage,
) {
    let result = rgb_displace_core(
        original_flat,
        r_offset,
        g_offset,
        b_offset,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn rgb_displace_core(
    flat: &RgbaImage,
    r_off: (i32, i32),
    g_off: (i32, i32),
    b_off: (i32, i32),
    mask: Option<&GrayImage>,
) -> RgbaImage {
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

                // Sample each channel from offset position.
                let rx = (x as i32 + r_off.0).clamp(0, w as i32 - 1) as usize;
                let ry = (y as i32 + r_off.1).clamp(0, h as i32 - 1) as usize;
                let gx = (x as i32 + g_off.0).clamp(0, w as i32 - 1) as usize;
                let gy = (y as i32 + g_off.1).clamp(0, h as i32 - 1) as usize;
                let bx = (x as i32 + b_off.0).clamp(0, w as i32 - 1) as usize;
                let by = (y as i32 + b_off.1).clamp(0, h as i32 - 1) as usize;

                row_out[pi] = src_raw[ry * stride + rx * 4];
                row_out[pi + 1] = src_raw[gy * stride + gx * 4 + 1];
                row_out[pi + 2] = src_raw[by * stride + bx * 4 + 2];
                // Alpha from center pixel.
                let si = y * stride + pi;
                row_out[pi + 3] = src_raw[si + 3];
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// ============================================================================
// ARTISTIC EFFECTS
// ============================================================================

// --- Ink ---

