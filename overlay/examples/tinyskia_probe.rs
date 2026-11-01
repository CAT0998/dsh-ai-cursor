//! T1 — is `tiny-skia` worth replacing the hand-written GDI path?
//!
//! Standalone feasibility probe. It is NOT part of the shipped overlay:
//!
//! * `src/*.rs` is untouched. `Canvas` and the `col`/`rect`/`shade` helpers are
//!   **copied** from `src/main.rs` (a bin crate's example cannot import the
//!   bin), and the copy is identical so group A is measured with exactly the
//!   code that ships.
//! * The scene is not invented: `artifacts/tinyskia/panel-hits.json` holds the
//!   real widget list read out of the running panel through its stdio protocol,
//!   so both groups draw the same 114 widgets at the same coordinates.
//!
//! Questions answered with numbers:
//!   1. one cursor frame:   A = GDI + compose()   vs   B = tiny-skia + swizzle
//!   2. one 452x1139 panel: same two groups
//!   3. does the premultiplied-RGBA -> premultiplied-BGRA swizzle eat the gain?
//!      (timed on its own, and again as part of B's total)
//!   4. do tiny-skia's coverage-AA edges survive `UpdateLayeredWindow`, or do
//!      they fringe black/white?  Checked in software (invariants + composites)
//!      AND on a real layered window captured over a white and a black backdrop.
//!
//!   cargo run --release --example tinyskia_probe
//!
//! Environment: FRAMES (default 1000), PROBE_HITS (widget JSON path),
//! PROBE_OUT (output directory, default ../artifacts/tinyskia).

use std::collections::HashMap;
use std::time::Instant;

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect as SkRect, Shader, Stroke, Transform};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, Ellipse, FillRect, GetDC, GetStockObject, GetTextExtentPoint32W,
    LineTo, MoveToEx, Polygon, ReleaseDC, RoundRect, SelectObject, SetBkMode, SetTextColor,
    TextOutW, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_USAGE, HDC, HGDIOBJ, NULL_BRUSH, NULL_PEN, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, RegisterClassExW,
    SetWindowPos, ShowWindow, UpdateLayeredWindow, HMENU, HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN,
    SWP_NOACTIVATE, SW_SHOW, SYSTEM_METRICS_INDEX, ULW_ALPHA, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

// ---------------------------------------------------------------- geometry ---
// Copied from src/main.rs so group A sizes its surfaces exactly like the app.
const W: i32 = 360;
const H: i32 = 240;
const SS: i32 = 2;
const TIP_X: f64 = 200.0;
const TIP_Y: f64 = 44.0;

const PW: i32 = 452;
const PH: i32 = 1139; // the task's worst-case panel height
const SS_PANEL: i32 = 2;

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
const ACCENT: u32 = 0x4FC3F7;

// ------------------------------------------------------------------ helpers ---

fn col(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF))
}

fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}

fn null_hwnd() -> HWND {
    HWND(std::ptr::null_mut())
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

fn rgb_parts(rgb: u32) -> (u8, u8, u8) {
    (((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8)
}

fn sk_color(rgb: u32, a: u8) -> tiny_skia::Color {
    let (r, g, b) = rgb_parts(rgb);
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

fn solid(rgb: u32, a: u8) -> Paint<'static> {
    Paint { shader: Shader::SolidColor(sk_color(rgb, a)), ..Default::default() }
}

// ============================================================== group A: GDI ==
// Verbatim copy of the shipping Canvas (src/main.rs). Do not "improve" it: the
// whole point is that group A is the code that is on screen today.

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

fn make_font(px: i32, weight: i32) -> HGDIOBJ {
    unsafe {
        CreateFontW(px, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 3, 0, w!("Microsoft YaHei UI")).into()
    }
}

impl Canvas {
    unsafe fn new_scaled(w: i32, h: i32, ss: i32, font_scale: f64) -> Canvas {
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
                title: make_font(-(15.0 * font_scale * ss as f64) as i32, 700),
                label: make_font(-(13.0 * font_scale * ss as f64) as i32, 400),
                button: make_font(-(13.0 * font_scale * ss as f64) as i32, 500),
                value: make_font(-(12.0 * font_scale * ss as f64) as i32, 400),
                badge: make_font(-(14.0 * font_scale * ss as f64) as i32, 600),
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
        let _ = RoundRect(
            self.hdc_big,
            r.left * s,
            r.top * s,
            r.right * s,
            r.bottom * s,
            radius * 2 * s,
            radius * 2 * s,
        );
        SelectObject(self.hdc_big, prev_pen);
        SelectObject(self.hdc_big, prev_br);
        let _ = DeleteObject(br);
    }

    unsafe fn stroke_round(&self, r: RECT, radius: i32, rgb: u32, width: i32) {
        let s = self.ss;
        let pen = CreatePen(PS_SOLID, width * s, col(rgb));
        let prev_pen = SelectObject(self.hdc_big, pen);
        let prev_br = SelectObject(self.hdc_big, GetStockObject(NULL_BRUSH));
        let _ = RoundRect(
            self.hdc_big,
            r.left * s,
            r.top * s,
            r.right * s,
            r.bottom * s,
            radius * 2 * s,
            radius * 2 * s,
        );
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
        let prev_br = SelectObject(self.hdc_big, GetStockObject(NULL_BRUSH));
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

    /// Ring/circle in supersampled device space (already multiplied by `ss`).
    unsafe fn ring_circle_ss(&self, cx: i32, cy: i32, rad: i32, width: i32, rgb: u32) {
        let pen = CreatePen(PS_SOLID, width, col(rgb));
        let prev_pen = SelectObject(self.hdc_big, pen);
        let prev_br = SelectObject(self.hdc_big, GetStockObject(NULL_BRUSH));
        let _ = Ellipse(self.hdc_big, cx - rad, cy - rad, cx + rad, cy + rad);
        SelectObject(self.hdc_big, prev_pen);
        SelectObject(self.hdc_big, prev_br);
        let _ = DeleteObject(pen);
    }

    unsafe fn fill_circle_ss(&self, cx: i32, cy: i32, rad: i32, rgb: u32) {
        let br = CreateSolidBrush(col(rgb));
        let prev_pen = SelectObject(self.hdc_big, GetStockObject(NULL_PEN));
        let prev_br = SelectObject(self.hdc_big, br);
        let _ = Ellipse(self.hdc_big, cx - rad, cy - rad, cx + rad, cy + rad);
        SelectObject(self.hdc_big, prev_pen);
        SelectObject(self.hdc_big, prev_br);
        let _ = DeleteObject(br);
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

    /// Transcribed from src/main.rs — the exact function being compared against.
    unsafe fn compose(&self, w: i32, h: i32, alpha: f64) {
        let s = self.ss as usize;
        let big = std::slice::from_raw_parts(
            self.bits_big,
            (self.w * self.ss * self.h * self.ss * 4) as usize,
        );
        let small =
            std::slice::from_raw_parts_mut(self.bits_small, (self.w * self.h * 4) as usize);
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

    fn small_mut(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.bits_small, (self.w * self.h * 4) as usize) }
    }
}

/// The whale body + badge, transcribed from `draw_cursor`'s default path.
unsafe fn cursor_a(cv: &Canvas, label: &str) {
    cv.clear();
    let s = SS as f64;
    let cx = (TIP_X * SS as f64) as i32;
    let cy = (TIP_Y * SS as f64) as i32;
    let p = |dx: f64, dy: f64| POINT { x: cx + (dx * s) as i32, y: cy + (dy * s) as i32 };

    let accent = ACCENT & 0xFFFFFF;
    let halo = accent;
    let belly = shade(accent, 1.72);
    let dark = shade(accent, 0.34);
    let ring = CreatePen(PS_SOLID, (2.0 * s) as i32, col(dark));
    let thin = CreatePen(PS_SOLID, s as i32, col(dark));
    let halo_br = CreateSolidBrush(col(halo));
    let belly_br = CreateSolidBrush(col(belly));
    let dark_br = CreateSolidBrush(col(dark));

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
    SelectObject(cv.hdc_big, halo_br);
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
    SelectObject(cv.hdc_big, halo_br);
    cv.fill_circle_ss(cx, cy, (5.0 * s) as i32, dark);
    cv.fill_circle_ss(cx, cy, (3.0 * s) as i32, belly);

    // badge: rounded rect + accent stroke + text (the "圆角底" of a cursor frame)
    let prev = SelectObject(cv.hdc_big, cv.fonts.badge);
    let buf: Vec<u16> = label.encode_utf16().collect();
    let mut ext = SIZE::default();
    let _ = GetTextExtentPoint32W(cv.hdc_big, &buf, &mut ext);
    SelectObject(cv.hdc_big, prev);
    let pad = (10.0 * s) as i32;
    let bh = (26.0 * s) as i32;
    let bx = cx - (6.0 * s) as i32;
    let by = cy + (44.0 * s) as i32;
    let bw = ext.cx + pad * 2;
    let br = CreateSolidBrush(col(0x0E151E));
    let pen = CreatePen(PS_SOLID, (2.0 * s) as i32, col(accent));
    let prev_pen = SelectObject(cv.hdc_big, pen);
    let prev_br = SelectObject(cv.hdc_big, br);
    let _ = RoundRect(
        cv.hdc_big,
        bx,
        by,
        bx + bw,
        by + bh,
        (10.0 * s) as i32,
        (10.0 * s) as i32,
    );
    SelectObject(cv.hdc_big, prev_pen);
    SelectObject(cv.hdc_big, prev_br);
    let _ = DeleteObject(br);
    let _ = DeleteObject(pen);

    let fx = (bx + pad) / SS;
    let fy = (by + (bh - ext.cy) / 2) / SS;
    cv.text_at(cv.fonts.badge, fx, fy, label, 0xEBF8FF);

    let _ = DeleteObject(ring);
    let _ = DeleteObject(thin);
    let _ = DeleteObject(halo_br);
    let _ = DeleteObject(belly_br);
    let _ = DeleteObject(dark_br);
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Header,
    Section,
    Label,
    LabelRight,
    Button,
    Slider,
    Swatch,
    Channel,
    Wheel,
    Preview,
    Sep,
    Chip,
}

impl Kind {
    fn parse(s: &str) -> Kind {
        match s {
            "Header" => Kind::Header,
            "Section" => Kind::Section,
            "Label" => Kind::Label,
            "LabelRight" => Kind::LabelRight,
            "Button" => Kind::Button,
            "Slider" => Kind::Slider,
            "Swatch" => Kind::Swatch,
            "Channel" => Kind::Channel,
            "Wheel" => Kind::Wheel,
            "Preview" => Kind::Preview,
            "Sep" => Kind::Sep,
            _ => Kind::Chip,
        }
    }
}

#[derive(Clone)]
struct Widget {
    id: String,
    kind: Kind,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    text: String,
    active: bool,
}

fn load_widgets(path: &str) -> Vec<Widget> {
    let txt = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let v: serde_json::Value = serde_json::from_str(&txt).expect("panel-hits.json");
    let hits = v["hits"].as_array().expect("hits[]");
    hits.iter()
        .map(|h| Widget {
            id: h["id"].as_str().unwrap_or("").to_string(),
            kind: Kind::parse(h["kind"].as_str().unwrap_or("")),
            x: h["x"].as_i64().unwrap_or(0) as i32,
            y: h["y"].as_i64().unwrap_or(0) as i32,
            w: h["w"].as_i64().unwrap_or(0) as i32,
            h: h["h"].as_i64().unwrap_or(0) as i32,
            text: h["text"].as_str().unwrap_or("").to_string(),
            active: h["active"].as_bool().unwrap_or(false),
        })
        .collect()
}

/// The panel's per-widget paint, transcribed from `draw_panel`'s match arms.
unsafe fn panel_a(cv: &Canvas, ws: &[Widget], h: i32) {
    cv.clear();
    let body = rect(0, 0, PW, h);
    cv.fill_round(body, 14, C_PANEL);
    cv.stroke_round(body, 14, C_BORDER, 1);
    cv.fill_round(rect(1, 1, PW - 1, 40), 13, C_PANEL_2);
    cv.fill_rect(rect(1, 30, PW - 1, 40), C_PANEL_2);

    for w in ws {
        let r = rect(w.x, w.y, w.x + w.w, w.y + w.h);
        match w.kind {
            Kind::Sep => {
                cv.fill_rect(rect(r.left, r.top, r.right, r.top + 1), C_SEP);
            }
            Kind::Label | Kind::LabelRight => {
                let font = if w.id == "title" { cv.fonts.title } else { cv.fonts.value };
                let rgb = if w.id == "title" { C_TEXT } else { C_MUTED };
                if w.kind == Kind::LabelRight {
                    let tw = cv.text_w(font, &w.text);
                    cv.text_at(font, r.right - tw, r.top, &w.text, rgb);
                } else {
                    cv.text_at(font, r.left, r.top, &w.text, rgb);
                }
            }
            Kind::Button => {
                let bg = if w.active { ACCENT } else { C_BTN };
                cv.fill_round(r, 8, bg);
                if w.active {
                    cv.stroke_round(r, 8, shade(ACCENT, 1.35), 1);
                }
                let fg = if w.active { C_DARK_TEXT } else { C_TEXT };
                let tw = cv.text_w(cv.fonts.button, &w.text);
                let tx = r.left + (r.right - r.left - tw) / 2;
                let ty = r.top + (r.bottom - r.top - 17) / 2;
                cv.text_at(cv.fonts.button, tx, ty, &w.text, fg);
            }
            Kind::Slider => {
                let cy = (r.top + r.bottom) / 2;
                let (a, b) = (r.left + 9, r.right - 9);
                cv.fill_round(rect(a, cy - 2, b, cy + 2), 2, C_TRACK);
                let value = 0.62;
                let kx = a + ((b - a) as f64 * value) as i32;
                cv.fill_round(rect(a, cy - 2, kx, cy + 2), 2, ACCENT);
                cv.fill_circle(kx, cy, 7, shade(ACCENT, 1.5));
                cv.ring_circle(kx, cy, 7, 1, ACCENT);
            }
            Kind::Swatch => {
                let cx = (r.left + r.right) / 2;
                let cy = (r.top + r.bottom) / 2;
                let rgb =
                    u32::from_str_radix(w.text.trim_start_matches('#'), 16).unwrap_or(0xFFFFFF);
                cv.fill_circle(cx, cy, 11, rgb);
                if w.active {
                    cv.ring_circle(cx, cy, 13, 2, 0xFFFFFF);
                }
            }
            Kind::Section => {
                if w.active {
                    cv.fill_round(r, 6, C_BTN_HOVER);
                }
                cv.text_at(cv.fonts.button, r.left + 2, r.top + 3, &w.text, C_MUTED);
            }
            Kind::Channel => {
                let cy = (r.top + r.bottom) / 2;
                let track = rect(r.left, cy - 5, r.right, cy + 5);
                let ch = 1usize; // green ramp
                let mut rgb = [0u32; 3];
                rgb[ch] = 255;
                let full = (rgb[0] << 16) | (rgb[1] << 8) | rgb[2];
                let wd = track.right - track.left;
                let steps = 48;
                for i in 0..steps {
                    let a = i as f64 / (steps - 1) as f64;
                    let x0 = track.left + (wd * i) / steps;
                    let x1 = track.left + (wd * (i + 1)) / steps;
                    let ch_fn = |v: u32| -> u32 {
                        let x = v as f64 * a;
                        x.round().clamp(0.0, 255.0) as u32
                    };
                    let c = (ch_fn((full >> 16) & 0xFF) << 16)
                        | (ch_fn((full >> 8) & 0xFF) << 8)
                        | ch_fn(full & 0xFF);
                    cv.fill_rect(rect(x0, track.top, x1, track.bottom), c);
                }
                let kx = track.left + (wd as f64 * 0.62) as i32;
                cv.fill_circle(kx, cy, 6, 0xFFFFFF);
                cv.ring_circle(kx, cy, 6, 1, 0x101820);
            }
            Kind::Wheel => {
                // The real wheel is a pre-rendered bitmap StretchBlt'ed in; the
                // probe draws its geometry so both groups pay a comparable cost.
                let side = (r.right - r.left).max(1);
                let cx = r.left + side / 2;
                let cy = r.top + side / 2;
                let k = side as f64 / 160.0;
                cv.fill_circle(cx, cy, (60.0 * k) as i32, 0x1A2230);
                cv.ring_circle(cx, cy, (60.0 * k) as i32, (18.0 * k).max(1.0) as i32, ACCENT);
                cv.fill_rect(
                    rect(
                        cx - (40.0 * k) as i32,
                        cy - (40.0 * k) as i32,
                        cx + (40.0 * k) as i32,
                        cy + (40.0 * k) as i32,
                    ),
                    0x25303F,
                );
                cv.ring_circle(
                    cx + (20.0 * k) as i32,
                    cy - (18.0 * k) as i32,
                    (6.0 * k).max(2.0) as i32,
                    2,
                    0xFFFFFF,
                );
                cv.ring_circle(
                    cx - (14.0 * k) as i32,
                    cy + (12.0 * k) as i32,
                    (5.0 * k).max(2.0) as i32,
                    2,
                    0xFFFFFF,
                );
            }
            Kind::Preview => {
                // The thumbnail itself is blitted after compose (draw_previews);
                // both groups pay that in the shared post pass.
                cv.fill_round(r, 6, 0x101820);
                cv.stroke_round(r, 6, C_BORDER, 1);
            }
            Kind::Header => {
                cv.text_at(cv.fonts.title, r.left, r.top, &w.text, C_TEXT);
            }
            Kind::Chip => {}
        }
    }

    // scroll indicator (only chrome, never clickable)
    let top = 45;
    let bottom = h - 4;
    let track_h = (bottom - top) as f64;
    let thumb_h = (track_h * 0.58) as i32;
    cv.fill_round(rect(PW - 7, top, PW - 3, bottom), 2, 0x1A2230);
    cv.fill_round(rect(PW - 7, top, PW - 3, top + thumb_h), 2, 0x3A4A5E);
}

// ========================================================= group B: tiny-skia ==

/// Glyph masks rasterised once and cached. A real port would do exactly this:
/// per-frame glyph rasterisation would be absurd, and GDI caches fonts too.
struct Glyphs {
    font: fontdue::Font,
    cache: HashMap<(char, u32), Glyph>,
    ascent: f32,
}

struct Glyph {
    w: usize,
    h: usize,
    left: i32,
    top: i32, // offset from the baseline
    cov: Vec<u8>,
    advance: f32,
}

impl Glyphs {
    fn new(px: f32) -> Glyphs {
        // Microsoft YaHei UI is what the GDI path selects in CreateFontW.
        let mut picked = None;
        for path in [
            "C:/Windows/Fonts/msyh.ttc",
            "C:/Windows/Fonts/msyhbd.ttc",
            "C:/Windows/Fonts/simhei.ttf",
            "C:/Windows/Fonts/segoeui.ttf",
        ] {
            let Ok(bytes) = std::fs::read(path) else { continue };
            let mut settings = fontdue::FontSettings::default();
            settings.collection_index = 0;
            match fontdue::Font::from_bytes(bytes, settings) {
                Ok(f) => {
                    let ascent =
                        f.horizontal_line_metrics(px).map(|m| m.ascent).unwrap_or(px * 0.8);
                    println!("  text: {path} px={px} ascent={ascent:.1}");
                    picked = Some((f, ascent));
                    break;
                }
                Err(e) => println!("  text: {path} rejected: {e}"),
            }
        }
        let (font, ascent) = picked.expect("a usable system font");
        Glyphs { font, cache: HashMap::new(), ascent }
    }

    fn get(&mut self, c: char, px: f32) -> &Glyph {
        let key = (c, px as u32);
        if !self.cache.contains_key(&key) {
            let (m, bitmap) = self.font.rasterize(c, px);
            let g = Glyph {
                w: m.width,
                h: m.height,
                left: m.xmin,
                top: -m.ymin - m.height as i32,
                cov: bitmap,
                advance: m.advance_width,
            };
            self.cache.insert(key, g);
        }
        self.cache.get(&key).unwrap()
    }

    /// Advance width of a whole string: the tiny-skia-side answer to `text_w`.
    fn width(&mut self, s: &str, px: f32) -> i32 {
        s.chars().map(|c| self.get(c, px).advance).sum::<f32>().round() as i32
    }

    /// Draw `s` with its top-left at (x, y) — the `TextOutW` convention.
    fn draw(&mut self, pm: &mut Pixmap, x: i32, y: i32, s: &str, px: f32, rgb: u32) {
        let (sr, sg, sb) = rgb_parts(rgb);
        let baseline = y as f32 + self.ascent;
        let w = pm.width() as i32;
        let h = pm.height() as i32;
        let data = pm.data_mut();
        let mut pen = x as f32;
        for c in s.chars() {
            let g = self.get(c, px);
            let gx = pen.round() as i32 + g.left;
            let gy = baseline.round() as i32 + g.top;
            for row in 0..g.h {
                let py = gy + row as i32;
                if py < 0 || py >= h {
                    continue;
                }
                for col in 0..g.w {
                    let pxx = gx + col as i32;
                    if pxx < 0 || pxx >= w {
                        continue;
                    }
                    let cov = g.cov[row * g.w + col] as u32;
                    if cov == 0 {
                        continue;
                    }
                    let o = ((py as usize) * (w as usize) + pxx as usize) * 4;
                    // source-over of an opaque colour carrying coverage, in the
                    // pixmap's own premultiplied space
                    let inv = 255 - cov;
                    data[o] = ((sr as u32 * cov + data[o] as u32 * inv) / 255) as u8;
                    data[o + 1] = ((sg as u32 * cov + data[o + 1] as u32 * inv) / 255) as u8;
                    data[o + 2] = ((sb as u32 * cov + data[o + 2] as u32 * inv) / 255) as u8;
                    data[o + 3] = (cov + data[o + 3] as u32 * inv / 255) as u8;
                }
            }
            pen += g.advance;
        }
    }
}

const KAPPA: f32 = 0.552_284_8;

fn round_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    let k = r * KAPPA;
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn poly_path(pts: &[(f32, f32)]) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        pb.line_to(p.0, p.1);
    }
    pb.close();
    pb.finish()
}

fn sk_fill_round(pm: &mut Pixmap, x: i32, y: i32, w: i32, h: i32, radius: i32, rgb: u32, a: u8) {
    if let Some(p) = round_rect_path(x as f32, y as f32, w as f32, h as f32, radius as f32) {
        pm.fill_path(&p, &solid(rgb, a), FillRule::Winding, Transform::identity(), None);
    }
}

fn sk_stroke_round(
    pm: &mut Pixmap,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    rgb: u32,
    width: f32,
    a: u8,
) {
    if let Some(p) = round_rect_path(x as f32, y as f32, w as f32, h as f32, radius as f32) {
        let stroke = Stroke { width, ..Default::default() };
        pm.stroke_path(&p, &solid(rgb, a), &stroke, Transform::identity(), None);
    }
}

fn sk_fill_rect(pm: &mut Pixmap, x: i32, y: i32, w: i32, h: i32, rgb: u32, a: u8) {
    if w <= 0 || h <= 0 {
        return;
    }
    let r = SkRect::from_ltrb(x as f32, y as f32, (x + w) as f32, (y + h) as f32).unwrap();
    pm.fill_rect(r, &solid(rgb, a), Transform::identity(), None);
}

fn sk_fill_circle(pm: &mut Pixmap, cx: i32, cy: i32, rad: i32, rgb: u32, a: u8) {
    if let Some(p) = PathBuilder::from_circle(cx as f32, cy as f32, rad as f32) {
        pm.fill_path(&p, &solid(rgb, a), FillRule::Winding, Transform::identity(), None);
    }
}

fn sk_ring_circle(pm: &mut Pixmap, cx: i32, cy: i32, rad: i32, width: f32, rgb: u32, a: u8) {
    if let Some(p) = PathBuilder::from_circle(cx as f32, cy as f32, rad as f32) {
        pm.stroke_path(&p, &solid(rgb, a), &Stroke { width, ..Default::default() },
                       Transform::identity(), None);
    }
}

fn sk_fill_poly(pm: &mut Pixmap, pts: &[(f32, f32)], rgb: u32, a: u8) {
    if let Some(p) = poly_path(pts) {
        pm.fill_path(&p, &solid(rgb, a), FillRule::Winding, Transform::identity(), None);
    }
}

fn sk_stroke_poly(pm: &mut Pixmap, pts: &[(f32, f32)], rgb: u32, width: f32) {
    if let Some(p) = poly_path(pts) {
        pm.stroke_path(&p, &solid(rgb, 255), &Stroke { width, ..Default::default() },
                       Transform::identity(), None);
    }
}

fn sk_line(pm: &mut Pixmap, x0: i32, y0: i32, x1: i32, y1: i32, rgb: u32, width: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(x0 as f32, y0 as f32);
    pb.line_to(x1 as f32, y1 as f32);
    if let Some(p) = pb.finish() {
        pm.stroke_path(&p, &solid(rgb, 255), &Stroke { width, ..Default::default() },
                       Transform::identity(), None);
    }
}

/// Cursor frame, group B. Same geometry as `cursor_a`, at 1:1 — tiny-skia's
/// coverage AA replaces the SS=2 supersampling, so there is no 4x sample cost.
fn cursor_b(pm: &mut Pixmap, g: &mut Glyphs, label: &str, text: bool) {
    pm.fill(tiny_skia::Color::TRANSPARENT);
    let sx = TIP_X as f32;
    let sy = TIP_Y as f32;
    let p = |dx: f32, dy: f32| (sx + dx, sy + dy);
    let accent = ACCENT & 0xFFFFFF;
    let belly = shade(accent, 1.72);
    let dark = shade(accent, 0.34);

    let tail = [p(-34.0, 16.0), p(-56.0, -2.0), p(-50.0, 28.0)];
    let body = [
        p(-38.0, 12.0),
        p(-30.0, 4.0),
        p(-4.0, 8.0),
        p(10.0, 18.0),
        p(2.0, 34.0),
        p(-26.0, 34.0),
    ];
    sk_fill_poly(pm, &tail, accent, 255);
    sk_stroke_poly(pm, &tail, dark, 2.0);
    sk_fill_poly(pm, &body, accent, 255);
    sk_stroke_poly(pm, &body, dark, 2.0);
    sk_fill_poly(
        pm,
        &[p(-28.0, 24.0), p(4.0, 20.0), p(-2.0, 32.0), p(-24.0, 32.0)],
        belly,
        255,
    );
    sk_fill_circle(pm, sx as i32, (sy + 16.0) as i32, 5, belly, 255);
    sk_fill_circle(pm, (sx + 1.0) as i32, (sy + 16.0) as i32, 2, dark, 255);
    let lx = sx - 22.0;
    let ly = sy + 6.0;
    for (dx, dy) in [(7.0, -18.0), (0.0, -22.0), (-7.0, -18.0)] {
        sk_line(pm, lx as i32, ly as i32, (lx + dx) as i32, (ly + dy) as i32, dark, 1.0);
    }
    sk_fill_circle(pm, sx as i32, sy as i32, 5, dark, 255);
    sk_fill_circle(pm, sx as i32, sy as i32, 3, belly, 255);

    // badge
    let tw = if text { g.width(label, 14.0) } else { 60 };
    let pad = 10;
    let bh = 26;
    let bx = sx as i32 - 6;
    let by = sy as i32 + 44;
    let bw = tw + pad * 2;
    sk_fill_round(pm, bx, by, bw, bh, 5, 0x0E151E, 255);
    sk_stroke_round(pm, bx, by, bw, bh, 5, accent, 2.0, 255);
    if text {
        g.draw(pm, bx + pad, by + (bh - 14) / 2, label, 14.0, 0xEBF8FF);
    }
}

/// Paint alpha for group B. 255 keeps tiny-skia on its opaque fast path; 250
/// reproduces a naive port that blends every primitive at the frame opacity.
static PAINT_ALPHA: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);

fn panel_b(pm: &mut Pixmap, ws: &[Widget], g: &mut Glyphs, h: i32, text: bool) {
    let ga = PAINT_ALPHA.load(std::sync::atomic::Ordering::Relaxed);
    pm.fill(tiny_skia::Color::TRANSPARENT);
    sk_fill_round(pm, 0, 0, PW, h, 14, C_PANEL, ga);
    sk_stroke_round(pm, 0, 0, PW, h, 14, C_BORDER, 1.0, ga);
    sk_fill_round(pm, 1, 1, PW - 2, 39, 13, C_PANEL_2, ga);
    sk_fill_rect(pm, 1, 30, PW - 2, 10, C_PANEL_2, ga);

    for w in ws {
        let (x0, y0, x1, y1) = (w.x, w.y, w.x + w.w, w.y + w.h);
        match w.kind {
            Kind::Sep => sk_fill_rect(pm, x0, y0, w.w, 1, C_SEP, 250),
            Kind::Label | Kind::LabelRight => {
                let (px, rgb) = if w.id == "title" { (15.0, C_TEXT) } else { (12.0, C_MUTED) };
                if text {
                    if w.kind == Kind::LabelRight {
                        let tw = g.width(&w.text, px);
                        g.draw(pm, x1 - tw, y0, &w.text, px, rgb);
                    } else {
                        g.draw(pm, x0, y0, &w.text, px, rgb);
                    }
                }
            }
            Kind::Button => {
                let bg = if w.active { ACCENT } else { C_BTN };
                sk_fill_round(pm, x0, y0, w.w, w.h, 8, bg, ga);
                if w.active {
                    sk_stroke_round(pm, x0, y0, w.w, w.h, 8, shade(ACCENT, 1.35), 1.0, ga);
                }
                let fg = if w.active { C_DARK_TEXT } else { C_TEXT };
                if text {
                    let tw = g.width(&w.text, 13.0);
                    g.draw(pm, x0 + (w.w - tw) / 2, y0 + (w.h - 17) / 2, &w.text, 13.0, fg);
                }
            }
            Kind::Slider => {
                let cy = (y0 + y1) / 2;
                let (a, b) = (x0 + 9, x1 - 9);
                sk_fill_round(pm, a, cy - 2, b - a, 4, 2, C_TRACK, ga);
                let kx = a + ((b - a) as f64 * 0.62) as i32;
                sk_fill_round(pm, a, cy - 2, kx - a, 4, 2, ACCENT, ga);
                sk_fill_circle(pm, kx, cy, 7, shade(ACCENT, 1.5), ga);
                sk_ring_circle(pm, kx, cy, 7, 1.0, ACCENT, ga);
            }
            Kind::Swatch => {
                let cx = (x0 + x1) / 2;
                let cy = (y0 + y1) / 2;
                let rgb =
                    u32::from_str_radix(w.text.trim_start_matches('#'), 16).unwrap_or(0xFFFFFF);
                sk_fill_circle(pm, cx, cy, 11, rgb, ga);
                if w.active {
                    sk_ring_circle(pm, cx, cy, 13, 2.0, 0xFFFFFF, ga);
                }
            }
            Kind::Section => {
                if w.active {
                    sk_fill_round(pm, x0, y0, w.w, w.h, 6, C_BTN_HOVER, ga);
                }
                if text {
                    g.draw(pm, x0 + 2, y0 + 3, &w.text, 13.0, C_MUTED);
                }
            }
            Kind::Channel => {
                let cy = (y0 + y1) / 2;
                let wd = x1 - x0;
                let steps = 48;
                for i in 0..steps {
                    let a = i as f32 / (steps - 1) as f32;
                    let sx0 = x0 + (wd * i) / steps;
                    let sx1 = x0 + (wd * (i + 1)) / steps;
                    let c = (255.0 * a) as u32; // green ramp
                    sk_fill_rect(pm, sx0, cy - 5, sx1 - sx0, 10, c << 8, ga);
                }
                let kx = x0 + (wd as f64 * 0.62) as i32;
                sk_fill_circle(pm, kx, cy, 6, 0xFFFFFF, ga);
                sk_ring_circle(pm, kx, cy, 6, 1.0, 0x101820, ga);
            }
            Kind::Wheel => {
                let side = w.w.max(1);
                let cx = x0 + side / 2;
                let cy = y0 + side / 2;
                let k = side as f32 / 160.0;
                sk_fill_circle(pm, cx, cy, (60.0 * k) as i32, 0x1A2230, ga);
                sk_ring_circle(pm, cx, cy, (60.0 * k) as i32, (18.0 * k).max(1.0), ACCENT, ga);
                sk_fill_rect(
                    pm,
                    cx - (40.0 * k) as i32,
                    cy - (40.0 * k) as i32,
                    (80.0 * k) as i32,
                    (80.0 * k) as i32,
                    0x25303F,
                    250,
                );
                sk_ring_circle(
                    pm,
                    cx + (20.0 * k) as i32,
                    cy - (18.0 * k) as i32,
                    (6.0 * k).max(2.0) as i32,
                    2.0,
                    0xFFFFFF,
                    250,
                );
                sk_ring_circle(
                    pm,
                    cx - (14.0 * k) as i32,
                    cy + (12.0 * k) as i32,
                    (5.0 * k).max(2.0) as i32,
                    2.0,
                    0xFFFFFF,
                    250,
                );
            }
            Kind::Preview => {
                sk_fill_round(pm, x0, y0, w.w, w.h, 6, 0x101820, ga);
                sk_stroke_round(pm, x0, y0, w.w, w.h, 6, C_BORDER, 1.0, ga);
            }
            Kind::Header => {
                if text {
                    g.draw(pm, x0, y0, &w.text, 15.0, C_TEXT);
                }
            }
            Kind::Chip => {}
        }
    }

    let top = 45;
    let bottom = h - 4;
    let thumb_h = ((bottom - top) as f64 * 0.58) as i32;
    sk_fill_round(pm, PW - 7, top, 4, bottom - top, 2, 0x1A2230, ga);
    sk_fill_round(pm, PW - 7, top, 4, thumb_h, 2, 0x3A4A5E, ga);
}

// ------------------------------------------------------------- the swizzle ---
// tiny-skia hands back premultiplied **RGBA**; UpdateLayeredWindow wants
// premultiplied **BGRA**. Same alpha, same premultiplication, only R and B swap —
// but it is a full pass over the surface, so it is timed on its own.

/// Byte-wise (the obvious way to write it).
fn swizzle_bytes(src: &[u8], dst: &mut [u8]) {
    for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
}

/// Word-wise swizzle that also folds the frame's global opacity in. This is the
/// right place for it: the pass already touches every pixel, whereas asking
/// tiny-skia to blend the whole panel at alpha 250 costs 2.5 ms (see the
/// per-primitive table) because a non-opaque solid fill loses its fast path.
fn swizzle_u32_op(src: &[u8], dst: &mut [u8], opacity: u8) {
    let n = src.len() / 4;
    let s = unsafe { std::slice::from_raw_parts(src.as_ptr() as *const u32, n) };
    let d = unsafe { std::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut u32, n) };
    if opacity == 255 {
        for i in 0..n {
            let v = s[i];
            d[i] = (v & 0xFF00_FF00) | ((v & 0x0000_00FF) << 16) | ((v >> 16) & 0x0000_00FF);
        }
        return;
    }
    // A 256-entry multiply table: the per-channel divide is what made this pass
    // 3x slower than the plain swizzle, and it also keeps every access in L1.
    let mut lut = [0u8; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        *v = ((i as u32 * opacity as u32) / 255) as u8;
    }
    for i in 0..n {
        let v = s[i];
        let sw = (v & 0xFF00_FF00) | ((v & 0x0000_00FF) << 16) | ((v >> 16) & 0x0000_00FF);
        d[i] = (lut[(sw & 0xFF) as usize] as u32)
            | ((lut[((sw >> 8) & 0xFF) as usize] as u32) << 8)
            | ((lut[((sw >> 16) & 0xFF) as usize] as u32) << 16)
            | ((lut[((sw >> 24) & 0xFF) as usize] as u32) << 24);
    }
}

/// Same job without a table, so LLVM can keep the loop vectorisable.
fn swizzle_u32_op_mul(src: &[u8], dst: &mut [u8], opacity: u8) {
    let n = src.len() / 4;
    let op = opacity as u32;
    let s = unsafe { std::slice::from_raw_parts(src.as_ptr() as *const u32, n) };
    let d = unsafe { std::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut u32, n) };
    for i in 0..n {
        let v = s[i];
        let sw = (v & 0xFF00_FF00) | ((v & 0x0000_00FF) << 16) | ((v >> 16) & 0x0000_00FF);
        let b0 = ((sw & 0xFF) * op) / 255;
        let b1 = (((sw >> 8) & 0xFF) * op) / 255;
        let b2 = (((sw >> 16) & 0xFF) * op) / 255;
        let b3 = (((sw >> 24) & 0xFF) * op) / 255;
        d[i] = b0 | (b1 << 8) | (b2 << 16) | (b3 << 24);
    }
}

/// Word-wise: one 32-bit load, three masks, one store. This is what a careful
/// implementation does, and it is the number quoted for group B.
fn swizzle_u32(src: &[u8], dst: &mut [u8]) {
    let n = src.len() / 4;
    let s = unsafe { std::slice::from_raw_parts(src.as_ptr() as *const u32, n) };
    let d = unsafe { std::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut u32, n) };
    for i in 0..n {
        let v = s[i];
        // little-endian RGBA is (A,B,G,R); BGRA is (A,R,G,B) -> swap bytes 0/2
        d[i] = (v & 0xFF00_FF00) | ((v & 0x0000_00FF) << 16) | ((v >> 16) & 0x0000_00FF);
    }
}

// ----------------------------------------------------------------- measuring --

#[derive(Debug, Clone, Copy)]
struct Stats {
    mean: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

impl Stats {
    fn of(mut v: Vec<f64>) -> Stats {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = v.len();
        let at = |q: f64| v[((n as f64 - 1.0) * q).round() as usize];
        Stats { mean: v.iter().sum::<f64>() / n as f64, p50: at(0.50), p95: at(0.95), p99: at(0.99), max: v[n - 1] }
    }
}

/// Cost of one call, in microseconds (for per-primitive attribution).
fn bench_ops<F: FnMut()>(name: &str, n: usize, mut f: F) {
    for _ in 0..20 {
        f();
    }
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    println!("{:<44}{:>10.3} us/op", name, t.elapsed().as_secs_f64() * 1e6 / n as f64);
}

fn bench<F: FnMut()>(warm: usize, n: usize, mut f: F) -> Stats {
    for _ in 0..warm {
        f();
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        f();
        v.push(t.elapsed().as_secs_f64() * 1e6);
    }
    Stats::of(v)
}

// --------------------------------------------------- edge / alpha verification --

/// Composite premultiplied BGRA over a solid background, the way the compositor
/// does it, so the result is what the user would see.
fn over_bg(bgra: &[u8], w: usize, h: usize, bg: (u8, u8, u8)) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 3];
    for i in 0..w * h {
        let a = bgra[i * 4 + 3] as u32;
        let inv = 255 - a;
        out[i * 3] = ((bgra[i * 4 + 2] as u32 * 255 + bg.0 as u32 * inv) / 255) as u8;
        out[i * 3 + 1] = ((bgra[i * 4 + 1] as u32 * 255 + bg.1 as u32 * inv) / 255) as u8;
        out[i * 3 + 2] = ((bgra[i * 4] as u32 * 255 + bg.2 as u32 * inv) / 255) as u8;
    }
    out
}

fn save_rgb_png(path: &str, rgb: &[u8], w: u32, h: u32) {
    let img = image::RgbImage::from_raw(w, h, rgb.to_vec()).expect("rgb image");
    img.save(path).expect("save png");
}

fn dump_rgba(path: &str, rgba: &[u8], w: u32, h: u32) {
    // Demultiplied view for human eyes (tiny-skia's own save_png does the same).
    let mut out = Vec::with_capacity(rgba.len());
    for p in rgba.chunks_exact(4) {
        let a = p[3] as u32;
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            out.push(((p[0] as u32 * 255 + a / 2) / a).min(255) as u8);
            out.push(((p[1] as u32 * 255 + a / 2) / a).min(255) as u8);
            out.push(((p[2] as u32 * 255 + a / 2) / a).min(255) as u8);
            out.push(p[3]);
        }
    }
    let img = image::RgbaImage::from_raw(w, h, out).expect("rgba image");
    img.save(path).expect("save png");
}

/// Pre-multiplied invariant: no colour channel may exceed alpha.
fn premul_violations(rgba: &[u8]) -> (usize, usize) {
    let mut over = 0;
    let mut opaque_edge = 0;
    for p in rgba.chunks_exact(4) {
        let a = p[3] as u32;
        if p[0] as u32 > a || p[1] as u32 > a || p[2] as u32 > a {
            over += 1;
        }
        if a == 0 && (p[0] | p[1] | p[2]) != 0 {
            opaque_edge += 1;
        }
    }
    (over, opaque_edge)
}

// ------------------------------------------------- a real UpdateLayeredWindow --

/// Grab screen pixels straight into a top-down DIB section (no GetDIBits).
unsafe fn capture_screen(x: i32, y: i32, w: i32, h: i32) -> Vec<u8> {
    let screen = GetDC(null_hwnd());
    let mem = CreateCompatibleDC(screen);
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0;
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(mem, &bmi, DIB_USAGE(0), &mut bits, HANDLE::default(), 0)
        .expect("CreateDIBSection(capture)");
    let old = SelectObject(mem, bmp);
    let _ = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    std::ptr::copy_nonoverlapping(bits as *const u8, buf.as_mut_ptr(), buf.len());
    SelectObject(mem, old);
    let _ = DeleteObject(bmp);
    let _ = DeleteDC(mem);
    ReleaseDC(null_hwnd(), screen);
    buf
}

unsafe extern "system" fn plain_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, m, w, l)
}

fn wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

/// A plain solid-colour window to put the layered window in front of.
unsafe fn make_bg(class: &str, brush_rgb: u32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let hinst = GetModuleHandleW(None).unwrap();
    let brush = CreateSolidBrush(col(brush_rgb));
    let name = wide(class);
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(plain_proc),
        hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0),
        hbrBackground: brush,
        lpszClassName: windows::core::PCWSTR(name.as_ptr()),
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc);
    let hwnd = CreateWindowExW(
        Default::default(),
        windows::core::PCWSTR(name.as_ptr()),
        w!("probe backdrop"),
        WS_POPUP,
        x,
        y,
        w,
        h,
        null_hwnd(),
        HMENU(std::ptr::null_mut()),
        windows::Win32::Foundation::HINSTANCE(hinst.0),
        None,
    )
    .expect("CreateWindowExW(bg)");
    let _ = ShowWindow(hwnd, SW_SHOW);
    hwnd
}

unsafe fn make_layered(name: &str, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let hinst = GetModuleHandleW(None).unwrap();
    let cls = wide(name);
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(plain_proc),
        hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0),
        lpszClassName: windows::core::PCWSTR(cls.as_ptr()),
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc);
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
        windows::core::PCWSTR(cls.as_ptr()),
        w!("tinyskia probe"),
        WS_POPUP,
        x,
        y,
        w,
        h,
        null_hwnd(),
        HMENU(std::ptr::null_mut()),
        windows::Win32::Foundation::HINSTANCE(hinst.0),
        None,
    )
    .expect("CreateWindowExW(layered)");
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
    let _ = ShowWindow(hwnd, SW_SHOW);
    hwnd
}

/// Push a premultiplied BGRA buffer at a layered window — the real call the app
/// makes in `Canvas::blit`.
unsafe fn push_layered(hwnd: HWND, bgra: &[u8], w: i32, h: i32, dst: POINT) {
    let screen = GetDC(null_hwnd());
    let mem = CreateCompatibleDC(screen);
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0;
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(mem, &bmi, DIB_USAGE(0), &mut bits, HANDLE::default(), 0)
        .expect("CreateDIBSection(push)");
    std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
    let old = SelectObject(mem, bmp);

    let size = SIZE { cx: w, cy: h };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let r = UpdateLayeredWindow(
        hwnd,
        screen,
        Some(&dst),
        Some(&size),
        mem,
        Some(&src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
    if let Err(e) = r {
        println!("  UpdateLayeredWindow failed: {e:?}");
    }

    SelectObject(mem, old);
    let _ = DeleteObject(bmp);
    let _ = DeleteDC(mem);
    ReleaseDC(null_hwnd(), screen);
}

// ------------------------------------------------------ shared sprite / post ---
/// One 64x64 RGBA blob used by both groups (renderer-agnostic, identical code).
fn make_sprite() -> Vec<u8> {
    let n = 64usize;
    let mut v = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let dx = (x as f64 - 31.5) / 31.5;
            let dy = (y as f64 - 31.5) / 31.5;
            let d = (dx * dx + dy * dy).sqrt();
            let a = (1.0 - d).clamp(0.0, 1.0);
            let o = (y * n + x) * 4;
            v[o] = (120.0 + 100.0 * dx).clamp(0.0, 255.0) as u8;
            v[o + 1] = (200.0 * a).clamp(0.0, 255.0) as u8;
            v[o + 2] = (255.0 * (1.0 - a * 0.5)).clamp(0.0, 255.0) as u8;
            v[o + 3] = (a * 220.0) as u8;
        }
    }
    v
}

/// Bilinear, premultiplied, plain over-blend: `sprite.rs::composite`'s maths with
/// the byte order parameterised, so both groups run the identical kernel.
#[allow(clippy::too_many_arguments)]
fn sprite_blend(
    dst: &mut [u8],
    dw: i32,
    dh: i32,
    dst_x: i32,
    dst_y: i32,
    src: &[u8],
    sw: u32,
    sh: u32,
    tw: u32,
    th: u32,
    opacity: f64,
    bgra: bool,
) {
    if tw == 0 || th == 0 || sw == 0 || sh == 0 || opacity <= 0.0 {
        return;
    }
    let sx_scale = sw as f64 / tw as f64;
    let sy_scale = sh as f64 / th as f64;
    for ty in 0..th as i32 {
        let py = dst_y + ty;
        if py < 0 || py >= dh {
            continue;
        }
        let v = (ty as f64 + 0.5) * sy_scale - 0.5;
        let y0 = v.floor().max(0.0) as i64;
        let y1 = (y0 + 1).min(sh as i64 - 1);
        let fy = (v - y0 as f64).clamp(0.0, 1.0);
        for tx in 0..tw as i32 {
            let px = dst_x + tx;
            if px < 0 || px >= dw {
                continue;
            }
            let u = (tx as f64 + 0.5) * sx_scale - 0.5;
            let x0 = u.floor().max(0.0) as i64;
            let x1 = (x0 + 1).min(sw as i64 - 1);
            let fx = (u - x0 as f64).clamp(0.0, 1.0);
            let mut acc = [0.0f64; 4];
            for (xi, yy, wgt) in [
                (x0, y0, (1.0 - fx) * (1.0 - fy)),
                (x1, y0, fx * (1.0 - fy)),
                (x0, y1, (1.0 - fx) * fy),
                (x1, y1, fx * fy),
            ] {
                if wgt <= 0.0 {
                    continue;
                }
                let o = ((yy as u32 * sw + xi as u32) * 4) as usize;
                let a = src[o + 3] as f64 / 255.0;
                acc[0] += src[o] as f64 * a * wgt;
                acc[1] += src[o + 1] as f64 * a * wgt;
                acc[2] += src[o + 2] as f64 * a * wgt;
                acc[3] += a * wgt;
            }
            let o = ((py as usize) * (dw as usize) + px as usize) * 4;
            let ia = 1.0 - acc[3];
            let put = |d: &mut [u8], off: usize, add: f64| {
                d[off] = (add + d[off] as f64 * ia).clamp(0.0, 255.0) as u8;
            };
            if bgra {
                put(dst, o, acc[2]);
                put(dst, o + 1, acc[1]);
                put(dst, o + 2, acc[0]);
            } else {
                put(dst, o, acc[0]);
                put(dst, o + 1, acc[1]);
                put(dst, o + 2, acc[2]);
            }
            put(dst, o + 3, acc[3] * 255.0);
        }
    }
}

fn fmt_us(v: f64) -> String {
    format!("{v:9.1}")
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let frames: usize = std::env::var("FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(1000);
    let hits_path = std::env::var("PROBE_HITS").unwrap_or_else(|_| {
        for p in ["../artifacts/tinyskia/panel-hits.json", "artifacts/tinyskia/panel-hits.json"] {
            if std::path::Path::new(p).exists() {
                return p.to_string();
            }
        }
        "../artifacts/tinyskia/panel-hits.json".to_string()
    });
    let out_dir = std::env::var("PROBE_OUT").unwrap_or_else(|_| "../artifacts/tinyskia".to_string());
    std::fs::create_dir_all(&out_dir).ok();

    let widgets = load_widgets(&hits_path);
    let sprite = make_sprite();
    let sw = unsafe { GetSystemMetrics(SYSTEM_METRICS_INDEX(SM_CXSCREEN.0)) };
    let sh = unsafe { GetSystemMetrics(SYSTEM_METRICS_INDEX(SM_CYSCREEN.0)) };
    println!("=== tiny-skia feasibility probe ===");
    println!("screen {sw}x{sh}, widgets {} from {hits_path}", widgets.len());
    println!("frames per case: {frames}\n");

    let cursor_a_cv = unsafe { Canvas::new_scaled(W, H, SS, 1.0) };
    let panel_a_cv = unsafe { Canvas::new_scaled(PW, PH, SS_PANEL, 1.0) };
    let mut cursor_b_buf = vec![0u8; (W * H * 4) as usize];
    let mut panel_b_buf = vec![0u8; (PW * PH * 4) as usize];

    let mut glyphs = Glyphs::new(13.0);
    let mut cursor_pm = Pixmap::new(W as u32, H as u32).unwrap();
    let mut panel_pm = Pixmap::new(PW as u32, PH as u32).unwrap();

    println!("-- correctness / frame A-vs-B --");
    unsafe { cursor_a(&cursor_a_cv, "AI 鼠标") };
    unsafe { cursor_a_cv.compose(W, H, 0.98) };
    cursor_b(&mut cursor_pm, &mut glyphs, "AI 鼠标", true);
    swizzle_u32_op(cursor_pm.data(), &mut cursor_b_buf, 250);
    unsafe { panel_a(&panel_a_cv, &widgets, PH) };
    unsafe { panel_a_cv.compose(PW, PH, 0.98) };
    panel_b(&mut panel_pm, &widgets, &mut glyphs, PH, true);
    swizzle_u32_op(panel_pm.data(), &mut panel_b_buf, 250);

    let a_px = cursor_a_cv.small_mut();
    let (mut diff, mut maxdiff, mut nonzero, mut cover_a, mut cover_b) = (0u64, 0i32, 0u64, 0u64, 0u64);
    for i in 0..(W * H) as usize {
        for c in 0..4 {
            let d = (a_px[i * 4 + c] as i32 - cursor_b_buf[i * 4 + c] as i32).abs();
            diff += d as u64;
            maxdiff = maxdiff.max(d);
            if d > 8 {
                nonzero += 1;
            }
        }
        if a_px[i * 4 + 3] > 0 {
            cover_a += 1;
        }
        if cursor_b_buf[i * 4 + 3] > 0 {
            cover_b += 1;
        }
    }
    println!(
        "cursor A vs B: mean |d| {:.2}/255, max {maxdiff}, channels off by >8: {nonzero}/{}",
        diff as f64 / (W * H * 4) as f64,
        W * H * 4
    );
    println!("cursor coverage: A {cover_a} px, B {cover_b} px");

    println!("\n-- premultiplied-alpha invariants (tiny-skia Pixmap is premul RGBA) --");
    let (over, opaque_edge) = premul_violations(cursor_pm.data());
    println!("cursor: channels exceeding alpha = {over}, alpha=0 with colour = {opaque_edge}");
    let (pover, popaque) = premul_violations(panel_pm.data());
    println!("panel : channels exceeding alpha = {pover}, alpha=0 with colour = {popaque}");

    let corner = |pm: &Pixmap, x: u32, y: u32| -> [u8; 4] {
        let o = ((y * pm.width() + x) * 4) as usize;
        [pm.data()[o], pm.data()[o + 1], pm.data()[o + 2], pm.data()[o + 3]]
    };
    println!(
        "cursor corner rgba: TL={:?} TR={:?} BL={:?} BR={:?}",
        corner(&cursor_pm, 0, 0),
        corner(&cursor_pm, W as u32 - 1, 0),
        corner(&cursor_pm, 0, H as u32 - 1),
        corner(&cursor_pm, W as u32 - 1, H as u32 - 1)
    );
    let mut edge_pixels = 0usize;
    let mut worst = 0i32;
    for p in cursor_pm.data().chunks_exact(4) {
        let a = p[3] as i32;
        if a > 0 && a < 255 {
            edge_pixels += 1;
            for c in 0..3 {
                worst = worst.max(p[c] as i32 - a);
            }
        }
    }
    println!("cursor partial-alpha (AA) pixels {edge_pixels}, worst (channel - alpha) {worst}");

    // ------------------------------------------------------------- artefacts
    let cy_white = over_bg(&cursor_b_buf, W as usize, H as usize, (255, 255, 255));
    let cy_black = over_bg(&cursor_b_buf, W as usize, H as usize, (0, 0, 0));
    save_rgb_png(&format!("{out_dir}/tinyskia-cursor-over-white.png"), &cy_white, W as u32, H as u32);
    save_rgb_png(&format!("{out_dir}/tinyskia-cursor-over-black.png"), &cy_black, W as u32, H as u32);
    dump_rgba(&format!("{out_dir}/tinyskia-cursor-straight.png"), cursor_pm.data(), W as u32, H as u32);
    let a_white = over_bg(a_px, W as usize, H as usize, (255, 255, 255));
    save_rgb_png(&format!("{out_dir}/gdi-cursor-over-white.png"), &a_white, W as u32, H as u32);
    let p_white = over_bg(&panel_b_buf, PW as usize, PH as usize, (255, 255, 255));
    let p_black = over_bg(&panel_b_buf, PW as usize, PH as usize, (0, 0, 0));
    save_rgb_png(&format!("{out_dir}/tinyskia-panel.png"), &p_white, PW as u32, PH as u32);
    save_rgb_png(&format!("{out_dir}/tinyskia-panel-over-black.png"), &p_black, PW as u32, PH as u32);
    let pa_px = panel_a_cv.small_mut();
    let pa_white = over_bg(pa_px, PW as usize, PH as usize, (255, 255, 255));
    save_rgb_png(&format!("{out_dir}/gdi-panel.png"), &pa_white, PW as u32, PH as u32);
    println!("artefacts written under {out_dir}/");

    // -------------------------------------------- real UpdateLayeredWindow test
    println!("\n-- real layered window over controlled backdrops --");
    let mut caps: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    unsafe {
        for (i, (tag, bg)) in [("white", 0xFFFFFFu32), ("black", 0x000000u32)].into_iter().enumerate()
        {
            let (bw, bh, bx, by) = (420, 300, 240, 160);
            let bgw = make_bg(&format!("TskProbeBg{tag}"), bg, bx, by, bw, bh);
            let lw = make_layered(&format!("TskProbeLw{tag}"), bx + 30, by + 30, W, H);
            std::thread::sleep(std::time::Duration::from_millis(300));
            push_layered(lw, &cursor_b_buf, W, H, POINT { x: bx + 30, y: by + 30 });
            std::thread::sleep(std::time::Duration::from_millis(300));
            let cap = capture_screen(bx + 30, by + 30, W, H);
            let mut rgb = vec![0u8; (W * H * 3) as usize];
            for p in 0..(W * H) as usize {
                rgb[p * 3] = cap[p * 4 + 2];
                rgb[p * 3 + 1] = cap[p * 4 + 1];
                rgb[p * 3 + 2] = cap[p * 4];
            }
            save_rgb_png(&format!("{out_dir}/layered-{tag}.png"), &rgb, W as u32, H as u32);

            let px = |x: usize, y: usize| -> (u8, u8, u8) {
                let o = (y * W as usize + x) * 4;
                (cap[o + 2], cap[o + 1], cap[o])
            };
            let bg_rgb = if bg == 0xFFFFFF { (255u8, 255u8, 255u8) } else { (0, 0, 0) };
            let mut inked = 0usize;
            for p in 0..(W as usize * H as usize) {
                let c = (cap[p * 4 + 2], cap[p * 4 + 1], cap[p * 4]);
                if (c.0 as i32 - bg_rgb.0 as i32).abs() > 2
                    || (c.1 as i32 - bg_rgb.1 as i32).abs() > 2
                    || (c.2 as i32 - bg_rgb.2 as i32).abs() > 2
                {
                    inked += 1;
                }
            }
            println!(
                "  over {tag}: corner(2,2)={:?} corner(355,235)={:?} badge-inside(200,100)={:?}",
                px(2, 2),
                px(W as usize - 5, H as usize - 5),
                px(200, 100)
            );
            println!("    non-backdrop pixels on screen: {inked} (frame coverage {cover_b})");

            let mut ramp = String::new();
            for x in 186..206usize {
                let c = px(x, 101);
                ramp.push_str(&format!("{x}:({},{},{}) ", c.0, c.1, c.2));
            }
            println!("    y=101 ramp across badge left edge: {ramp}");
            let mut corner_ramp = String::new();
            for y in 86..97usize {
                let c = px(196, y);
                corner_ramp.push_str(&format!("{y}:({},{},{}) ", c.0, c.1, c.2));
            }
            println!("    x=196 vertical ramp across badge top-left corner: {corner_ramp}");
            let software = if bg == 0xFFFFFF { &cy_white } else { &cy_black };
            let (mut clean, mut worst_d) = (0usize, 0i32);
            for p in 0..(W as usize * H as usize) {
                let mut d = 0i32;
                for c in 0..3 {
                    d = d.max((software[p * 3 + c] as i32 - rgb[p * 3 + c] as i32).abs());
                }
                if d <= 2 {
                    clean += 1;
                }
                worst_d = worst_d.max(d);
            }
            println!(
                "    vs software SrcOver: {clean}/{} pixels within 2/255, worst delta {worst_d}",
                W * H
            );
            caps[i] = rgb;
            let _ = DestroyWindow(lw);
            let _ = DestroyWindow(bgw);
        }
    }
    if caps[0].is_empty() {
        println!("  layered-window check did NOT run (no capture)");
    }

    // -------------------------------------------------------------- timings
    let warm = 30;
    println!("\n-- timing ({frames} frames each, warm {warm}, µs/frame) --");

    let a_cursor = bench(warm, frames, || unsafe {
        cursor_a(&cursor_a_cv, "AI 鼠标");
        cursor_a_cv.compose(W, H, 0.98);
    });
    let a_cursor_compose = bench(warm, frames, || unsafe {
        cursor_a_cv.compose(W, H, 0.98);
    });
    let b_cursor_clear = bench(warm, frames, || {
        cursor_pm.fill(tiny_skia::Color::TRANSPARENT);
    });
    let b_cursor_draw = bench(warm, frames, || {
        cursor_b(&mut cursor_pm, &mut glyphs, "AI 鼠标", true);
    });
    let b_cursor_notext = bench(warm, frames, || {
        cursor_b(&mut cursor_pm, &mut glyphs, "AI 鼠标", false);
    });
    let b_cursor_swz_u32 = bench(warm, frames, || {
        swizzle_u32(cursor_pm.data(), &mut cursor_b_buf);
    });
    let b_cursor_swz_op = bench(warm, frames, || {
        swizzle_u32_op(cursor_pm.data(), &mut cursor_b_buf, 250);
    });
    let b_cursor_swz_mul = bench(warm, frames, || {
        swizzle_u32_op_mul(cursor_pm.data(), &mut cursor_b_buf, 250);
    });
    let b_cursor_swz_bytes = bench(warm, frames, || {
        swizzle_bytes(cursor_pm.data(), &mut cursor_b_buf);
    });
    let b_cursor_total = bench(warm, frames, || {
        cursor_b(&mut cursor_pm, &mut glyphs, "AI 鼠标", true);
        swizzle_u32_op_mul(cursor_pm.data(), &mut cursor_b_buf, 250);
    });
    let cursor_post = bench(warm, frames, || {
        sprite_blend(&mut cursor_b_buf, W, H, 236, 40, &sprite, 64, 64, 48, 48, 1.0, true);
    });

    let a_panel = bench(warm, frames, || unsafe {
        panel_a(&panel_a_cv, &widgets, PH);
        panel_a_cv.compose(PW, PH, 0.98);
    });
    let a_panel_compose = bench(warm, frames, || unsafe {
        panel_a_cv.compose(PW, PH, 0.98);
    });
    let b_panel_clear = bench(warm, frames, || {
        panel_pm.fill(tiny_skia::Color::TRANSPARENT);
    });
    let b_panel_draw = bench(warm, frames, || {
        panel_b(&mut panel_pm, &widgets, &mut glyphs, PH, true);
    });
    let b_panel_notext = bench(warm, frames, || {
        panel_b(&mut panel_pm, &widgets, &mut glyphs, PH, false);
    });
    let b_panel_swz_u32 = bench(warm, frames, || {
        swizzle_u32(panel_pm.data(), &mut panel_b_buf);
    });
    let b_panel_swz_op = bench(warm, frames, || {
        swizzle_u32_op(panel_pm.data(), &mut panel_b_buf, 250);
    });
    let b_panel_swz_mul = bench(warm, frames, || {
        swizzle_u32_op_mul(panel_pm.data(), &mut panel_b_buf, 250);
    });
    let b_panel_swz_bytes = bench(warm, frames, || {
        swizzle_bytes(panel_pm.data(), &mut panel_b_buf);
    });
    let b_panel_total = bench(warm, frames, || {
        panel_b(&mut panel_pm, &widgets, &mut glyphs, PH, true);
        swizzle_u32_op_mul(panel_pm.data(), &mut panel_b_buf, 250);
    });
    // the naive port: every primitive blended at frame opacity, plain swizzle
    PAINT_ALPHA.store(250, std::sync::atomic::Ordering::Relaxed);
    let b_panel_naive = bench(warm, frames, || {
        panel_b(&mut panel_pm, &widgets, &mut glyphs, PH, true);
        swizzle_u32(panel_pm.data(), &mut panel_b_buf);
    });
    PAINT_ALPHA.store(255, std::sync::atomic::Ordering::Relaxed);
    let panel_post = bench(warm, frames, || {
        for (i, wd) in widgets.iter().filter(|w| w.kind == Kind::Preview).enumerate() {
            sprite_blend(
                &mut panel_b_buf,
                PW,
                PH,
                wd.x + 4,
                wd.y + 4,
                &sprite,
                64,
                64,
                (wd.w - 8).max(1) as u32,
                (wd.h - 8).max(1) as u32,
                1.0,
                true,
            );
            let _ = i;
        }
    });

    println!("\n{:<38}{:>10}{:>10}{:>10}{:>10}{:>10}", "case", "mean", "p50", "p95", "p99", "max");
    let row = |name: &str, s: Stats| {
        println!(
            "{:<38}{:>10}{:>10}{:>10}{:>10}{:>10}",
            name,
            fmt_us(s.mean),
            fmt_us(s.p50),
            fmt_us(s.p95),
            fmt_us(s.p99),
            fmt_us(s.max)
        );
    };
    row("A cursor: GDI draw + compose", a_cursor);
    row("   A cursor: compose() alone", a_cursor_compose);
    row("   B cursor: clear only", b_cursor_clear);
    row("   B cursor: draw, no text", b_cursor_notext);
    row("B cursor: tiny-skia draw", b_cursor_draw);
    row("   B cursor: swizzle u32", b_cursor_swz_u32);
    row("   B cursor: swizzle byte", b_cursor_swz_bytes);
    row("   B cursor: swizzle u32 + opacity (lut)", b_cursor_swz_op);
    row("   B cursor: swizzle u32 + opacity (mul)", b_cursor_swz_mul);
    row("B cursor: total (draw + swizzle)", b_cursor_total);
    row("   both: sprite post-blend", cursor_post);
    row("A panel: GDI draw + compose", a_panel);
    row("   A panel: compose() alone", a_panel_compose);
    row("   B panel: clear only", b_panel_clear);
    row("   B panel: draw, no text", b_panel_notext);
    row("B panel: tiny-skia draw", b_panel_draw);
    row("   B panel: swizzle u32", b_panel_swz_u32);
    row("   B panel: swizzle byte", b_panel_swz_bytes);
    row("   B panel: swizzle u32 + opacity (lut)", b_panel_swz_op);
    row("   B panel: swizzle u32 + opacity (mul)", b_panel_swz_mul);
    row("B panel: NAIVE port (alpha 250 per prim)", b_panel_naive);
    row("B panel: total (draw + swizzle)", b_panel_total);
    row("   both: 3 preview thumbnails", panel_post);

    let pct = |n: f64, d: f64| if d == 0.0 { f64::NAN } else { 100.0 * n / d };
    println!("\n-- verdict arithmetic --");
    println!(
        "cursor: A {:.1}us vs B {:.1}us => {:.2}x  (B saves {:.1}%)",
        a_cursor.mean,
        b_cursor_total.mean,
        a_cursor.mean / b_cursor_total.mean,
        pct(a_cursor.mean - b_cursor_total.mean, a_cursor.mean)
    );
    println!(
        "panel : A {:.1}us vs B {:.1}us => {:.2}x  (B saves {:.1}%)",
        a_panel.mean,
        b_panel_total.mean,
        a_panel.mean / b_panel_total.mean,
        pct(a_panel.mean - b_panel_total.mean, a_panel.mean)
    );
    println!(
        "cursor swizzle (with opacity) = {:.1}% of B total, {:.3}x the cost of compose()",
        pct(b_cursor_swz_mul.mean, b_cursor_total.mean),
        b_cursor_swz_mul.mean / a_cursor_compose.mean
    );
    println!(
        "panel  swizzle (with opacity) = {:.1}% of B total, {:.3}x the cost of compose()",
        pct(b_panel_swz_mul.mean, b_panel_total.mean),
        b_panel_swz_mul.mean / a_panel_compose.mean
    );
    println!(
        "if the port is naive (per-primitive alpha 250): panel {:.1}us => {:.2}x instead of {:.2}x",
        b_panel_naive.mean,
        a_panel.mean / b_panel_naive.mean,
        a_panel.mean / b_panel_total.mean
    );
    println!(
        "draw-only: B vs A's draw+compose => cursor {:.2}x, panel {:.2}x",
        a_cursor.mean / b_cursor_draw.mean,
        a_panel.mean / b_panel_draw.mean
    );
    println!(
        "p95 (the frame that actually stutters): cursor A {:.1} vs B {:.1} | panel A {:.1} vs B {:.1}",
        a_cursor.p95, b_cursor_total.p95, a_panel.p95, b_panel_total.p95
    );

    // -------------------------------------------------- per-primitive costs ---
    // 1000 ops each; answers "where do the 5 ms of a panel frame go".
    let ops = 1000;
    println!("
-- per-primitive cost (tiny-skia, us per call) --");
    bench_ops("fill 452x1139 rounded bg (alpha 250)", ops, || {
        sk_fill_round(&mut panel_pm, 0, 0, PW, PH, 14, C_PANEL, 250);
    });
    bench_ops("fill 452x1139 rounded bg (alpha 255)", ops, || {
        sk_fill_round(&mut panel_pm, 0, 0, PW, PH, 14, C_PANEL, 255);
    });
    bench_ops("fill 452x1139 via pm.fill (memset path)", ops, || {
        panel_pm.fill(sk_color(C_PANEL, 255));
    });
    bench_ops("fill 100x30 rounded rect (alpha 250)", ops, || {
        sk_fill_round(&mut panel_pm, 100, 100, 100, 30, 8, C_BTN, 250);
    });
    bench_ops("stroke 100x30 rounded rect (w1)", ops, || {
        sk_stroke_round(&mut panel_pm, 100, 100, 100, 30, 8, C_BORDER, 1.0, 250);
    });
    bench_ops("fill 100x10 rect (alpha 250)", ops, || {
        sk_fill_rect(&mut panel_pm, 100, 100, 100, 10, C_SEP, 250);
    });
    bench_ops("fill circle r=11", ops, || {
        sk_fill_circle(&mut panel_pm, 100, 100, 11, ACCENT, 250);
    });
    bench_ops("ring circle r=60 w18 (wheel torus)", ops, || {
        sk_ring_circle(&mut panel_pm, 100, 100, 60, 18.0, ACCENT, 250);
    });
    bench_ops("text 6 glyphs 13px", ops, || {
        glyphs.draw(&mut panel_pm, 100, 100, "AI 鼠标", 13.0, C_TEXT);
    });
    bench_ops("GDI FillRect 100x30 (=1 fill_round)", ops, || unsafe {
        panel_a_cv.fill_rect(rect(100, 100, 200, 130), C_BTN);
    });
    bench_ops("GDI TextOut 6 glyphs 13px", ops, || unsafe {
        panel_a_cv.text_at(panel_a_cv.fonts.button, 100, 100, "AI 鼠标", C_TEXT);
    });
    bench_ops("GDI text_w (GetTextExtent)", ops, || unsafe {
        let _ = panel_a_cv.text_w(panel_a_cv.fonts.button, "AI 鼠标");
    });
}
