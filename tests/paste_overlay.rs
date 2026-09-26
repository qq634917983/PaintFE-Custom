use image::{Rgba, RgbaImage};
use paintfe::canvas::CanvasState;
use paintfe::ops::clipboard::PasteOverlay;

#[test]
fn horizontal_flip_is_applied_when_overlay_commits() {
    let source = RgbaImage::from_fn(2, 1, |x, _| {
        if x == 0 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 0, 255, 255])
        }
    });
    let mut overlay = PasteOverlay::new(source, 2, 1);
    overlay.anti_aliasing = false;
    overlay.toggle_flip_horizontal();

    let mut state = CanvasState::new(2, 1);
    overlay.commit(&mut state);

    assert_eq!(state.layers[0].pixels.get_pixel(0, 0).0, [0, 0, 255, 255]);
    assert_eq!(state.layers[0].pixels.get_pixel(1, 0).0, [255, 0, 0, 255]);
}

#[test]
fn vertical_flip_round_trips_through_transform_history() {
    let source = RgbaImage::new(1, 2);
    let mut overlay = PasteOverlay::new(source, 1, 2);
    let before = overlay.transform();

    overlay.toggle_flip_vertical();
    assert!(overlay.transform().flip_vertical);

    overlay.set_transform(before);
    assert!(!overlay.transform().flip_vertical);
}
