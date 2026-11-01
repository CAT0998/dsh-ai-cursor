//! The widget vocabulary shared by the panel framework and every section module.
//!
//! A section never draws and never handles clicks itself: it emits `Hit`s and
//! commits to nothing else. `layout()` is the single source of truth for both
//! painting and hit-testing, so a section cannot drift out of alignment, and a
//! new section needs no changes to the drawing code.

use windows::Win32::Foundation::RECT;

/// The header strip (title + collapse/close + drag zone) is never scrolled: it is
/// the fixed chrome above the scrollable content. Everything else lives below it.
pub const HEADER_BOTTOM: i32 = 41;

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
    /// A live thumbnail of a picture slot (animates with the sprite).
    Preview,
    /// The minimap box: screen outline + window rects + the cursor dot.
    Minimap,
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

/// One AI cursor's editable look, as the panel shows it.
///
/// This is the shape of a *mouse preset* too: what the ‹ › selector displays and
/// what a named save stores. When the selected cursor is another process the panel
/// keeps this as a mirror and forwards the edit; when it is this process the same
/// struct is written straight into `St`.
#[derive(Clone, Default)]
pub struct CursorSettings {
    pub theme: String,
    pub label: String,
    pub accent: u32,
    pub opacity: f64,
    pub scale: f64,
    pub badge: bool,
}

impl CursorSettings {
    /// The look a fresh cursor starts with; `Default` alone would be opacity 0.
    pub fn initial() -> CursorSettings {
        CursorSettings {
            theme: "whale".to_string(),
            label: "AI 鼠标".to_string(),
            accent: 0x4FC3F7,
            opacity: 1.0,
            scale: 1.0,
            badge: true,
        }
    }
}

/// Everything a section may read. Kept as a plain copy so layout stays a pure
/// function of state — the same input always produces the same widgets.
#[allow(dead_code)]
#[derive(Clone)]
pub struct PanelView {    pub full: bool,
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
    /// The reserved custom section's current contents.
    pub custom: CustomView,
    /// Milliseconds into the sprite timeline; drives thumbnail animation.
    pub epoch_ms: u64,
    /// Which artwork slot the editor block below the rows is editing.
    pub art_sel: usize,
    /// Window width in device px. The CONTENT is always authored at `PW` (452);
    /// a wider window simply leaves the right-hand margin empty, a narrower one
    /// scrolls horizontally. Nothing is ever stretched.
    pub panel_w: i32,
    /// Window height in device px (it is also the content viewport height).
    pub panel_h: i32,
    /// Width of the content viewport: `min(panel_w, PW)`.
    pub view_w: i32,
    /// Horizontal scroll offset in design px, 0..=(PW - view_w).
    pub hscroll: i32,
    /// Selected cursors, one column header each.
    pub columns: Vec<(String, u32)>,
    pub columns_collapsed: bool,
    /// Names from presets.json, read by the overlay so the panel can list them.
    pub presets: Vec<String>,
    /// Which preset entry is highlighted (0-based), if any.
    pub preset_sel: usize,
    /// The human may drag the cursor with their own pointer.
    pub drag_enabled: bool,
    pub dragging: bool,
    /// Minimap: zoom (0.5..2), opacity (0.2..1), visible.
    pub map_zoom: f64,
    pub map_opacity: f64,
    pub map_visible: bool,
    pub map_window: bool,
    /// Scroll offset in design px. The sections are laid out from `48 - scroll`,
    /// so one offset moves the paint AND every hit box together.
    pub scroll: i32,
    /// Design height the whole section stack needs (may exceed `view_h`).
    pub content_h: i32,
    /// Height of the visible viewport (the window's client height). The content is
    /// capped by the monitor's work area when the height is automatic, so the last
    /// section can never hide under the taskbar again.
    pub view_h: i32,
    /// The PANEL's own accent. Menu themes drive this one only; the cursor keeps
    /// using `accent`. Two colours is what makes "菜单主题" and "AI 鼠标设置" truly
    /// separate rather than one control doing both.
    pub panel_accent: u32,
    /// Menu theme preset names (read-only, from `menu-themes.json`).
    pub menu_themes: Vec<String>,
    /// Which menu theme is highlighted in the list.
    pub menu_theme_sel: usize,
    /// Mouse preset names (read-only, from `cursor-presets.json`).
    pub mouse_presets: Vec<String>,
    /// Which mouse preset is highlighted in the list.
    pub mouse_preset_sel: usize,
    /// The cursor the ‹ › arrows currently select, and how many exist.
    pub sel_name: String,
    pub sel_index: usize,
    pub sel_total: usize,
    /// The selected cursor's editable look: the mouse section reads THIS, not the
    /// local `St`, because the selected cursor may be a different process.
    pub sel_settings: CursorSettings,
}

impl PanelView {
    /// Widest horizontal scroll offset, in design px.
    ///
    /// Zero unless the window is narrower than the content's design width, and that
    /// is the whole rule: the panel only ever scrolls sideways because the WINDOW is
    /// too narrow for a 452-wide column, never because the content overflowed.
    pub fn max_hscroll(&self) -> i32 {
        (crate::PW - self.view_w.max(0)).max(0)
    }
}

#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct CustomView {
    /// Overrides the section header when the JSON supplies one.
    pub title: Option<String>,
    pub rows: Vec<CustomRowView>,
    /// JSON/IO problem, rendered instead of silently showing nothing.
    pub error: Option<String>,
    /// What to show while the slot is still empty.
    pub hint: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone)]
pub enum CustomRowView {
    Label(String),
    Button(String),
}

/// Widget ids for the custom section, indexed by row. Fixed strings so a custom
/// row costs nothing at draw time; the row itself is looked up by index.
pub const CUSTOM_IDS: [&str; 12] = [
    "cus0", "cus1", "cus2", "cus3", "cus4", "cus5",
    "cus6", "cus7", "cus8", "cus9", "cus10", "cus11",
];

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
    pub offset: (f64, f64),
    pub rot: f64,
}

pub const SWATCHES: [u32; 8] = [
    0x4FC3F7, 0x7C5CFF, 0xFF6B8B, 0x34D399, 0xF5A524, 0xE2E8F0, 0xFF4D4F, 0x22D3EE,
];
pub const SWATCH_IDS: [&str; 8] = ["sw0", "sw1", "sw2", "sw3", "sw4", "sw5", "sw6", "sw7"];
/// The same palette for the menu bar's OWN accent. Separate ids because these two
/// choose different things: `sw*` = the cursor colour, `mt*` = the panel colour.
pub const MT_SWATCH_IDS: [&str; 8] = ["mt0", "mt1", "mt2", "mt3", "mt4", "mt5", "mt6", "mt7"];

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
