// =============================================================================
// Integration tests — Selection masks
// =============================================================================
//
// Tests selection shape generation, boolean operations (add/subtract/intersect),
// translate, fill, delete, and clear on CanvasState.

mod common;

use image::Rgba;
use paintfe::canvas::{CanvasState, SelectionMode, SelectionShape};

// =============================================================================
// Rectangle selection
// =============================================================================

#[test]
fn rect_selection_replace() {
    let mut state = CanvasState::new(100, 100);
    let shape = SelectionShape::Rectangle {
        min_x: 10,
        min_y: 10,
        max_x: 50,
        max_y: 50,
    };
    state.apply_selection_shape(&shape, SelectionMode::Replace);

    assert!(state.has_selection());
    let mask = state.selection_mask.as_ref().unwrap();
    assert_eq!(mask.get_pixel(20, 20).0[0], 255, "inside rect");
    assert_eq!(mask.get_pixel(0, 0).0[0], 0, "outside rect");
    assert_eq!(mask.get_pixel(10, 10).0[0], 255, "on edge (inclusive)");
    assert_eq!(mask.get_pixel(51, 20).0[0], 0, "just outside right edge");
}

#[test]
fn ellipse_selection_replace() {
    let mut state = CanvasState::new(100, 100);
    let shape = SelectionShape::Ellipse {
        cx: 50.0,
        cy: 50.0,
        rx: 20.0,
        ry: 20.0,
    };
    state.apply_selection_shape(&shape, SelectionMode::Replace);

    assert!(state.has_selection());
    let mask = state.selection_mask.as_ref().unwrap();
    assert_eq!(mask.get_pixel(50, 50).0[0], 255, "center of ellipse");
    assert_eq!(mask.get_pixel(0, 0).0[0], 0, "far corner");
}

// =============================================================================
// Boolean operations
// =============================================================================

#[test]
fn selection_add() {
    let mut state = CanvasState::new(100, 100);

    // First rectangle
    let r1 = SelectionShape::Rectangle {
        min_x: 0,
        min_y: 0,
        max_x: 30,
        max_y: 30,
    };
    state.apply_selection_shape(&r1, SelectionMode::Replace);

    // Add second rectangle
    let r2 = SelectionShape::Rectangle {
        min_x: 70,
        min_y: 70,
        max_x: 99,
        max_y: 99,
    };
    state.apply_selection_shape(&r2, SelectionMode::Add);

    let mask = state.selection_mask.as_ref().unwrap();
    assert_eq!(mask.get_pixel(15, 15).0[0], 255, "first rect selected");
    assert_eq!(mask.get_pixel(85, 85).0[0], 255, "second rect selected");
    assert_eq!(mask.get_pixel(50, 50).0[0], 0, "gap between rects");
}

#[test]
fn selection_subtract() {
    let mut state = CanvasState::new(100, 100);

    // Full rect
    let full = SelectionShape::Rectangle {
        min_x: 0,
        min_y: 0,
        max_x: 99,
        max_y: 99,
    };
    state.apply_selection_shape(&full, SelectionMode::Replace);

    // Subtract center
    let center = SelectionShape::Rectangle {
        min_x: 30,
        min_y: 30,
        max_x: 70,
        max_y: 70,
    };
    state.apply_selection_shape(&center, SelectionMode::Subtract);

    let mask = state.selection_mask.as_ref().unwrap();
    assert_eq!(mask.get_pixel(50, 50).0[0], 0, "center subtracted");
    assert_eq!(mask.get_pixel(10, 10).0[0], 255, "corner remains");
}

#[test]
fn selection_intersect() {
    let mut state = CanvasState::new(100, 100);

    let r1 = SelectionShape::Rectangle {
        min_x: 0,
        min_y: 0,
        max_x: 60,
        max_y: 60,
    };
    state.apply_selection_shape(&r1, SelectionMode::Replace);

    let r2 = SelectionShape::Rectangle {
        min_x: 40,
        min_y: 40,
        max_x: 99,
        max_y: 99,
    };
    state.apply_selection_shape(&r2, SelectionMode::Intersect);

    let mask = state.selection_mask.as_ref().unwrap();
    assert_eq!(mask.get_pixel(50, 50).0[0], 255, "overlap region");
    assert_eq!(mask.get_pixel(10, 10).0[0], 0, "only in first rect");
    assert_eq!(mask.get_pixel(90, 90).0[0], 0, "only in second rect");
}

// =============================================================================
// Clear selection
// =============================================================================

#[test]
fn clear_selection_removes_mask() {
    let mut state = CanvasState::new(100, 100);
    let shape = SelectionShape::Rectangle {
        min_x: 10,
        min_y: 10,
        max_x: 50,
        max_y: 50,
    };
    state.apply_selection_shape(&shape, SelectionMode::Replace);
    assert!(state.has_selection());

    state.clear_selection();
    assert!(!state.has_selection());
}

// =============================================================================
// Translate selection
// =============================================================================

#[test]
fn translate_selection_moves_mask() {
    let mut state = CanvasState::new(100, 100);
    let shape = SelectionShape::Rectangle {
        min_x: 10,
        min_y: 10,
        max_x: 30,
        max_y: 30,
    };
    state.apply_selection_shape(&shape, SelectionMode::Replace);

    state.translate_selection(20, 20);

    let mask = state.selection_mask.as_ref().unwrap();
    // Original position should be clear, new position selected
    assert_eq!(mask.get_pixel(15, 15).0[0], 0, "old position cleared");
    assert_eq!(mask.get_pixel(35, 35).0[0], 255, "new position selected");
}

#[test]
fn translate_selection_clips_at_edge() {
    let mut state = CanvasState::new(100, 100);
    let shape = SelectionShape::Rectangle {
        min_x: 80,
        min_y: 80,
        max_x: 99,
        max_y: 99,
    };
    state.apply_selection_shape(&shape, SelectionMode::Replace);

    // Move right/down so part goes off-canvas
    state.translate_selection(10, 10);

    let mask = state.selection_mask.as_ref().unwrap();
    assert_eq!(mask.get_pixel(95, 95).0[0], 255, "in-bounds portion");
    // Original position should be clear
    assert_eq!(mask.get_pixel(85, 85).0[0], 0, "old position");
}

// =============================================================================
// Fill & delete selected pixels
// =============================================================================

#[test]
fn fill_selected_pixels_fills_area() {
    let mut state = CanvasState::new(100, 100);
    let shape = SelectionShape::Rectangle {
        min_x: 10,
        min_y: 10,
        max_x: 50,
        max_y: 50,
    };
    state.apply_selection_shape(&shape, SelectionMode::Replace);

    // Background is white — fill selection with red
    state.fill_selected_pixels(Rgba([255, 0, 0, 255]));

    let px_inside = *state.layers[0].pixels.get_pixel(20, 20);
    let px_outside = *state.layers[0].pixels.get_pixel(0, 0);
    assert_eq!(px_inside, Rgba([255, 0, 0, 255]), "filled area is red");
    assert_eq!(px_outside, Rgba([255, 255, 255, 255]), "outside unchanged");
}

#[test]
fn delete_selected_pixels_makes_transparent() {
    let mut state = CanvasState::new(100, 100);
    let shape = SelectionShape::Rectangle {
        min_x: 10,
        min_y: 10,
        max_x: 50,
        max_y: 50,
    };
    state.apply_selection_shape(&shape, SelectionMode::Replace);

    state.delete_selected_pixels();

    let px_inside = *state.layers[0].pixels.get_pixel(20, 20);
    let px_outside = *state.layers[0].pixels.get_pixel(0, 0);
    assert_eq!(px_inside[3], 0, "deleted area is transparent");
    assert_eq!(px_outside[3], 255, "outside unchanged");
}

// =============================================================================
// SelectionShape::contains
// =============================================================================

#[test]
fn selection_shape_contains_rect() {
    let shape = SelectionShape::Rectangle {
        min_x: 10,
        min_y: 20,
        max_x: 30,
        max_y: 40,
    };
    assert_eq!(shape.contains(15, 25), 255);
    assert_eq!(shape.contains(0, 0), 0);
    assert_eq!(shape.contains(10, 20), 255, "edges inclusive");
    assert_eq!(shape.contains(30, 40), 255, "max edges inclusive");
    assert_eq!(shape.contains(31, 25), 0, "past max_x");
}

#[test]
fn selection_shape_contains_ellipse() {
    let shape = SelectionShape::Ellipse {
        cx: 50.0,
        cy: 50.0,
        rx: 10.0,
        ry: 10.0,
    };
    assert_eq!(shape.contains(50, 50), 255, "center");
    assert_eq!(shape.contains(0, 0), 0, "far away");
}
