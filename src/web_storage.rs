// Thin wrapper over `localStorage` (wasm32 only) for small persisted flags
// that need to survive across page reloads, e.g. "has this browser already
// seen the first-run welcome popup?" There's no real filesystem to write a
// settings file to on web, so this is the only persistence available.

const WELCOME_SEEN_KEY: &str = "paintfe_welcome_seen_v1";
const SETTINGS_KEY: &str = "paintfe_settings_v1";

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Has this browser already dismissed the first-run welcome popup?
pub fn has_seen_welcome() -> bool {
    local_storage()
        .and_then(|s| s.get_item(WELCOME_SEEN_KEY).ok().flatten())
        .is_some()
}

/// Record that the welcome popup was dismissed, so it never shows again on
/// this browser.
pub fn mark_welcome_seen() {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(WELCOME_SEEN_KEY, "1");
    }
}

pub fn load_settings() -> Option<String> {
    local_storage()?.get_item(SETTINGS_KEY).ok().flatten()
}

pub fn save_settings(settings: &str) -> Result<(), String> {
    let storage = local_storage().ok_or_else(|| "browser storage is unavailable".to_string())?;
    storage
        .set_item(SETTINGS_KEY, settings)
        .map_err(|_| "browser storage rejected the settings update".to_string())
}

/// Best-effort phone/tablet detection via the user-agent string. Used only
/// to show a heads-up that PaintFE - Web is a desktop-oriented experience;
/// nothing is blocked based on this, it's purely informational.
pub fn is_mobile_device() -> bool {
    let Some(ua) = web_sys::window().and_then(|w| w.navigator().user_agent().ok()) else {
        return false;
    };
    let ua = ua.to_lowercase();
    ["mobi", "android", "iphone", "ipad", "ipod"]
        .iter()
        .any(|needle| ua.contains(needle))
}
