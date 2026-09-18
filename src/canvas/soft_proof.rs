    ColorImage {
        size,
        source_size: egui::Vec2::new(img.width() as f32, img.height() as f32),
        pixels: color_pixels,
    }
}

// ============================================================================
// CMYK Soft Proof — display-only gamut-compressed preview
// ============================================================================
//
// Simulates how the image will look when printed in CMYK by applying:
//   1. RGB → naïve CMYK conversion
//   2. Gray Component Replacement (GCR) — shifts common CMY ink to K
//   3. Total ink limit (300% of 400% max) — desaturates over-inked pixels
//   4. sRGB gamut compression — vivid blues/greens are pulled inward
//   5. Paper white simulation — darkens highlights slightly (paper isn't 100% white)
//   6. CMYK → RGB back-conversion
//
// The result visibly desaturates out-of-gamut colours (vivid blues, greens,
// purples, neon tones) and slightly mutes highlights, matching what users
// would see in a professional CMYK proofing tool.

/// Apply CMYK soft proof to a single premultiplied Color32 pixel.
#[inline]
fn cmyk_soft_proof_pixel(c: Color32) -> Color32 {
    let a = c.a();
    if a == 0 {
        return c;
    }

    // Un-premultiply to get linear RGB 0..255
    let (r, g, b) = if a == 255 {
        (c.r() as f32, c.g() as f32, c.b() as f32)
    } else {
        let inv_a = 255.0 / a as f32;
        (
            (c.r() as f32 * inv_a).min(255.0),
            (c.g() as f32 * inv_a).min(255.0),
            (c.b() as f32 * inv_a).min(255.0),
        )
    };

    // Normalise to 0..1
    let rn = r / 255.0;
    let gn = g / 255.0;
    let bn = b / 255.0;

    // ---- Step 1: RGB → naïve CMYK ----
    let max_rgb = rn.max(gn).max(bn);
    if max_rgb <= 0.0 {
        // Pure black — unchanged
        return c;
    }
    let k_naive = 1.0 - max_rgb;
    let inv_k = 1.0 / max_rgb; // == 1/(1-k_naive)
    let c0 = (1.0 - rn - k_naive) * inv_k;
    let m0 = (1.0 - gn - k_naive) * inv_k;
    let y0 = (1.0 - bn - k_naive) * inv_k;

    // ---- Step 2: GCR (Gray Component Replacement) ----
    // Move a portion of the common CMY component into K.
    // GCR ratio of 0.5 is moderate (lighter than Photoshop's "Heavy" GCR).
    let gcr_ratio = 0.5_f32;
    let gray = c0.min(m0).min(y0);
    let k_add = gray * gcr_ratio;
    let mut cf = c0 - k_add;
    let mut mf = m0 - k_add;
    let mut yf = y0 - k_add;
    let mut kf = k_naive + k_add * (1.0 - k_naive); // scale k_add into K space

    // ---- Step 3: Total ink limit (300% of 400% max) ----
    let total_ink = cf + mf + yf + kf;
    let ink_limit = 3.0_f32;
    if total_ink > ink_limit {
        let scale = ink_limit / total_ink;
        cf *= scale;
        mf *= scale;
        yf *= scale;
        // K is preserved (it's cheaper ink), scale CMY only
        // But re-check: if still over, scale K too
        let total2 = cf + mf + yf + kf;
        if total2 > ink_limit {
            kf *= ink_limit / total2;
        }
    }

    // ---- Step 4: Gamut compression for vivid sRGB blues/greens ----
    // Real CMYK (SWOP/Fogra) can't reproduce very saturated blues or greens.
    // Apply subtle desaturation to high-saturation, low-K colours.
    let sat = 1.0 - cf.min(mf).min(yf) / (cf.max(mf).max(yf).max(0.001));
    let bright = 1.0 - kf;
    // Compress factor: stronger for vivid bright colours
    let compress = 1.0 - 0.12 * sat * bright;
    cf *= compress;
    mf *= compress;
    yf *= compress;

    // ---- Step 5: Paper white simulation ----
    // Real paper is ~92-96% reflective.  Nudge K up slightly for highlights.
    kf = kf + 0.03 * (1.0 - kf);

    // ---- Step 6: CMYK → RGB ----
    let ro = ((1.0 - cf) * (1.0 - kf) * 255.0).round().clamp(0.0, 255.0) as u8;
    let go = ((1.0 - mf) * (1.0 - kf) * 255.0).round().clamp(0.0, 255.0) as u8;
    let bo = ((1.0 - yf) * (1.0 - kf) * 255.0).round().clamp(0.0, 255.0) as u8;

    // Re-premultiply
    if a == 255 {
        Color32::from_rgba_premultiplied(ro, go, bo, 255)
    } else {
        let af = a as f32 / 255.0;
        Color32::from_rgba_premultiplied(
            (ro as f32 * af).round() as u8,
            (go as f32 * af).round() as u8,
            (bo as f32 * af).round() as u8,
            a,
        )
    }
}

/// Apply CMYK soft proof to a buffer of Color32 pixels (rayon-parallelised).
fn apply_cmyk_soft_proof(src: &[Color32]) -> Vec<Color32> {
    src.par_iter().map(|&c| cmyk_soft_proof_pixel(c)).collect()
}
