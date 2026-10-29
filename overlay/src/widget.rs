//! The widget vocabulary shared by the panel framework and every section module.
//!
//! A section never draws and never handles clicks itself: it emits `Hit`s and
//! commits to nothing else. `layout()` is the single source of truth for both
//! painting and hit-testing, so a section cannot drift out of alignment, and a
//! new section needs no changes to the drawing code.

use windows::Win32::Foundation::RECT;

/// Reserved surface: `Wheel` and the fields below are consumed by the modules
/// that are still being filled in, so they are deliberately not wired yet.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Chip,
    Header,
    /// A section header row: title + chevron, toggles the section open/closed.
    Section,
    Label,
    LabelRight,
    Button,
    Slider,
    Swatch,
    /// A horizontal RGB channel slider: `index` 0/1/2 = R/G/B.
    Channel,
    /// The colour wheel (hue ring); owned by the colours section.
    Wheel,
    Sep,
}

#[derive(Clone)]
pub struct Hit {
    pub r: RECT,
    pub kind: Kind,
    pub id: &'static str,
    pub text: String,
    pub active: bool,
    pub hover: bool,
    /// Extra payload for parameterised widgets (channel index, wheel handle, ...).
    pub data: i32,
}

impl Hit {
    pub fn new(r: RECT, kind: Kind, id: &'static str) -> Self {
        Hit { r, kind, id, text: String::new(), active: false, hover: false, data: 0 }
    }
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = t.into();
        self
    }
    pub fn active(mut self, a: bool) -> Self {
        self.active = a;
        self
    }
    pub fn hover(mut self, h: bool) -> Self {
        self.hover = h;
        self
    }
    pub fn data(mut self, d: i32) -> Self {
        self.data = d;
        self
    }
    pub fn hit(&self, x: i32, y: i32) -> bool {
        x >= self.r.left && x < self.r.right && y >= self.r.top && y < self.r.bottom
    }
}

/// Everything a section may read. Kept as a plain copy so layout stays a pure
/// function of state — the same input always produces the same widgets.
#[allow(dead_code)]
#[derive(Clone)]
pub struct PanelView {
    pub full: bool,
    pub theme: String,
    pub badge: bool,
    pub opacity: f64,
    pub scale: f64,
    pub accent: u32,
    pub permission: u8,
    pub visible: bool,
    pub hover: String,
    pub cur: (f64, f64),
    pub fps: f64,
    pub label: String,
    /// Ids of the sections the user has collapsed.
    pub collapsed: Vec<String>,
    /// Per-slot summary lines for the artwork section.
    pub slots: [SlotView; 3],
    /// Pictures discovered in the asset folder.
    pub assets: Vec<String>,
    /// Colour channels, 0-255.
    pub rgb: [u8; 3],
}

#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct SlotView {
    pub name: String,
    pub source: String,
    pub size: f64,
    pub opacity: f64,
    pub flip: bool,
    pub frames: usize,
    pub error: Option<String>,
}

pub const SWATCHES: [u32; 8] = [
    0x4FC3F7, 0x7C5CFF, 0xFF6B8B, 0x34D399, 0xF5A524, 0xE2E8F0, 0xFF4D4F, 0x22D3EE,
];
pub const SWATCH_IDS: [&str; 8] = ["sw0", "sw1", "sw2", "sw3", "sw4", "sw5", "sw6", "sw7"];

pub fn hex(rgb: u32) -> String {
    format!("#{:06X}", rgb & 0xFFFFFF)
}

pub fn channels_of(rgb: u32) -> [u8; 3] {
    [((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8]
}

#[allow(dead_code)]
pub fn pack_channels(c: [u8; 3]) -> u32 {
    ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32
}
