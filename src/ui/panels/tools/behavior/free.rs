
// ---------------------------------------------------------------------------
// Glyph Edit Mode Helpers (Phase 5 — Batch 9)
// ---------------------------------------------------------------------------

/// Draw a dotted rectangle outline.
fn draw_dotted_rect(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: Color32,
    thickness: f32,
    dash_len: f32,
    gap_len: f32,
) {
    draw_dotted_quad(
        painter,
        [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ],
        color,
        thickness,
        dash_len,
        gap_len,
    );
}

/// Draw a dotted outline connecting four screen-space corner points (a rotated rect).
fn draw_dotted_quad(
    painter: &egui::Painter,
    corners: [Pos2; 4],
    color: Color32,
    thickness: f32,
    dash_len: f32,
    gap_len: f32,
) {
    let stroke = egui::Stroke::new(thickness, color);
    let edges = [
        (corners[0], corners[1]),
        (corners[1], corners[2]),
        (corners[2], corners[3]),
        (corners[3], corners[0]),
    ];
    for (start, end) in edges {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let length = (dx * dx + dy * dy).sqrt();
        if length < 0.1 {
            continue;
        }
        let nx = dx / length;
        let ny = dy / length;
        let mut t = 0.0f32;
        while t < length {
            let seg_end = (t + dash_len).min(length);
            painter.line_segment(
                [
                    Pos2::new(start.x + nx * t, start.y + ny * t),
                    Pos2::new(start.x + nx * seg_end, start.y + ny * seg_end),
                ],
                stroke,
            );
            t = seg_end + gap_len;
        }
    }
}

/// Rotate a screen-space point around a center by the given angle (radians).
fn rotate_screen_point(p: Pos2, center: Pos2, angle: f32) -> Pos2 {
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let dx = p.x - center.x;
    let dy = p.y - center.y;
    Pos2::new(
        center.x + dx * cos_a - dy * sin_a,
        center.y + dx * sin_a + dy * cos_a,
    )
}

fn color_luminance(c: Color32) -> f32 {
    let r = c.r() as f32 / 255.0;
    let g = c.g() as f32 / 255.0;
    let b = c.b() as f32 / 255.0;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast_text_color(fill: Color32) -> Color32 {
    let luminance = color_luminance(fill);
    let contrast_with_black = (luminance + 0.05) / 0.05;
    let contrast_with_white = 1.05 / (luminance + 0.05);
    if contrast_with_black >= contrast_with_white {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{CanvasState, TiledImage};
    use image::Rgba;

    #[test]
    fn fill_press_commits_previous_preview_and_applies_new_fill() {
        let mut tools = ToolsPanel::default();
        let mut canvas_state = CanvasState::new(4, 4);
        let committed_color = Rgba([255, 0, 0, 255]);

        let mut preview = TiledImage::new(canvas_state.width, canvas_state.height);
        *preview.get_pixel_mut(0, 0) = committed_color;
        canvas_state.preview_layer = Some(preview);

        tools.fill_state.active_fill = Some(ActiveFillRegion {
            start_x: 0,
            start_y: 0,
            layer_idx: canvas_state.active_layer_index,
            target_color: *canvas_state.layers[canvas_state.active_layer_index]
                .pixels
                .get_pixel(0, 0),
            region_index: None,
            fill_mask: Vec::new(),
            fill_bbox: None,
            last_threshold: None,
        });

        tools.handle_fill_click(
            &mut canvas_state,
            (3, 3),
            false,
            false,
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            None,
        );

        // Previous preview should be committed to the layer
        assert_eq!(
            *canvas_state.layers[canvas_state.active_layer_index]
                .pixels
                .get_pixel(0, 0),
            committed_color
        );

        // New fill should be applied directly at the click position
        assert_eq!(
            *canvas_state.layers[canvas_state.active_layer_index]
                .pixels
                .get_pixel(3, 3),
            Rgba([0, 0, 255, 255])
        );
    }
}
