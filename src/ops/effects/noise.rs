pub fn add_noise(
    state: &mut CanvasState,
    layer_idx: usize,
    amount: f32,
    noise_type: NoiseType,
    monochrome: bool,
    seed: u32,
    scale: f32,
    octaves: u32,
) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = add_noise_core(
        &flat,
        amount,
        noise_type,
        monochrome,
        seed,
        scale,
        octaves,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn add_noise_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    amount: f32,
    noise_type: NoiseType,
    monochrome: bool,
    seed: u32,
    scale: f32,
    octaves: u32,
    original_flat: &RgbaImage,
) {
    let result = add_noise_core(
        original_flat,
        amount,
        noise_type,
        monochrome,
        seed,
        scale,
        octaves,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

/// Simple 2D Perlin-style value noise.
fn perlin_noise_2d(x: f32, y: f32, seed: u32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - xi as f32;
    let yf = y - yi as f32;

    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let u = fade(xf);
    let v = fade(yf);

    let n00 = hash_f32(xi as u32, yi as u32, seed);
    let n10 = hash_f32((xi + 1) as u32, yi as u32, seed);
    let n01 = hash_f32(xi as u32, (yi + 1) as u32, seed);
    let n11 = hash_f32((xi + 1) as u32, (yi + 1) as u32, seed);

    let nx0 = n00 + u * (n10 - n00);
    let nx1 = n01 + u * (n11 - n01);
    nx0 + v * (nx1 - nx0)
}

pub fn add_noise_core(
    flat: &RgbaImage,
    amount: f32,
    noise_type: NoiseType,
    monochrome: bool,
    seed: u32,
    scale: f32,
    octaves: u32,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    let inv_scale = 1.0 / scale.max(0.1);
    let oct = octaves.clamp(1, 8);

    apply_per_pixel(flat, mask, |x, y, r, g, b, a| {
        let sx = x as f32 * inv_scale;
        let sy = y as f32 * inv_scale;

        let noise_val = match noise_type {
            NoiseType::Uniform => {
                // For uniform, scale controls block size: quantize coordinates
                let qx = (x as f32 * inv_scale).floor() as u32;
                let qy = (y as f32 * inv_scale).floor() as u32;
                hash_f32(qx, qy, seed) * 2.0 - 1.0
            }
            NoiseType::Gaussian => {
                let qx = (x as f32 * inv_scale).floor() as u32;
                let qy = (y as f32 * inv_scale).floor() as u32;
                let u1 = hash_f32(qx, qy, seed).max(0.0001);
                let u2 = hash_f32(qx, qy, seed.wrapping_add(7));
                (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos() * 0.33
            }
            NoiseType::Perlin => turbulence_2d(sx, sy, seed, oct, 0.5) * 2.0 - 1.0,
        };

        let strength = amount * 255.0 / 100.0;
        if monochrome {
            let n = noise_val * strength;
            (r + n, g + n, b + n, a)
        } else {
            let nr = match noise_type {
                NoiseType::Perlin => (turbulence_2d(sx, sy, seed, oct, 0.5) * 2.0 - 1.0) * strength,
                _ => {
                    let qx = (x as f32 * inv_scale).floor() as u32;
                    let qy = (y as f32 * inv_scale).floor() as u32;
                    (hash_f32(qx, qy, seed) * 2.0 - 1.0) * strength
                }
            };
            let ng = match noise_type {
                NoiseType::Perlin => {
                    (turbulence_2d(sx, sy, seed.wrapping_add(1), oct, 0.5) * 2.0 - 1.0) * strength
                }
                _ => {
                    let qx = (x as f32 * inv_scale).floor() as u32;
                    let qy = (y as f32 * inv_scale).floor() as u32;
                    (hash_f32(qx, qy, seed.wrapping_add(1)) * 2.0 - 1.0) * strength
                }
            };
            let nb = match noise_type {
                NoiseType::Perlin => {
                    (turbulence_2d(sx, sy, seed.wrapping_add(2), oct, 0.5) * 2.0 - 1.0) * strength
                }
                _ => {
                    let qx = (x as f32 * inv_scale).floor() as u32;
                    let qy = (y as f32 * inv_scale).floor() as u32;
                    (hash_f32(qx, qy, seed.wrapping_add(2)) * 2.0 - 1.0) * strength
                }
            };
            (r + nr, g + ng, b + nb, a)
        }
    })
}

// --- Reduce Noise (bilateral-like filter) ---

pub fn reduce_noise(state: &mut CanvasState, layer_idx: usize, strength: f32, radius: u32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = reduce_noise_core(&flat, strength, radius, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn reduce_noise_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    strength: f32,
    radius: u32,
    original_flat: &RgbaImage,
) {
    let result = reduce_noise_core(
        original_flat,
        strength,
        radius,
        state.selection_mask.as_ref(),
    );
    commit_to_layer(state, layer_idx, &result);
}

pub fn reduce_noise_core(
    flat: &RgbaImage,
    strength: f32,
    radius: u32,
    mask: Option<&GrayImage>,
) -> RgbaImage {
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    if w == 0 || h == 0 {
        return flat.clone();
    }

    let r = radius.max(1) as i32;
    let sigma_s = r as f32;
    let sigma_r = strength * 2.55; // 0-100 â†’ 0-255

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

                let center_si = y * stride + pi;
                let cr = src_raw[center_si] as f32;
                let cg = src_raw[center_si + 1] as f32;
                let cb = src_raw[center_si + 2] as f32;

                let mut sum_r = 0.0f32;
                let mut sum_g = 0.0f32;
                let mut sum_b = 0.0f32;
                let mut sum_a = 0.0f32;
                let mut weight_sum = 0.0f32;

                for dy in -r..=r {
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    for dx in -r..=r {
                        let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                        let si = sy * stride + sx * 4;
                        let pr = src_raw[si] as f32;
                        let pg = src_raw[si + 1] as f32;
                        let pb = src_raw[si + 2] as f32;
                        let pa = src_raw[si + 3] as f32;

                        let spatial = (dx * dx + dy * dy) as f32 / (2.0 * sigma_s * sigma_s);
                        let diff_r = cr - pr;
                        let diff_g = cg - pg;
                        let diff_b = cb - pb;
                        let range = (diff_r * diff_r + diff_g * diff_g + diff_b * diff_b)
                            / (2.0 * sigma_r * sigma_r + 0.001);
                        let weight = (-spatial - range).exp();

                        sum_r += pr * weight;
                        sum_g += pg * weight;
                        sum_b += pb * weight;
                        sum_a += pa * weight;
                        weight_sum += weight;
                    }
                }

                if weight_sum > 0.0 {
                    let inv = 1.0 / weight_sum;
                    row_out[pi] = (sum_r * inv).round().clamp(0.0, 255.0) as u8;
                    row_out[pi + 1] = (sum_g * inv).round().clamp(0.0, 255.0) as u8;
                    row_out[pi + 2] = (sum_b * inv).round().clamp(0.0, 255.0) as u8;
                    row_out[pi + 3] = (sum_a * inv).round().clamp(0.0, 255.0) as u8;
                } else {
                    row_out[pi..pi + 4].copy_from_slice(&src_raw[center_si..center_si + 4]);
                }
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// --- Median filter ---

pub fn median_filter(state: &mut CanvasState, layer_idx: usize, radius: u32) {
    if layer_idx >= state.layers.len() {
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let result = median_core(&flat, radius, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

pub fn median_filter_from_flat(
    state: &mut CanvasState,
    layer_idx: usize,
    radius: u32,
    original_flat: &RgbaImage,
) {
    let result = median_core(original_flat, radius, state.selection_mask.as_ref());
    commit_to_layer(state, layer_idx, &result);
}

/// GPU-accelerated median filter. Falls back to CPU for radius > 7 or when a
/// selection mask is present (native), or always on web — the GPU route
/// reads back its result with a blocking `device.poll(Wait)`, which can't
/// complete synchronously in the browser (no real thread parking — the
/// map_async callback fires on a later microtask a blocking wait can never
/// reach) and hangs the tab. See `invert_colors_gpu` in ops/adjustments.rs
/// for the same pattern.
#[cfg(target_arch = "wasm32")]
pub fn median_filter_gpu(
    state: &mut CanvasState,
    layer_idx: usize,
    radius: u32,
    _gpu: &crate::gpu::GpuRenderer,
) {
    median_filter(state, layer_idx, radius);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn median_filter_gpu(
    state: &mut CanvasState,
    layer_idx: usize,
    radius: u32,
    gpu: &crate::gpu::GpuRenderer,
) {
    if layer_idx >= state.layers.len() {
        return;
    }
    if state.selection_mask.is_some() || radius > 7 {
        median_filter(state, layer_idx, radius);
        return;
    }
    let flat = state.layers[layer_idx].pixels.to_rgba_image();
    let (w, h) = (flat.width(), flat.height());
    if let Some(result_data) = gpu.median_rgba(flat.as_raw(), w, h, radius) {
        let result = image::RgbaImage::from_raw(w, h, result_data).unwrap();
        commit_to_layer(state, layer_idx, &result);
    } else {
        median_filter(state, layer_idx, radius);
    }
}

#[cfg(target_arch = "wasm32")]
pub fn median_filter_from_flat_gpu(
    state: &mut CanvasState,
    layer_idx: usize,
    radius: u32,
    original_flat: &RgbaImage,
    _gpu: &crate::gpu::GpuRenderer,
) {
    median_filter_from_flat(state, layer_idx, radius, original_flat);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn median_filter_from_flat_gpu(
    state: &mut CanvasState,
    layer_idx: usize,
    radius: u32,
    original_flat: &RgbaImage,
    gpu: &crate::gpu::GpuRenderer,
) {
    if state.selection_mask.is_some() || radius > 7 {
        median_filter_from_flat(state, layer_idx, radius, original_flat);
        return;
    }
    let (w, h) = (original_flat.width(), original_flat.height());
    if let Some(result_data) = gpu.median_rgba(original_flat.as_raw(), w, h, radius) {
        let result = image::RgbaImage::from_raw(w, h, result_data).unwrap();
        commit_to_layer(state, layer_idx, &result);
    } else {
        median_filter_from_flat(state, layer_idx, radius, original_flat);
    }
}

pub fn median_core(flat: &RgbaImage, radius: u32, mask: Option<&GrayImage>) -> RgbaImage {
    let w = flat.width() as usize;
    let h = flat.height() as usize;
    if w == 0 || h == 0 {
        return flat.clone();
    }

    let r = radius.max(1) as i32;
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
            let mut channels: [Vec<u8>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
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

                for c in &mut channels {
                    c.clear();
                }
                for dy in -r..=r {
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    for dx in -r..=r {
                        let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                        let si = sy * stride + sx * 4;
                        for c in 0..4 {
                            channels[c].push(src_raw[si + c]);
                        }
                    }
                }
                for c in 0..4 {
                    channels[c].sort_unstable();
                    row_out[pi + c] = channels[c][channels[c].len() / 2];
                }
            }
        });

    RgbaImage::from_raw(w as u32, h as u32, dst_raw).unwrap()
}

// ============================================================================
// STYLIZE EFFECTS
// ============================================================================

// --- Glow ---

