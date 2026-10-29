//! The colour wheel: a hue ring around an HSV square.
//!
//! Painting it per frame would mean ~70k pixel evaluations every time the panel
//! repaints, so it renders once into an off-screen surface and is blitted from
//! there. The square depends on the hue, so it is re-rendered only when the hue
//! actually changes.
//!
//! The surface keeps the same magenta key colour the rest of the panel uses, so
//! the existing downsample pass turns uncovered pixels transparent and
//! antialiases the edges for free.

use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_USAGE, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use windows::Win32::Foundation::HANDLE;

/// Outer radius of the hue ring, in logical px.
pub const R_OUT: f64 = 64.0;
/// Inner radius of the hue ring.
pub const R_IN: f64 = 49.0;
/// Side of the inner square.
pub const SQ: f64 = 58.0;
/// The whole widget's box, in logical px.
pub const D: i32 = 132;

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> u32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let c = |x: f64| (x * 255.0).round().clamp(0.0, 255.0) as u32;
    (c(r) << 16) | (c(g) << 8) | c(b)
}

pub fn rgb_to_hsv(rgb: u32) -> (f64, f64, f64) {
    let r = ((rgb >> 16) & 0xFF) as f64 / 255.0;
    let g = ((rgb >> 8) & 0xFF) as f64 / 255.0;
    let b = (rgb & 0xFF) as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        (((g - b) / d).rem_euclid(6.0)) / 6.0
    } else if max == g {
        (((b - r) / d) + 2.0) / 6.0
    } else {
        (((r - g) / d) + 4.0) / 6.0
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

pub struct Wheel {
    hdc: HDC,
    _bmp: HBITMAP,
    bits: *mut u8,
    ss: i32,
    /// Hue (0..1) the square was rendered for; `-1` forces a re-render.
    rendered_hue: f64,
    pub hue: f64,
    pub sat: f64,
    pub val: f64,
}

unsafe impl Send for Wheel {}

impl Wheel {
    pub unsafe fn new(ss: i32) -> Wheel {
        let screen = windows::Win32::Graphics::Gdi::GetDC(None);
        let hdc = CreateCompatibleDC(screen);
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen);

        let n = (D * ss) as i32;
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = n;
        bmi.bmiHeader.biHeight = -n;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(hdc, &bmi, DIB_USAGE(0), &mut bits, HANDLE::default(), 0)
            .expect("wheel DIB");
        SelectObject(hdc, HGDIOBJ(bmp.0));
        Wheel {
            hdc,
            _bmp: bmp,
            bits: bits as *mut u8,
            ss,
            rendered_hue: -1.0,
            hue: 0.0,
            sat: 1.0,
            val: 1.0,
        }
    }

    pub fn set_from_rgb(&mut self, rgb: u32) {
        let (h, s, v) = rgb_to_hsv(rgb);
        self.hue = h;
        self.sat = s;
        self.val = v;
    }

    pub fn rgb(&self) -> u32 {
        hsv_to_rgb(self.hue, self.sat, self.val)
    }

    /// Re-render the ring and square if needed.
    pub unsafe fn ensure(&mut self) {
        if (self.rendered_hue - self.hue).abs() < 1e-9 {
            return;
        }
        self.rendered_hue = self.hue;
        let ss = self.ss;
        let n = (D * ss) as usize;
        let c = n as f64 / 2.0;
        let keyb = [255u8, 0, 255, 255]; // BGRA magenta
        let px = std::slice::from_raw_parts_mut(self.bits, n * n * 4);

        for y in 0..n {
            for x in 0..n {
                let dx = (x as f64 + 0.5 - c) / ss as f64;
                let dy = (y as f64 + 0.5 - c) / ss as f64;
                let r = (dx * dx + dy * dy).sqrt();
                let o = (y * n + x) * 4;
                let colour = if r <= R_OUT && r >= R_IN {
                    let ang = dy.atan2(dx);
                    let hue = ((ang / std::f64::consts::TAU) + 1.0) % 1.0;
                    Some(hsv_to_rgb(1.0 - hue, 1.0, 1.0))
                } else if dx.abs() <= SQ / 2.0 && dy.abs() <= SQ / 2.0 {
                    let s = (dx / SQ + 0.5).clamp(0.0, 1.0);
                    let v = (0.5 - dy / SQ).clamp(0.0, 1.0);
                    Some(hsv_to_rgb(self.hue, s, v))
                } else {
                    None
                };
                match colour {
                    Some(rgb) => {
                        px[o] = (rgb & 0xFF) as u8;
                        px[o + 1] = ((rgb >> 8) & 0xFF) as u8;
                        px[o + 2] = ((rgb >> 16) & 0xFF) as u8;
                        px[o + 3] = 255;
                    }
                    None => {
                        px[o] = keyb[0];
                        px[o + 1] = keyb[1];
                        px[o + 2] = keyb[2];
                        px[o + 3] = 255;
                    }
                }
            }
        }
    }

    /// Blit at a position given in the panel canvas's own (supersampled) space.
    pub unsafe fn blit(&self, dst: HDC, x_ss: i32, y_ss: i32) {
        let n = D * self.ss;
        let _ = BitBlt(dst, x_ss, y_ss, n, n, self.hdc, 0, 0, SRCCOPY);
    }

    /// Map a click inside the wheel box to a colour. Returns true when it landed
    /// on the ring or the square.
    pub fn hit(&mut self, lx: i32, ly: i32) -> bool {
        let c = D as f64 / 2.0;
        let dx = lx as f64 - c;
        let dy = ly as f64 - c;
        let r = (dx * dx + dy * dy).sqrt();
        if r <= R_OUT && r >= R_IN {
            let ang = dy.atan2(dx);
            self.hue = ((ang / std::f64::consts::TAU) + 1.0).rem_euclid(1.0);
            self.hue = (1.0 - self.hue).rem_euclid(1.0);
            true
        } else if dx.abs() <= SQ / 2.0 && dy.abs() <= SQ / 2.0 {
            self.sat = (dx / SQ + 0.5).clamp(0.0, 1.0);
            self.val = (0.5 - dy / SQ).clamp(0.0, 1.0);
            true
        } else {
            false
        }
    }
}

impl Drop for Wheel {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self._bmp.0));
            let _ = DeleteDC(self.hdc);
        }
    }
}
