impl ToolsPanel {
    fn canvas_to_screen(&self, canvas_pos: (u32, u32), canvas_rect: Rect, zoom: f32) -> Pos2 {
        let (x, y) = canvas_pos;
        Pos2::new(
            canvas_rect.min.x + x as f32 * zoom,
            canvas_rect.min.y + y as f32 * zoom,
        )
    }

    /// Convert canvas pixel coordinates to screen Pos2 (float version)
    fn canvas_pos2_to_screen(&self, canvas_pos: Pos2, canvas_rect: Rect, zoom: f32) -> Pos2 {
        Pos2::new(
            canvas_rect.min.x + canvas_pos.x * zoom,
            canvas_rect.min.y + canvas_pos.y * zoom,
        )
    }

    /// Convert screen Pos2 to canvas pixel coordinates
    fn screen_to_canvas_pos2(&self, screen_pos: Pos2, canvas_rect: Rect, zoom: f32) -> Pos2 {
        Pos2::new(
            (screen_pos.x - canvas_rect.min.x) / zoom,
            (screen_pos.y - canvas_rect.min.y) / zoom,
        )
    }

    /// Calculate cubic Bézier point at t (0.0 to 1.0)
    fn bezier_point(&self, p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, t: f32) -> Pos2 {
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        Pos2::new(
            mt3 * p0.x + 3.0 * mt2 * t * p1.x + 3.0 * mt * t2 * p2.x + t3 * p3.x,
            mt3 * p0.y + 3.0 * mt2 * t * p1.y + 3.0 * mt * t2 * p2.y + t3 * p3.y,
        )
    }

    /// Calculate bounding box for Bézier curve based on control points
    pub fn get_bezier_bounds(
        &self,
        control_points: [Pos2; 4],
        canvas_width: u32,
        canvas_height: u32,
    ) -> Rect {
        // Find min/max X and Y of control points
        let mut min_x = control_points[0].x;
        let mut max_x = control_points[0].x;
        let mut min_y = control_points[0].y;
        let mut max_y = control_points[0].y;

        for point in &control_points[1..] {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }

        // Padding includes brush radius plus extra space for arrowheads
        let arrow_extra = if self.line_state.line_tool.options.end_shape == LineEndShape::Arrow {
            (self.properties.size * 3.0).max(8.0) + (self.properties.size * 1.5).max(4.0)
        } else {
            0.0
        };
        let padding = self.properties.size / 2.0 + 2.0 + arrow_extra;
        min_x = (min_x - padding).max(0.0);
        min_y = (min_y - padding).max(0.0);
        max_x = (max_x + padding).min(canvas_width as f32);
        max_y = (max_y + padding).min(canvas_height as f32);

        Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
    }

    /// Rasterize a cubic Bézier curve to the preview layer
    pub fn rasterize_bezier(
        &self,
        canvas_state: &mut CanvasState,
        control_points: [Pos2; 4],
        color: Color32,
        pattern: LinePattern,
        cap_style: CapStyle,
        last_bounds: Option<Rect>, // Previous frame's bounds for focused clearing
    ) {
        // Convert Color32 to high-precision f32 for drawing
        let color_f32 = [
            color.r() as f32 / 255.0,
            color.g() as f32 / 255.0,
            color.b() as f32 / 255.0,
            color.a() as f32 / 255.0,
        ];
        // Create or clear preview layer
        if canvas_state.preview_layer.is_none()
            || canvas_state.preview_layer.as_ref().unwrap().width() != canvas_state.width
            || canvas_state.preview_layer.as_ref().unwrap().height() != canvas_state.height
        {
            canvas_state.preview_layer =
                Some(TiledImage::new(canvas_state.width, canvas_state.height));
        }
        // Line tool uses the tool's blending mode for preview
        canvas_state.preview_blend_mode = self.properties.blending_mode;
        if canvas_state.edit_layer_mask
            && canvas_state
                .layers
                .get(canvas_state.active_layer_index)
                .is_some_and(|l| l.has_live_mask())
        {
            canvas_state.preview_force_composite = true;
            canvas_state.preview_targets_mask = true;
            canvas_state.preview_mask_reveal = false;
            canvas_state.preview_is_eraser = false;
        } else {
            canvas_state.preview_targets_mask = false;
            canvas_state.preview_mask_reveal = false;
        }

        // OPTIMIZATION: Only clear the previous frame's bounding box instead of entire image
        if let Some(ref mut preview) = canvas_state.preview_layer {
            if let Some(bounds) = last_bounds {
                // Focused clear: drop chunks that overlap the previous bounds
                let min_x = bounds.min.x.floor().max(0.0) as u32;
                let max_x = bounds.max.x.ceil().min(canvas_state.width as f32) as u32;
                let min_y = bounds.min.y.floor().max(0.0) as u32;
                let max_y = bounds.max.y.ceil().min(canvas_state.height as f32) as u32;
                preview.clear_region(min_x, min_y, max_x, max_y);
            } else {
                // First frame: clear entire layer
                preview.clear();
            }

            let [p0, p1, p2, p3] = control_points;

            // Much tighter spacing for smooth lines - use 10% of size for better quality
            let spacing = (self.properties.size * 0.1).max(0.5);

            // Adaptive sampling based on curve length estimate
            let chord_len = (p3 - p0).length();
            let control_net_len = (p1 - p0).length() + (p2 - p1).length() + (p3 - p2).length();
            let total_length = control_net_len + chord_len;

            // Calculate steps based on tighter spacing for smooth rendering
            let steps = (total_length / spacing).ceil() as usize;
            let steps = steps.clamp(20, 5000); // Higher max for quality

            // Track cumulative distance for pattern rendering
            let mut cumulative_distance = 0.0;
            let mut last_pos: Option<Pos2> = None;

            // Pattern parameters (in pixels)
            let (on_length, off_length) = match pattern {
                LinePattern::Solid => (0.0, 0.0), // No pattern
                LinePattern::Dotted => (self.properties.size * 0.5, self.properties.size * 1.5), // Dot size, larger gap
                LinePattern::Dashed => (self.properties.size * 2.0, self.properties.size * 1.5), // Dash, gap
            };
            let pattern_cycle = on_length + off_length;
            let selection_mask = canvas_state.selection_mask.as_ref();

            // Collect all points first for cap style processing
            let mut line_points = Vec::new();

            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let pos = self.bezier_point(p0, p1, p2, p3, t);

                // Update cumulative distance
                if let Some(prev_pos) = last_pos {
                    cumulative_distance += (pos - prev_pos).length();
                }
                last_pos = Some(pos);

                let fx = pos.x;
                let fy = pos.y;

                if fx >= 0.0
                    && fy >= 0.0
                    && (fx as u32) < canvas_state.width
                    && (fy as u32) < canvas_state.height
                {
                    // Check pattern based on cumulative distance
                    let should_draw = match pattern {
                        LinePattern::Solid => true,
                        LinePattern::Dotted | LinePattern::Dashed => {
                            let pos_in_cycle = cumulative_distance % pattern_cycle;
                            pos_in_cycle < on_length
                        }
                    };

                    if should_draw {
                        if !Self::selection_allows(selection_mask, fx as u32, fy as u32) {
                            continue;
                        }
                        line_points.push((fx, fy, i == 0, i == steps));
                    }
                }
            }

            // Determine which endpoints have arrows (skip dot drawing there)
            let end_shape = self.line_state.line_tool.options.end_shape;
            let arrow_side = self.line_state.line_tool.options.arrow_side;

            // Draw all points
            for &(fx, fy, is_start, is_end) in &line_points {
                // For flat caps, skip drawing circles at the very endpoints
                if cap_style == CapStyle::Flat && (is_start || is_end) {
                    continue;
                }

                // Normal drawing for round caps or middle points
                self.draw_bezier_dot(preview, (fx, fy), color_f32, selection_mask);
            }

            // Draw arrowheads if enabled
            if end_shape == LineEndShape::Arrow {
                let arrow_length = (self.properties.size * 3.0).max(8.0);
                let arrow_half_width = (self.properties.size * 1.5).max(4.0);
                // Push tip forward so the triangle fully covers the line endpoint
                let tip_advance = self.properties.size + self.properties.size / 2.0;

                // End arrow
                if arrow_side == ArrowSide::End || arrow_side == ArrowSide::Both {
                    // Tangent at t=1: B'(1) = 3(P3 - P2)
                    let tx = 3.0 * (p3.x - p2.x);
                    let ty = 3.0 * (p3.y - p2.y);
                    let len = (tx * tx + ty * ty).sqrt().max(0.001);
                    let dx = tx / len;
                    let dy = ty / len;
                    let tip = Pos2::new(p3.x + dx * tip_advance, p3.y + dy * tip_advance);
                    let base_center =
                        Pos2::new(tip.x - dx * arrow_length, tip.y - dy * arrow_length);
                    let px = -dy;
                    let py = dx;
                    let wing1 = Pos2::new(
                        base_center.x + px * arrow_half_width,
                        base_center.y + py * arrow_half_width,
                    );
                    let wing2 = Pos2::new(
                        base_center.x - px * arrow_half_width,
                        base_center.y - py * arrow_half_width,
                    );
                    self.draw_filled_triangle(
                        preview,
                        tip,
                        wing1,
                        wing2,
                        color_f32,
                        canvas_state.width,
                        canvas_state.height,
                        selection_mask,
                    );
                }

                // Start arrow (points backward along the curve)
                if arrow_side == ArrowSide::Start || arrow_side == ArrowSide::Both {
                    // Tangent at t=0: B'(0) = 3(P1 - P0)
                    let tx = 3.0 * (p1.x - p0.x);
                    let ty = 3.0 * (p1.y - p0.y);
                    let len = (tx * tx + ty * ty).sqrt().max(0.001);
                    let dx = tx / len;
                    let dy = ty / len;
                    let tip = Pos2::new(p0.x - dx * tip_advance, p0.y - dy * tip_advance);
                    let base_center =
                        Pos2::new(tip.x + dx * arrow_length, tip.y + dy * arrow_length);
                    let px = -dy;
                    let py = dx;
                    let wing1 = Pos2::new(
                        base_center.x + px * arrow_half_width,
                        base_center.y + py * arrow_half_width,
                    );
                    let wing2 = Pos2::new(
                        base_center.x - px * arrow_half_width,
                        base_center.y - py * arrow_half_width,
                    );
                    self.draw_filled_triangle(
                        preview,
                        tip,
                        wing1,
                        wing2,
                        color_f32,
                        canvas_state.width,
                        canvas_state.height,
                        selection_mask,
                    );
                }
            }
        }
    }

    /// Draw a filled anti-aliased triangle on the preview layer (for arrowheads).
    fn draw_filled_triangle(
        &self,
        preview: &mut TiledImage,
        a: Pos2,
        b: Pos2,
        c: Pos2,
        color_f32: [f32; 4],
        canvas_w: u32,
        canvas_h: u32,
        selection_mask: Option<&image::GrayImage>,
    ) {
        // 1px AA fade for crisp edges
        let fade_px = 1.0_f32;

        // Bounding box expanded by fade zone
        let min_x = (a.x.min(b.x).min(c.x) - fade_px).floor().max(0.0) as u32;
        let max_x =
            ((a.x.max(b.x).max(c.x) + fade_px).ceil() as u32).min(canvas_w.saturating_sub(1));
        let min_y = (a.y.min(b.y).min(c.y) - fade_px).floor().max(0.0) as u32;
        let max_y =
            ((a.y.max(b.y).max(c.y) + fade_px).ceil() as u32).min(canvas_h.saturating_sub(1));

        let [src_r, src_g, src_b, src_a] = color_f32;

        // Signed pixel distance from point to edge (positive = inside)
        #[inline]
        fn edge_dist(v0: Pos2, v1: Pos2, px: f32, py: f32) -> f32 {
            let ex = v1.x - v0.x;
            let ey = v1.y - v0.y;
            let len = (ex * ex + ey * ey).sqrt().max(0.001);
            ((ex) * (py - v0.y) - (ey) * (px - v0.x)) / len
        }

        // Determine winding direction
        let area_sign = {
            let ex = b.x - a.x;
            let ey = b.y - a.y;
            let cross = ex * (c.y - a.y) - ey * (c.x - a.x);
            if cross >= 0.0 { 1.0f32 } else { -1.0f32 }
        };

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if !Self::selection_allows(selection_mask, x, y) {
                    continue;
                }
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;

                let d0 = edge_dist(a, b, px, py) * area_sign;
                let d1 = edge_dist(b, c, px, py) * area_sign;
                let d2 = edge_dist(c, a, px, py) * area_sign;

                let min_dist = d0.min(d1).min(d2);

                if min_dist < -fade_px {
                    continue;
                }

                // Smoothstep AA fade
                let alpha = if min_dist >= fade_px {
                    src_a
                } else {
                    let t = ((min_dist + fade_px) / (2.0 * fade_px)).clamp(0.0, 1.0);
                    let smooth = t * t * (3.0 - 2.0 * t);
                    smooth * src_a
                };

                if alpha <= 0.0 {
                    continue;
                }

                let pixel = preview.get_pixel_mut(x, y);
                let base_a = pixel[3] as f32 / 255.0;
                if alpha > base_a {
                    *pixel = image::Rgba([
                        (src_r * 255.0) as u8,
                        (src_g * 255.0) as u8,
                        (src_b * 255.0) as u8,
                        (alpha * 255.0) as u8,
                    ]);
                }
            }
        }
    }

    /// Draw a segment as part of Bézier curve with spacing
    fn draw_bezier_segment(
        &self,
        preview: &mut TiledImage,
        start: (f32, f32),
        end: (f32, f32),
        color_f32: [f32; 4],
    ) {
        let (x0, y0) = start;
        let (x1, y1) = end;

        // Calculate spacing (25% of brush diameter)
        let spacing = (self.properties.size * 0.25).max(1.0);
        let radius = self.properties.size / 2.0;

        // Calculate distance
        let dx = x1 - x0;
        let dy = y1 - y0;
        let distance = (dx * dx + dy * dy).sqrt();

        // If distance is too small, just draw one circle
        if distance < 0.1 {
            if (start.0 as u32) < preview.width() && (start.1 as u32) < preview.height() {
                let anti_alias = self.line_state.line_tool.options.anti_alias;
                self.draw_bezier_circle_with_hardness(
                    preview, start, color_f32, radius, 0.95, anti_alias, None,
                );
            }
            return;
        }

        // Calculate number of steps
        let num_steps = (distance / spacing).ceil() as usize;

        // Draw circles at spaced intervals
        for i in 0..=num_steps {
            let t = (i as f32 * spacing / distance).min(1.0);
            let x = x0 + dx * t;
            let y = y0 + dy * t;

            if (x as u32) < preview.width() && (y as u32) < preview.height() {
                let anti_alias = self.line_state.line_tool.options.anti_alias;
                self.draw_bezier_circle_with_hardness(
                    preview,
                    (x, y),
                    color_f32,
                    radius,
                    0.95,
                    anti_alias,
                    None,
                );
            }
        }

        // Do NOT force-draw the end point - only draw at spacing intervals
    }

    /// Draw a single dot for Bézier curve (sub-pixel position for smooth AA)
    fn draw_bezier_dot(
        &self,
        preview: &mut TiledImage,
        pos: (f32, f32),
        color_f32: [f32; 4],
        selection_mask: Option<&image::GrayImage>,
    ) {
        let radius = self.properties.size / 2.0;
        let anti_alias = self.line_state.line_tool.options.anti_alias;
        // High hardness for crisp edges with ~2px AA fade (matching arrow sharpness)
        self.draw_bezier_circle_with_hardness(
            preview,
            pos,
            color_f32,
            radius,
            0.95,
            anti_alias,
            selection_mask,
        );
    }

    /// Draw a circle on the preview layer for Bézier curves with custom hardness override
    /// Uses sub-pixel (f32) center for smooth anti-aliasing without grid artifacts
    fn draw_bezier_circle_with_hardness(
        &self,
        preview: &mut TiledImage,
        pos: (f32, f32),
        color_f32: [f32; 4],
        radius: f32,
        forced_hardness: f32,
        anti_alias: bool,
        selection_mask: Option<&image::GrayImage>,
    ) {
        let (cx, cy) = pos;

        // Extend the sampling area beyond the nominal radius so the soft fade
        // region actually gets drawn.  For tiny radii (< 1.5) we only need 1px
        // of AA feather; for larger radii the fade scales with softness.
        let aa_pad = if anti_alias {
            if radius < 1.5 {
                1.5  // 1px AA feather for thin lines
            } else {
                (radius * (1.0 - forced_hardness)).max(2.0) + 2.0
            }
        } else {
            1.0
        };
        let outer_radius = radius + aa_pad;

        let min_x = (cx - outer_radius).max(0.0) as u32;
        let max_x = ((cx + outer_radius).ceil() as u32).min(preview.width() - 1);
        let min_y = (cy - outer_radius).max(0.0) as u32;
        let max_y = ((cy + outer_radius).ceil() as u32).min(preview.height() - 1);

        // Unpack high-precision source
        let [src_r, src_g, src_b, src_a] = color_f32;

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if !Self::selection_allows(selection_mask, x, y) {
                    continue;
                }
                // Sub-pixel distance from float center for smooth AA
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let dist = (dx * dx + dy * dy).sqrt();

                // Compute alpha — this returns 0.0 for pixels beyond the
                // effective (soft) radius, so no explicit dist guard needed.
                let alpha = self.compute_line_alpha(dist, radius, forced_hardness, anti_alias);
                if alpha <= 0.0 {
                    continue;
                }

                let pixel_alpha = alpha * src_a;

                // MAX blending: keep the highest alpha at each pixel.
                // Since every stamp uses the same RGB colour, taking max(alpha)
                // produces perfectly smooth edges with no scalloping from
                // overlapping circle stamps.
                let pixel = preview.get_pixel_mut(x, y);
                let base_a = pixel[3] as f32 / 255.0;

                if pixel_alpha > base_a {
                    *pixel = Rgba([
                        (src_r * 255.0) as u8,
                        (src_g * 255.0) as u8,
                        (src_b * 255.0) as u8,
                        (pixel_alpha * 255.0) as u8,
                    ]);
                }
            }
        }
    }

    /// Magic Wand selection — spawn async Dijkstra minimax distance map computation.
    /// Result arrives via channel; tolerance changes re-threshold instantly.
    fn cached_active_layer_rgba(
        cache: &mut Option<FlatLayerCache>,
        canvas_state: &CanvasState,
    ) -> Option<Arc<[u8]>> {
        let layer_index = canvas_state.active_layer_index;
        let layer = canvas_state.layers.get(layer_index)?;

        let needs_refresh = cache.as_ref().is_none_or(|entry| {
            entry.layer_index != layer_index
                || entry.gpu_generation != layer.gpu_generation
                || entry.width != canvas_state.width
                || entry.height != canvas_state.height
        });

        if needs_refresh {
            let data: Arc<[u8]> =
                Arc::from(layer.pixels.to_rgba_image().into_raw().into_boxed_slice());
            *cache = Some(FlatLayerCache {
                layer_index,
                gpu_generation: layer.gpu_generation,
                width: canvas_state.width,
                height: canvas_state.height,
                data: data.clone(),
            });
            Some(data)
        } else {
            cache.as_ref().map(|entry| entry.data.clone())
        }
    }

    #[inline]
    fn selection_allows(selection_mask: Option<&image::GrayImage>, x: u32, y: u32) -> bool {
        selection_mask.is_none_or(|mask| {
            x < mask.width() && y < mask.height() && mask.get_pixel(x, y).0[0] != 0
        })
    }

    fn adjust_preview_region_for_selection(
        preview: &mut TiledImage,
        off_x: i32,
        off_y: i32,
        region_w: u32,
        region_h: u32,
        selection_mask: Option<&image::GrayImage>,
        outside_alpha_scale: f32,
    ) {
        let Some(mask) = selection_mask else {
            return;
        };

        for row in 0..region_h {
            let gy = off_y + row as i32;
            if gy < 0 || gy >= preview.height() as i32 {
                continue;
            }
            for col in 0..region_w {
                let gx = off_x + col as i32;
                if gx < 0 || gx >= preview.width() as i32 {
                    continue;
                }
                let x = gx as u32;
                let y = gy as u32;
                if Self::selection_allows(Some(mask), x, y) {
                    continue;
                }
                let pixel = preview.get_pixel_mut(x, y);
                if pixel[3] == 0 {
                    continue;
                }
                if outside_alpha_scale <= 0.0 {
                    pixel[3] = 0;
                } else {
                    pixel[0] = ((pixel[0] as u16 + 128) / 2) as u8;
                    pixel[1] = ((pixel[1] as u16 + 128) / 2) as u8;
                    pixel[2] = ((pixel[2] as u16 + 128) / 2) as u8;
                    pixel[3] = ((pixel[3] as f32) * outside_alpha_scale).round() as u8;
                }
            }
        }
    }

}

