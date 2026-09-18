impl PaintFEApp {
    fn process_ui_control_requests(&mut self, ctx: &egui::Context) {
        let requests: Vec<_> = self.ui_control_receiver.try_iter().collect();
        for request in requests {
            let response = self.execute_ui_control_command(&request.command, ctx);
            let _ = request.response.send(response);
        }
    }

    fn execute_ui_control_command(&mut self, input: &str, ctx: &egui::Context) -> String {
        let input = input.trim();
        let (verb, argument) = input
            .split_once(char::is_whitespace)
            .map(|(verb, rest)| (verb, rest.trim()))
            .unwrap_or((input, ""));

        let result: Result<String, String> = (|| match verb.to_ascii_lowercase().as_str() {
            "status" => {
                let project = self.active_project();
                let (zoom, _) = self.canvas.view_state();
                Ok(serde_json::json!({
                    "ok": true,
                    "project": project.map(|p| p.name.as_str()),
                    "project_index": self.active_project_index,
                    "project_count": self.projects.len(),
                    "width": project.map(|p| p.canvas_state.width),
                    "height": project.map(|p| p.canvas_state.height),
                    "layers": project.map(|p| p.canvas_state.layers.len()),
                    "dirty": project.map(|p| p.is_dirty),
                    "tool": format!("{:?}", self.tools_panel.active_tool),
                    "brush_size": self.tools_panel.properties.size,
                    "primary_color": color_hex(self.colors_panel.get_primary_color()),
                    "secondary_color": color_hex(self.colors_panel.get_secondary_color()),
                    "zoom_percent": (zoom * 100.0).round(),
                    "pending_io": self.pending_io_ops,
                    "pending_filters": self.pending_filter_jobs,
                })
                .to_string())
            }
            "focus" => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                Ok("focused".to_owned())
            }
            "open" => {
                if argument.is_empty() {
                    return Err("usage: open <path>".to_owned());
                }
                let path = std::path::PathBuf::from(argument);
                if !path.exists() {
                    return Err(format!("file does not exist: {}", path.display()));
                }
                let path = path.canonicalize().unwrap_or(path);
                self.open_file_by_path(path.clone(), ctx.input(|i| i.time));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                Ok(format!("opening {}", path.display()))
            }
            "new" => {
                let parts: Vec<_> = argument.split_whitespace().collect();
                if parts.len() != 2 {
                    return Err("usage: new <width> <height>".to_owned());
                }
                let width = parse_dimension(parts[0])?;
                let height = parse_dimension(parts[1])?;
                self.new_project(width, height);
                Ok(format!("created {width}x{height} document"))
            }
            "tool" => {
                let tool = parse_tool(argument)?;
                self.tools_panel.change_tool(tool);
                Ok(format!("tool={tool:?}"))
            }
            "color" | "primary-color" => {
                let color = parse_color(argument)?;
                self.colors_panel.set_primary_color(color);
                Ok(format!("primary_color={}", color_hex(color)))
            }
            "secondary-color" => {
                let color = parse_color(argument)?;
                self.colors_panel.set_secondary_color(color);
                Ok(format!("secondary_color={}", color_hex(color)))
            }
            "brush-size" | "size" => {
                let size: f32 = argument
                    .parse()
                    .map_err(|_| "brush-size expects a number".to_owned())?;
                if !size.is_finite() || !(0.1..=2000.0).contains(&size) {
                    return Err("brush-size must be between 0.1 and 2000".to_owned());
                }
                self.tools_panel.properties.size = size;
                Ok(format!("brush_size={size}"))
            }
            "zoom" => {
                if argument.eq_ignore_ascii_case("fit") {
                    self.canvas.reset_zoom();
                    Ok("zoom=fit".to_owned())
                } else {
                    let text = argument.trim_end_matches('%');
                    let percent: f32 = text
                        .parse()
                        .map_err(|_| "zoom expects a percentage or 'fit'".to_owned())?;
                    if !percent.is_finite() || !(1.0..=6400.0).contains(&percent) {
                        return Err("zoom must be between 1 and 6400 percent".to_owned());
                    }
                    let (_, pan) = self.canvas.view_state();
                    self.canvas.set_view_state(percent / 100.0, pan);
                    Ok(format!("zoom={percent}%"))
                }
            }
            "undo" => {
                let project = self
                    .active_project_mut()
                    .ok_or_else(|| "no active document".to_owned())?;
                let description = project
                    .history
                    .undo(&mut project.canvas_state)
                    .ok_or_else(|| "nothing to undo".to_owned())?;
                project.is_dirty = true;
                self.canvas.gpu_clear_layers();
                Ok(format!("undid {description}"))
            }
            "redo" => {
                let project = self
                    .active_project_mut()
                    .ok_or_else(|| "no active document".to_owned())?;
                let description = project
                    .history
                    .redo(&mut project.canvas_state)
                    .ok_or_else(|| "nothing to redo".to_owned())?;
                project.is_dirty = true;
                self.canvas.gpu_clear_layers();
                Ok(format!("redid {description}"))
            }
            "save" => {
                self.handle_save(ctx.input(|i| i.time));
                Ok("save requested".to_owned())
            }
            "help" => Ok(crate::ui_control::HELP.to_owned()),
            "" => Err("empty command; use 'help'".to_owned()),
            _ => Err(format!("unknown command '{verb}'; use 'help'")),
        })();

        match result {
            Ok(message) if message.starts_with('{') => message,
            Ok(message) => format!("OK {message}"),
            Err(message) => format!("ERR {message}"),
        }
    }
}

fn parse_dimension(value: &str) -> Result<u32, String> {
    let value: u32 = value
        .parse()
        .map_err(|_| format!("invalid dimension: {value}"))?;
    if !(1..=100_000).contains(&value) {
        return Err("dimensions must be between 1 and 100000".to_owned());
    }
    Ok(value)
}

fn parse_color(value: &str) -> Result<egui::Color32, String> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 && hex.len() != 8 {
        return Err("color must be #RRGGBB or #RRGGBBAA".to_owned());
    }
    let byte = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&hex[range], 16).map_err(|_| "invalid hex color".to_owned())
    };
    Ok(egui::Color32::from_rgba_unmultiplied(
        byte(0..2)?,
        byte(2..4)?,
        byte(4..6)?,
        if hex.len() == 8 { byte(6..8)? } else { 255 },
    ))
}

fn color_hex(color: egui::Color32) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color.r(),
        color.g(),
        color.b(),
        color.a()
    )
}

fn parse_tool(name: &str) -> Result<crate::components::tools::Tool, String> {
    use crate::components::tools::Tool;
    let normalized = name.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    let tool = match normalized.as_str() {
        "brush" => Tool::Brush,
        "eraser" => Tool::Eraser,
        "pencil" => Tool::Pencil,
        "line" => Tool::Line,
        "rectangle-select" | "rect-select" => Tool::RectangleSelect,
        "ellipse-select" => Tool::EllipseSelect,
        "move-pixels" => Tool::MovePixels,
        "move-selection" => Tool::MoveSelection,
        "magic-wand" => Tool::MagicWand,
        "fill" => Tool::Fill,
        "color-picker" => Tool::ColorPicker,
        "gradient" => Tool::Gradient,
        "content-aware" | "content-aware-brush" => Tool::ContentAwareBrush,
        "liquify" => Tool::Liquify,
        "mesh-warp" => Tool::MeshWarp,
        "color-remover" => Tool::ColorRemover,
        "smudge" => Tool::Smudge,
        "clone-stamp" => Tool::CloneStamp,
        "text" => Tool::Text,
        "perspective-crop" => Tool::PerspectiveCrop,
        "lasso" => Tool::Lasso,
        "zoom" => Tool::Zoom,
        "pan" => Tool::Pan,
        "shapes" | "shape" => Tool::Shapes,
        _ => return Err(format!("unknown tool: {name}")),
    };
    Ok(tool)
}

#[cfg(test)]
mod ui_control_tests {
    use super::*;

    #[test]
    fn parses_colors() {
        assert_eq!(parse_color("#12ab34").unwrap(), egui::Color32::from_rgb(0x12, 0xab, 0x34));
        assert!(parse_color("red").is_err());
    }

    #[test]
    fn parses_tool_aliases() {
        assert_eq!(parse_tool("rect_select").unwrap(), crate::components::tools::Tool::RectangleSelect);
        assert_eq!(parse_tool("clone stamp").unwrap(), crate::components::tools::Tool::CloneStamp);
    }
}
