import io

p = 'overlay/src/main.rs'
s = io.open(p, encoding='utf-8', newline='').read()
n = [0]


def rep(tag, old, new):
    global s
    if old in s:
        s = s.replace(old, new, 1)
        n[0] += 1
    else:
        print('MISS:', tag)


# ---- scale the composed image at present time -------------------------------
rep('present-scaled', "    unsafe fn present(&self, hwnd: isize, w: i32, h: i32, dst: Option<POINT>, alpha: f64) {",
    """    /// Present the panel at a zoom factor.
    ///
    /// The layout is always computed at its design size, so nothing has to know
    /// about the zoom; only the finished image is stretched. That is what makes
    /// the controls and the text scale together and re-flow as one piece.
    unsafe fn present_scaled(
        &self,
        hwnd: isize,
        dw: i32,
        dh: i32,
        zoom: f64,
        dst: Option<POINT>,
        alpha: f64,
    ) {
        let tw = ((dw as f64) * zoom).round().max(1.0) as i32;
        let th = ((dh as f64) * zoom).round().max(1.0) as i32;
        self.compose(dw, dh, alpha);
        if (zoom - 1.0).abs() < 0.001 {
            self.blit(hwnd, dw, dh, dst);
            return;
        }
        // A scratch surface for the stretched frame. Creating it per present is
        // acceptable: the panel only repaints when something changed.
        let mem = CreateCompatibleDC(self.hdc_screen);
        let bmp = CreateCompatibleBitmap(self.hdc_screen, tw, th);
        let prev = SelectObject(mem, bmp);
        let _ = SetStretchBltMode(mem, HALFTONE);
        let _ = StretchBlt(mem, 0, 0, tw, th, self.hdc_small, 0, 0, dw, dh, SRCCOPY);

        let size = SIZE { cx: tw, cy: th };
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
            mem,
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        SelectObject(mem, prev);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
    }

    unsafe fn present(&self, hwnd: isize, w: i32, h: i32, dst: Option<POINT>, alpha: f64) {""")

rep('imports', "use windows::Win32::Graphics::Gdi::{\n    BeginPaint,",
    "use windows::Win32::Graphics::Gdi::{\n    BeginPaint,")
rep('imports2', "    BitBlt, CreateCompatibleDC,",
    "    BitBlt, CreateCompatibleDC, CreateCompatibleBitmap, HALFTONE, SetStretchBltMode, StretchBlt,")

# ---- state -------------------------------------------------------------------
rep('st-zoom', "    panel_px_h: i32,",
    "    panel_px_h: i32,\n    /// Panel zoom: layout stays at design size, only the image is stretched.\n    panel_zoom: f64,")
rep('st-zoom-init', "            panel_px_h: 0,",
    "            panel_px_h: 0,\n            panel_zoom: 1.0,")

# ---- resize the window by the zoom, and present scaled ----------------------
rep('panel-resize-zoom', "                let w = if p_full { PW } else { CHIP_W };",
    "                let z = st.lock().unwrap().panel_zoom;\n                let w = if p_full { ((PW as f64) * z).round() as i32 } else { CHIP_W };")
rep('panel-resize-h', "                let h = if p_full { p_h } else { CHIP_H };",
    "                let h = if p_full { ((p_h as f64) * z).round() as i32 } else { CHIP_H };")

io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('replacements:', n[0])
