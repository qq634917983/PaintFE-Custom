// ---------------------------------------------------------------------------
// Core data types
// ---------------------------------------------------------------------------

/// Complete text layer state — everything needed to re-rasterize.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextLayerData {
    /// Independent text blocks within this layer.
    pub blocks: Vec<TextBlock>,
    /// Layer-level effects applied after rasterization.
    pub effects: TextEffects,
    /// Cache invalidation counter — bumped on every edit.
    #[serde(skip)]
    pub cache_generation: u64,
    /// Generation that was last rasterized into `layer.pixels`.
    #[serde(skip)]
    pub raster_generation: u64,
    /// Counter for generating unique block IDs.
    #[serde(skip)]
    pub next_block_id: u64,
    /// Cached pre-effects RGBA buffer (text only, no effects).
    /// When only effect parameters change, skip text re-rasterization.
    #[serde(skip)]
    pub cached_text_rgba: Vec<u8>,
    /// Generation counter for the text content cache (separate from effects).
    #[serde(skip)]
    pub text_content_generation: u64,
    /// The generation at which `cached_text_rgba` was last computed.
    #[serde(skip)]
    pub cached_text_generation: u64,
    /// Dimensions of the cached text RGBA buffer.
    #[serde(skip)]
    pub cached_text_w: u32,
    #[serde(skip)]
    pub cached_text_h: u32,
    /// Generation counter for block position changes (not content).
    #[serde(skip)]
    pub position_generation: u64,
    /// The position generation at which `cached_text_rgba` was last computed.
    #[serde(skip)]
    pub cached_position_generation: u64,
}

/// Cached rasterization result for a single text block.
/// Stores the tight pixel buffer and the origin used to compute offsets.
/// On position-only change, new offsets are derived without re-rasterizing.
#[derive(Clone, Debug)]
pub struct CachedBlockRaster {
    pub buf: Vec<u8>,
    pub buf_w: u32,
    pub buf_h: u32,
    pub off_x: i32,
    pub off_y: i32,
    pub origin: [f32; 2],
    pub content_generation: u64,
}

/// A single text block (paragraph/text field) within a text layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextBlock {
    pub id: u64,
    /// Position of the text block origin on the canvas (pixels).
    pub position: [f32; 2],
    /// Rotation in radians around the block origin.
    pub rotation: f32,
    /// The rich text content (attributed runs).
    pub runs: Vec<TextRun>,
    /// Paragraph-level properties.
    pub paragraph: ParagraphStyle,
    /// Optional bounding box width for text wrapping (None = no wrap).
    pub max_width: Option<f32>,
    /// Optional bounding box height (None = auto-height from content).
    /// When set, the visual box height is max(content_height, max_height).
    #[serde(default)]
    pub max_height: Option<f32>,
    /// Geometric warp applied to this block (Phase 4 — Batches 7+8).
    #[serde(default)]
    pub warp: TextWarp,
    /// Per-glyph vertex overrides for manual glyph editing (Phase 5 — Batch 9).
    #[serde(default)]
    pub glyph_overrides: Vec<GlyphOverride>,
    /// Cached rasterized buffer for fast position-only updates.
    #[serde(skip)]
    pub cached_raster: Option<CachedBlockRaster>,
}

// ---------------------------------------------------------------------------
// Per-Glyph Vertex Override (Phase 5 — Batch 9)
// ---------------------------------------------------------------------------

/// Per-glyph vertex override for manual glyph-level editing.
/// Stores position offset, rotation, and scale for an individual glyph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlyphOverride {
    /// Index into the laid-out glyph sequence (flat across all runs).
    pub glyph_index: usize,
    /// Position offset in pixels (added to the computed glyph position).
    pub position_offset: [f32; 2],
    /// Per-glyph rotation in radians around the glyph center.
    pub rotation: f32,
    /// Per-glyph uniform scale factor (1.0 = normal).
    pub scale: f32,
}

impl Default for GlyphOverride {
    fn default() -> Self {
        Self {
            glyph_index: 0,
            position_offset: [0.0, 0.0],
            rotation: 0.0,
            scale: 1.0,
        }
    }
}

impl GlyphOverride {
    /// Returns true if this override is effectively identity (no visible change).
    pub fn is_identity(&self) -> bool {
        self.position_offset[0].abs() < 0.001
            && self.position_offset[1].abs() < 0.001
            && self.rotation.abs() < 0.001
            && (self.scale - 1.0).abs() < 0.001
    }
}

/// A contiguous run of text with uniform formatting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub style: TextStyle,
}

/// Character-level formatting attributes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextStyle {
    pub font_family: String,
    pub font_weight: u16,
    pub font_size: f32,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub color: [u8; 4],
    pub letter_spacing: f32,
    pub baseline_offset: f32,
    pub width_scale: f32,
    pub height_scale: f32,
}

/// Paragraph-level formatting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParagraphStyle {
    pub alignment: TextAlignment,
    pub line_spacing: f32,
    pub indent: f32,
}

/// Text alignment for paragraphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

// ---------------------------------------------------------------------------
// Geometric Warps (Phase 4 — Batches 7+8)
// ---------------------------------------------------------------------------

/// Geometric warp for a text block.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum TextWarp {
    #[default]
    None,
    /// Text bent along an arc (cup/bulge).
    Arc(ArcWarp),
    /// Text arranged in a circle.
    Circular(CircularWarp),
    /// Text follows a Bézier curve path.
    PathFollow(PathFollowWarp),
    /// Text warped via an envelope (top + bottom curves).
    Envelope(EnvelopeWarp),
}

impl TextWarp {
    /// Returns a display name for the warp type.
    pub fn name(&self) -> &'static str {
        match self {
            TextWarp::None => "None",
            TextWarp::Arc(_) => "Arc",
            TextWarp::Circular(_) => "Circular",
            TextWarp::PathFollow(_) => "Path Follow",
            TextWarp::Envelope(_) => "Envelope",
        }
    }

    /// Returns a list of all warp type names (for UI dropdown).
    pub fn all_names() -> &'static [&'static str] {
        &["None", "Arc", "Circular", "Path Follow", "Envelope"]
    }

    /// Create a warp variant from its name string.
    pub fn from_name(name: &str) -> Self {
        match name {
            "Arc" => TextWarp::Arc(ArcWarp::default()),
            "Circular" => TextWarp::Circular(CircularWarp::default()),
            "Path Follow" => TextWarp::PathFollow(PathFollowWarp::default()),
            "Envelope" => TextWarp::Envelope(EnvelopeWarp::default()),
            _ => TextWarp::None,
        }
    }
}

impl PartialEq for TextWarp {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// Arc warp: bends text along an arc.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArcWarp {
    /// Bend amount: -1.0 = concave, 0.0 = flat, 1.0 = convex.
    pub bend: f32,
    /// Horizontal distortion (-1..1).
    pub horizontal_distortion: f32,
    /// Vertical distortion (-1..1).
    pub vertical_distortion: f32,
}

impl Default for ArcWarp {
    fn default() -> Self {
        Self {
            bend: 0.5,
            horizontal_distortion: 0.0,
            vertical_distortion: 0.0,
        }
    }
}

/// Circular warp: arranges text around a circle.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CircularWarp {
    /// Radius of the circle baseline (pixels).
    pub radius: f32,
    /// Start angle (radians).
    pub start_angle: f32,
    /// Whether text goes clockwise.
    pub clockwise: bool,
}

impl Default for CircularWarp {
    fn default() -> Self {
        Self {
            radius: 150.0,
            start_angle: -std::f32::consts::FRAC_PI_2,
            clockwise: true,
        }
    }
}

/// Path follow warp: text follows a Bézier curve.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PathFollowWarp {
    /// Cubic Bézier control points defining the baseline path.
    /// Groups of 4: [start, cp1, cp2, end] per segment.
    pub control_points: Vec<[f32; 2]>,
}

impl Default for PathFollowWarp {
    fn default() -> Self {
        // Default: a simple horizontal path with slight curve
        Self {
            control_points: vec![[0.0, 0.0], [100.0, -50.0], [200.0, -50.0], [300.0, 0.0]],
        }
    }
}

/// Envelope warp: deforms text between two Bézier curves.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvelopeWarp {
    /// Top boundary curve control points (cubic Bézier, groups of 4).
    pub top_curve: Vec<[f32; 2]>,
    /// Bottom boundary curve control points (cubic Bézier, groups of 4).
    pub bottom_curve: Vec<[f32; 2]>,
}

impl Default for EnvelopeWarp {
    fn default() -> Self {
        Self {
            top_curve: vec![[0.0, -30.0], [100.0, -60.0], [200.0, -60.0], [300.0, -30.0]],
            bottom_curve: vec![[0.0, 30.0], [100.0, 60.0], [200.0, 60.0], [300.0, 30.0]],
        }
    }
}

/// Layer-level text effects (Batch 5+6).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TextEffects {
    pub outline: Option<OutlineEffect>,
    pub shadow: Option<ShadowEffect>,
    pub inner_shadow: Option<InnerShadowEffect>,
    pub gradient_fill: Option<GradientFillEffect>,
    pub texture_fill: Option<TextureFillEffect>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutlineEffect {
    pub color: [u8; 4],
    pub width: f32,
    pub position: OutlinePosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutlinePosition {
    Inside,
    Outside,
    Center,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShadowEffect {
    pub color: [u8; 4],
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub spread: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InnerShadowEffect {
    pub color: [u8; 4],
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
}

/// Texture fill effect: fills text glyphs with a tiled image pattern.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextureFillEffect {
    /// Embedded texture image data (PNG bytes).
    pub texture_data: Vec<u8>,
    /// Decoded texture dimensions (cached on load).
    pub texture_width: u32,
    pub texture_height: u32,
    /// Scale factor for texture tiling (1.0 = 1:1).
    pub scale: f32,
    /// Offset for texture tiling origin.
    pub offset: [f32; 2],
}

/// Gradient fill effect: fills text glyphs with a configurable linear gradient.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GradientFillEffect {
    pub start_color: [u8; 4],
    pub end_color: [u8; 4],
    /// Gradient angle in degrees.
    pub angle_degrees: f32,
    /// Distance in pixels from start to end color.
    pub scale: f32,
    /// Translation of the gradient field in canvas pixels.
    pub offset: [f32; 2],
    pub repeat: bool,
}

impl Default for OutlineEffect {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
            width: 2.0,
            position: OutlinePosition::Outside,
        }
    }
}

impl Default for ShadowEffect {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 180],
            offset_x: 4.0,
            offset_y: 4.0,
            blur_radius: 5.0,
            spread: 0.0,
        }
    }
}

impl Default for InnerShadowEffect {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 128],
            offset_x: 2.0,
            offset_y: 2.0,
            blur_radius: 3.0,
        }
    }
}

impl Default for TextureFillEffect {
    fn default() -> Self {
        Self {
            texture_data: Vec::new(),
            texture_width: 0,
            texture_height: 0,
            scale: 1.0,
            offset: [0.0, 0.0],
        }
    }
}

impl Default for GradientFillEffect {
    fn default() -> Self {
        Self {
            start_color: [255, 255, 255, 255],
            end_color: [0, 0, 0, 255],
            angle_degrees: 0.0,
            scale: 200.0,
            offset: [0.0, 0.0],
            repeat: false,
        }
    }
}

impl TextEffects {
    /// Returns true if any effect is enabled.
    pub fn has_any(&self) -> bool {
        self.outline.is_some()
            || self.shadow.is_some()
            || self.inner_shadow.is_some()
            || self.gradient_fill.is_some()
            || self.texture_fill.is_some()
    }
}

// ---------------------------------------------------------------------------
// Text Selection Model (Batch 3)
// ---------------------------------------------------------------------------

/// A position within a block's run-based text model.
/// `run_index` identifies which run, `byte_offset` identifies where within that run's text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunPosition {
    pub run_index: usize,
    pub byte_offset: usize,
}

/// Selection within a single text block. Anchor is where the selection started,
/// cursor is where it currently is. If anchor == cursor, there is no selection
/// (just a caret).
#[derive(Clone, Copy, Debug, Default)]
pub struct TextSelection {
    pub anchor: RunPosition,
    pub cursor: RunPosition,
}

/// Per-run layout result — bounding info for hit-testing and cursor positioning.
#[derive(Clone, Debug)]
pub struct RunLayoutInfo {
    /// Per-character cumulative x-advance from the line start (after alignment offset).
    /// Length = number of chars + 1 (entry 0 = 0.0).
    pub char_advances: Vec<f32>,
    /// Baseline y offset from block origin for this line segment.
    pub baseline_y: f32,
    /// Line height for this run's font/size.
    pub line_height: f32,
    /// Ascent for this run's font/size.
    pub ascent: f32,
}

/// Result of multi-run layout computation.
#[derive(Clone, Debug)]
pub struct BlockLayout {
    /// Per-run layout metrics.
    pub runs: Vec<RunLayoutInfo>,
    /// Per-line info: (line_start_run_idx, line_widths, max_ascent, max_line_height)
    pub lines: Vec<LineInfo>,
    /// Total bounding width of the block.
    pub total_width: f32,
    /// Total bounding height of the block.
    pub total_height: f32,
}

/// Info about a single line in the block layout.
#[derive(Clone, Debug)]
pub struct LineInfo {
    pub width: f32,
    pub max_ascent: f32,
    pub max_line_height: f32,
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: crate::ops::text::preferred_default_font_family(),
            font_weight: 400,
            font_size: 48.0,
            italic: false,
            underline: false,
            strikethrough: false,
            color: [255, 255, 255, 255],
            letter_spacing: 0.0,
            baseline_offset: 0.0,
            width_scale: 1.0,
            height_scale: 1.0,
        }
    }
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            alignment: TextAlignment::Left,
            line_spacing: 1.0,
            indent: 0.0,
        }
    }
}

impl Default for TextLayerData {
    fn default() -> Self {
        Self {
            blocks: vec![TextBlock {
                id: 1,
                position: [100.0, 100.0],
                rotation: 0.0,
                runs: vec![TextRun {
                    text: String::new(),
                    style: TextStyle::default(),
                }],
                paragraph: ParagraphStyle::default(),
                max_width: None,
                max_height: None,
                warp: TextWarp::None,
                glyph_overrides: Vec::new(),
                cached_raster: None,
            }],
            effects: TextEffects::default(),
            cache_generation: 1,
            raster_generation: 0,
            next_block_id: 2,
            cached_text_rgba: Vec::new(),
            text_content_generation: 0,
            cached_text_generation: 0,
            cached_text_w: 0,
            cached_text_h: 0,
            position_generation: 0,
            cached_position_generation: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Rasterization
// ---------------------------------------------------------------------------

