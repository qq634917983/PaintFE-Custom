impl ToolsPanel {
    fn draw_bezier_handles(&self, painter: &egui::Painter, canvas_rect: Rect, zoom: f32) {
        let points = self.line_state.line_tool.control_points;

        // Draw control lines (dashed)
        let p0_screen = self.canvas_pos2_to_screen(points[0], canvas_rect, zoom);
        let p1_screen = self.canvas_pos2_to_screen(points[1], canvas_rect, zoom);
        let p2_screen = self.canvas_pos2_to_screen(points[2], canvas_rect, zoom);
        let p3_screen = self.canvas_pos2_to_screen(points[3], canvas_rect, zoom);

        // Draw dashed control lines with transparency
        painter.line_segment(
            [p0_screen, p1_screen],
            (1.0, Color32::from_rgba_unmultiplied(150, 150, 150, 180)),
        );
        painter.line_segment(
            [p2_screen, p3_screen],
            (1.0, Color32::from_rgba_unmultiplied(150, 150, 150, 180)),
        );

        // Draw handles with high transparency (30% opaque) to see through them
        let handle_colors = [
            Color32::from_rgba_unmultiplied(0, 200, 0, 77), // Start - green, 30% opaque
            Color32::from_rgba_unmultiplied(100, 150, 255, 77), // Control1 - light blue, 30% opaque
            Color32::from_rgba_unmultiplied(100, 150, 255, 77), // Control2 - light blue, 30% opaque
            Color32::from_rgba_unmultiplied(200, 0, 0, 77), // End - red, 30% opaque
        ];

        for (i, &point) in points.iter().enumerate() {
            let screen_pos = self.canvas_pos2_to_screen(point, canvas_rect, zoom);
            painter.circle_filled(screen_pos, 6.0, handle_colors[i]);
            painter.circle_stroke(
                screen_pos,
                6.0,
                (2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 100)),
            );
        }

        // ── Pan handle — drawn as a diamond with a 4-arrow cross, offset from P0 ──
        // Positioned 22px up-left of P0 in screen space. Distinct from the regular
        // endpoint handles: different shape, colour and stronger outline so it clearly
        // communicates "move the whole line".
        let pan_offset = egui::Vec2::new(-22.0, -22.0);
        let ph = p0_screen + pan_offset;

        // Dashed connector line from P0 to pan handle so it's visually anchored
        painter.line_segment(
            [p0_screen, ph],
            (1.0, Color32::from_rgba_unmultiplied(200, 200, 200, 100)),
        );

        // Choose colour: brighter when hovering or dragging
        let is_active = self.line_state.line_tool.pan_handle_dragging
            || self.line_state.line_tool.pan_handle_hovering;
        let fill_alpha: u8 = if is_active { 210 } else { 130 };
        let ring_alpha: u8 = if is_active { 255 } else { 160 };
        let fill = Color32::from_rgba_unmultiplied(255, 200, 50, fill_alpha); // amber
        let ring = Color32::from_rgba_unmultiplied(255, 255, 255, ring_alpha);
        let outline = Color32::from_rgba_unmultiplied(80, 60, 0, fill_alpha); // dark amber border

        // Diamond shape (rotate square 45°)
        let r = if is_active { 8.0f32 } else { 7.0f32 };
        let diamond = vec![
            ph + egui::Vec2::new(0.0, -r),
            ph + egui::Vec2::new(r, 0.0),
            ph + egui::Vec2::new(0.0, r),
            ph + egui::Vec2::new(-r, 0.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            diamond.clone(),
            fill,
            egui::Stroke::new(1.5, outline),
        ));
        // White ring for contrast
        painter.add(egui::Shape::closed_line(
            diamond,
            egui::Stroke::new(0.8, ring),
        ));

        // 4-arrow cross drawn as four small filled triangles pointing outward
        let arm = r * 0.55;
        let tip = r * 0.90;
        let w = r * 0.28;
        let arrow_color = Color32::from_rgba_unmultiplied(60, 40, 0, fill_alpha);
        for (ax, ay) in [(0.0f32, -1.0f32), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
            // perpendicular
            let (px_dir, py_dir) = (-ay, ax);
            let tri = vec![
                ph + egui::Vec2::new(ax * tip, ay * tip),
                ph + egui::Vec2::new(ax * arm + px_dir * w, ay * arm + py_dir * w),
                ph + egui::Vec2::new(ax * arm - px_dir * w, ay * arm - py_dir * w),
            ];
            painter.add(egui::Shape::convex_polygon(
                tri,
                arrow_color,
                egui::Stroke::NONE,
            ));
        }
    }

    /// Commit the Bézier curve from preview layer to the actual active layer.
    /// Only iterates populated preview chunks (~5-20 chunks) instead of all W×H pixels.
    fn commit_bezier_to_layer(&self, canvas_state: &mut CanvasState, _color_f32: [f32; 4]) {
        let width = canvas_state.width;
        let height = canvas_state.height;
        let blend_mode = self.properties.blending_mode;

        // Extract only populated chunk data from preview (clone to release borrow)
        let preview_chunks: Vec<(u32, u32, image::RgbaImage)> = match &canvas_state.preview_layer {
            Some(preview) => preview
                .chunk_keys()
                .filter_map(|(cx, cy)| preview.get_chunk(cx, cy).map(|c| (cx, cy, c.clone())))
                .collect(),
            None => return,
        };

        // Extract selection mask pointer before mutable borrow of active layer
        let mask_ptr = canvas_state
            .selection_mask
            .as_ref()
            .map(|m| m as *const GrayImage);

        if let Some(active_layer) = canvas_state.get_active_layer_mut() {
            let mask_ref = mask_ptr.map(|p| unsafe { &*p });
            for (cx, cy, preview_chunk) in &preview_chunks {
                let base_x = cx * CHUNK_SIZE;
                let base_y = cy * CHUNK_SIZE;
                let cw = CHUNK_SIZE.min(width.saturating_sub(base_x));
                let ch = CHUNK_SIZE.min(height.saturating_sub(base_y));

                for ly in 0..ch {
                    for lx in 0..cw {
                        // Skip pixels outside the selection mask
                        if let Some(mask) = mask_ref {
                            let gx = base_x + lx;
                            let gy = base_y + ly;
                            if gx < mask.width() && gy < mask.height() {
                                if mask.get_pixel(gx, gy).0[0] == 0 {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }

                        let preview_pixel = *preview_chunk.get_pixel(lx, ly);
                        if preview_pixel[3] > 0 {
                            let layer_pixel =
                                active_layer.pixels.get_pixel_mut(base_x + lx, base_y + ly);
                            *layer_pixel = CanvasState::blend_pixel_static(
                                *layer_pixel,
                                preview_pixel,
                                blend_mode,
                                1.0,
                            );
                        }
                    }
                }
            }
        }
    }

    /// Commit the eraser mask from preview layer to the actual active layer.
    /// Applies the mask's alpha channel as erase strength (reducing the layer's alpha).
    /// Only iterates populated preview chunks for efficiency.
    fn commit_eraser_to_layer(&self, canvas_state: &mut CanvasState) {
        // Eraser already draws at mirrored positions per-frame, so no
        // mirror_preview_layer() needed here (would double-mirror).

        let width = canvas_state.width;
        let height = canvas_state.height;

        // Extract only populated chunk data from preview (clone to release borrow)
        let preview_chunks: Vec<(u32, u32, image::RgbaImage)> = match &canvas_state.preview_layer {
            Some(preview) => preview
                .chunk_keys()
                .filter_map(|(cx, cy)| preview.get_chunk(cx, cy).map(|c| (cx, cy, c.clone())))
                .collect(),
            None => return,
        };

        // Extract selection mask pointer before mutable borrow of active layer
        let mask_ptr = canvas_state
            .selection_mask
            .as_ref()
            .map(|m| m as *const GrayImage);

        if let Some(active_layer) = canvas_state.get_active_layer_mut() {
            let mask_ref = mask_ptr.map(|p| unsafe { &*p });
            for (cx, cy, preview_chunk) in &preview_chunks {
                let base_x = cx * CHUNK_SIZE;
                let base_y = cy * CHUNK_SIZE;
                let cw = CHUNK_SIZE.min(width.saturating_sub(base_x));
                let ch = CHUNK_SIZE.min(height.saturating_sub(base_y));

                for ly in 0..ch {
                    for lx in 0..cw {
                        // Skip pixels outside the selection mask
                        if let Some(mask) = mask_ref {
                            let gx = base_x + lx;
                            let gy = base_y + ly;
                            if gx < mask.width() && gy < mask.height() {
                                if mask.get_pixel(gx, gy).0[0] == 0 {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }

                        let mask_pixel = *preview_chunk.get_pixel(lx, ly);
                        if mask_pixel[3] > 0 {
                            let layer_pixel =
                                active_layer.pixels.get_pixel_mut(base_x + lx, base_y + ly);
                            // Reduce the layer pixel's alpha by the mask strength
                            let mask_strength = mask_pixel[3] as f32 / 255.0;
                            let current_a = layer_pixel[3] as f32 / 255.0;
                            let new_a = (current_a * (1.0 - mask_strength)).max(0.0);
                            layer_pixel[3] = (new_a * 255.0) as u8;
                        }
                    }
                }
            }
        }
    }

    /// Commit preview alpha into the active layer mask.
    /// `reveal=true` reduces conceal alpha (mask eraser), `reveal=false` increases conceal.
    fn commit_preview_to_layer_mask(&self, canvas_state: &mut CanvasState, reveal: bool) {
        let width = canvas_state.width;
        let height = canvas_state.height;

        let (preview_ptr, preview_chunk_keys): (*const TiledImage, Vec<(u32, u32)>) =
            match &canvas_state.preview_layer {
                Some(preview) => (preview as *const TiledImage, preview.chunk_keys().collect()),
                None => return,
            };

        let mask_ptr = canvas_state
            .selection_mask
            .as_ref()
            .map(|m| m as *const GrayImage);

        if let Some(active_layer) = canvas_state.layers.get_mut(canvas_state.active_layer_index) {
            active_layer.ensure_mask();
            let Some(mask) = active_layer.mask.as_mut() else {
                return;
            };
            active_layer.mask_enabled = true;
            let selection = mask_ptr.map(|p| unsafe { &*p });
            let preview = unsafe { &*preview_ptr };

            for (cx, cy) in preview_chunk_keys {
                let Some(preview_chunk) = preview.get_chunk(cx, cy) else {
                    continue;
                };
                let base_x = cx * CHUNK_SIZE;
                let base_y = cy * CHUNK_SIZE;
                let cw = CHUNK_SIZE.min(width.saturating_sub(base_x));
                let ch = CHUNK_SIZE.min(height.saturating_sub(base_y));

                for ly in 0..ch {
                    for lx in 0..cw {
                        let gx = base_x + lx;
                        let gy = base_y + ly;

                        if let Some(sel) = selection {
                            if gx >= sel.width() || gy >= sel.height() {
                                continue;
                            }
                            if sel.get_pixel(gx, gy).0[0] == 0 {
                                continue;
                            }
                        }

                        let strength = preview_chunk.get_pixel(lx, ly)[3];
                        if strength == 0 {
                            continue;
                        }

                        let mut mp = *mask.get_pixel(gx, gy);
                        let old = mp[3] as u32;
                        let s = strength as u32;
                        let new_alpha = if reveal {
                            ((old * (255 - s)) / 255) as u8
                        } else {
                            (old + ((255 - old) * s) / 255) as u8
                        };
                        mp[3] = new_alpha;
                        mask.put_pixel(gx, gy, mp);
                    }
                }
            }
        }
    }

    // ========================================================================
    // SPEED-ADAPTIVE EMA SMOOTHING
    // ========================================================================
    // The brush position is smoothed using an exponential moving average (EMA)
    // applied inline in the painting loop above.  This replaces the previous
    // Catmull-Rom spline approach which was ineffective because 1000 Hz
    // sub-frame events produce segments too short (~2 px) for meaningful
    // spline curvature.  The EMA naturally accumulates across consecutive
    // direction changes, rounding off corners regardless of input density.

}

