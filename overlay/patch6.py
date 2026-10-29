import io

p = 'src/main.rs'
s = io.open(p, encoding='utf-8', newline='').read()
orig = s
applied, missed = [], []


def rep(tag, old, new, count=1):
    global s
    if old in s:
        s = s.replace(old, new, count)
        applied.append(tag)
    else:
        missed.append(tag)


# ---------------------------------------------------------------- module ----
rep('mod', "use std::io::{BufRead, Write};",
    "mod sprite;\n\nuse std::io::{BufRead, Write};")
rep('use-path', "use std::path::Path;\n", "")  # no-op guard if absent
rep('use-sprite', "use windows::core::{w, PCWSTR};",
    "use std::path::Path;\n\nuse sprite::SpriteCache;\nuse windows::core::{w, PCWSTR};")

# ------------------------------------------------------- present -> split ----
rep('present-sig',
    "    unsafe fn present(&self, hwnd: isize, w: i32, h: i32, dst: Option<POINT>, alpha: f64) {",
    "    unsafe fn compose(&self, w: i32, h: i32, alpha: f64) {")

rep('present-tail',
    """        let size = SIZE { cx: w, cy: h };
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
    }""",
    """    }

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
    }""")

# ------------------------------------------------------------- slot model ----
rep('slot-struct',
    "#[derive(Clone)]\nstruct CursorView {",
    """/// One customisable picture slot. `source` is `""` (off), `"builtin"` (the vector
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

    fn is_off(&self) -> bool {
        !self.visible || self.source.is_empty()
    }
}

fn slot_is_image(slot: &Slot) -> bool {
    slot.is_image()
}

static SPRITES: OnceLock<Mutex<SpriteCache>> = OnceLock::new();

fn sprites() -> &'static Mutex<SpriteCache> {
    SPRITES.get_or_init(|| Mutex::new(SpriteCache::default()))
}

#[derive(Clone)]
struct CursorView {""")

rep('cursorview-fields',
    """    scale: f64,
    opacity: f64,
}

unsafe fn draw_cursor(""",
    """    scale: f64,
    opacity: f64,
    slots: [Slot; 3],
    /// Milliseconds since the sprite timeline started; drives GIF frame choice.
    epoch_ms: u64,
}

unsafe fn draw_cursor(""")

# ---------------------------------------------------------------- St fields ----
rep('st-fields',
    """    want_dump: Option<(String, bool)>, // (path, panel?)""",
    """    slots: [Slot; 3],
    sprite_epoch: Instant,
    want_dump: Option<(String, bool)>, // (path, panel?)""")

rep('st-init',
    """            want_dump: None,
            dump_done: false,""",
    """            slots: [Slot::new(SLOT_CURSOR), Slot::new(SLOT_HINT), Slot::new(SLOT_ORNAMENT)],
            sprite_epoch: now,
            want_dump: None,
            dump_done: false,""")

rep('cursor-view-fn',
    """        scale: st.scale,
        opacity: st.opacity,
    }
}""",
    """        scale: st.scale,
        opacity: st.opacity,
        slots: st.slots.clone(),
        epoch_ms: Instant::now().duration_since(st.sprite_epoch).as_millis() as u64,
    }
}""")

# ------------------------------------------------------- draw_cursor gating ----
rep('gate-cursor',
    "    match v.theme.as_str() {\n        \"dot\" => {",
    "    if !slot_is_image(&v.slots[SLOT_CURSOR]) {\n    match v.theme.as_str() {\n        \"dot\" => {")

rep('gate-cursor-end',
    "\n    if v.badge && !v.label.is_empty() {",
    "\n    }\n\n    if !slot_is_image(&v.slots[SLOT_HINT]) && v.badge && !v.label.is_empty() {")

# --------------------------------------------------------- composite sprites ----
rep('draw-tail',
    """    let dst = POINT {
        x: (v.cur.0 - TIP_X).round() as i32,
        y: (v.cur.1 - TIP_Y).round() as i32,
    };
    let alpha = v.opacity.clamp(0.1, 1.0);
    cv.present(state().lock().unwrap().hwnd, W, H, Some(dst), alpha);
}""",
    """    // Rasterise the vector pass, then blend the image slots over it. Images carry
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
}""")

io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('applied:', applied)
print('MISSED :', missed)
print('changed:', orig != s)
