use crate::assets::Assets;
use eframe::egui;
use egui::Color32;

fn swatch_border() -> Color32 {
    Color32::from_rgba_unmultiplied(90, 90, 90, 128)
}

pub struct PalettePanel {
    recent: Vec<Color32>,
    palette: Vec<Color32>,
    selected_index: usize,
}

impl Default for PalettePanel {
    fn default() -> Self {
        Self {
            recent: default_recent_colors(),
            palette: default_palette(),
            selected_index: 0,
        }
    }
}

impl PalettePanel {
    pub fn serialize_recent_colors(&self) -> String {
        self.recent
            .iter()
            .take(6)
            .map(|c| format!("{:02X}{:02X}{:02X}{:02X}", c.r(), c.g(), c.b(), c.a()))
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn load_recent_colors_from_serialized(&mut self, serialized: &str) {
        let mut parsed: Vec<Color32> = serialized
            .split(',')
            .filter_map(|token| {
                let t = token.trim();
                if t.len() != 8 {
                    return None;
                }
                let r = u8::from_str_radix(&t[0..2], 16).ok()?;
                let g = u8::from_str_radix(&t[2..4], 16).ok()?;
                let b = u8::from_str_radix(&t[4..6], 16).ok()?;
                let a = u8::from_str_radix(&t[6..8], 16).ok()?;
                Some(Color32::from_rgba_unmultiplied(r, g, b, a))
            })
            .collect();

        if parsed.is_empty() {
            self.recent = default_recent_colors();
            return;
        }

        parsed.truncate(6);
        self.recent = parsed;
    }

    pub fn observe_color(&mut self, color: Color32) {
        if self.recent.first().is_some_and(|c| *c == color) {
            return;
        }
        self.recent.retain(|c| *c != color);
        self.recent.insert(0, color);
        if self.recent.len() > 6 {
            self.recent.truncate(6);
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        _assets: &Assets,
        primary_color: Color32,
        secondary_color: Color32,
    ) -> Option<(Color32, bool)> {
        let mut action: Option<(Color32, bool)> = None;

        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                draw_recent_grid(ui, &self.recent, 3, 18.0, -1.0, 6, |button, color| {
                    action = Some((color, button == egui::PointerButton::Secondary));
                });
            });

            ui.add_space(3.0);
            ui.separator();
            ui.add_space(3.0);

            self.draw_palette_grid(ui, primary_color, secondary_color, &mut action);
        });

        action
    }

    pub fn save_palette_dialog(&self) {
        self.save_palette_to_file();
    }

    pub fn load_palette_dialog(&mut self) {
        self.load_palette_from_file();
    }

    pub fn reset_palette_default(&mut self) {
        self.palette = default_palette();
        self.selected_index = 0;
    }

    pub fn reset_recent_default(&mut self) {
        self.recent = default_recent_colors();
    }

    fn draw_palette_grid(
        &mut self,
        ui: &mut egui::Ui,
        primary_color: Color32,
        secondary_color: Color32,
        action: &mut Option<(Color32, bool)>,
    ) {
        let columns = 12usize;
        let cell_size = 18.0;
        let spacing = -1.0;

        if self.palette.is_empty() {
            ui.label(egui::RichText::new("(empty)").small().weak());
            return;
        }

        let rows = self.palette.len().div_ceil(columns);
        let width = columns as f32 * cell_size + (columns.saturating_sub(1)) as f32 * spacing;
        let height = rows as f32 * cell_size + (rows.saturating_sub(1)) as f32 * spacing;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

        for (i, swatch) in self.palette.iter_mut().enumerate() {
            let col = i % columns;
            let row = i / columns;
            let min = egui::pos2(
                rect.min.x + col as f32 * (cell_size + spacing),
                rect.min.y + row as f32 * (cell_size + spacing),
            );
            let swatch_rect = egui::Rect::from_min_size(min, egui::vec2(cell_size, cell_size));
            let response = ui.interact(
                swatch_rect,
                ui.id().with(("palette_swatch", "palette", i)),
                egui::Sense::click(),
            );

            ui.painter().rect_filled(swatch_rect, 0.0, *swatch);
            ui.painter().rect_stroke(
                swatch_rect,
                0.0,
                egui::Stroke::new(1.0, swatch_border()),
                egui::StrokeKind::Inside,
            );

            if response.clicked_by(egui::PointerButton::Primary) {
                self.selected_index = i;
                *action = Some((*swatch, false));
            }

            response.context_menu(|ui| {
                if ui.button("Save Primary to Slot").clicked() {
                    *swatch = primary_color;
                    self.selected_index = i;
                    ui.close();
                }
                if ui.button("Save Secondary to Slot").clicked() {
                    *swatch = secondary_color;
                    self.selected_index = i;
                    ui.close();
                }
            });
        }
    }

    fn save_palette_to_file(&self) {
        let mut out = String::new();
        for c in &self.palette {
            out.push_str(&format!(
                "{:02X}{:02X}{:02X}{:02X}\n",
                c.r(),
                c.g(),
                c.b(),
                c.a()
            ));
        }

        #[cfg(target_arch = "wasm32")]
        {
            crate::web_fs::trigger_download("paintfe.pfepalette", out.as_bytes());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Palette", &["pfepalette"])
                .set_file_name("paintfe.pfepalette")
                .save_file()
            else {
                return;
            };
            let _ = std::fs::write(path, out);
        }
    }

    /// Not available on web — no native file picker for palette files.
    #[cfg(target_arch = "wasm32")]
    fn load_palette_from_file(&mut self) {}

    #[cfg(not(target_arch = "wasm32"))]
    fn load_palette_from_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Palette", &["pfepalette"])
            .pick_file()
        else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };

        let mut loaded = Vec::new();
        for line in text.lines() {
            let t = line.trim();
            if t.len() != 8 {
                continue;
            }
            let r = u8::from_str_radix(&t[0..2], 16).ok();
            let g = u8::from_str_radix(&t[2..4], 16).ok();
            let b = u8::from_str_radix(&t[4..6], 16).ok();
            let a = u8::from_str_radix(&t[6..8], 16).ok();
            if let (Some(r), Some(g), Some(b), Some(a)) = (r, g, b, a) {
                loaded.push(Color32::from_rgba_unmultiplied(r, g, b, a));
            }
        }

        if loaded.len() >= 24 {
            loaded.truncate(24);
            self.palette = loaded;
            self.selected_index = 0;
        }
    }
}

fn draw_recent_grid(
    ui: &mut egui::Ui,
    colors: &[Color32],
    columns: usize,
    cell_size: f32,
    spacing: f32,
    max_count: usize,
    mut on_click: impl FnMut(egui::PointerButton, Color32),
) {
    let count = colors.len().min(max_count);
    if count == 0 {
        ui.label(egui::RichText::new("(empty)").small().weak());
        return;
    }

    let rows = count.div_ceil(columns);
    let width = columns as f32 * cell_size + (columns.saturating_sub(1)) as f32 * spacing;
    let height = rows as f32 * cell_size + (rows.saturating_sub(1)) as f32 * spacing;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

    for (i, c) in colors.iter().take(count).enumerate() {
        let col = i % columns;
        let row = i / columns;
        let min = egui::pos2(
            rect.min.x + col as f32 * (cell_size + spacing),
            rect.min.y + row as f32 * (cell_size + spacing),
        );
        let swatch_rect = egui::Rect::from_min_size(min, egui::vec2(cell_size, cell_size));
        let response = ui.interact(
            swatch_rect,
            ui.id().with(("palette_swatch", "recent", i)),
            egui::Sense::click(),
        );

        ui.painter().rect_filled(swatch_rect, 0.0, *c);
        ui.painter().rect_stroke(
            swatch_rect,
            0.0,
            egui::Stroke::new(1.0, swatch_border()),
            egui::StrokeKind::Inside,
        );

        if response.clicked_by(egui::PointerButton::Primary) {
            on_click(egui::PointerButton::Primary, *c);
        }
        if response.clicked_by(egui::PointerButton::Secondary) {
            on_click(egui::PointerButton::Secondary, *c);
        }
    }
}

fn default_palette() -> Vec<Color32> {
    vec![
        // 2 rows x 12 columns. Top row is the main/vibrant sequence,
        // bottom row is darker/lighter companion tones.
        // Top row (12):
        Color32::from_rgb(0, 0, 0),
        Color32::from_rgb(64, 64, 64),
        Color32::from_rgb(255, 0, 0),
        Color32::from_rgb(255, 102, 0),
        Color32::from_rgb(255, 170, 0),
        Color32::from_rgb(255, 255, 0),
        Color32::from_rgb(173, 255, 47),
        Color32::from_rgb(0, 200, 0),
        Color32::from_rgb(0, 200, 200),
        Color32::from_rgb(0, 120, 255),
        Color32::from_rgb(128, 64, 255),
        Color32::from_rgb(255, 0, 200),
        // Bottom row (12):
        Color32::from_rgb(255, 255, 255),
        Color32::from_rgb(160, 160, 160),
        Color32::from_rgb(128, 0, 0),
        Color32::from_rgb(153, 60, 0),
        Color32::from_rgb(153, 85, 0),
        Color32::from_rgb(128, 128, 0),
        Color32::from_rgb(85, 128, 0),
        Color32::from_rgb(0, 128, 0),
        Color32::from_rgb(0, 102, 102),
        Color32::from_rgb(0, 0, 128),
        Color32::from_rgb(75, 0, 130),
        Color32::from_rgb(128, 0, 128),
    ]
}

fn default_recent_colors() -> Vec<Color32> {
    vec![
        Color32::from_rgb(40, 40, 40),
        Color32::from_rgb(70, 70, 70),
        Color32::from_rgb(100, 100, 100),
        Color32::from_rgb(130, 130, 130),
        Color32::from_rgb(165, 165, 165),
        Color32::from_rgb(200, 200, 200),
    ]
}
