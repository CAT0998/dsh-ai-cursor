import io

p = 'src/main.rs'
s = io.open(p, encoding='utf-8', newline='').read()
applied, missed = [], []


def rep(tag, old, new):
    global s
    if old in s:
        s = s.replace(old, new, 1)
        applied.append(tag)
    else:
        missed.append(tag)


rep('mod', "mod sections;", "mod custom;\nmod sections;")
rep('use', "use widget::{channels_of, hex, Hit, Kind, PanelView, SlotView, SWATCHES};",
    "use widget::{channels_of, hex, Hit, Kind, PanelView, SlotView, SWATCHES};")

# ---- state -------------------------------------------------------------------
rep('st-fields', "    panel_px_h: i32,",
    "    panel_px_h: i32,\n    asset_dir: String,\n    custom: custom::CustomSpec,")
rep('st-init', "            panel_px_h: PH,",
    "            panel_px_h: PH,\n            asset_dir: String::new(),\n            custom: custom::CustomSpec::default(),")

# ---- view --------------------------------------------------------------------
rep('view-of', "        rgb: channels_of(st.accent),\n    }\n}",
    """        rgb: channels_of(st.accent),
        custom: custom_view(st),
    }
}

fn custom_view(st: &St) -> widget::CustomView {
    let mut v = widget::CustomView {
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
}""")

# ---- assets command also loads the custom spec --------------------------------
rep('assets-load', """            found.sort();
            st.assets = found.clone();""",
    """            found.sort();
            st.assets = found.clone();
            st.asset_dir = dir.clone();
            st.custom = custom::load(&dir);""")

# ---- previews: composite after the vector pass -------------------------------
rep('draw-present-split',
    "    cv.present(state().lock().unwrap().panel_hwnd, PW, h, None, 0.98);",
    """    cv.compose(PW, h, 0.98);
    draw_previews(cv, v, hits, h);
    cv.blit(state().lock().unwrap().panel_hwnd, PW, h, None);""")

rep('draw-previews', "fn view_of(st: &St) -> PanelView {",
    """/// Draw the live thumbnails for the artwork slots.
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

fn view_of(st: &St) -> PanelView {""")

# ---- animation also refreshes the panel previews -----------------------------
rep('anim-panel', """                if changed {
                    st.lock().unwrap().dirty = true;
                }""",
    """                if changed {
                    let mut s = st.lock().unwrap();
                    s.dirty = true;
                    if s.panel_shown {
                        // the artwork thumbnails animate too
                        s.panel_dirty = true;
                    }
                }""")

# ---- custom rows dispatch ----------------------------------------------------
rep('toggle-custom', """        _ => {
            if let Some(sec) = sections::SECTIONS.iter().find(|d| d.header == id) {""",
    """        _ => {
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
            } else if let Some(sec) = sections::SECTIONS.iter().find(|d| d.header == id) {""")

rep('apply-set', "fn cycle_slot_source(st: &mut St, slot_idx: usize, dir: i32) {",
    """/// The `set` escape hatch of the custom module: a tiny, explicit whitelist of
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
            st.accent = rgb;
            st.dirty = true;
        }
    }
    st.panel_dirty = true;
}

fn cycle_slot_source(st: &mut St, slot_idx: usize, dir: i32) {""")

io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('applied:', applied)
print('MISSED :', missed)
