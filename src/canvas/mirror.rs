/// Mirror symmetry mode for the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MirrorMode {
    #[default]
    None,
    /// Left↔Right symmetry (vertical axis at center)
    Horizontal,
    /// Top↔Bottom symmetry (horizontal axis at center)
    Vertical,
    /// 4-way symmetry (both axes)
    Quarters,
}

impl MirrorMode {
    /// Cycle to the next mode.
    pub fn next(self) -> Self {
        match self {
            MirrorMode::None => MirrorMode::Horizontal,
            MirrorMode::Horizontal => MirrorMode::Vertical,
            MirrorMode::Vertical => MirrorMode::Quarters,
            MirrorMode::Quarters => MirrorMode::None,
        }
    }

    pub fn is_active(self) -> bool {
        self != MirrorMode::None
    }

    /// Produce mirrored positions for a given canvas coordinate.
    /// Always includes the original position first.
    /// Uses ArrayVec-style inline storage (no heap allocation).
    pub fn mirror_positions(self, x: f32, y: f32, w: u32, h: u32) -> MirrorPositions {
        let wf = w as f32 - 1.0;
        let hf = h as f32 - 1.0;
        match self {
            MirrorMode::None => MirrorPositions {
                data: [(x, y), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)],
                len: 1,
            },
            MirrorMode::Horizontal => MirrorPositions {
                data: [(x, y), (wf - x, y), (0.0, 0.0), (0.0, 0.0)],
                len: 2,
            },
            MirrorMode::Vertical => MirrorPositions {
                data: [(x, y), (x, hf - y), (0.0, 0.0), (0.0, 0.0)],
                len: 2,
            },
            MirrorMode::Quarters => MirrorPositions {
                data: [(x, y), (wf - x, y), (x, hf - y), (wf - x, hf - y)],
                len: 4,
            },
        }
    }

    /// Produce mirrored positions for integer coordinates.
    pub fn mirror_positions_u32(self, x: u32, y: u32, w: u32, h: u32) -> MirrorPositionsU32 {
        let wx = w.saturating_sub(1).saturating_sub(x);
        let hy = h.saturating_sub(1).saturating_sub(y);
        match self {
            MirrorMode::None => MirrorPositionsU32 {
                data: [(x, y), (0, 0), (0, 0), (0, 0)],
                len: 1,
            },
            MirrorMode::Horizontal => MirrorPositionsU32 {
                data: [(x, y), (wx, y), (0, 0), (0, 0)],
                len: 2,
            },
            MirrorMode::Vertical => MirrorPositionsU32 {
                data: [(x, y), (x, hy), (0, 0), (0, 0)],
                len: 2,
            },
            MirrorMode::Quarters => MirrorPositionsU32 {
                data: [(x, y), (wx, y), (x, hy), (wx, hy)],
                len: 4,
            },
        }
    }
}

/// Inline array of up to 4 mirrored positions (no heap allocation).
pub struct MirrorPositions {
    pub data: [(f32, f32); 4],
    pub len: usize,
}

impl MirrorPositions {
    pub fn iter(&self) -> impl Iterator<Item = &(f32, f32)> {
        self.data[..self.len].iter()
    }
}

/// Inline array of up to 4 mirrored positions (integer coordinates).
pub struct MirrorPositionsU32 {
    pub data: [(u32, u32); 4],
    pub len: usize,
}

impl MirrorPositionsU32 {
    pub fn iter(&self) -> impl Iterator<Item = &(u32, u32)> {
        self.data[..self.len].iter()
    }
}

