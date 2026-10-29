//! ai-mouse-overlay — click-through "AI cursor" + floating control panel.
//!
//! Two layered Win32 windows, one process, one line protocol:
//!
//!   AiMouseOverlay : the agent's own cursor. Click-through, never takes focus,
//!                    excluded from screen capture so the agent's screenshots stay clean.
//!   AiMousePanel   : a floating, collapsible control bar the human can actually click.
//!                    Draws its own widgets with GDI and hit-tests them itself.
//!
//! Protocol: one JSON object per line on stdin, one JSON response per line on stdout.
//!   {"id":"1","cmd":"show"}                       {"id":"2","cmd":"move","x":100,"y":200}
//!   {"id":"3","cmd":"panel","visible":true}       {"id":"4","cmd":"opacity","value":0.7}
//!   {"id":"5","cmd":"color","hex":"#4FC3F7"}      {"id":"6","cmd":"permission","level":2}
//!   {"id":"7","cmd":"status"}                     {"cmd":"quit"}
//!
//! Nothing here moves the real mouse pointer, and nothing here takes keyboard focus:
//! the panel is WS_EX_NOACTIVATE, so clicking a button never steals the foreground.

mod custom;
mod sections;
mod wheel;
mod sprite;
mod widget;

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use std::path::Path;

use sprite::SpriteCache;
use widget::{channels_of, hex, Hit, Kind, PanelView, SlotView, SWATCHES};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush, DeleteDC,
    DeleteObject, Ellipse, FillRect, GetDC, GetStockObject, GetTextExtentPoint32W, LineTo, MoveToEx,
    Polygon, ReleaseDC, RoundRect, SelectObject, SetBkMode, SetTextColor, TextOutW, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_USAGE, HDC, HGDIOBJ,
    NULL_PEN, PS_SOLID, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetSystemMetrics, HMENU,
    PeekMessageW, RegisterClassExW, SetWindowDisplayAffinity, SetWindowPos, ShowWindow,
    TranslateMessage, UpdateLayeredWindow, HWND_TOPMOST, MA_NOACTIVATE, MSG, PM_REMOVE,
    SET_WINDOW_POS_FLAGS, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, SYSTEM_METRICS_INDEX, ULW_ALPHA,
    WDA_EXCLUDEFROMCAPTURE, WDA_NONE, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_QUIT, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

// ---------------------------------------------------------------- geometry ---

const W: i32 = 360; // cursor window width  (device px)
const H: i32 = 240; // cursor window height (device px)
const SS: i32 = 4; // supersample factor for the cursor artwork
const TIP_X: f64 = 200.0; // where the cursor tip sits inside its window
const TIP_Y: f64 = 44.0;

const PW: i32 = 452; // expanded panel width
const PH: i32 = 396; // expanded panel height
const CHIP_W: i32 = 132; // collapsed panel width
const CHIP_H: i32 = 40; // collapsed panel height
const SS_PANEL: i32 = 2; // supersample factor for the panel
const PH_MAX: i32 = 900; // the panel canvas is allocated this tall and clipped per frame

// Palette. Accent-dependent colours are derived from the live accent value.
const C_PANEL: u32 = 0x0F141C;
const C_PANEL_2: u32 = 0x151C26;
const C_BORDER: u32 = 0x2C3A4C;
const C_SEP: u32 = 0x243040;
const C_BTN: u32 = 0x1B2432;
const C_BTN_HOVER: u32 = 0x27354A;
const C_TEXT: u32 = 0xE8F0F8;
const C_MUTED: u32 = 0x8C9AAA;
const C_TRACK: u32 = 0x27323F;
const C_DARK_TEXT: u32 = 0x0A121A;

fn col(rgb: u32) -> COLORREF {
    // COLORREF is 0x00BBGGRR while callers pass 0xRRGGBB: swap the outer bytes.
    COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF))
}

/// `f` < 1 darkens toward black, `f` > 1 lightens toward white.
fn shade(rgb: u32, f: f64) -> u32 {
    let ch = |v: u32| -> u32 {
        let x = v as f64;
        let y = if f <= 1.0 { x * f } else { x + (255.0 - x) * (f - 1.0) };
        y.clamp(0.0, 255.0) as u32
    };
    (ch((rgb >> 16) & 0xFF) << 16) | (ch((rgb >> 8) & 0xFF) << 8) | ch(rgb & 0xFF)
}

/// Linear blend between two 0xRRGGBB colours.
fn blend(a: u32, b: u32, t: f64) -> u32 {
    let ch = |sh: u32| -> u32 {
        let x = ((a >> sh) & 0xFF) as f64;
        let y = ((b >> sh) & 0xFF) as f64;
        (x + (y - x) * t).round().clamp(0.0, 255.0) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}

fn null_hwnd() -> HWND {
    HWND(std::ptr::null_mut())
}

fn hwnd_of(v: isize) -> HWND {
    HWND(v as *mut core::ffi::c_void)
}

// ------------------------------------------------------------------- state ---

#[derive(Clone)]
struct St {
    // --- cursor window ---
    hwnd: isize,
    cur: (f64, f64),
    from: (f64, f64),
    target: (f64, f64),
    t0: Instant,
    dur_ms: f64,
    dirty: bool,
    visible: bool,
    label: String,
    label_wanted: bool,
    theme: String,
    badge: bool,
    opacity: f64,
    accent: u32,
    scale: f64,
    idle_ms: u64,
    last_activity: Instant,
    frames: u64,
    frame_window_start: Instant,
    fps: f64,
    quit: bool,
    want_show: bool,
    want_hide: bool,
    capture_visible: bool,
    want_affinity: Option<bool>,

    // --- panel window ---
    panel_hwnd: isize,
    panel_shown: bool,
    panel_full: bool,
    panel_pos: (i32, i32),
    panel_dirty: bool,
    want_panel_show: bool,
    want_panel_hide: bool,
    want_panel_resize: bool,
    want_panel_move: bool,
    hover: String,
    /// What the panel footer last rendered, so it can be refreshed when the cursor
    /// moves on its own (the panel is otherwise only repainted on widget changes).
    drawn_visible: bool,
    drawn_pos: (i32, i32),
    footer_tick: Instant,
    drag_slider: Option<&'static str>,
    drag_panel: Option<(i32, i32)>,
    drag_wheel: bool,

    // --- permission ---
    permission: u8,
    prev_permission: u8,

    slots: [Slot; 3],
    sprite_epoch: Instant,
    collapsed: Vec<String>,
    assets: Vec<String>,
    panel_px_h: i32,
    asset_dir: String,
    custom: custom::CustomSpec,
    want_dump: Option<(String, bool)>, // (path, panel?)
    dump_done: bool,
    dump_error: Option<String>,
}

impl St {
    fn new() -> Self {
        let now = Instant::now();
        St {
            hwnd: 0,
            cur: (0.0, 0.0),
            from: (0.0, 0.0),
            target: (0.0, 0.0),
            t0: now,
            dur_ms: 0.0,
            dirty: true,
            visible: false,
            label: "AI 鼠标".to_string(),
            label_wanted: true,
            theme: "whale".to_string(),
            badge: true,
            opacity: 1.0,
            accent: 0x4FC3F7,
            scale: 1.0,
            idle_ms: 60_000,
            last_activity: now,
            frames: 0,
            frame_window_start: now,
            fps: 0.0,
            quit: false,
            want_show: false,
            want_hide: false,
            capture_visible: false,
            want_affinity: None,
            panel_hwnd: 0,
            panel_shown: false,
            panel_full: true,
            panel_pos: (0, 0),
            panel_dirty: true,
            want_panel_show: false,
            want_panel_hide: false,
            want_panel_resize: false,
            want_panel_move: false,
            hover: String::new(),
            drawn_visible: false,
            drawn_pos: (i32::MIN, i32::MIN),
            footer_tick: now,
            drag_slider: None,
            drag_panel: None,
            drag_wheel: false,
            permission: 3,
            prev_permission: 3,
            slots: [Slot::new(SLOT_CURSOR), Slot::new(SLOT_HINT), Slot::new(SLOT_ORNAMENT)],
            sprite_epoch: now,
            collapsed: sections::SECTIONS
                .iter()
                .filter(|d| d.default_collapsed)
                .map(|d| d.key.to_string())
                .collect(),
            assets: Vec::new(),
            panel_px_h: PH,
            asset_dir: String::new(),
            custom: custom::CustomSpec::default(),
            want_dump: None,
            dump_done: false,
            dump_error: None,
        }
    }
}

static STATE: OnceLock<Arc<Mutex<St>>> = OnceLock::new();
static HINST: OnceLock<usize> = OnceLock::new();

fn state() -> &'static Arc<Mutex<St>> {
    STATE.get().expect("state")
}

// ------------------------------------------------------------ widget model ---

fn panel_layout(v: &PanelView) -> Vec<Hit> {
    let mut o: Vec<Hit> = Vec::with_capacity(96);

    if !v.full {
        let mut c = Hit::new(rect(0, 0, CHIP_W, CHIP_H), Kind::Chip, "expand").text("\u{2261}  AI 鼠标");
        c.active = v.permission > 0;
        c.hover = v.hover == "expand";
        o.push(c);
        return o;
    }

    let pad = sections::PAD;
    let right = PW - pad;
    o.push(
        Hit::new(rect(right - 60, 8, right - 34, 32), Kind::Button, "collapse")
            .text("\u{2500}")
            .hover(v.hover == "collapse"),
    );
    o.push(
        Hit::new(rect(right - 26, 8, right, 32), Kind::Button, "close")
            .text("\u{2715}")
            .hover(v.hover == "close"),
    );
    o.push(Hit::new(rect(0, 0, PW, 40), Kind::Header, "drag"));
    o.push(Hit::new(rect(pad, 0, 200, 40), Kind::Label, "title").text("AI 鼠标"));
    o.push(Hit::new(rect(pad, 40, right, 41), Kind::Sep, "sep1"));

    let bottom = sections::build(v, 48, &mut o);

    o.push(Hit::new(rect(pad, bottom, right, bottom + 1), Kind::Sep, "sep5"));
    let perm_name = match v.permission {
        0 => "停止",
        1 => "只读",
        2 => "操作",
        _ => "管理员",
    };
    let footer = format!(
        "{} \u{00B7} ({:.0},{:.0}) \u{00B7} fps {:.0} \u{00B7} 权限 {}",
        if v.visible { "可见" } else { "隐藏" },
        v.cur.0,
        v.cur.1,
        v.fps,
        perm_name
    );
    o.push(Hit::new(rect(pad, bottom + 6, right, bottom + 24), Kind::Label, "footer").text(footer));
    o
}

// ------------------------------------------------------------------ canvas ---
struct Fonts {
    title: HGDIOBJ,
    label: HGDIOBJ,
    button: HGDIOBJ,
    value: HGDIOBJ,
    badge: HGDIOBJ,
}

struct Canvas {
    hdc_screen: HDC,
    hdc_big: HDC,
    hdc_small: HDC,
    bits_big: *mut u8,
    bits_small: *mut u8,
    objs: Vec<HGDIOBJ>,
    w: i32,
    h: i32,
    ss: i32,
    fonts: Fonts,
}

unsafe impl Send for Canvas {}

unsafe fn make_font(px: i32, weight: i32) -> HGDIOBJ {
    CreateFontW(px, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 3, 0, w!("Microsoft YaHei UI")).into()
}

impl Canvas {
    unsafe fn new(w: i32, h: i32, ss: i32) -> Canvas {
        let hdc_screen = GetDC(null_hwnd());
        let hdc_big = CreateCompatibleDC(hdc_screen);
        let hdc_small = CreateCompatibleDC(hdc_screen);

        let mut bmi_big = BITMAPINFO::default();
        bmi_big.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi_big.bmiHeader.biWidth = w * ss;
        bmi_big.bmiHeader.biHeight = -(h * ss); // top-down
        bmi_big.bmiHeader.biPlanes = 1;
        bmi_big.bmiHeader.biBitCount = 32;
        bmi_big.bmiHeader.biCompression = BI_RGB.0;

        let mut bmi_small = bmi_big;
        bmi_small.bmiHeader.biWidth = w;
        bmi_small.bmiHeader.biHeight = -h;

        let mut bits_big: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut bits_small: *mut core::ffi::c_void = std::ptr::null_mut();

        let bmp_big =
            CreateDIBSection(hdc_big, &bmi_big, DIB_USAGE(0), &mut bits_big, HANDLE::default(), 0)
                .expect("CreateDIBSection(big)");
        let bmp_small = CreateDIBSection(
            hdc_small,
            &bmi_small,
            DIB_USAGE(0),
            &mut bits_small,
            HANDLE::default(),
            0,
        )
        .expect("CreateDIBSection(small)");

        SelectObject(hdc_big, bmp_big);
        SelectObject(hdc_small, bmp_small);

        Canvas {
            hdc_screen,
            hdc_big,
            hdc_small,
            bits_big: bits_big as *mut u8,
            bits_small: bits_small as *mut u8,
            objs: vec![bmp_big.into(), bmp_small.into()],
            w,
            h,
            ss,
            fonts: Fonts {
                title: make_font(-15 * ss, 700),
                label: make_font(-13 * ss, 400),
                button: make_font(-13 * ss, 500),
                value: make_font(-12 * ss, 400),
                badge: make_font(-14 * ss, 600),
            },
        }
    }

    /// Clear the supersampled surface to the transparency key colour.
    unsafe fn clear(&self) {
        let key = CreateSolidBrush(col(0xFF00FF));
        let full = rect(0, 0, self.w * self.ss, self.h * self.ss);
        FillRect(self.hdc_big, &full, key);
        let _ = DeleteObject(key);
    }

    unsafe fn fill_round(&self, r: RECT, radius: i32, rgb: u32) {
        let s = self.ss;
        let br = CreateSolidBrush(col(rgb));
        let prev_pen = SelectObject(self.hdc_big, GetStockObject(NULL_PEN));
        let prev_br = SelectObject(self.hdc_big, br);
        let _ = RoundRect(self.hdc_big, r.left * s, r.top * s, r.right * s, r.bottom * s, radius * 2 * s, radius * 2 * s);
        SelectObject(self.hdc_big, prev_pen);
        SelectObject(self.hdc_big, prev_br);
        let _ = DeleteObject(br);
    }

    unsafe fn stroke_round(&self, r: RECT, radius: i32, rgb: u32, width: i32) {
        let s = self.ss;
        let pen = CreatePen(PS_SOLID, width * s, col(rgb));
        let prev_pen = SelectObject(self.hdc_big, pen);
        let prev_br = SelectObject(self.hdc_big, GetStockObject(windows::Win32::Graphics::Gdi::NULL_BRUSH));
        let _ = RoundRect(self.hdc_big, r.left * s, r.top * s, r.right * s, r.bottom * s, radius * 2 * s, radius * 2 * s);
        SelectObject(self.hdc_big, prev_pen);
        SelectObject(self.hdc_big, prev_br);
        let _ = DeleteObject(pen);
    }

    unsafe fn fill_rect(&self, r: RECT, rgb: u32) {
        let s = self.ss;
        let br = CreateSolidBrush(col(rgb));
        let rr = rect(r.left * s, r.top * s, r.right * s, r.bottom * s);
        FillRect(self.hdc_big, &rr, br);
        let _ = DeleteObject(br);
    }

    unsafe fn fill_circle(&self, cx: i32, cy: i32, rad: i32, rgb: u32) {
        let s = self.ss;
        let br = CreateSolidBrush(col(rgb));
        let prev_pen = SelectObject(self.hdc_big, GetStockObject(NULL_PEN));
        let prev_br = SelectObject(self.hdc_big, br);
        let _ = Ellipse(
            self.hdc_big,
            (cx - rad) * s,
            (cy - rad) * s,
            (cx + rad) * s,
            (cy + rad) * s,
        );
        SelectObject(self.hdc_big, prev_pen);
        SelectObject(self.hdc_big, prev_br);
        let _ = DeleteObject(br);
    }

    unsafe fn ring_circle(&self, cx: i32, cy: i32, rad: i32, width: i32, rgb: u32) {
        let s = self.ss;
        let pen = CreatePen(PS_SOLID, width * s, col(rgb));
        let prev_pen = SelectObject(self.hdc_big, pen);
        let prev_br =
            SelectObject(self.hdc_big, GetStockObject(windows::Win32::Graphics::Gdi::NULL_BRUSH));
        let _ = Ellipse(
            self.hdc_big,
            (cx - rad) * s,
            (cy - rad) * s,
            (cx + rad) * s,
            (cy + rad) * s,
        );
        SelectObject(self.hdc_big, prev_pen);
        SelectObject(self.hdc_big, prev_br);
        let _ = DeleteObject(pen);
    }

    unsafe fn text_w(&self, font: HGDIOBJ, s: &str) -> i32 {
        let prev = SelectObject(self.hdc_big, font);
        let buf: Vec<u16> = s.encode_utf16().collect();
        let mut ext = SIZE::default();
        let _ = GetTextExtentPoint32W(self.hdc_big, &buf, &mut ext);
        SelectObject(self.hdc_big, prev);
        ext.cx
    }

    unsafe fn text_at(&self, font: HGDIOBJ, x: i32, y: i32, s: &str, rgb: u32) {
        let sf = self.ss;
        let prev = SelectObject(self.hdc_big, font);
        SetBkMode(self.hdc_big, TRANSPARENT);
        SetTextColor(self.hdc_big, col(rgb));
        let buf: Vec<u16> = s.encode_utf16().collect();
        let _ = TextOutW(self.hdc_big, x * sf, y * sf, &buf);
        SelectObject(self.hdc_big, prev);
    }

    /// Composite the supersampled buffer over the transparency key and blit it.
    ///
    /// `dst = None` keeps the window where it is. This matters: `UpdateLayeredWindow`
    /// *moves* the window to `pptDst`, so passing a fixed point would pin a window
    /// that the user can drag back to that point on every repaint.
    unsafe fn compose(&self, w: i32, h: i32, alpha: f64) {
        let s = self.ss as usize;
        let big = std::slice::from_raw_parts(self.bits_big, (self.w * self.ss * self.h * self.ss * 4) as usize);
        let small = std::slice::from_raw_parts_mut(self.bits_small, (self.w * self.h * 4) as usize);
        let stride_big = (self.w * self.ss) as usize;
        let samples = (s * s) as u32;
        let alpha_mul = alpha.clamp(0.0, 1.0);

        for oy in 0..h {
            for ox in 0..w {
                let (mut sr, mut sg, mut sb, mut n) = (0u32, 0u32, 0u32, 0u32);
                for sy in 0..s as i32 {
                    let row = ((oy * self.ss + sy) as usize) * stride_big;
                    for sx in 0..s as i32 {
                        let i = (row + (ox * self.ss + sx) as usize) * 4;
                        let (b, g, r) = (big[i] as u32, big[i + 1] as u32, big[i + 2] as u32);
                        if r == 255 && g == 0 && b == 255 {
                            continue; // transparency key
                        }
                        sr += r;
                        sg += g;
                        sb += b;
                        n += 1;
                    }
                }
                let o = ((oy * self.w + ox) * 4) as usize;
                if n == 0 {
                    small[o] = 0;
                    small[o + 1] = 0;
                    small[o + 2] = 0;
                    small[o + 3] = 0;
                } else {
                    let a = (n * 255 / samples) as f64 * alpha_mul;
                    let ai = a as u32;
                    let r = sr / n;
                    let g = sg / n;
                    let b = sb / n;
                    small[o] = ((b * ai) / 255) as u8;
                    small[o + 1] = ((g * ai) / 255) as u8;
                    small[o + 2] = ((r * ai) / 255) as u8;
                    small[o + 3] = ai as u8;
                }
            }
        }

    }

    /// The finished ARGB surface, for layers composited after the vector pass.
    fn small_mut(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.bits_small, (self.w * self.h * 4) as usize) }
    }

    unsafe fn blit(&self, hwnd: isize, w: i32, h: i32, dst: Option<POINT>) {
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd_of(hwnd),
            self.hdc_screen,
            dst.as_ref().map(|p| p as *const POINT),
            Some(&size),
            self.hdc_small,
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
    }

    unsafe fn present(&self, hwnd: isize, w: i32, h: i32, dst: Option<POINT>, alpha: f64) {
        self.compose(w, h, alpha);
        self.blit(hwnd, w, h, dst);
    }

    /// Write the current frame to a 32-bit BMP composited over a dark background,
    /// so self-tests can inspect the artwork without screen capture.
    unsafe fn dump_bmp(&self, path: &str, w: i32, h: i32) {
        let data_len = (w * h * 4) as u32;
        let mut out = vec![0u8; 54 + data_len as usize];
        out[0] = b'B';
        out[1] = b'M';
        out[2..6].copy_from_slice(&(54u32 + data_len).to_le_bytes());
        out[10..14].copy_from_slice(&54u32.to_le_bytes());
        out[14..18].copy_from_slice(&40u32.to_le_bytes());
        out[18..22].copy_from_slice(&w.to_le_bytes());
        out[22..26].copy_from_slice(&(-h).to_le_bytes());
        out[26..28].copy_from_slice(&1u16.to_le_bytes());
        out[28..30].copy_from_slice(&32u16.to_le_bytes());
        out[34..38].copy_from_slice(&data_len.to_le_bytes());

        let src = std::slice::from_raw_parts(self.bits_small, (self.w * self.h * 4) as usize);
        let bg = 0x20u32;
        for y in 0..h {
            for x in 0..w {
                let o = ((y * self.w + x) * 4) as usize;
                let off = 54 + ((y * w + x) * 4) as usize;
                let a = src[o + 3] as u32;
                let b = src[o] as u32;
                let g = src[o + 1] as u32;
                let r = src[o + 2] as u32;
                let under = bg * (255 - a) / 255;
                out[off] = (b + under) as u8;
                out[off + 1] = (g + under) as u8;
                out[off + 2] = (r + under) as u8;
                out[off + 3] = 255;
            }
        }
        let _ = std::fs::write(path, &out);
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            for o in &self.objs {
                let _ = DeleteObject(*o);
            }
            for f in [
                self.fonts.title,
                self.fonts.label,
                self.fonts.button,
                self.fonts.value,
                self.fonts.badge,
            ] {
                let _ = DeleteObject(f);
            }
            let _ = DeleteDC(self.hdc_big);
            let _ = DeleteDC(self.hdc_small);
            let _ = ReleaseDC(null_hwnd(), self.hdc_screen);
        }
    }
}

// ------------------------------------------------------------- cursor paint ---

/// One customisable picture slot. `source` is `""` (off), `"builtin"` (the vector
/// artwork) or an absolute path to an image the plugin shell resolved.
#[derive(Clone)]
struct Slot {
    visible: bool,
    source: String,
    /// Target width in device pixels; 0 keeps the image's natural size.
    size: f64,
    /// Where the image's hotspot sits relative to the cursor tip, in device px.
    offset: (f64, f64),
    /// Which point of the image lands on `offset`, normalised 0..1.
    hotspot: (f64, f64),
    opacity: f64,
    flip_x: bool,
    last_frame: usize,
}

const SLOT_CURSOR: usize = 0;
const SLOT_HINT: usize = 1;
const SLOT_ORNAMENT: usize = 2;

impl Slot {
    fn new(kind: usize) -> Self {
        let (offset, hotspot) = match kind {
            SLOT_HINT => ((12.0, 50.0), (0.0, 0.0)),
            SLOT_ORNAMENT => ((-80.0, 10.0), (0.5, 0.5)),
            _ => ((0.0, 0.0), (0.0, 0.0)),
        };
        Slot {
            visible: true,
            source: "builtin".to_string(),
            size: 0.0,
            offset,
            hotspot,
            opacity: 1.0,
            flip_x: false,
            last_frame: usize::MAX,
        }
    }

    fn is_image(&self) -> bool {
        self.visible && !self.source.is_empty() && self.source != "builtin"
    }
}

fn slot_is_image(slot: &Slot) -> bool {
    slot.is_image()
}

static SPRITES: OnceLock<Mutex<SpriteCache>> = OnceLock::new();
static WHEEL: OnceLock<Mutex<wheel::Wheel>> = OnceLock::new();

fn wheel() -> &'static Mutex<wheel::Wheel> {
    WHEEL.get_or_init(|| Mutex::new(unsafe { wheel::Wheel::new(SS_PANEL) }))
}

/// Single place that changes the accent, so the colour wheel never drifts out of
/// sync with the colour set by a swatch, a channel bar or a tool call.
fn set_accent(st: &mut St, rgb: u32) {
    st.accent = rgb & 0xFFFFFF;
    if let Some(w) = WHEEL.get() {
        w.lock().unwrap().set_from_rgb(st.accent);
    }
    st.dirty = true;
    st.panel_dirty = true;
}

fn sprites() -> &'static Mutex<SpriteCache> {
    SPRITES.get_or_init(|| Mutex::new(SpriteCache::default()))
}

#[derive(Clone)]
struct CursorView {
    cur: (f64, f64),
    label: String,
    theme: String,
    badge: bool,
    accent: u32,
    scale: f64,
    opacity: f64,
    slots: [Slot; 3],
    /// Milliseconds since the sprite timeline started; drives GIF frame choice.
    epoch_ms: u64,
}

unsafe fn draw_cursor(cv: &Canvas, v: &CursorView) {
    cv.clear();
    let s = SS as f64 * v.scale;
    let cx = (TIP_X * SS as f64) as i32;
    let cy = (TIP_Y * SS as f64) as i32;
    let p = |dx: f64, dy: f64| POINT { x: cx + (dx * s) as i32, y: cy + (dy * s) as i32 };

    let accent = v.accent & 0xFFFFFF;
    let halo = accent;
    let belly = shade(accent, 1.72);
    let dark = shade(accent, 0.34);
    let ring = CreatePen(PS_SOLID, (2.0 * s) as i32, col(dark));
    let thin = CreatePen(PS_SOLID, s as i32, col(dark));
    let halo_br = CreateSolidBrush(col(halo));
    let belly_br = CreateSolidBrush(col(belly));
    let dark_br = CreateSolidBrush(col(dark));

    if !slot_is_image(&v.slots[SLOT_CURSOR]) {
    match v.theme.as_str() {
        "dot" => {
            cv.fill_circle_ss(cx, cy, (15.0 * s) as i32, halo);
            cv.ring_circle_ss(cx, cy, (15.0 * s) as i32, (2.0 * s) as i32, dark);
            cv.fill_circle_ss(cx, cy, (7.0 * s) as i32, belly);
            cv.ring_circle_ss(cx, cy, (7.0 * s) as i32, (1.5 * s) as i32, dark);
        }
        "arrow" => {
            // classic pointer: apex at the tip, tail down-left
            SelectObject(cv.hdc_big, halo_br);
            SelectObject(cv.hdc_big, ring);
            let body = [
                p(0.0, 0.0),
                p(0.0, 30.0),
                p(7.5, 23.0),
                p(13.0, 37.0),
                p(20.0, 34.0),
                p(14.5, 20.5),
                p(25.0, 20.0),
            ];
            let _ = Polygon(cv.hdc_big, &body);
            SelectObject(cv.hdc_big, belly_br);
            let inner = [p(4.0, 6.0), p(4.0, 22.0), p(9.0, 17.0), p(13.5, 28.0), p(16.0, 27.0)];
            let _ = Polygon(cv.hdc_big, &inner);
        }
        _ => {
            // whale facing right: nose at the tip, body down-left
            SelectObject(cv.hdc_big, halo_br);
            SelectObject(cv.hdc_big, ring);
            let tail = [p(-34.0, 16.0), p(-56.0, -2.0), p(-50.0, 28.0)];
            let _ = Polygon(cv.hdc_big, &tail);
            let body = [
                p(-38.0, 12.0),
                p(-30.0, 4.0),
                p(-4.0, 8.0),
                p(10.0, 18.0),
                p(2.0, 34.0),
                p(-26.0, 34.0),
            ];
            let _ = Polygon(cv.hdc_big, &body);
            SelectObject(cv.hdc_big, belly_br);
            let bel = [p(-28.0, 24.0), p(4.0, 20.0), p(-2.0, 32.0), p(-24.0, 32.0)];
            let _ = Polygon(cv.hdc_big, &bel);
            cv.fill_circle_ss(cx - (0.0 * s) as i32, cy + (16.0 * s) as i32, (5.0 * s) as i32, belly);
            SelectObject(cv.hdc_big, dark_br);
            cv.fill_circle_ss(cx + s as i32, cy + (16.0 * s) as i32, (2.0 * s) as i32, dark);
            SelectObject(cv.hdc_big, thin);
            let sx = cx - (22.0 * s) as i32;
            let sy = cy + (6.0 * s) as i32;
            for (dx, dy) in [(7.0, -18.0), (0.0, -22.0), (-7.0, -18.0)] {
                let _ = MoveToEx(cv.hdc_big, sx, sy, None);
                let _ = LineTo(cv.hdc_big, sx + (dx * s) as i32, sy + (dy * s) as i32);
            }
            cv.fill_circle_ss(cx, cy, (5.0 * s) as i32, dark);
            cv.fill_circle_ss(cx, cy, (3.0 * s) as i32, belly);
        }
    }

    }

    if !slot_is_image(&v.slots[SLOT_HINT]) && v.badge && !v.label.is_empty() {
        let prev = SelectObject(cv.hdc_big, cv.fonts.badge);
        let buf: Vec<u16> = v.label.encode_utf16().collect();
        let mut ext = SIZE::default();
        let _ = GetTextExtentPoint32W(cv.hdc_big, &buf, &mut ext);
        SelectObject(cv.hdc_big, prev);

        let pad = (10.0 * s) as i32;
        let bh = (26.0 * s) as i32;
        let bx = cx - (6.0 * s) as i32;
        let by = if v.theme == "dot" { cy + (16.0 * s) as i32 } else { cy + (44.0 * s) as i32 };
        let bw = ext.cx + pad * 2;
        let ssf = SS;
        let rr = rect(bx, by, bx + bw, by + bh);
        let br = CreateSolidBrush(col(0x0E151E));
        let pen = CreatePen(PS_SOLID, (2.0 * s) as i32, col(accent));
        let prev_pen = SelectObject(cv.hdc_big, pen);
        let prev_br = SelectObject(cv.hdc_big, br);
        let _ = RoundRect(
            cv.hdc_big,
            rr.left * ssf,
            rr.top * ssf,
            rr.right * ssf,
            rr.bottom * ssf,
            (10.0 * s) as i32,
            (10.0 * s) as i32,
        );
        SelectObject(cv.hdc_big, prev_pen);
        SelectObject(cv.hdc_big, prev_br);
        let _ = DeleteObject(br);
        let _ = DeleteObject(pen);

        let prev = SelectObject(cv.hdc_big, cv.fonts.badge);
        SetBkMode(cv.hdc_big, TRANSPARENT);
        SetTextColor(cv.hdc_big, col(0xEBF8FF));
        let _ = TextOutW(cv.hdc_big, bx + pad, by + (bh - ext.cy) / 2, &buf);
        SelectObject(cv.hdc_big, prev);
    }

    let _ = DeleteObject(ring);
    let _ = DeleteObject(thin);
    let _ = DeleteObject(halo_br);
    let _ = DeleteObject(belly_br);
    let _ = DeleteObject(dark_br);

    // Rasterise the vector pass, then blend the image slots over it. Images carry
    // their own per-pixel alpha, which the key-colour path cannot represent.
    let alpha = v.opacity.clamp(0.1, 1.0);
    cv.compose(W, H, alpha);

    {
        let mut cache = sprites().lock().unwrap();
        for idx in [SLOT_ORNAMENT, SLOT_CURSOR, SLOT_HINT] {
            let slot = &v.slots[idx];
            if !slot.is_image() {
                continue;
            }
            let Some(Ok(spr)) = cache.get(Path::new(&slot.source)) else {
                continue;
            };
            let tw = if slot.size > 0.0 { slot.size.round().max(1.0) as u32 } else { spr.w };
            let th = ((tw as f64 / spr.aspect()).round().max(1.0)) as u32;
            let frame = &spr.frames[spr.frame_at(v.epoch_ms).min(spr.frames.len() - 1)];
            let dx = (TIP_X + slot.offset.0 - slot.hotspot.0 * tw as f64).round() as i32;
            let dy = (TIP_Y + slot.offset.1 - slot.hotspot.1 * th as f64).round() as i32;
            sprite::composite(
                cv.small_mut(),
                W,
                H,
                dx,
                dy,
                frame,
                spr.w,
                spr.h,
                tw,
                th,
                slot.opacity * alpha,
                slot.flip_x,
            );
        }
    }

    let dst = POINT {
        x: (v.cur.0 - TIP_X).round() as i32,
        y: (v.cur.1 - TIP_Y).round() as i32,
    };
    cv.blit(state().lock().unwrap().hwnd, W, H, Some(dst));
}

impl Canvas {
    /// Ring in supersampled device space (already multiplied by `ss`).
    unsafe fn ring_circle_ss(&self, cx: i32, cy: i32, rad: i32, width: i32, rgb: u32) {
        let pen = CreatePen(PS_SOLID, width, col(rgb));
        let prev_pen = SelectObject(self.hdc_big, pen);
        let prev_br =
            SelectObject(self.hdc_big, GetStockObject(windows::Win32::Graphics::Gdi::NULL_BRUSH));
        let _ = Ellipse(self.hdc_big, cx - rad, cy - rad, cx + rad, cy + rad);
        SelectObject(self.hdc_big, prev_pen);
        SelectObject(self.hdc_big, prev_br);
        let _ = DeleteObject(pen);
    }

    /// Circle in supersampled device space (already multiplied by `ss`).
    unsafe fn fill_circle_ss(&self, cx: i32, cy: i32, rad: i32, rgb: u32) {
        let br = CreateSolidBrush(col(rgb));
        let prev_pen = SelectObject(self.hdc_big, GetStockObject(NULL_PEN));
        let prev_br = SelectObject(self.hdc_big, br);
        let _ = Ellipse(self.hdc_big, cx - rad, cy - rad, cx + rad, cy + rad);
        SelectObject(self.hdc_big, prev_pen);
        SelectObject(self.hdc_big, prev_br);
        let _ = DeleteObject(br);
    }
}

// -------------------------------------------------------------- panel paint ---

unsafe fn draw_panel(cv: &Canvas, v: &PanelView, hits: &[Hit], h: i32) {
    cv.clear();

    if !v.full {
        let r = rect(0, 0, CHIP_W, CHIP_H);
        cv.fill_round(r, 10, C_PANEL);
        cv.stroke_round(r, 10, if v.permission > 0 { v.accent } else { C_BORDER }, 1);
        let dot_rgb = if v.permission > 0 { v.accent } else { C_MUTED };
        cv.fill_circle(20, CHIP_H / 2, 5, dot_rgb);
        if let Some(h) = hits.first() {
            cv.text_at(cv.fonts.button, 34, (CHIP_H - 18) / 2, &h.text, C_TEXT);
        }
        cv.present(state().lock().unwrap().panel_hwnd, CHIP_W, CHIP_H, None, 0.98);
        return;
    }

    // background
    let body = rect(0, 0, PW, h);
    cv.fill_round(body, 14, C_PANEL);
    cv.stroke_round(body, 14, C_BORDER, 1);
    // header strip
    cv.fill_round(rect(1, 1, PW - 1, 40), 13, C_PANEL_2);
    cv.fill_rect(rect(1, 30, PW - 1, 40), C_PANEL_2);

    for h in hits {
        match h.kind {
            Kind::Sep => {
                cv.fill_rect(rect(h.r.left, h.r.top, h.r.right, h.r.top + 1), C_SEP);
            }
            Kind::Label | Kind::LabelRight => {
                let font = if h.id == "title" { cv.fonts.title } else { cv.fonts.value };
                let rgb = if h.id == "title" { C_TEXT } else { C_MUTED };
                let y = h.r.top;
                if h.kind == Kind::LabelRight {
                    let tw = cv.text_w(font, &h.text);
                    cv.text_at(font, h.r.right - tw, y, &h.text, rgb);
                } else {
                    cv.text_at(font, h.r.left, y, &h.text, rgb);
                }
            }
            Kind::Button => {
                let bg = if h.active {
                    v.accent
                } else if h.hover {
                    C_BTN_HOVER
                } else {
                    C_BTN
                };
                cv.fill_round(h.r, 8, bg);
                if h.active {
                    cv.stroke_round(h.r, 8, shade(v.accent, 1.35), 1);
                }
                let fg = if h.active { C_DARK_TEXT } else { C_TEXT };
                let tw = cv.text_w(cv.fonts.button, &h.text);
                let th = 17;
                let tx = h.r.left + (h.r.right - h.r.left - tw) / 2;
                let ty = h.r.top + (h.r.bottom - h.r.top - th) / 2;
                cv.text_at(cv.fonts.button, tx, ty, &h.text, fg);
            }
            Kind::Slider => {
                let cy = (h.r.top + h.r.bottom) / 2;
                let knob = (h.r.left + 9, h.r.right - 9);
                let track = rect(knob.0, cy - 2, knob.1, cy + 2);
                cv.fill_round(track, 2, C_TRACK);
                let value = if h.id == "opacity" {
                    ((v.opacity - 0.15) / 0.85).clamp(0.0, 1.0)
                } else {
                    ((v.scale - 0.6) / 1.4).clamp(0.0, 1.0)
                };
                let kx = knob.0 + ((knob.1 - knob.0) as f64 * value) as i32;
                cv.fill_round(rect(knob.0, cy - 2, kx, cy + 2), 2, v.accent);
                cv.fill_circle(kx, cy, 7, if h.hover { 0xFFFFFF } else { shade(v.accent, 1.5) });
                cv.ring_circle(kx, cy, 7, 1, v.accent);
            }
            Kind::Swatch => {
                let cx = (h.r.left + h.r.right) / 2;
                let cy = (h.r.top + h.r.bottom) / 2;
                let rgb = u32::from_str_radix(h.text.trim_start_matches('#'), 16).unwrap_or(0xFFFFFF);
                cv.fill_circle(cx, cy, 11, rgb);
                if h.active {
                    cv.ring_circle(cx, cy, 13, 2, 0xFFFFFF);
                } else if h.hover {
                    cv.ring_circle(cx, cy, 13, 1, C_BORDER);
                }
            }
            Kind::Section => {
                if h.hover {
                    cv.fill_round(h.r, 6, C_BTN_HOVER);
                }
                cv.text_at(
                    cv.fonts.button,
                    h.r.left + 2,
                    h.r.top + 3,
                    &h.text,
                    if h.hover { C_TEXT } else { C_MUTED },
                );
            }
            Kind::Channel => {
                let cy = (h.r.top + h.r.bottom) / 2;
                let track = rect(h.r.left, cy - 5, h.r.right, cy + 5);
                let ch = h.data.clamp(0, 2) as usize;
                let mut rgb = [0u32; 3];
                rgb[ch] = 255;
                let full = (rgb[0] << 16) | (rgb[1] << 8) | rgb[2];
                let w = track.right - track.left;
                let steps = 48;
                for i in 0..steps {
                    let a = i as f64 / (steps - 1) as f64;
                    let x0 = track.left + (w * i) / steps;
                    let x1 = track.left + (w * (i + 1)) / steps;
                    cv.fill_rect(rect(x0, track.top, x1, track.bottom), blend(0x000000, full, a));
                }
                let value = h.text.trim().parse::<f64>().unwrap_or(0.0) / 255.0;
                let kx = track.left + (w as f64 * value) as i32;
                cv.fill_circle(kx, cy, 6, 0xFFFFFF);
                cv.ring_circle(kx, cy, 6, 1, 0x101820);
            }
            Kind::Wheel => {
                let mut wh = wheel().lock().unwrap();
                if !wh.is_synced() {
                    wh.set_from_rgb(v.accent);
                }
                wh.ensure();
                wh.blit(cv.hdc_big, h.r.left * cv.ss, h.r.top * cv.ss);
                let cx = h.r.left + wheel::D / 2;
                let cy = h.r.top + wheel::D / 2;
                // hue marker on the ring
                let angle_hue = (1.0 - wh.hue).rem_euclid(1.0);
                let ang = angle_hue * std::f64::consts::TAU;
                let rr = (wheel::R_IN + wheel::R_OUT) / 2.0;
                let mx = cx + (rr * ang.cos()) as i32;
                let my = cy + (rr * ang.sin()) as i32;
                cv.ring_circle(mx, my, 6, 2, 0xFFFFFF);
                cv.ring_circle(mx, my, 7, 1, 0x101820);
                // saturation/value marker inside the square
                let sx = cx + ((wh.sat - 0.5) * wheel::SQ) as i32;
                let sy = cy + ((0.5 - wh.val) * wheel::SQ) as i32;
                cv.ring_circle(sx, sy, 5, 2, 0xFFFFFF);
                cv.ring_circle(sx, sy, 6, 1, 0x101820);
            }
            Kind::Preview => {}
            Kind::Header | Kind::Chip => {}
        }
    }

    cv.compose(PW, h, 0.98);
    draw_previews(cv, v, hits, h);
    cv.blit(state().lock().unwrap().panel_hwnd, PW, h, None);
}

// ---------------------------------------------------------------- windows ----

unsafe fn make_window(class: PCWSTR, title: PCWSTR, ex: u32, w: i32, h: i32) -> HWND {
    let hinst = HINSTANCE(HINST.get().copied().expect("hinst") as *mut core::ffi::c_void);
    let hwnd = CreateWindowExW(
        windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(ex),
        class,
        title,
        WS_POPUP,
        0,
        0,
        w,
        h,
        null_hwnd(),
        HMENU(std::ptr::null_mut()),
        hinst,
        None,
    )
    .expect("CreateWindowExW");
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, w, h, SET_WINDOW_POS_FLAGS(0));
    hwnd
}

unsafe fn make_overlay() -> HWND {
    let hwnd = make_window(
        w!("AiMouseOverlay"),
        w!("AI Mouse Overlay"),
        WS_EX_LAYERED.0
            | WS_EX_TRANSPARENT.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_NOACTIVATE.0
            | WS_EX_TOPMOST.0,
        W,
        H,
    );
    let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    hwnd
}

unsafe fn make_panel(x: i32, y: i32) -> HWND {
    let hwnd = make_window(
        w!("AiMousePanel"),
        w!("AI Mouse Panel"),
        WS_EX_LAYERED.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_NOACTIVATE.0
            | WS_EX_TOPMOST.0,
        PW,
        PH,
    );
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        x,
        y,
        PW,
        PH,
        SWP_NOACTIVATE | SWP_NOZORDER,
    );
    let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    hwnd
}

/// Draw the live thumbnails for the artwork slots.
///
/// The sprite cache lock is taken and released here, never held across a call
/// that locks the state mutex — the window procedure takes them in the opposite
/// order (state, then sprites) and holding both would deadlock the panel.
unsafe fn draw_previews(cv: &Canvas, v: &PanelView, hits: &[Hit], h: i32) {
    let boxes: Vec<(i32, i32, i32, i32, usize)> = hits
        .iter()
        .filter(|h| h.kind == Kind::Preview)
        .map(|h| (h.r.left, h.r.top, h.r.right - h.r.left, h.r.bottom - h.r.top, h.data.max(0) as usize))
        .collect();
    if boxes.is_empty() {
        return;
    }
    let mut cache = sprites().lock().unwrap();
    for (bx, by, bw, bh, idx) in boxes {
        let Some(slot) = v.slots.get(idx) else { continue };
        if !slot.source.starts_with("builtin") && slot.source.is_empty() {
            continue;
        }
        let Some(Ok(spr)) = cache.get(Path::new(&slot.source)) else { continue };
        let frame = &spr.frames[spr.frame_at(v.epoch_ms).min(spr.frames.len() - 1)];
        // fit inside the box, keeping the aspect ratio
        let scale = ((bw as f64 - 4.0) / spr.w as f64)
            .min((bh as f64 - 4.0) / spr.h as f64)
            .max(0.01);
        let tw = (spr.w as f64 * scale).round().max(1.0) as u32;
        let th = (spr.h as f64 * scale).round().max(1.0) as u32;
        let dx = bx + (bw - tw as i32) / 2;
        let dy = by + (bh - th as i32) / 2;
        sprite::composite(cv.small_mut(), PW, h, dx, dy,
                          frame, spr.w, spr.h, tw, th, 1.0, false);
    }
}

fn view_of(st: &St) -> PanelView {
    PanelView {
        full: st.panel_full,
        theme: st.theme.clone(),
        badge: st.badge,
        opacity: st.opacity,
        scale: st.scale,
        accent: st.accent,
        permission: st.permission,
        visible: st.visible,
        hover: st.hover.clone(),
        cur: st.cur,
        fps: st.fps,
        label: st.label.clone(),
        collapsed: st.collapsed.clone(),
        slots: [
            slot_view(&st.slots[SLOT_CURSOR], "光标"),
            slot_view(&st.slots[SLOT_HINT], "提示"),
            slot_view(&st.slots[SLOT_ORNAMENT], "附属"),
        ],
        assets: st.assets.clone(),
        rgb: channels_of(st.accent),
        custom: custom_view(st),
        epoch_ms: Instant::now().duration_since(st.sprite_epoch).as_millis() as u64,
    }
}

fn custom_view(st: &St) -> widget::CustomView {
    let mut v = widget::CustomView {
        title: st.custom.title.clone(),
        error: st.custom.error.clone(),
        hint: custom::hint_lines(&st.asset_dir),
        rows: Vec::new(),
    };
    if st.custom.is_empty() && st.custom.error.is_none() {
        return v;
    }
    for row in &st.custom.rows {
        v.rows.push(match row {
            custom::CustomRow::Label(t) => widget::CustomRowView::Label(t.clone()),
            custom::CustomRow::Button { text, .. } => widget::CustomRowView::Button(text.clone()),
        });
    }
    v
}

fn slot_view(slot: &Slot, name: &str) -> SlotView {
    let mut v = SlotView {
        name: name.to_string(),
        source: slot.source.clone(),
        size: slot.size,
        opacity: slot.opacity,
        flip: slot.flip_x,
        frames: 1,
        error: None,
    };
    if slot.is_image() {
        let mut cache = sprites().lock().unwrap();
        if let Some(r) = cache.get(Path::new(&slot.source)) {
            match r {
                Ok(spr) => v.frames = spr.frames.len(),
                Err(e) => v.error = Some(e.clone()),
            }
        }
    }
    v
}

/// Height the panel needs for the current expansion state.
fn panel_height(v: &PanelView) -> i32 {
    if !v.full {
        return CHIP_H;
    }
    let mut sink: Vec<Hit> = Vec::new();
    let bottom = sections::build(v, 48, &mut sink);
    (bottom + 30).clamp(160, PH_MAX)
}

fn cursor_view(st: &St) -> CursorView {
    CursorView {
        cur: st.cur,
        label: if st.label_wanted { st.label.clone() } else { String::new() },
        theme: st.theme.clone(),
        badge: st.badge,
        accent: st.accent,
        scale: st.scale,
        opacity: st.opacity,
        slots: st.slots.clone(),
        epoch_ms: Instant::now().duration_since(st.sprite_epoch).as_millis() as u64,
    }
}

// ------------------------------------------------------------- panel input ---

/// Apply one clicked widget id. Pure state changes only: anything that needs a
/// Win32 call is recorded as a `want_*` flag and executed by the message loop,
/// because a window function may re-enter this thread's wndproc (and the state
/// mutex is not re-entrant).
fn on_toggle(st: &mut St, id: &str) {
    match id {
        "show" => {
            st.visible = true;
            st.want_show = true;
            st.last_activity = Instant::now();
            st.dirty = true;
        }
        "hide" => {
            st.visible = false;
            st.want_hide = true;
            st.dirty = true;
        }
        "stop" => {
            st.prev_permission = st.permission.max(2);
            st.permission = 0;
            st.visible = false;
            st.want_hide = true;
            st.dirty = true;
        }
        "start" => {
            st.permission = st.prev_permission.max(2);
            st.visible = true;
            st.want_show = true;
            st.last_activity = Instant::now();
            st.dirty = true;
        }
        "badge_on" => {
            st.badge = true;
            st.dirty = true;
        }
        "badge_off" => {
            st.badge = false;
            st.dirty = true;
        }
        "collapse" | "close" => {
            st.panel_full = false;
            st.want_panel_resize = true;
            st.panel_dirty = true;
        }
        "expand" => {
            st.panel_full = true;
            st.want_panel_resize = true;
            st.panel_dirty = true;
        }
        "theme_whale" => {
            st.theme = "whale".into();
            st.dirty = true;
        }
        "theme_dot" => {
            st.theme = "dot".into();
            st.dirty = true;
        }
        "theme_arrow" => {
            st.theme = "arrow".into();
            st.dirty = true;
        }
        _ => {
            if let Some(rest) = id.strip_prefix("cus") {
                if let Ok(i) = rest.parse::<usize>() {
                    if let Some((action, set)) = st.custom.action_of(i) {
                        let action = action.to_string();
                        let set = set.cloned();
                        if !action.is_empty() {
                            on_toggle(st, &action);
                        }
                        if let Some(set) = set {
                            apply_custom_set(st, &set);
                        }
                    }
                }
            } else if let Some(sec) = sections::SECTIONS.iter().find(|d| d.header == id) {
                if let Some(pos) = st.collapsed.iter().position(|k| k == sec.key) {
                    st.collapsed.remove(pos);
                } else {
                    st.collapsed.push(sec.key.to_string());
                }
            } else if id.starts_with("art_prev") || id.starts_with("art_next") {
                let dir: i32 = if id.starts_with("art_prev") { -1 } else { 1 };
                let slot = id[8..].parse::<usize>().unwrap_or(0);
                cycle_slot_source(st, slot, dir);
            } else if let Some(rest) = id.strip_prefix("perm_") {
                let lvl: u8 = rest.parse().unwrap_or(3).min(3);
                if lvl == 0 {
                    st.prev_permission = st.permission.max(2);
                    st.permission = 0;
                    st.visible = false;
                    st.want_hide = true;
                } else {
                    st.permission = lvl;
                    st.prev_permission = lvl;
                    st.visible = true;
                    st.want_show = true;
                    st.last_activity = Instant::now();
                }
                st.dirty = true;
            } else if let Some(rest) = id.strip_prefix("sw") {
                if let Ok(i) = rest.parse::<usize>() {
                    if let Some(rgb) = SWATCHES.get(i) {
                        set_accent(st, *rgb);
                    }
                }
            }
        }
    }
    st.panel_dirty = true;
}

fn id_channel(id: &str) -> i32 {
    match id {
        "ch_r" => 0,
        "ch_g" => 1,
        _ => 2,
    }
}

/// Set one 0-255 colour channel from the pointer position on its bar.
fn channel_value(st: &mut St, channel: i32, x: i32, hits: &[Hit]) {
    let channel = channel.clamp(0, 2);
    let id = match channel {
        0 => "ch_r",
        1 => "ch_g",
        _ => "ch_b",
    };
    let Some(h) = hits.iter().find(|h| h.id == id && h.kind == Kind::Channel) else {
        return;
    };
    let t = ((x - h.r.left) as f64 / (h.r.right - h.r.left).max(1) as f64).clamp(0.0, 1.0);
    let mut c = channels_of(st.accent);
    c[channel as usize] = (t * 255.0).round() as u8;
    set_accent(st, ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32);
}

unsafe fn slider_value(st: &mut St, id: &str, x: i32, hits: &[Hit]) {
    let Some(h) = hits.iter().find(|h| h.id == id) else { return };
    let (a, b) = (h.r.left + 9, h.r.right - 9);
    let t = ((x - a) as f64 / (b - a) as f64).clamp(0.0, 1.0);
    if id == "opacity" {
        st.opacity = 0.15 + t * 0.85;
        st.dirty = true;
    } else {
        st.scale = 0.6 + t * 1.4;
        st.dirty = true;
    }
    st.panel_dirty = true;
}

/// Pick the widget under a point.
///
/// The panel's header is a single full-width rectangle drawn for dragging, and it
/// geometrically contains `─` and `✕`. Resolving by "first rectangle that
/// contains the point" therefore turned a click on either button into a window
/// drag. Interactive widgets always win over the drag zone, and labels/separators
/// never take a click at all.
fn pick<'a>(hits: &'a [Hit], x: i32, y: i32) -> Option<&'a Hit> {
    fn rank(k: Kind) -> u8 {
        match k {
            Kind::Button | Kind::Swatch | Kind::Chip | Kind::Section => 0,
            Kind::Slider | Kind::Channel | Kind::Wheel => 1,
            Kind::Header => 2,
            Kind::Label | Kind::LabelRight | Kind::Sep | Kind::Preview => 3,
        }
    }
    hits.iter().filter(|h| h.hit(x, y)).min_by_key(|h| rank(h.kind))
}

/// Cycle a picture slot: builtin vector art -> every picture in the asset folder
/// -> off. The panel half of the artwork module; the tools can set an exact path.
/// The `set` escape hatch of the custom module: a tiny, explicit whitelist of
/// numeric/boolean settings. Anything else is ignored rather than guessed at.
fn apply_custom_set(st: &mut St, set: &serde_json::Value) {
    if let Some(v) = set.get("opacity").and_then(|x| x.as_f64()) {
        st.opacity = v.clamp(0.15, 1.0);
        st.dirty = true;
    }
    if let Some(v) = set.get("scale").and_then(|x| x.as_f64()) {
        st.scale = v.clamp(0.6, 2.0);
        st.dirty = true;
    }
    if let Some(b) = set.get("badge").and_then(|x| x.as_bool()) {
        st.badge = b;
        st.dirty = true;
    }
    if let Some(t) = set.get("theme").and_then(|x| x.as_str()) {
        if matches!(t, "whale" | "dot" | "arrow") {
            st.theme = t.to_string();
            st.dirty = true;
        }
    }
    if let Some(hexs) = set.get("color").and_then(|x| x.as_str()) {
        if let Some(rgb) = parse_hex(hexs) {
            set_accent(st, rgb);
        }
    }
    st.panel_dirty = true;
}

fn cycle_slot_source(st: &mut St, slot_idx: usize, dir: i32) {
    if slot_idx >= st.slots.len() {
        return;
    }
    let options: Vec<String> = std::iter::once("builtin".to_string())
        .chain(st.assets.iter().cloned())
        .chain(std::iter::once(String::new()))
        .collect();
    let current = st.slots[slot_idx].source.clone();
    let pos = options.iter().position(|o| *o == current).unwrap_or(0) as i32;
    let next = (pos + dir).rem_euclid(options.len() as i32) as usize;
    st.slots[slot_idx].source = options[next].clone();
    st.slots[slot_idx].last_frame = usize::MAX;
    st.sprite_epoch = Instant::now();
    st.dirty = true;
    st.panel_dirty = true;
}

fn client_xy(lparam: LPARAM) -> (i32, i32) {
    let v = lparam.0 as u32;
    ((v & 0xFFFF) as i16 as i32, ((v >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let (x, y) = client_xy(lparam);
            let s = state();
            let mut st = s.lock().unwrap();
            if let Some((ox, oy)) = st.drag_panel {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                st.panel_pos = (p.x - ox, p.y - oy);
                st.want_panel_move = true;
                return LRESULT(0);
            }
            if st.drag_wheel {
                let hits = panel_layout(&view_of(&st));
                if let Some(h) = hits.iter().find(|h| h.kind == Kind::Wheel) {
                    let lx = x - h.r.left;
                    let ly = y - h.r.top;
                    let mut wh = wheel().lock().unwrap();
                    if wh.hit(lx, ly) {
                        let rgb = wh.rgb();
                        drop(wh);
                        st.accent = rgb & 0xFFFFFF;
                        st.dirty = true;
                        st.panel_dirty = true;
                    }
                }
                return LRESULT(0);
            }
            if let Some(id) = st.drag_slider {
                let hits = panel_layout(&view_of(&st));
                let is_channel = hits.iter().any(|h| h.id == id && h.kind == Kind::Channel);
                if is_channel {
                    let ch = id_channel(id);
                    channel_value(&mut st, ch, x, &hits);
                } else {
                    slider_value(&mut st, id, x, &hits);
                }
                return LRESULT(0);
            }
            let hits = panel_layout(&view_of(&st));
            let hover = pick(&hits, x, y)
                .filter(|h| {
                    matches!(
                        h.kind,
                        Kind::Button | Kind::Swatch | Kind::Chip | Kind::Section | Kind::Channel | Kind::Wheel
                    )
                })
                .map(|h| h.id.to_string())
                .unwrap_or_default();
            if hover != st.hover {
                st.hover = hover;
                st.panel_dirty = true;
            }
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = client_xy(lparam);
            let s = state();
            let mut st = s.lock().unwrap();
            let hits = panel_layout(&view_of(&st));
            let mut handled = false;
            if let Some(h) = pick(&hits, x, y) {
                match h.kind {
                    Kind::Header => {
                        let mut p = POINT::default();
                        let _ = GetCursorPos(&mut p);
                        st.drag_panel = Some((p.x - st.panel_pos.0, p.y - st.panel_pos.1));
                        handled = true;
                    }
                    Kind::Wheel => {
                        let lx = x - h.r.left;
                        let ly = y - h.r.top;
                        let mut wh = wheel().lock().unwrap();
                        if wh.hit(lx, ly) {
                            let rgb = wh.rgb();
                            drop(wh);
                            st.accent = rgb & 0xFFFFFF;
                            st.dirty = true;
                            st.panel_dirty = true;
                        }
                        st.drag_wheel = true;
                        handled = true;
                    }
                    Kind::Slider | Kind::Channel => {
                        let id = h.id;
                        let kind = h.kind;
                        let data = h.data;
                        st.drag_slider = Some(id);
                        if kind == Kind::Channel {
                            channel_value(&mut st, data, x, &hits);
                        } else {
                            slider_value(&mut st, id, x, &hits);
                        }
                        handled = true;
                    }
                    Kind::Button | Kind::Swatch | Kind::Chip | Kind::Section => {
                        let id = h.id;
                        drop(st);
                        {
                            let mut st = s.lock().unwrap();
                            on_toggle(&mut st, id);
                        }
                        let _ = SetCapture(hwnd);
                        return LRESULT(0);
                    }
                    _ => {}
                }
            }
            if handled {
                let _ = SetCapture(hwnd);
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let s = state();
            {
                let mut st = s.lock().unwrap();
                st.drag_slider = None;
                st.drag_panel = None;
                st.drag_wheel = false;
                st.panel_dirty = true;
            }
            let _ = ReleaseCapture();
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

// ---------------------------------------------------------------- protocol ---

fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t;
    1.0 - u * u * u
}

fn reply(v: serde_json::Value) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{}", v);
    let _ = out.flush();
}

/// Minimum permission level a command needs. `None` = always allowed (system
/// plumbing: the plugin shell configures these before anyone can click anything).
fn min_perm(cmd: &str) -> Option<u8> {
    match cmd {
        "show" | "hide" | "panel" | "panel_expanded" => Some(1),
        "move" => Some(2),
        "label" | "theme" | "badge" | "opacity" | "color" | "scale" | "permission" | "pause"
        | "resume" => Some(3),
        _ => None,
    }
}

fn parse_hex(s: &str) -> Option<u32> {
    let t = s.trim().trim_start_matches('#');
    u32::from_str_radix(t, 16).ok().filter(|v| *v <= 0xFFFFFF)
}

fn handle_line(line: &str) {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            reply(serde_json::json!({"ok": false, "error": format!("bad json: {e}")}));
            return;
        }
    };
    let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("").to_string();

    let s = state();
    let mut st = s.lock().unwrap();

    if let Some(need) = min_perm(&cmd) {
        if st.permission < need {
            let out = serde_json::json!({
                "id": id,
                "ok": false,
                "error": format!("permission denied: `{cmd}` needs level {need}, current level {}", st.permission),
                "permission": st.permission,
                "visible": st.visible,
            });
            reply(out);
            return;
        }
    }

    let mut out = serde_json::json!({"id": id, "ok": true});
    match cmd.as_str() {
        "show" => {
            st.visible = true;
            st.dirty = true;
            st.last_activity = Instant::now();
            st.want_show = true;
        }
        "hide" => {
            st.visible = false;
            st.dirty = true;
            st.want_hide = true;
        }
        "move" => {
            let x = v.get("x").and_then(|n| n.as_f64()).unwrap_or(st.cur.0);
            let y = v.get("y").and_then(|n| n.as_f64()).unwrap_or(st.cur.1);
            st.from = st.cur;
            st.target = (x, y);
            st.t0 = Instant::now();
            st.dur_ms = v.get("duration_ms").and_then(|n| n.as_f64()).unwrap_or(320.0).max(0.0);
            st.visible = true;
            st.dirty = true;
            st.last_activity = Instant::now();
            st.want_show = true;
        }
        "panel" => {
            let on = v.get("visible").and_then(|b| b.as_bool()).unwrap_or(true);
            st.panel_shown = on;
            if on {
                st.want_panel_show = true;
            } else {
                st.want_panel_hide = true;
            }
            st.panel_dirty = true;
        }
        "panel_expanded" => {
            let on = v.get("expanded").and_then(|b| b.as_bool()).unwrap_or(true);
            if on != st.panel_full {
                st.panel_full = on;
                st.want_panel_resize = true;
                st.panel_dirty = true;
            }
        }
        "opacity" => {
            if let Some(x) = v.get("value").and_then(|n| n.as_f64()) {
                st.opacity = x.clamp(0.15, 1.0);
                st.dirty = true;
                st.panel_dirty = true;
            }
        }
        "scale" => {
            if let Some(x) = v.get("value").and_then(|n| n.as_f64()) {
                st.scale = x.clamp(0.6, 2.0);
                st.dirty = true;
                st.panel_dirty = true;
            }
        }
        "color" => {
            if let Some(hexs) = v.get("hex").and_then(|t| t.as_str()) {
                if let Some(rgb) = parse_hex(hexs) {
                    set_accent(&mut st, rgb);
                }
            }
        }
        "sprite" => {
            let slot_name = v.get("slot").and_then(|x| x.as_str()).unwrap_or("");
            let idx = match slot_name {
                "cursor" => Some(SLOT_CURSOR),
                "hint" => Some(SLOT_HINT),
                "ornament" => Some(SLOT_ORNAMENT),
                _ => None,
            };
            match idx {
                None => {
                    out["ok"] = serde_json::json!(false);
                    out["error"] = serde_json::json!("slot must be cursor | hint | ornament");
                }
                Some(i) => {
                    let mut restart = false;
                    let slot = &mut st.slots[i];
                    if let Some(src) = v.get("source").and_then(|x| x.as_str()) {
                        if slot.source != src {
                            slot.source = src.to_string();
                            slot.last_frame = usize::MAX;
                            restart = true;
                        }
                    }
                    if let Some(b) = v.get("visible").and_then(|x| x.as_bool()) {
                        slot.visible = b;
                    }
                    if let Some(n) = v.get("size").and_then(|x| x.as_f64()) {
                        slot.size = if n > 0.0 { n } else { 0.0 };
                    }
                    if let Some(n) = v.get("opacity").and_then(|x| x.as_f64()) {
                        slot.opacity = n.clamp(0.0, 1.0);
                    }
                    if let Some(b) = v.get("flip").and_then(|x| x.as_bool()) {
                        slot.flip_x = b;
                    }
                    for (key, target) in [("offset", 0usize), ("hotspot", 1usize)] {
                        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
                            let a = arr.first().and_then(|x| x.as_f64());
                            let b = arr.get(1).and_then(|x| x.as_f64());
                            if let (Some(a), Some(b)) = (a, b) {
                                if target == 0 {
                                    slot.offset = (a, b);
                                } else {
                                    slot.hotspot = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
                                }
                            }
                        }
                    }
                    if restart {
                        st.sprite_epoch = Instant::now();
                    }
                    st.dirty = true;
                    // the artwork section shows a live thumbnail of this slot
                    st.panel_dirty = true;
                }
            }
        }
        "assets" => {
            // Scan the folder the plugin shell resolved; the panel cycles through
            // these when the user presses ◀ / ▶ on a slot.
            let dir = v.get("dir").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let mut found: Vec<String> = Vec::new();
            if !dir.is_empty() {
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for entry in rd.flatten() {
                        let path = entry.path();
                        let ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.to_ascii_lowercase())
                            .unwrap_or_default();
                        if matches!(
                            ext.as_str(),
                            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif"
                                | "tiff" | "svg"
                        ) {
                            found.push(path.to_string_lossy().to_string());
                        }
                    }
                }
            }
            found.sort();
            st.assets = found.clone();
            st.asset_dir = dir.clone();
            st.custom = custom::load(&dir);
            st.panel_dirty = true;
            out["assets"] = serde_json::json!(found);
            out["count"] = serde_json::json!(found.len());
        }
        "sprite_rescan" => {
            sprites().lock().unwrap().clear();
            for slot in st.slots.iter_mut() {
                slot.last_frame = usize::MAX;
            }
            st.sprite_epoch = Instant::now();
            st.dirty = true;
        }
        "badge" => {
            if let Some(b) = v.get("visible").and_then(|b| b.as_bool()) {
                st.badge = b;
                st.dirty = true;
                st.panel_dirty = true;
            }
        }
        "affinity" => {
            // capture_visible=true -> WDA_NONE (the overlay shows up in screenshots,
            // for self-tests); false -> WDA_EXCLUDEFROMCAPTURE (production default).
            // Windows lets a window opt out of capture only once; switching back
            // returns ERROR_NOT_ENOUGH_MEMORY. So a change recreates the windows, and
            // that must happen on the message-pumping thread.
            let cap = v.get("capture_visible").and_then(|b| b.as_bool()).unwrap_or(false);
            st.want_affinity = Some(cap);
        }
        "dump" => {
            let path = v
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or("ai-mouse-frame.bmp")
                .to_string();
            let panel = v.get("target").and_then(|t| t.as_str()) == Some("panel");
            st.want_dump = Some((path, panel));
            st.dump_done = false;
            st.dump_error = None;
        }
        "label" => {
            if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                st.label = t.to_string();
                st.label_wanted = !t.is_empty();
                st.dirty = true;
                st.panel_dirty = true;
            }
        }
        "idle" => {
            if let Some(ms) = v.get("idle_ms").and_then(|n| n.as_u64()) {
                st.idle_ms = ms;
            }
        }
        "theme" => {
            if let Some(t) = v.get("theme").and_then(|t| t.as_str()) {
                if matches!(t, "whale" | "dot" | "arrow") {
                    st.theme = t.to_string();
                    st.dirty = true;
                    st.panel_dirty = true;
                } else {
                    out["ok"] = serde_json::json!(false);
                    out["error"] = serde_json::json!("theme must be whale | dot | arrow");
                }
            }
        }
        "permission" => {
            // The agent may only LOWER the ceiling; only the panel (the human) raises it.
            if let Some(lvl) = v.get("level").and_then(|n| n.as_u64()) {
                let lvl = (lvl.min(3)) as u8;
                if lvl <= st.permission {
                    if lvl == 0 {
                        st.prev_permission = st.permission.max(2);
                        st.visible = false;
                        st.want_hide = true;
                    }
                    st.permission = lvl;
                } else {
                    out["ok"] = serde_json::json!(false);
                    out["error"] = serde_json::json!(format!(
                        "the agent may only lower the permission level; ask the human to raise it in the panel (current {})",
                        st.permission
                    ));
                }
                st.dirty = true;
                st.panel_dirty = true;
            }
        }
        "pause" => {
            st.prev_permission = st.permission.max(2);
            st.permission = 0;
            st.visible = false;
            st.want_hide = true;
            st.dirty = true;
            st.panel_dirty = true;
        }
        "resume" => {
            st.permission = st.prev_permission.max(2);
            st.visible = true;
            st.want_show = true;
            st.last_activity = Instant::now();
            st.dirty = true;
            st.panel_dirty = true;
        }
        "quit" => {
            st.quit = true;
        }
        _ => {}
    }

    if cmd == "dump" {
        // The window thread owns the GDI surfaces, so the write happens there.
        // Block until it is done: "ok" must mean the file exists.
        drop(st);
        for _ in 0..600 {
            if state().lock().unwrap().dump_done {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        st = state().lock().unwrap();
    }

    out["visible"] = serde_json::json!(st.visible);
    if cmd == "dump" {
        out["dump_error"] = serde_json::json!(st.dump_error);
    }
    if cmd == "move" {
        // A committed move reports where it is going, not the frame it is on.
        out["x"] = serde_json::json!(st.target.0);
        out["y"] = serde_json::json!(st.target.1);
    } else {
        out["x"] = serde_json::json!(st.cur.0);
        out["y"] = serde_json::json!(st.cur.1);
    }
    out["label"] = serde_json::json!(st.label);
    out["theme"] = serde_json::json!(st.theme);
    out["badge"] = serde_json::json!(st.badge);
    out["opacity"] = serde_json::json!(st.opacity);
    out["scale"] = serde_json::json!(st.scale);
    out["color"] = serde_json::json!(hex(st.accent));
    out["permission"] = serde_json::json!(st.permission);
    out["paused"] = serde_json::json!(st.permission == 0);
    out["hwnd"] = serde_json::json!(format!("0x{:X}", st.hwnd));
    out["panel_hwnd"] = serde_json::json!(format!("0x{:X}", st.panel_hwnd));
    out["panel"] = serde_json::json!(st.panel_shown);
    out["panel_expanded"] = serde_json::json!(st.panel_full);
    out["panel_x"] = serde_json::json!(st.panel_pos.0);
    out["panel_y"] = serde_json::json!(st.panel_pos.1);
    let mut slot_out = serde_json::Map::new();
    {
        let mut cache = sprites().lock().unwrap();
        for (i, slot) in st.slots.iter().enumerate() {
            let name = match i {
                SLOT_CURSOR => "cursor",
                SLOT_HINT => "hint",
                _ => "ornament",
            };
            let mut j = serde_json::json!({
                "source": slot.source,
                "visible": slot.visible,
                "size": slot.size,
                "offset": [slot.offset.0, slot.offset.1],
                "hotspot": [slot.hotspot.0, slot.hotspot.1],
                "opacity": slot.opacity,
                "flip": slot.flip_x,
            });
            if slot.is_image() {
                match cache.get(Path::new(&slot.source)) {
                    Some(Ok(spr)) => {
                        j["frames"] = serde_json::json!(spr.frames.len());
                        j["natural"] = serde_json::json!([spr.w, spr.h]);
                    }
                    Some(Err(e)) => j["error"] = serde_json::json!(e),
                    None => {}
                }
            }
            slot_out.insert(name.to_string(), j);
        }
    }
    out["slots"] = serde_json::Value::Object(slot_out);
    out["frames"] = serde_json::json!(st.frames);
    out["fps"] = serde_json::json!(st.fps);
    out["capture_visible"] = serde_json::json!(st.capture_visible);
    out["excluded_from_capture"] = serde_json::json!(!st.capture_visible);
    reply(out);
}

// -------------------------------------------------------------------- main ---

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let st = Arc::new(Mutex::new(St::new()));
        let _ = STATE.set(st.clone());

        let reader_state = st.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut lock = stdin.lock();
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match lock.read_until(b'\n', &mut buf) {
                    Ok(0) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf).trim().to_string();
                        if !line.is_empty() {
                            let _ = &reader_state;
                            handle_line(&line);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let hinst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).unwrap().into();
        let _ = HINST.set(hinst.0 as usize);

        for (class, proc_) in [
            (w!("AiMouseOverlay"), overlay_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT),
            (w!("AiMousePanel"), panel_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT),
        ] {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc_),
                hInstance: hinst,
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }

        let sw = GetSystemMetrics(SYSTEM_METRICS_INDEX(SM_CXSCREEN.0));
        let sh = GetSystemMetrics(SYSTEM_METRICS_INDEX(SM_CYSCREEN.0));
        let px = sw - PW - 24;
        let py = (sh - PH) / 2;

        let hwnd = make_overlay();
        let panel = make_panel(px, py);
        {
            let mut s = st.lock().unwrap();
            s.hwnd = hwnd.0 as isize;
            s.panel_hwnd = panel.0 as isize;
            s.panel_pos = (px, py);
        }

        reply(serde_json::json!({
            "ok": true, "event": "ready",
            "hwnd": format!("0x{:X}", hwnd.0 as isize),
            "panel_hwnd": format!("0x{:X}", panel.0 as isize),
            "class": "AiMouseOverlay",
            "panel_class": "AiMousePanel",
            "size": [W, H],
            "panel_size": [PW, PH],
            "chip_size": [CHIP_W, CHIP_H],
            "excluded_from_capture": true,
            "version": env!("CARGO_PKG_VERSION"),
        }));

        let cursor_cv = Canvas::new(W, H, SS);
        let panel_cv = Canvas::new(PW, PH_MAX, SS_PANEL);

        loop {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, null_hwnd(), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let now = Instant::now();
            let quit = { st.lock().unwrap().quit };
            if quit {
                break;
            }

            {
                let mut s = st.lock().unwrap();
                let t = (now - s.t0).as_secs_f64() * 1000.0;
                if s.dur_ms > 0.0 && t < s.dur_ms {
                    let p = ease_out_cubic(t / s.dur_ms);
                    s.cur = (
                        s.from.0 + (s.target.0 - s.from.0) * p,
                        s.from.1 + (s.target.1 - s.from.1) * p,
                    );
                    s.dirty = true;
                } else if s.cur != s.target {
                    s.cur = s.target;
                    s.dirty = true;
                }
                if s.visible
                    && s.idle_ms > 0
                    && now.duration_since(s.last_activity).as_millis() as u64 > s.idle_ms
                {
                    s.visible = false;
                    s.dirty = true;
                    s.want_hide = true;
                }
            }

            // Deferred Win32 actions run on the window thread, never while the
            // state lock is held (these calls can re-enter the wndproc).
            // The panel grows and shrinks with its sections, so the requested
            // height is recomputed from the live expansion state.
            {
                let want_h = {
                    let s = st.lock().unwrap();
                    if s.panel_full {
                        panel_height(&view_of(&s))
                    } else {
                        CHIP_H
                    }
                };
                let mut s = st.lock().unwrap();
                if s.panel_px_h != want_h {
                    s.panel_px_h = want_h;
                    s.want_panel_resize = true;
                    s.panel_dirty = true;
                }
            }

            let (do_show, do_hide, affinity, dump, p_show, p_hide, p_resize, p_move, p_pos, p_full, p_h) = {
                let mut s = st.lock().unwrap();
                let show = s.want_show;
                let hide = s.want_hide;
                s.want_show = false;
                s.want_hide = false;
                let aff = s.want_affinity.take();
                let d = s.want_dump.take();
                let ps = s.want_panel_show;
                let ph = s.want_panel_hide;
                let pr = s.want_panel_resize;
                let pm = s.want_panel_move;
                s.want_panel_show = false;
                s.want_panel_hide = false;
                s.want_panel_resize = false;
                s.want_panel_move = false;
                s.panel_dirty = s.panel_dirty || ps || ph || pr;
                (show, hide, aff, d, ps, ph, pr, pm, s.panel_pos, s.panel_full, s.panel_px_h)
            };

            let (ohwnd, phwnd) = {
                let s = st.lock().unwrap();
                (s.hwnd, s.panel_hwnd)
            };

            if do_hide {
                let _ = ShowWindow(hwnd_of(ohwnd), SW_HIDE);
            }
            if do_show {
                let _ = ShowWindow(hwnd_of(ohwnd), SW_SHOWNOACTIVATE);
            }

            if let Some(cap) = affinity {
                let current = { st.lock().unwrap().capture_visible };
                if cap != current {
                    if p_full {
                        // recreate both windows: Windows only honours the capture
                        // opt-out once per window.
                        let _ = DestroyWindow(hwnd_of(ohwnd));
                        let _ = DestroyWindow(hwnd_of(phwnd));
                        let fresh = make_overlay();
                        let (x, y) = { st.lock().unwrap().panel_pos };
                        let fresh_panel = make_panel(x, y);
                        if cap {
                            let _ = SetWindowDisplayAffinity(fresh, WDA_NONE);
                            let _ = SetWindowDisplayAffinity(fresh_panel, WDA_NONE);
                        } else {
                            let _ = SetWindowDisplayAffinity(fresh, WDA_EXCLUDEFROMCAPTURE);
                            let _ =
                                SetWindowDisplayAffinity(fresh_panel, WDA_EXCLUDEFROMCAPTURE);
                        }
                        let (visible, panel_shown) = {
                            let mut s = st.lock().unwrap();
                            s.hwnd = fresh.0 as isize;
                            s.panel_hwnd = fresh_panel.0 as isize;
                            s.capture_visible = cap;
                            s.dirty = true;
                            s.panel_dirty = true;
                            (s.visible, s.panel_shown)
                        };
                        if visible {
                            let _ = ShowWindow(fresh, SW_SHOWNOACTIVATE);
                        }
                        if panel_shown {
                            let _ = ShowWindow(fresh_panel, SW_SHOWNOACTIVATE);
                        }
                    }
                }
            }

            if p_hide {
                let _ = ShowWindow(hwnd_of(phwnd), SW_HIDE);
            }
            if p_show {
                let _ = ShowWindow(hwnd_of(phwnd), SW_SHOWNOACTIVATE);
            }
            if p_resize {
                let w = if p_full { PW } else { CHIP_W };
                let h = if p_full { p_h } else { CHIP_H };
                let _ = SetWindowPos(
                    hwnd_of(phwnd),
                    HWND_TOPMOST,
                    p_pos.0,
                    p_pos.1,
                    w,
                    h,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            if p_move {
                let _ = SetWindowPos(
                    hwnd_of(phwnd),
                    HWND_TOPMOST,
                    p_pos.0,
                    p_pos.1,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }

            if let Some((path, target_panel)) = dump {
                if target_panel {
                    let vw = { view_of(&st.lock().unwrap()) };
                    let hits = panel_layout(&vw);
                    let (w, h) = if vw.full { (PW, panel_height(&vw)) } else { (CHIP_W, CHIP_H) };
                    draw_panel(&panel_cv, &vw, &hits, h);
                    panel_cv.dump_bmp(&path, w, h);
                } else {
                    let cvv = { cursor_view(&st.lock().unwrap()) };
                    draw_cursor(&cursor_cv, &cvv);
                    cursor_cv.dump_bmp(&path, W, H);
                }
                let mut s = st.lock().unwrap();
                s.dump_done = true;
                s.dump_error = if std::path::Path::new(&path).exists() {
                    None
                } else {
                    Some(format!("could not write {path}"))
                };
            }

            // Advance animated sprites: repaint only when the visible frame
            // actually changes, so an idle GIF does not burn a redraw per tick.
            // Sprite frames advance whether or not the cursor is on screen: the
            // artwork thumbnails in the panel have to animate on their own.
            {
                let epoch = now.duration_since(st.lock().unwrap().sprite_epoch).as_millis() as u64;
                let mut changed = false;
                {
                    let mut s = st.lock().unwrap();
                    let sources: Vec<(usize, String)> = s
                        .slots
                        .iter()
                        .enumerate()
                        .filter(|(_, slot)| slot.is_image())
                        .map(|(i, slot)| (i, slot.source.clone()))
                        .collect();
                    if !sources.is_empty() {
                        let mut cache = sprites().lock().unwrap();
                        for (i, path) in sources {
                            if let Some(Ok(spr)) = cache.get(Path::new(&path)) {
                                if spr.frames.len() > 1 {
                                    let f = spr.frame_at(epoch);
                                    if s.slots[i].last_frame != f {
                                        s.slots[i].last_frame = f;
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
                if changed {
                    let mut s = st.lock().unwrap();
                    if s.visible {
                        s.dirty = true;
                    }
                    if s.panel_shown {
                        s.panel_dirty = true;
                    }
                }
            }

            // Keep the panel's status line honest. The cursor glides on its own
            // thread of control, so cap the refresh at ~4 Hz instead of repainting
            // the whole panel every frame.
            {
                let mut s = st.lock().unwrap();
                if s.panel_shown && s.panel_full {
                    let pos = (s.cur.0.round() as i32, s.cur.1.round() as i32);
                    let stale = s.visible != s.drawn_visible || pos != s.drawn_pos;
                    if stale && now.duration_since(s.footer_tick) >= Duration::from_millis(250) {
                        s.footer_tick = now;
                        s.drawn_visible = s.visible;
                        s.drawn_pos = pos;
                        s.panel_dirty = true;
                    }
                }
            }

            let (draw_cursor_now, cvv) = {
                let mut s = st.lock().unwrap();
                if s.visible && s.dirty {
                    s.dirty = false;
                    s.frames += 1;
                    (true, cursor_view(&s))
                } else {
                    (false, cursor_view(&s))
                }
            };
            if draw_cursor_now {
                draw_cursor(&cursor_cv, &cvv);
            }

            let (draw_panel_now, pv) = {
                let mut s = st.lock().unwrap();
                if s.panel_shown && s.panel_dirty {
                    s.panel_dirty = false;
                    (true, view_of(&s))
                } else {
                    (false, view_of(&s))
                }
            };
            if draw_panel_now {
                let hits = panel_layout(&pv);
                let h = if pv.full { panel_height(&pv) } else { CHIP_H };
                draw_panel(&panel_cv, &pv, &hits, h);
            }

            {
                let mut s = st.lock().unwrap();
                let el = now.duration_since(s.frame_window_start);
                if el >= Duration::from_secs(1) {
                    s.fps = s.frames as f64 / el.as_secs_f64();
                    s.frames = 0;
                    s.frame_window_start = now;
                }
            }

            std::thread::sleep(Duration::from_millis(8));
        }

        let (ohwnd, phwnd) = {
            let s = st.lock().unwrap();
            (s.hwnd, s.panel_hwnd)
        };
        let _ = DestroyWindow(hwnd_of(phwnd));
        let _ = DestroyWindow(hwnd_of(ohwnd));
        drop(cursor_cv);
        drop(panel_cv);
        std::process::exit(0);
    }
}
