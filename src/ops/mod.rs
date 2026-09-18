pub mod adjustments;
pub mod ai;
pub mod canvas_ops;
pub mod clipboard;
pub mod color_removal;
pub mod dialogs;
pub mod effect_dialogs;
pub mod effects;
pub mod filters;
#[cfg(target_arch = "wasm32")]
pub mod google_fonts;
pub mod inpaint;
pub mod print;
pub mod scripting;
pub mod shapes;
pub mod text;
pub mod text_layer;
pub mod transform;

/// Open a URL in a new browser tab (web only) — e.g. linking out to the
/// desktop-download page from an in-app prompt.
#[cfg(target_arch = "wasm32")]
pub fn open_url_in_new_tab(url: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.open_with_url_and_target(url, "_blank");
    }
}
