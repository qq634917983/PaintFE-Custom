use crate::log_info;

impl ToolsPanel {
    pub fn tool_order_csv(&self) -> String {
        self.tool_order
            .iter()
            .map(|tool| tool.key())
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn set_tool_order_from_csv(&mut self, csv: &str) {
        let mut order = Vec::new();
        for key in csv.split(',') {
            if let Some(tool) = Tool::from_key(key)
                && !order.contains(&tool)
            {
                order.push(tool);
            }
        }
        // Append tools introduced by newer versions instead of hiding them.
        for tool in Tool::default_order() {
            if !order.contains(&tool) {
                order.push(tool);
            }
        }
        self.tool_order = order;
    }

    pub fn reset_tool_order(&mut self) {
        self.tool_order = Tool::default_order();
    }

    pub fn change_tool(&mut self, new_tool: Tool) {
        if self.active_tool != new_tool {
            log_info!("Tool: switch to {:?}", new_tool);
            // Deactivate perspective crop when switching away
            if self.active_tool == Tool::PerspectiveCrop {
                self.perspective_crop_state.active = false;
                self.perspective_crop_state.dragging_corner = None;
            }
            self.active_tool = new_tool;
            // Auto-init perspective crop ÔÇö need canvas dims, so flag for
            // lazy init in handle_input on next frame.
            if new_tool == Tool::PerspectiveCrop {
                self.perspective_crop_state.needs_auto_init = true;
            }
            // Note: Actual commitment will be handled in handle_input with canvas_state access
        }
    }

    /// Get the name of the active tool for display in context bar
    pub fn active_tool_name(&self) -> String {
        match self.active_tool {
            Tool::Brush => t!("tool.brush"),
            Tool::Eraser => t!("tool.eraser"),
            Tool::Pencil => t!("tool.pencil"),
            Tool::Line => t!("tool.line"),
            Tool::RectangleSelect => t!("tool.rectangle_select"),
            Tool::EllipseSelect => t!("tool.ellipse_select"),
            Tool::MovePixels => t!("tool.move_pixels"),
            Tool::MoveSelection => t!("tool.move_selection"),
            Tool::MagicWand => t!("tool.magic_wand"),
            Tool::Fill => t!("tool.fill"),
            Tool::ColorPicker => t!("tool.color_picker"),
            Tool::Gradient => t!("tool.gradient"),
            Tool::ContentAwareBrush => t!("tool.content_aware_fill"),
            Tool::Liquify => t!("tool.liquify"),
            Tool::MeshWarp => t!("tool.mesh_warp"),
            Tool::ColorRemover => t!("tool.color_remover"),
            Tool::Smudge => "Smudge".to_string(),
            Tool::CloneStamp => t!("tool.clone_stamp"),
            Tool::Text => t!("tool.text"),
            Tool::PerspectiveCrop => t!("tool.perspective_crop"),
            Tool::Lasso => t!("tool.lasso"),
            Tool::Zoom => t!("tool.zoom"),
            Tool::Pan => t!("tool.pan"),
            Tool::Shapes => t!("tool.shapes"),
        }
    }

    /// Short usage hint for a given tool ÔÇö displayed at bottom-left of the app on hover.
    pub fn tool_hint_for(tool: Tool) -> String {
        match tool {
            Tool::Brush => "Left-click to paint. Right-click for secondary color. Hold Shift for straight lines.".into(),
            Tool::Pencil => "Left-click to draw 1px aliased lines. Hold Shift for straight lines.".into(),
            Tool::Eraser => "Left-click to erase. Removes pixels from the active layer.".into(),
            Tool::Line => "Click and drag to draw a straight line. Adjust width in options.".into(),
            Tool::RectangleSelect => "Click and drag to create a rectangular selection.".into(),
            Tool::EllipseSelect => "Click and drag to create an elliptical selection.".into(),
            Tool::MovePixels => "Click + drag to move selected pixels. No selection = move entire layer.".into(),
            Tool::MoveSelection => "Click + drag to move the selection boundary without affecting pixels.".into(),
            Tool::MagicWand => "Click to select contiguous areas of similar color. Adjust tolerance in options.".into(),
            Tool::Fill => "Click to flood-fill an area with the primary color.".into(),
            Tool::ColorPicker => "Left-click to pick primary color. Right-click for secondary color.".into(),
            Tool::Gradient => "Click and drag to draw a gradient on the active layer.".into(),
            Tool::Lasso => "Click to place points, or drag freehand, to create an irregular selection.".into(),
            Tool::Zoom => "Click to zoom in. Drag a rectangle to zoom to area. Hold Alt to zoom out.".into(),
            Tool::Pan => "Click and drag to pan the canvas viewport.".into(),
            Tool::CloneStamp => "Ctrl+click to set source. Then paint to clone from source area.".into(),
            Tool::ContentAwareBrush => "Paint over an area to remove it using content-aware fill.".into(),
            Tool::Liquify => "Click and drag to push/warp pixels in the brush direction.".into(),
            Tool::MeshWarp => "Drag control points to warp the image with a smooth mesh grid.".into(),
            Tool::ColorRemover => "Paint over a color to remove it, making those pixels transparent.".into(),
            Tool::Smudge => "Click and drag to smudge/blend colors in the stroke direction.".into(),
            Tool::Text => "Click to place text. Configure font, size, and color in options.".into(),
            Tool::PerspectiveCrop => "Drag the four corners to define a perspective crop region.".into(),
            Tool::Shapes => "Click and drag to draw shapes. Hold Shift for constrained proportions.".into(),
        }
    }
}

#[cfg(test)]
mod tool_order_tests {
    use super::*;

    #[test]
    fn custom_order_round_trips_and_keeps_all_tools() {
        let mut panel = ToolsPanel::default();
        panel.set_tool_order_from_csv("eraser,brush,pan");
        assert_eq!(panel.tool_order[0..3], [Tool::Eraser, Tool::Brush, Tool::Pan]);
        assert_eq!(panel.tool_order.len(), Tool::default_order().len());
        let csv = panel.tool_order_csv();
        let mut restored = ToolsPanel::default();
        restored.set_tool_order_from_csv(&csv);
        assert_eq!(restored.tool_order, panel.tool_order);
    }
}
