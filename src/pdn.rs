//! Read-only Paint.NET PDN project import through the isolated compatibility host.

use serde::Deserialize;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use crate::canvas::{BlendMode, CanvasState, Layer, TiledImage};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const MAX_HEADER: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PdnResponse {
    ok: bool,
    error: Option<String>,
    width: u32,
    height: u32,
    layers: Vec<PdnLayer>,
    pixel_length: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PdnLayer {
    name: String,
    visible: bool,
    opacity: u8,
    blend_mode: String,
}

pub fn load_pdn(path: &Path) -> Result<CanvasState, String> {
    let host = crate::paintdotnet_plugins::host_path().map_err(|error| {
        format!(
            "PDN import requires the Paint.NET compatibility host. \
Build or install the host, then try again. ({error})"
        )
    })?;
    let mut command = Command::new(host);
    command
        .arg("--read-pdn")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to start PDN reader: {error}"))?;
    let stdout = child.stdout.take().ok_or("PDN reader stdout unavailable")?;
    let child = Arc::new(Mutex::new(child));
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(read_response(stdout));
    });
    let response = match receiver.recv_timeout(Duration::from_secs(60)) {
        Ok(result) => result,
        Err(_) => {
            if let Ok(mut child) = child.lock() {
                let _ = child.kill();
            }
            Err("PDN import timed out".to_string())
        }
    }?;
    if let Ok(mut child) = child.lock() {
        let _ = child.wait();
    }

    let expected_per_layer = (response.width as usize)
        .checked_mul(response.height as usize)
        .and_then(|size| size.checked_mul(4))
        .ok_or("PDN dimensions overflow")?;
    crate::io::validate_open_dimensions(response.width, response.height)?;
    if response.layers.len() > 256 {
        return Err("PDN project contains more than 256 layers".to_string());
    }
    let expected_total = expected_per_layer
        .checked_mul(response.layers.len())
        .ok_or("PDN layer data size overflow")?;
    if response.pixels.len() != expected_total {
        return Err("PDN reader returned an invalid pixel payload".to_string());
    }

    let mut state = CanvasState::new(response.width, response.height);
    state.layers.clear();
    for (index, metadata) in response.layers.into_iter().enumerate() {
        let start = index * expected_per_layer;
        let image = image::RgbaImage::from_raw(
            response.width,
            response.height,
            response.pixels[start..start + expected_per_layer].to_vec(),
        )
        .ok_or("Invalid PDN layer image")?;
        let mut layer = Layer::new(
            metadata.name,
            response.width,
            response.height,
            image::Rgba([0, 0, 0, 0]),
        );
        layer.pixels = TiledImage::from_rgba_image(&image);
        layer.visible = metadata.visible;
        layer.opacity = metadata.opacity as f32 / 255.0;
        layer.blend_mode = blend_mode(&metadata.blend_mode);
        state.layers.push(layer);
    }
    if state.layers.is_empty() {
        return Err("PDN project contains no layers".to_string());
    }
    state.active_layer_index = state.layers.len() - 1;
    state.composite_cache = None;
    Ok(state)
}

struct DecodedResponse {
    width: u32,
    height: u32,
    layers: Vec<PdnLayer>,
    pixels: Vec<u8>,
}

fn read_response(mut input: impl Read) -> Result<DecodedResponse, String> {
    let mut length = [0; 4];
    input
        .read_exact(&mut length)
        .map_err(|error| error.to_string())?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_HEADER {
        return Err("PDN reader returned an invalid header".to_string());
    }
    let mut header = vec![0; length];
    input
        .read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    let response: PdnResponse =
        serde_json::from_slice(&header).map_err(|error| error.to_string())?;
    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "PDN import failed".to_string()));
    }
    let mut pixels = vec![0; response.pixel_length];
    input
        .read_exact(&mut pixels)
        .map_err(|error| error.to_string())?;
    Ok(DecodedResponse {
        width: response.width,
        height: response.height,
        layers: response.layers,
        pixels,
    })
}

fn blend_mode(name: &str) -> BlendMode {
    match name {
        "Multiply" => BlendMode::Multiply,
        "Additive" => BlendMode::Additive,
        "ColorBurn" => BlendMode::ColorBurn,
        "ColorDodge" => BlendMode::ColorDodge,
        "Reflect" => BlendMode::Reflect,
        "Glow" => BlendMode::Glow,
        "Overlay" => BlendMode::Overlay,
        "Difference" => BlendMode::Difference,
        "Negation" => BlendMode::Negation,
        "Lighten" => BlendMode::Lighten,
        "Darken" => BlendMode::Darken,
        "Screen" => BlendMode::Screen,
        "Xor" => BlendMode::Xor,
        _ => BlendMode::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_net_blend_modes_map_to_paintfe() {
        assert_eq!(blend_mode("Multiply"), BlendMode::Multiply);
        assert_eq!(blend_mode("Additive"), BlendMode::Additive);
        assert_eq!(blend_mode("ColorDodge"), BlendMode::ColorDodge);
        assert_eq!(blend_mode("future-mode"), BlendMode::Normal);
    }

    #[test]
    fn invalid_pdn_is_rejected_by_the_isolated_reader() {
        if crate::paintdotnet_plugins::host_path().is_err() {
            return;
        }
        let path = std::env::temp_dir().join(format!("paintfe-invalid-{}.pdn", std::process::id()));
        std::fs::write(&path, b"not a Paint.NET project").expect("write malformed fixture");
        let error = match load_pdn(&path) {
            Ok(_) => panic!("malformed PDN was accepted"),
            Err(error) => error,
        };
        let _ = std::fs::remove_file(path);
        assert!(
            error.contains("PDN") || error.contains("project"),
            "{error}"
        );
    }

    #[test]
    fn real_pdn_fixture_imports_layers_and_metadata() {
        if crate::paintdotnet_plugins::host_path().is_err() {
            return;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pdn/layers-opacity-additive.pdn");
        let state = load_pdn(&path).expect("fixture imports");

        assert_eq!(state.width, 800);
        assert_eq!(state.height, 600);
        assert_eq!(state.layers.len(), 2);
        assert_eq!(state.layers[0].name, "Background");
        assert!(state.layers[0].visible);
        assert_eq!(state.layers[0].opacity, 1.0);
        assert_eq!(state.layers[0].blend_mode, BlendMode::Normal);
        assert_eq!(state.layers[1].name, "Layer 2");
        assert!(state.layers[1].visible);
        assert!((state.layers[1].opacity - (161.0 / 255.0)).abs() < f32::EPSILON);
        assert_eq!(state.layers[1].blend_mode, BlendMode::Additive);
    }
}
