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
        Self::tool_name_for(self.active_tool)
    }

    /// Localized display name used consistently by the toolbar and context bar.
    pub fn tool_name_for(tool: Tool) -> String {
        match tool {
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
            Tool::Smudge => t!("tool.smudge"),
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
            Tool::Brush => t!("tool.hint.brush"),
            Tool::Pencil => t!("tool.hint.pencil"),
            Tool::Eraser => t!("tool.hint.eraser"),
            Tool::Line => t!("tool.hint.line"),
            Tool::RectangleSelect => t!("tool.hint.rectangle_select"),
            Tool::EllipseSelect => t!("tool.hint.ellipse_select"),
            Tool::MovePixels => t!("tool.hint.move_pixels"),
            Tool::MoveSelection => t!("tool.hint.move_selection"),
            Tool::MagicWand => t!("tool.hint.magic_wand"),
            Tool::Fill => t!("tool.hint.fill"),
            Tool::ColorPicker => t!("tool.hint.color_picker"),
            Tool::Gradient => t!("tool.hint.gradient"),
            Tool::Lasso => t!("tool.hint.lasso"),
            Tool::Zoom => t!("tool.hint.zoom"),
            Tool::Pan => t!("tool.hint.pan"),
            Tool::CloneStamp => t!("tool.hint.clone_stamp"),
            Tool::ContentAwareBrush => t!("tool.hint.content_aware"),
            Tool::Liquify => t!("tool.hint.liquify"),
            Tool::MeshWarp => t!("tool.hint.mesh_warp"),
            Tool::ColorRemover => t!("tool.hint.color_remover"),
            Tool::Smudge => t!("tool.hint.smudge"),
            Tool::Text => t!("tool.hint.text"),
            Tool::PerspectiveCrop => t!("tool.hint.perspective_crop"),
            Tool::Shapes => t!("tool.hint.shapes"),
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
