fn load_font_for_style(style: &TextStyle) -> Option<FontArc> {
    text::load_system_font(&style.font_family, style.font_weight, style.italic)
}

/// Trim an RGBA buffer to the smallest rectangle containing non-transparent pixels.
/// Returns `(x, y, w, h, trimmed_data)` or `None` if fully transparent.
fn trim_to_content(data: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    let (w_us, h_us) = (w as usize, h as usize);
    let mut min_x = w_us;
    let mut min_y = h_us;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    for y in 0..h_us {
        for x in 0..w_us {
            let a = data[(y * w_us + x) * 4 + 3];
            if a > 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if max_x < min_x || max_y < min_y {
        return None;
    }
    let tw = max_x - min_x + 1;
    let th = max_y - min_y + 1;
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th {
        let src_off = ((min_y + y) * w_us + min_x) * 4;
        let dst_off = y * tw * 4;
        out[dst_off..dst_off + tw * 4].copy_from_slice(&data[src_off..src_off + tw * 4]);
    }
    Some((min_x as u32, min_y as u32, tw as u32, th as u32, out))
}

/// Blit an RGBA buffer onto a TiledImage using alpha compositing.
fn blit_rgba_buffer(
    target: &mut TiledImage,
    buf: &[u8],
    buf_w: u32,
    buf_h: u32,
    off_x: i32,
    off_y: i32,
    canvas_w: u32,
    canvas_h: u32,
) {
    for py in 0..buf_h {
        let cy = off_y + py as i32;
        if cy < 0 || cy >= canvas_h as i32 {
            continue;
        }
        for px in 0..buf_w {
            let cx = off_x + px as i32;
            if cx < 0 || cx >= canvas_w as i32 {
                continue;
            }
            let src_idx = (py as usize * buf_w as usize + px as usize) * 4;
            let sa = buf[src_idx + 3];
            if sa == 0 {
                continue;
            }
            let sr = buf[src_idx];
            let sg = buf[src_idx + 1];
            let sb = buf[src_idx + 2];
            target.put_pixel(cx as u32, cy as u32, Rgba([sr, sg, sb, sa]));
        }
    }
}

// ===========================================================================
// Text Warp Application (Phase 4 — Batches 7+8)
// ===========================================================================

/// Apply a geometric warp to a rasterized text block.
/// Takes the flat-rasterized block RGBA and returns a warped version.
/// The warp is applied as a pixel-level displacement: for each output pixel,
/// compute where it came from in the source, and bilinear-sample.
fn apply_block_warp(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    warp: &TextWarp,
) -> Option<(Vec<u8>, u32, u32, i32, i32)> {
    match warp {
        TextWarp::None => None,
        TextWarp::Arc(arc) => Some(apply_arc_warp(src, src_w, src_h, arc)),
        TextWarp::Circular(circ) => Some(apply_circular_warp(src, src_w, src_h, circ)),
        TextWarp::PathFollow(pf) => Some(apply_path_follow_warp(src, src_w, src_h, pf)),
        TextWarp::Envelope(env) => Some(apply_envelope_warp(src, src_w, src_h, env)),
    }
}

/// Arc warp: bend text along an arc.
/// bend > 0 curves upward (convex), bend < 0 curves downward (concave).
fn apply_arc_warp(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    arc: &ArcWarp,
) -> (Vec<u8>, u32, u32, i32, i32) {
    if arc.bend.abs() < 0.001 {
        return (src.to_vec(), src_w, src_h, 0, 0);
    }

    let w = src_w as f32;
    let h = src_h as f32;

    // The arc angle spans from bend * PI (at full bend = semicircle)
    let angle = arc.bend * std::f32::consts::PI;
    let radius = if angle.abs() > 0.01 {
        w / (2.0 * (angle / 2.0).sin())
    } else {
        w * 100.0 // effectively flat
    };

    // Compute output bounds by sampling corners and midpoints
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    let samples = 32;
    for i in 0..=samples {
        let t = i as f32 / samples as f32;
        let sx = t * w;
        for sy in [0.0, h] {
            let (dx, dy) = arc_map_point(sx, sy, w, h, radius, angle, arc);
            min_x = min_x.min(dx);
            max_x = max_x.max(dx);
            min_y = min_y.min(dy);
            max_y = max_y.max(dy);
        }
    }

    let margin = 2.0;
    min_x -= margin;
    min_y -= margin;
    max_x += margin;
    max_y += margin;

    let out_w = (max_x - min_x).ceil() as u32;
    let out_h = (max_y - min_y).ceil() as u32;
    if out_w == 0 || out_h == 0 || out_w > 8192 || out_h > 8192 {
        return (src.to_vec(), src_w, src_h, 0, 0);
    }

    let off_x = min_x.floor() as i32;
    let off_y = min_y.floor() as i32;

    let mut out = vec![0u8; (out_w * out_h * 4) as usize];
    let sw = src_w as usize;

    out.par_chunks_mut((out_w * 4) as usize)
        .enumerate()
        .for_each(|(oy, row)| {
            for ox in 0..out_w as usize {
                let dx = ox as f32 + min_x;
                let dy = oy as f32 + min_y;
                // Inverse map: find source pixel for this output pixel
                if let Some((sx, sy)) = arc_inverse_map(dx, dy, w, h, radius, angle, arc)
                    && sx >= 0.0
                    && sx < w
                    && sy >= 0.0
                    && sy < h
                {
                    let pixel = bilinear_sample(src, sw, src_h as usize, sx, sy);
                    let pi = ox * 4;
                    row[pi] = pixel[0];
                    row[pi + 1] = pixel[1];
                    row[pi + 2] = pixel[2];
                    row[pi + 3] = pixel[3];
                }
            }
        });

    (out, out_w, out_h, off_x, off_y)
}

/// Forward map: source(sx,sy) -> destination(dx,dy) for arc warp.
fn arc_map_point(
    sx: f32,
    sy: f32,
    w: f32,
    h: f32,
    radius: f32,
    angle: f32,
    arc: &ArcWarp,
) -> (f32, f32) {
    let cx = w / 2.0;

    // Normalized position along text width [-1, 1]
    let t = (sx - cx) / (w / 2.0);

    // Angle for this position
    let theta = t * angle / 2.0;

    // Center of curvature is below (positive bend) or above (negative bend)
    let r_sign = if angle > 0.0 { 1.0 } else { -1.0 };
    let r_abs = radius.abs();

    // Distance from baseline (sy=0 at top, h at bottom)
    let sy_norm = sy / h; // 0..1

    // Radial position: baseline at radius, top of text further from center
    let r = r_abs - (1.0 - sy_norm) * h * r_sign;

    let dx = cx + r * theta.sin();
    let dy = r_abs - r * theta.cos() * r_sign;

    // Apply distortion
    let hdist = arc.horizontal_distortion;
    let vdist = arc.vertical_distortion;
    let dx = dx + (dx - cx) * hdist;
    let dy = dy + (dy - h / 2.0) * vdist;

    (dx, dy)
}

/// Inverse map: destination(dx,dy) -> source(sx,sy) for arc warp.
fn arc_inverse_map(
    dx: f32,
    dy: f32,
    w: f32,
    h: f32,
    radius: f32,
    angle: f32,
    arc: &ArcWarp,
) -> Option<(f32, f32)> {
    let cx = w / 2.0;
    let r_sign = if angle > 0.0 { 1.0 } else { -1.0 };
    let r_abs = radius.abs();

    // Undo distortion
    let hdist = arc.horizontal_distortion;
    let vdist = arc.vertical_distortion;
    let dx = if hdist.abs() > 0.001 {
        cx + (dx - cx) / (1.0 + hdist)
    } else {
        dx
    };
    let dy = if vdist.abs() > 0.001 {
        h / 2.0 + (dy - h / 2.0) / (1.0 + vdist)
    } else {
        dy
    };

    // Compute polar coordinates relative to center of curvature
    let rel_x = dx - cx;
    let rel_y = r_abs - dy * r_sign;

    let r = (rel_x * rel_x + (rel_y * r_sign) * (rel_y * r_sign)).sqrt();
    let theta = rel_x.atan2(rel_y * r_sign);

    // Check if the angle is within the arc range
    if angle.abs() > 0.01 && theta.abs() > (angle / 2.0).abs() + 0.1 {
        return None;
    }

    // Compute source x from angle
    let t = if angle.abs() > 0.01 {
        theta / (angle / 2.0)
    } else {
        (dx - cx) / (w / 2.0)
    };
    let sx = cx + t * w / 2.0;

    // Compute source y from radial distance
    let sy_norm = 1.0 - (r_abs - r) / (h * r_sign);
    let sy = sy_norm * h;

    Some((sx, sy))
}

/// Circular warp: arrange text around a circle.
fn apply_circular_warp(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    circ: &CircularWarp,
) -> (Vec<u8>, u32, u32, i32, i32) {
    let w = src_w as f32;
    let h = src_h as f32;
    let r = circ.radius.max(10.0);

    // Total angle subtended by the text
    let _total_angle = w / r;
    let dir = if circ.clockwise { 1.0 } else { -1.0 };

    // Output bounds: circle bounding box with margin
    let r_outer = r + h;
    let out_size = (r_outer * 2.0 + 4.0).ceil() as u32;
    let out_cx = out_size as f32 / 2.0;
    let out_cy = out_size as f32 / 2.0;

    // Offset relative to source buffer: center circle output on source center
    // (same pattern as arc warp: src_center - out_center)
    let off_x = (w / 2.0 - out_cx).round() as i32;
    let off_y = (h / 2.0 - out_cy).round() as i32;

    let mut out = vec![0u8; (out_size * out_size * 4) as usize];
    let sw = src_w as usize;

    out.par_chunks_mut((out_size * 4) as usize)
        .enumerate()
        .for_each(|(oy, row)| {
            for ox in 0..out_size as usize {
                let px = ox as f32 - out_cx;
                let py = oy as f32 - out_cy;
                let dist = (px * px + py * py).sqrt();

                // Check if within the annular ring
                if dist < r || dist > r_outer {
                    continue;
                }

                // Angle of this pixel
                let pixel_angle = py.atan2(px);
                // Relative angle from start angle
                let mut rel_angle = (pixel_angle - circ.start_angle) * dir;
                // Normalize to [0, 2*PI)
                while rel_angle < 0.0 {
                    rel_angle += std::f32::consts::TAU;
                }
                while rel_angle >= std::f32::consts::TAU {
                    rel_angle -= std::f32::consts::TAU;
                }

                // Map angle to source x
                let sx = rel_angle * r;
                if sx < 0.0 || sx >= w {
                    continue;
                }

                // Map radial distance to source y (outer = top of text)
                let sy = r_outer - dist;
                if sy < 0.0 || sy >= h {
                    continue;
                }

                let pixel = bilinear_sample(src, sw, src_h as usize, sx, sy);
                let pi = ox * 4;
                row[pi] = pixel[0];
                row[pi + 1] = pixel[1];
                row[pi + 2] = pixel[2];
                row[pi + 3] = pixel[3];
            }
        });

    (out, out_size, out_size, off_x, off_y)
}

/// Path follow warp: place text along a Bézier curve.
fn apply_path_follow_warp(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    pf: &PathFollowWarp,
) -> (Vec<u8>, u32, u32, i32, i32) {
    if pf.control_points.len() < 4 {
        return (src.to_vec(), src_w, src_h, 0, 0);
    }

    let w = src_w as f32;
    let h = src_h as f32;

    // Build arc-length table for the cubic Bézier
    let lut_size = 256;
    let (arc_lengths, total_arc_len) = build_arc_length_table(&pf.control_points[0..4], lut_size);

    // Compute output bounds by sampling along the path
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    let samples = 64;
    for i in 0..=samples {
        let s = (i as f32 / samples as f32) * total_arc_len;
        let t = arc_length_to_t(s, &arc_lengths, total_arc_len);
        let (px, py) = eval_cubic_bezier(&pf.control_points[0..4], t);
        let (_, ny) = eval_cubic_bezier_tangent(&pf.control_points[0..4], t);
        // Extend bounds by text height in the normal direction
        for offset in [-h, 0.0, h] {
            min_x = min_x.min(px - offset.abs());
            max_x = max_x.max(px + offset.abs());
            min_y = min_y.min(py + offset + ny.abs() * h);
            max_y = max_y.max(py - offset - ny.abs() * h);
        }
    }

    let margin = h + 10.0;
    min_x -= margin;
    min_y -= margin;
    max_x += margin;
    max_y += margin;

    let out_w = ((max_x - min_x).ceil() as u32).min(4096);
    let out_h = ((max_y - min_y).ceil() as u32).min(4096);
    if out_w == 0 || out_h == 0 {
        return (src.to_vec(), src_w, src_h, 0, 0);
    }

    let off_x = min_x.floor() as i32;
    let off_y = min_y.floor() as i32;

    let sw = src_w as usize;
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];

    // For each output pixel, find the closest point on the path,
    // compute distance along path (= source x) and perpendicular distance (= source y)
    out.par_chunks_mut((out_w * 4) as usize)
        .enumerate()
        .for_each(|(oy, row)| {
            for ox in 0..out_w as usize {
                let px = ox as f32 + min_x;
                let py = oy as f32 + min_y;

                // Find closest t on the curve using iterative refinement
                if let Some((sx, sy)) = inverse_path_follow(
                    px,
                    py,
                    &pf.control_points[0..4],
                    &arc_lengths,
                    total_arc_len,
                    h,
                ) && sx >= 0.0
                    && sx < w
                    && sy >= 0.0
                    && sy < h
                {
                    let pixel = bilinear_sample(src, sw, src_h as usize, sx, sy);
                    let pi = ox * 4;
                    row[pi] = pixel[0];
                    row[pi + 1] = pixel[1];
                    row[pi + 2] = pixel[2];
                    row[pi + 3] = pixel[3];
                }
            }
        });

    (out, out_w, out_h, off_x, off_y)
}

/// Envelope warp: deform text between two Bézier curves.
fn apply_envelope_warp(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    env: &EnvelopeWarp,
) -> (Vec<u8>, u32, u32, i32, i32) {
    if env.top_curve.len() < 4 || env.bottom_curve.len() < 4 {
        return (src.to_vec(), src_w, src_h, 0, 0);
    }

    let w = src_w as f32;
    let h = src_h as f32;

    // Compute output bounds from both curves
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    let samples = 64;
    for i in 0..=samples {
        let t = i as f32 / samples as f32;
        let (tx, ty) = eval_cubic_bezier(&env.top_curve[0..4], t);
        let (bx, by) = eval_cubic_bezier(&env.bottom_curve[0..4], t);
        min_x = min_x.min(tx).min(bx);
        max_x = max_x.max(tx).max(bx);
        min_y = min_y.min(ty).min(by);
        max_y = max_y.max(ty).max(by);
    }

    let margin = 4.0;
    min_x -= margin;
    min_y -= margin;
    max_x += margin;
    max_y += margin;

    let out_w = ((max_x - min_x).ceil() as u32).min(4096);
    let out_h = ((max_y - min_y).ceil() as u32).min(4096);
    if out_w == 0 || out_h == 0 {
        return (src.to_vec(), src_w, src_h, 0, 0);
    }

    let off_x = min_x.floor() as i32;
    let off_y = min_y.floor() as i32;

    let sw = src_w as usize;
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];

    out.par_chunks_mut((out_w * 4) as usize)
        .enumerate()
        .for_each(|(oy, row)| {
            for ox in 0..out_w as usize {
                let px = ox as f32 + min_x;
                let py = oy as f32 + min_y;

                // Find t such that px is between top_curve(t).x and bottom_curve(t).x
                // Simple approach: use normalized x position as t
                let t = (px - min_x) / (max_x - min_x - 2.0 * margin).max(1.0);
                if !(0.0..=1.0).contains(&t) {
                    continue;
                }

                let (_, top_y) = eval_cubic_bezier(&env.top_curve[0..4], t);
                let (_, bot_y) = eval_cubic_bezier(&env.bottom_curve[0..4], t);

                let span = bot_y - top_y;
                if span.abs() < 0.001 {
                    continue;
                }

                // Vertical interpolation: where does py fall between top and bottom?
                let v = (py - top_y) / span;
                if !(0.0..=1.0).contains(&v) {
                    continue;
                }

                // Map to source coordinates
                let sx = t * w;
                let sy = v * h;

                if sx >= 0.0 && sx < w && sy >= 0.0 && sy < h {
                    let pixel = bilinear_sample(src, sw, src_h as usize, sx, sy);
                    let pi = ox * 4;
                    row[pi] = pixel[0];
                    row[pi + 1] = pixel[1];
                    row[pi + 2] = pixel[2];
                    row[pi + 3] = pixel[3];
                }
            }
        });

    (out, out_w, out_h, off_x, off_y)
}

// ---------------------------------------------------------------------------
// Bézier curve helpers
// ---------------------------------------------------------------------------

/// Evaluate a cubic Bézier at parameter t.
fn eval_cubic_bezier(pts: &[[f32; 2]], t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let u2 = u * u;
    let t2 = t * t;
    let x = u2 * u * pts[0][0]
        + 3.0 * u2 * t * pts[1][0]
        + 3.0 * u * t2 * pts[2][0]
        + t2 * t * pts[3][0];
    let y = u2 * u * pts[0][1]
        + 3.0 * u2 * t * pts[1][1]
        + 3.0 * u * t2 * pts[2][1]
        + t2 * t * pts[3][1];
    (x, y)
}

/// Evaluate the tangent (first derivative) of a cubic Bézier at parameter t.
fn eval_cubic_bezier_tangent(pts: &[[f32; 2]], t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let dx = 3.0 * u * u * (pts[1][0] - pts[0][0])
        + 6.0 * u * t * (pts[2][0] - pts[1][0])
        + 3.0 * t * t * (pts[3][0] - pts[2][0]);
    let dy = 3.0 * u * u * (pts[1][1] - pts[0][1])
        + 6.0 * u * t * (pts[2][1] - pts[1][1])
        + 3.0 * t * t * (pts[3][1] - pts[2][1]);
    (dx, dy)
}

/// Build an arc-length lookup table for a cubic Bézier curve.
/// Returns (cumulative_lengths, total_length).
fn build_arc_length_table(pts: &[[f32; 2]], steps: usize) -> (Vec<f32>, f32) {
    let mut lengths = Vec::with_capacity(steps + 1);
    lengths.push(0.0);
    let mut total = 0.0;
    let (mut prev_x, mut prev_y) = eval_cubic_bezier(pts, 0.0);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let (x, y) = eval_cubic_bezier(pts, t);
        let dx = x - prev_x;
        let dy = y - prev_y;
        total += (dx * dx + dy * dy).sqrt();
        lengths.push(total);
        prev_x = x;
        prev_y = y;
    }
    (lengths, total)
}

/// Convert an arc-length distance to a Bézier parameter t using the LUT.
fn arc_length_to_t(s: f32, lengths: &[f32], total: f32) -> f32 {
    if s <= 0.0 {
        return 0.0;
    }
    if s >= total {
        return 1.0;
    }
    // Binary search
    let n = lengths.len() - 1;
    let mut lo = 0usize;
    let mut hi = n;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if lengths[mid] < s {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 {
        return 0.0;
    }
    let seg_len = lengths[lo] - lengths[lo - 1];
    let frac = if seg_len > 0.0 {
        (s - lengths[lo - 1]) / seg_len
    } else {
        0.0
    };
    ((lo - 1) as f32 + frac) / n as f32
}

/// Inverse path-follow mapping: given a point (px, py), find the corresponding
/// source coordinates (sx, sy) where sx = arc-length along curve, sy = perpendicular distance.
fn inverse_path_follow(
    px: f32,
    py: f32,
    pts: &[[f32; 2]],
    arc_lengths: &[f32],
    total_arc_len: f32,
    text_height: f32,
) -> Option<(f32, f32)> {
    // Coarse search: find closest t
    let coarse_steps = 64;
    let mut best_t = 0.0f32;
    let mut best_dist_sq = f32::MAX;

    for i in 0..=coarse_steps {
        let t = i as f32 / coarse_steps as f32;
        let (cx, cy) = eval_cubic_bezier(pts, t);
        let dist_sq = (px - cx) * (px - cx) + (py - cy) * (py - cy);
        if dist_sq < best_dist_sq {
            best_dist_sq = dist_sq;
            best_t = t;
        }
    }

    // Fine refinement around the best coarse t
    let step = 1.0 / coarse_steps as f32;
    let mut t_lo = (best_t - step).max(0.0);
    let mut t_hi = (best_t + step).min(1.0);
    for _ in 0..8 {
        let t_mid = (t_lo + t_hi) / 2.0;
        let t_a = (t_lo + t_mid) / 2.0;
        let t_b = (t_mid + t_hi) / 2.0;
        let (ax, ay) = eval_cubic_bezier(pts, t_a);
        let (bx, by) = eval_cubic_bezier(pts, t_b);
        let da = (px - ax) * (px - ax) + (py - ay) * (py - ay);
        let db = (px - bx) * (px - bx) + (py - by) * (py - by);
        if da < db {
            t_hi = t_mid;
        } else {
            t_lo = t_mid;
        }
    }
    let t = (t_lo + t_hi) / 2.0;

    // Get curve point and tangent
    let (cx, cy) = eval_cubic_bezier(pts, t);
    let (tx, ty) = eval_cubic_bezier_tangent(pts, t);
    let tangent_len = (tx * tx + ty * ty).sqrt();
    if tangent_len < 0.0001 {
        return None;
    }

    // Normal vector (perpendicular to tangent, pointing "up" from curve)
    let nx = -ty / tangent_len;
    let ny = tx / tangent_len;

    // Signed perpendicular distance from curve
    let rel_x = px - cx;
    let rel_y = py - cy;
    let perp_dist = rel_x * nx + rel_y * ny;

    // Source x = arc length at this t
    let sx = arc_length_to_t_inverse(t, arc_lengths, total_arc_len);

    // Source y = perpendicular distance mapped to text height
    // perp_dist > 0 means above curve, maps to top of text (sy=0)
    let sy = text_height / 2.0 - perp_dist;

    Some((sx, sy))
}

/// Convert a Bézier t parameter back to arc-length distance.
fn arc_length_to_t_inverse(t: f32, lengths: &[f32], _total: f32) -> f32 {
    let n = lengths.len() - 1;
    let idx_f = t * n as f32;
    let idx = (idx_f as usize).min(n - 1);
    let frac = idx_f - idx as f32;
    lengths[idx] + frac * (lengths[idx + 1] - lengths[idx])
}

/// Bilinear sample from an RGBA buffer.
fn bilinear_sample(src: &[u8], w: usize, h: usize, x: f32, y: f32) -> [u8; 4] {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let sample = |sx: i32, sy: i32| -> [f32; 4] {
        if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
            return [0.0; 4];
        }
        let idx = (sy as usize * w + sx as usize) * 4;
        [
            src[idx] as f32,
            src[idx + 1] as f32,
            src[idx + 2] as f32,
            src[idx + 3] as f32,
        ]
    };

    let p00 = sample(x0, y0);
    let p10 = sample(x0 + 1, y0);
    let p01 = sample(x0, y0 + 1);
    let p11 = sample(x0 + 1, y0 + 1);

    let mut result = [0u8; 4];
    for c in 0..4 {
        let v = p00[c] * (1.0 - fx) * (1.0 - fy)
            + p10[c] * fx * (1.0 - fy)
            + p01[c] * (1.0 - fx) * fy
            + p11[c] * fx * fy;
        result[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    result
}

// ===========================================================================
// Text Effects Rendering (Batch 5+6)
// ===========================================================================

/// Extract an alpha coverage mask from an RGBA buffer.
/// Returns a Vec<f32> with values in [0.0, 1.0], one per pixel.
fn extract_coverage_mask(rgba: &[u8], w: u32, h: u32) -> Vec<f32> {
    let count = (w as usize) * (h as usize);
    let mut mask = vec![0.0f32; count];
    mask.par_chunks_mut(w as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let row_off = y * w as usize * 4;
            for x in 0..w as usize {
                let a = rgba[row_off + x * 4 + 3];
                row[x] = a as f32 / 255.0;
            }
        });
    mask
}

