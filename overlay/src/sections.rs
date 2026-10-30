//! Panel sections: the extension framework.
//!
//! A *section* is one collapsible block of the floating panel. It describes
//! itself (id, title, default state) and **emits widgets**; it never paints and
//! never interprets clicks. That keeps the drawing and hit-testing code shared,
//! and it means adding a section is a local, mechanical change — which is what
//! makes the reserved `custom` slot usable by someone (or some AI) who is not
//! touching the renderer.
//!
//! Body builders return the y coordinate they finished at, so sections stack.

use crate::rect;
use crate::widget::{
    channels_of, hex, CustomRowView, Hit, Kind, PanelView, SlotView, CUSTOM_IDS, SWATCH_IDS,
    SWATCHES,
};

pub const PAD: i32 = 14;
pub const SECTION_TITLE_H: i32 = 24;

pub struct SecDef {
    /// Stable key used by the collapse set and by the panel's own dispatch.
    pub key: &'static str,
    /// Widget id of the section header (clicking it toggles).
    pub header: &'static str,
    pub title: &'static str,
    pub default_collapsed: bool,
    pub render: fn(&PanelView, i32, i32, &mut Vec<Hit>) -> i32,
}

/// The panel's sections, in display order. Adding a module here is the whole
/// registration step; `docs/MODULES.md` documents the contract.
pub const SECTIONS: &[SecDef] = &[
    SecDef {
        key: "actions",
        header: "sec_actions",
        title: "操作",
        default_collapsed: false,
        render: body_actions,
    },
    SecDef {
        key: "appearance",
        header: "sec_appearance",
        title: "外观",
        default_collapsed: false,
        render: body_appearance,
    },
    SecDef {
        key: "style",
        header: "sec_style",
        title: "造型",
        default_collapsed: true,
        render: body_style,
    },
    SecDef {
        key: "colors",
        header: "sec_colors",
        title: "颜色",
        default_collapsed: false,
        render: body_colors,
    },
    SecDef {
        key: "artwork",
        header: "sec_artwork",
        title: "图案（自定义鼠标 / 提示 / 附属）",
        default_collapsed: true,
        render: body_artwork,
    },
    SecDef {
        key: "minimap",
        header: "sec_minimap",
        title: "小地图",
        default_collapsed: true,
        render: body_minimap,
    },
    SecDef {
        key: "permission",
        header: "sec_permission",
        title: "权限等级",
        default_collapsed: false,
        render: body_permission,
    },
    SecDef {
        key: "custom",
        header: "sec_custom",
        title: "自定义模块（空位）",
        default_collapsed: true,
        render: body_custom,
    },
];

pub fn is_collapsed(v: &PanelView, key: &str) -> bool {
    v.collapsed.iter().any(|k| k == key)
}

fn hov(v: &PanelView, id: &str) -> bool {
    v.hover == id
}

fn content_right() -> i32 {
    crate::PW - PAD
}

fn col_w() -> i32 {
    (content_right() - PAD - 3 * 8) / 4
}

/// Build the whole section stack starting at `y`. Returns the bottom y.
pub fn build(v: &PanelView, mut y: i32, out: &mut Vec<Hit>) -> i32 {
    if !v.full {
        return y;
    }
    for sec in SECTIONS {
        let collapsed = is_collapsed(v, sec.key);
        // header row: chevron + title, click toggles
        let title = if sec.key == "custom" {
            v.custom.title.clone().unwrap_or_else(|| sec.title.to_string())
        } else {
            sec.title.to_string()
        };
        out.push(
            Hit::new(
                rect(PAD, y, content_right(), y + SECTION_TITLE_H),
                Kind::Section,
                sec.header,
            )
            .text(format!("{} {}", if collapsed { "\u{25B8}" } else { "\u{25BE}" }, title))
            .hover(hov(v, sec.header)),
        );
        y += SECTION_TITLE_H;
        if !collapsed {
            y = (sec.render)(v, PAD, y, out);
            y += 4;
        }
        out.push(Hit::new(rect(PAD, y, content_right(), y + 1), Kind::Sep, "sec_sep"));
        y += 7;
    }
    y
}

// ------------------------------------------------------------------ bodies ---

fn body_actions(v: &PanelView, x: i32, mut y: i32, out: &mut Vec<Hit>) -> i32 {
    let w = col_w();
    let acts: [(&'static str, &str, bool); 4] = [
        ("show", "显示", v.visible),
        ("hide", "隐藏", !v.visible),
        ("stop", "停止", v.permission == 0),
        ("start", "启用", v.permission > 0),
    ];
    for (i, (id, text, on)) in acts.iter().enumerate() {
        let bx = x + i as i32 * (w + 8);
        out.push(
            Hit::new(rect(bx, y, bx + w, y + 34), Kind::Button, id)
                .text(*text)
                .active(*on)
                .hover(hov(v, id)),
        );
    }
    y += 38;
    out.push(Hit::new(rect(x, y, x + 34, y + 16), Kind::Label, "l_drag").text("拖拽"));
    out.push(
        Hit::new(rect(x + 38, y - 2, x + 130, y + 22), Kind::Button, "drag_toggle")
            .text(if v.drag_enabled { "可用" } else { "禁用" })
            .active(v.drag_enabled)
            .hover(hov(v, "drag_toggle")),
    );
    out.push(
        Hit::new(rect(x + 140, y, content_right(), y + 18), Kind::Label, "l_drag_hint")
            .text(if v.dragging { "正在被用户拖动…" } else { "按住光标即可用真实鼠标拖动它" }),
    );
    y + 28
}

fn body_appearance(v: &PanelView, x: i32, y: i32, out: &mut Vec<Hit>) -> i32 {
    out.push(Hit::new(rect(x, y, content_right(), y + 18), Kind::Label, "l_badge").text("AI 提示"));
    out.push(
        Hit::new(rect(x + 116, y - 3, x + 166, y + 23), Kind::Button, "badge_on")
            .text("开")
            .active(v.badge)
            .hover(hov(v, "badge_on")),
    );
    out.push(
        Hit::new(rect(x + 174, y - 3, x + 224, y + 23), Kind::Button, "badge_off")
            .text("关")
            .active(!v.badge)
            .hover(hov(v, "badge_off")),
    );
    out.push(
        Hit::new(rect(x + 240, y, content_right(), y + 18), Kind::LabelRight, "l_opacity")
            .text(format!("透明度 {}%", (v.opacity * 100.0).round() as i32)),
    );
    out.push(
        Hit::new(rect(x, y + 28, content_right(), y + 46), Kind::Slider, "opacity")
            .data(((v.opacity - 0.15) / 0.85 * 1000.0).clamp(0.0, 1000.0) as i32),
    );
    y + 50
}

fn body_style(v: &PanelView, x: i32, y: i32, out: &mut Vec<Hit>) -> i32 {
    let themes: [(&'static str, &str, &str); 3] =
        [("theme_whale", "鲸鱼", "whale"), ("theme_dot", "圆点", "dot"), ("theme_arrow", "箭头", "arrow")];
    for (i, (id, text, key)) in themes.iter().enumerate() {
        let bx = x + i as i32 * 74;
        out.push(
            Hit::new(rect(bx, y, bx + 66, y + 26), Kind::Button, id)
                .text(*text)
                .active(v.theme == *key)
                .hover(hov(v, id)),
        );
    }
    out.push(
        Hit::new(rect(x + 226, y, content_right(), y + 18), Kind::LabelRight, "l_scale")
            .text(format!("大小 {}%", (v.scale * 100.0).round() as i32)),
    );
    out.push(
        Hit::new(rect(x, y + 28, content_right(), y + 46), Kind::Slider, "scale")
            .data(((v.scale - 0.6) / 1.4 * 1000.0).clamp(0.0, 1000.0) as i32),
    );
    y + 50
}

fn body_colors(v: &PanelView, x: i32, mut y: i32, out: &mut Vec<Hit>) -> i32 {
    // presets
    for (i, (id, rgb)) in SWATCH_IDS.iter().zip(SWATCHES.iter()).enumerate() {
        let bx = x + i as i32 * 36;
        out.push(
            Hit::new(rect(bx, y, bx + 26, y + 26), Kind::Swatch, id)
                .text(hex(*rgb))
                .active(v.accent & 0xFFFFFF == *rgb)
                .hover(hov(v, id)),
        );
    }
    out.push(
        Hit::new(rect(x + 268, y + 4, content_right(), y + 22), Kind::LabelRight, "l_hex")
            .text(hex(v.accent)),
    );
    y += 30;

    // 256-level RGB channels (P2)
    let labels = ["R", "G", "B"];
    let channels = channels_of(v.accent);
    for (i, label) in labels.iter().enumerate() {
        let id: &'static str = match i {
            0 => "ch_r",
            1 => "ch_g",
            _ => "ch_b",
        };
        out.push(
            Hit::new(rect(x, y + 4, x + 18, y + 22), Kind::Label, id)
                .text(*label)
                .data(i as i32),
        );
        out.push(
            Hit::new(rect(x + 24, y + 2, x + 300, y + 20), Kind::Channel, id)
                .text(format!("{:3}", channels[i]))
                .active(false)
                .hover(hov(v, id))
                .data(i as i32),
        );
        out.push(
            Hit::new(rect(x + 308, y + 4, content_right(), y + 22), Kind::LabelRight, id)
                .text(format!("{:3}", channels[i]))
                .data(i as i32),
        );
        y += 22;
    }

    // colour wheel: hue ring + HSV square, drawn from a cached surface
    let d = crate::wheel::D;
    out.push(
        Hit::new(rect(x, y + 6, x + d, y + 6 + d), Kind::Wheel, "wheel")
            .hover(hov(v, "wheel")),
    );
    out.push(
        Hit::new(rect(x + d + 10, y + 12, content_right(), y + 30), Kind::LabelRight, "l_wheel_hex")
            .text(hex(v.accent)),
    );
    out.push(
        Hit::new(rect(x + d + 10, y + 36, content_right(), y + 54), Kind::Label, "l_wheel_hint")
            .text("外环选色相"),
    );
    out.push(
        Hit::new(rect(x + d + 10, y + 54, content_right(), y + 72), Kind::Label, "l_wheel_hint2")
            .text("内块选明度/饱和"),
    );
    y + 6 + d + 8
}

const PREVIEW_IDS: [&'static str; 3] = ["prev0", "prev1", "prev2"];

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
            .data(idx as i32)
            .active(v.art_sel == idx)
            .hover(hov(v, PREVIEW_IDS[idx])),
    );
    let label = format!("{} {}", slot.name, source_short(slot));
    out.push(Hit::new(rect(x + 54, y, x + 250, y + 20), Kind::Label, "l_slot").text(label));
    out.push(
        Hit::new(rect(x + 250, y - 2, x + 280, y + 22), Kind::Button, prev_id)
            .text("\u{2039}")
            .hover(hov(v, prev_id))
            .data(idx as i32),
    );
    out.push(
        Hit::new(rect(x + 284, y - 2, x + 314, y + 22), Kind::Button, next_id)
            .text("\u{203A}")
            .hover(hov(v, next_id))
            .data(idx as i32),
    );
    out.push(
        Hit::new(rect(x + 324, y, content_right(), y + 18), Kind::LabelRight, "l_slotinfo")
            .text(if slot.frames > 1 { format!("{}帧", slot.frames) } else { String::from("静图") }),
    );
    y + 30
}

fn source_short(slot: &SlotView) -> String {
    if let Some(err) = &slot.error {
        return format!("加载失败: {err}");
    }
    if slot.source == "builtin" {
        "内置矢量".to_string()
    } else if slot.source.is_empty() {
        "关闭".to_string()
    } else {
        slot.source.rsplit(['\\', '/']).next().unwrap_or(&slot.source).to_string()
    }
}

fn body_artwork(v: &PanelView, x: i32, mut y: i32, out: &mut Vec<Hit>) -> i32 {
    let rows: [(&'static str, &'static str); 3] =
        [("art_prev0", "art_next0"), ("art_prev1", "art_next1"), ("art_prev2", "art_next2")];
    for (i, (prev, next)) in rows.iter().enumerate() {
        y = slot_row(v, x, y, i, &v.slots[i], prev, next, out);
    }
    let assets = if v.assets.is_empty() {
        "素材目录为空".to_string()
    } else {
        format!("素材目录 {} 个文件", v.assets.len())
    };
    out.push(
        Hit::new(rect(x, y + 2, content_right(), y + 20), Kind::Label, "l_art_note")
            .text(format!("‹ › 循环切换；点缩略图选中再调参数（{}）", assets)),
    );
    y += 26;

    // ---- editor for the selected slot -------------------------------------
    let sel = v.art_sel.min(2);
    let slot = &v.slots[sel];
    out.push(
        Hit::new(rect(x, y, content_right(), y + 18), Kind::Label, "l_art_edit").text(format!(
            "调整「{}」：{}× 偏移({:.0},{:.0}) {:.0}%",
            slot.name,
            if slot.size > 0.0 { format!("{:.0}", slot.size) } else { "原始".to_string() },
            slot.offset.0,
            slot.offset.1,
            slot.opacity * 100.0
        )),
    );
    y += 20;

    let rows2: [(&'static str, &str, f64, String); 4] = [
        (
            "art_size",
            "大小",
            ((if slot.size > 0.0 { slot.size } else { 64.0 } - 8.0) / 248.0).clamp(0.0, 1.0),
            if slot.size > 0.0 { format!("{:.0}px", slot.size) } else { "原始".to_string() },
        ),
        (
            "art_offx",
            "横向",
            ((slot.offset.0 + 200.0) / 400.0).clamp(0.0, 1.0),
            format!("{:.0}", slot.offset.0),
        ),
        (
            "art_offy",
            "纵向",
            ((slot.offset.1 + 200.0) / 400.0).clamp(0.0, 1.0),
            format!("{:.0}", slot.offset.1),
        ),
        (
            "art_opacity",
            "透明",
            slot.opacity.clamp(0.0, 1.0),
            format!("{:.0}%", slot.opacity * 100.0),
        ),
    ];
    for (id, label, frac, text) in rows2 {
        out.push(Hit::new(rect(x, y + 4, x + 34, y + 20), Kind::Label, "l_art_c").text(label));
        out.push(
            Hit::new(rect(x + 38, y + 2, x + 330, y + 20), Kind::Slider, id)
                .data((frac * 1000.0) as i32)
                .hover(hov(v, id)),
        );
        out.push(
            Hit::new(rect(x + 336, y + 4, content_right(), y + 20), Kind::LabelRight, "l_art_v")
                .text(text),
        );
        y += 22;
    }
    out.push(
        Hit::new(rect(x, y + 2, x + 90, y + 26), Kind::Button, "art_flip")
            .text(if slot.flip { "翻转：开" } else { "翻转：关" })
            .active(slot.flip)
            .hover(hov(v, "art_flip")),
    );
    out.push(
        Hit::new(rect(x + 98, y + 2, x + 180, y + 26), Kind::Button, "art_reset")
            .text("重置")
            .hover(hov(v, "art_reset")),
    );
    y + 30
}

fn body_minimap(v: &PanelView, x: i32, y: i32, out: &mut Vec<Hit>) -> i32 {
    out.push(
        Hit::new(rect(x, y, x + 168, y + 30), Kind::Button, "map_window_toggle")
            .text(if v.map_window { "小地图窗口：开" } else { "小地图窗口：关" })
            .active(v.map_window)
            .hover(hov(v, "map_window_toggle")),
    );
    out.push(
        Hit::new(rect(x + 176, y + 6, content_right(), y + 24), Kind::Label, "l_map_hint")
            .text("独立进程 · 可拖动缩放最小化"),
    );
    y + 34
}

fn body_permission(v: &PanelView, x: i32, y: i32, out: &mut Vec<Hit>) -> i32 {
    let w = col_w();
    let perms: [(&'static str, &str, u8); 4] = [
        ("perm_0", "停止", 0),
        ("perm_1", "只读", 1),
        ("perm_2", "操作", 2),
        ("perm_3", "管理员", 3),
    ];
    for (i, (id, text, lvl)) in perms.iter().enumerate() {
        let bx = x + i as i32 * (w + 8);
        out.push(
            Hit::new(rect(bx, y, bx + w, y + 30), Kind::Button, id)
                .text(*text)
                .active(v.permission == *lvl)
                .hover(hov(v, id)),
        );
    }
    y + 34
}

/// The reserved slot, filled from `custom-section.json` in the asset folder.
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
}
