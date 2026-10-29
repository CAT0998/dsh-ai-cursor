import io

# ---------------------------------------------------------------- widget.rs ---
p = 'src/widget.rs'
s = io.open(p, encoding='utf-8', newline='').read()
s = s.replace("""    /// The colour wheel (hue ring); owned by the colours section.
    Wheel,""",
"""    /// The colour wheel (hue ring); owned by the colours section.
    Wheel,
    /// A live thumbnail of a picture slot (animates with the sprite).
    Preview,""")
s = s.replace("""    /// Colour channels, 0-255.
    pub rgb: [u8; 3],
}""",
"""    /// Colour channels, 0-255.
    pub rgb: [u8; 3],
    /// The reserved custom section's current contents.
    pub custom: CustomView,
}

#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct CustomView {
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
];""")
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('widget.rs patched')

# -------------------------------------------------------------- sections.rs ---
p = 'src/sections.rs'
s = io.open(p, encoding='utf-8', newline='').read()

s = s.replace("""use crate::widget::{
    channels_of, hex, Hit, Kind, PanelView, SlotView, SWATCH_IDS, SWATCHES,
};""",
"""use crate::widget::{
    channels_of, hex, CustomRowView, Hit, Kind, PanelView, SlotView, CUSTOM_IDS, SWATCH_IDS,
    SWATCHES,
};""")

# artwork rows get a preview box
s = s.replace("""fn slot_row(
    v: &PanelView,
    x: i32,
    y: i32,
    idx: usize,
    slot: &SlotView,
    prev_id: &'static str,
    next_id: &'static str,
    out: &mut Vec<Hit>,
) -> i32 {
    let label = format!("{} {}", slot.name, source_short(slot));
    out.push(Hit::new(rect(x, y, x + 250, y + 20), Kind::Label, "l_slot").text(label));
    out.push(
        Hit::new(rect(x + 256, y - 2, x + 286, y + 22), Kind::Button, prev_id)
            .text("\\u{2039}")
            .hover(hov(v, prev_id))
            .data(idx as i32),
    );
    out.push(
        Hit::new(rect(x + 290, y - 2, x + 320, y + 22), Kind::Button, next_id)
            .text("\\u{203A}")
            .hover(hov(v, next_id))
            .data(idx as i32),
    );
    out.push(
        Hit::new(rect(x + 330, y, content_right(), y + 18), Kind::LabelRight, "l_slotinfo")
            .text(if slot.frames > 1 { format!("{}帧", slot.frames) } else { String::from("静图") }),
    );
    y + 24
}""",
"""const PREVIEW_IDS: [&'static str; 3] = ["prev0", "prev1", "prev2"];

fn slot_row(
    v: &PanelView,
    x: i32,
    y: i32,
    idx: usize,
    slot: &SlotView,
    prev_id: &'static str,
    next_id: &'static str,
    out: &mut Vec<Hit>,
) -> i32 {
    // live thumbnail: the same frames the cursor uses, so an animated slot
    // previews as an animation
    out.push(
        Hit::new(rect(x, y - 4, x + 46, y + 24), Kind::Preview, PREVIEW_IDS[idx])
            .data(idx as i32),
    );
    let label = format!("{} {}", slot.name, source_short(slot));
    out.push(Hit::new(rect(x + 54, y, x + 250, y + 20), Kind::Label, "l_slot").text(label));
    out.push(
        Hit::new(rect(x + 250, y - 2, x + 280, y + 22), Kind::Button, prev_id)
            .text("\\u{2039}")
            .hover(hov(v, prev_id))
            .data(idx as i32),
    );
    out.push(
        Hit::new(rect(x + 284, y - 2, x + 314, y + 22), Kind::Button, next_id)
            .text("\\u{203A}")
            .hover(hov(v, next_id))
            .data(idx as i32),
    );
    out.push(
        Hit::new(rect(x + 324, y, content_right(), y + 18), Kind::LabelRight, "l_slotinfo")
            .text(if slot.frames > 1 { format!("{}帧", slot.frames) } else { String::from("静图") }),
    );
    y + 30
}""")

# the real custom body
s = s.replace("""/// The reserved slot. It is a real, registered module with no widgets of its
/// own; `docs/MODULES.md` explains how to give it a body without touching the
/// renderer, and this row is what the user sees until then.
fn body_custom(_v: &PanelView, x: i32, y: i32, out: &mut Vec<Hit>) -> i32 {
    out.push(
        Hit::new(rect(x, y, content_right(), y + 20), Kind::Label, "l_custom")
            .text("空位：见 docs/MODULES.md，可直接写进这个分区"),
    );
    y + 24
}""",
"""/// The reserved slot, filled from `custom-section.json` in the asset folder.
fn body_custom(v: &PanelView, x: i32, mut y: i32, out: &mut Vec<Hit>) -> i32 {
    if let Some(err) = &v.custom.error {
        out.push(
            Hit::new(rect(x, y, content_right(), y + 18), Kind::Label, "l_custom")
                .text(format!("自定义模块有误：{err}")),
        );
        return y + 22;
    }
    if v.custom.rows.is_empty() {
        for line in &v.custom.hint {
            out.push(
                Hit::new(rect(x, y, content_right(), y + 16), Kind::Label, "l_custom")
                    .text(line.clone()),
            );
            y += 18;
        }
        return y;
    }
    for (i, row) in v.custom.rows.iter().enumerate() {
        match row {
            CustomRowView::Label(text) => {
                out.push(Hit::new(rect(x, y, content_right(), y + 18), Kind::Label, "l_custom").text(text.clone()));
                y += 20;
            }
            CustomRowView::Button(text) => {
                let id = CUSTOM_IDS.get(i).copied().unwrap_or("cus0");
                out.push(
                    Hit::new(rect(x, y, x + 180, y + 26), Kind::Button, id)
                        .text(text.clone())
                        .hover(hov(v, id))
                        .data(i as i32),
                );
                y += 30;
            }
        }
    }
    y
}""")
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('sections.rs patched')
