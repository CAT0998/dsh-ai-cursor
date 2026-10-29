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


# ------------------------------------------------------------ sprite command ----
rep('sprite-cmd',
    """        "badge" => {""",
    """        "sprite" => {
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
                    let slot = &mut st.slots[i];
                    if let Some(src) = v.get("source").and_then(|x| x.as_str()) {
                        if slot.source != src {
                            slot.source = src.to_string();
                            slot.last_frame = usize::MAX;
                            st.sprite_epoch = Instant::now();
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
                    st.dirty = true;
                }
            }
        }
        "sprite_rescan" => {
            sprites().lock().unwrap().clear();
            for slot in st.slots.iter_mut() {
                slot.last_frame = usize::MAX;
            }
            st.sprite_epoch = Instant::now();
            st.dirty = true;
        }
        "badge" => {""")

# ------------------------------------------------------------- status slots ----
rep('status-slots',
    """    out["panel_tile"] = serde_json::json!(st.panel_full);""",
    """    out["panel_tile"] = serde_json::json!(st.panel_full);""", 0)

rep('status-slots2',
    """    out["frames"] = serde_json::json!(st.frames);""",
    """    let mut slot_out = serde_json::Map::new();
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
    out["frames"] = serde_json::json!(st.frames);""")

# ------------------------------------------------------- animation ticker ----
rep('anim-tick',
    """            // Keep the panel's status line honest.""",
    """            // Advance animated sprites: repaint only when the visible frame
            // actually changes, so an idle GIF does not burn a redraw per tick.
            if visible {
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
                    st.lock().unwrap().dirty = true;
                }
            }

            // Keep the panel's status line honest.""")

io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('applied:', applied)
print('MISSED :', missed)
print('changed:', orig != s)
