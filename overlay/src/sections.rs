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
    channels_of, hex, CustomRowView, Hit, Kind, PanelView, SlotView, CUSTOM_IDS, MT_SWATCH_IDS,
    SWATCH_IDS, SWATCHES,
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
///
/// Structure rule (2026-11-01 redesign, see `docs/PANEL-REDESIGN.md`): the panel is
/// split into **菜单主题** (the menu bar's own chrome: accent + zoom) and
/// **AI 鼠标设置** (one cursor's look). The two never share a control, so choosing a
/// menu theme can no longer repaint the cursor and vice versa.
pub const SECTIONS: &[SecDef] = &[
    SecDef {
        key: "actions",
        header: "sec_actions",
        title: "操作",
        default_collapsed: false,
        render: body_actions,
    },
    SecDef {
        key: "menu_theme",
        header: "sec_menu_theme",
        title: "菜单主题",
        default_collapsed: true,
        render: body_menu_theme,
    },
    SecDef {
        key: "mouse",
        header: "sec_mouse",
        title: "AI 鼠标设置",
        default_collapsed: false,
        render: body_mouse,
    },
    SecDef {
        key: "mouse_presets",
        header: "sec_mouse_presets",
        title: "鼠标预设",
        default_collapsed: true,
        render: body_mouse_presets,
    },
    SecDef {
        key: "artwork",
        header: "sec_artwork",
        title: "图案（自定义鼠标 / 提示 / 附属）",
        // 展开：大小 / 横向 / 纵向 / 透明度都在这里面，收起来用户找不到
        default_collapsed: false,
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

/// Everything the layout needs, derived from one number.
///
/// Rectangles must never be written as bare constants again: they come from here,
/// so changing `zoom` moves and resizes every dependent widget together, and the
/// hit test (which uses the same values) always matches what is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub zoom: f64,
    pub width: i32,
}

impl Layout {
    pub fn new(zoom: f64) -> Layout {
        let z = zoom.clamp(0.6, 2.0);
        Layout {
            zoom: z,
            width: (crate::PW as f64 * z).round() as i32,
        }
    }
    fn sc(&self, v: i32) -> i32 {
        (v as f64 * self.zoom).round() as i32
    }
    pub fn pad(&self) -> i32 {
        self.sc(14)
    }
    pub fn gap(&self) -> i32 {
        self.sc(8)
    }
    pub fn row(&self) -> i32 {
        self.sc(22)
    }
    pub fn btn_h(&self) -> i32 {
        self.sc(34)
    }
    pub fn content_right(&self) -> i32 {
        self.width - self.pad()
    }
    pub fn button_row(&self, y: i32, n: i32) -> (i32, i32) {
        let total = self.content_right() - self.pad();
        let w = (total - self.gap() * (n - 1)) / n.max(1);
        (w, y)
    }
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
    // Geometry is authored once at the design size and scaled here, at the single
    // exit point. Drawing and hit testing both consume these hits, so they cannot
    // drift apart when the zoom changes.
    let zoom = v.zoom.clamp(0.6, 2.0);
    let start = out.len();
    let bottom = build_design(v, y, out);
    // A grip in the footer: dragging it changes the zoom. Authored at the design
    // size like everything else, so it scales with the panel.
    out.push(
        Hit::new(rect(content_right() - 84, bottom + 4, content_right(), bottom + 24), Kind::Button, "panel_grip")
            .text("大小")
            .hover(hov(v, "panel_grip")),
    );
    if (zoom - 1.0).abs() > 0.001 {
        for h in out[start..].iter_mut() {
            h.r.left = (h.r.left as f64 * zoom).round() as i32;
            h.r.top = (h.r.top as f64 * zoom).round() as i32;
            h.r.right = (h.r.right as f64 * zoom).round() as i32;
            h.r.bottom = (h.r.bottom as f64 * zoom).round() as i32;
        }
    }
    return (bottom as f64 * zoom).round() as i32;
}

fn build_design(v: &PanelView, mut y: i32, out: &mut Vec<Hit>) -> i32 {
    // Selected cursors get one header each across the top of the menu bar.
    if !v.columns.is_empty() {
        let n = v.columns.len() as i32;
        let gap = 6;
        // the fold chip takes a slot at the right end of the strip
        let fold_w = 26;
        let w = (content_right() - PAD - gap * (n - 1) - fold_w - gap) / n.max(1);
        for (i, (name, color)) in v.columns.iter().enumerate() {
            let x = PAD + i as i32 * (w + gap);
            out.push(
                Hit::new(rect(x, y - 4, x + w - 22, y + 20), Kind::Button, "col_head")
                    .text(name.clone())
                    .data(i as i32),
            );
            // 新建 / 关闭该栏的入口；索引进 id，因为 on_toggle 拿不到 Hit.data
            let add_id = format!("col_add_{i}");
            out.push(
                Hit::new(
                    rect(x + w - 20, y - 4, x + w, y + 20),
                    Kind::Button,
                    Box::leak(add_id.into_boxed_str()),
                )
                .text("+"),
            );
            out.push(
                Hit::new(rect(x, y + 22, x + w, y + 25), Kind::Sep, "sep_col")
                    .data((*color & 0xFFFFFF) as i32),
            );
        }
        let fx = PAD + n * (w + gap);
        out.push(
            Hit::new(rect(fx, y - 4, fx + fold_w, y + 20), Kind::Button, "col_fold")
                .text(if v.columns_collapsed { ">" } else { "v" }),
        );
        y += 30;
    }
    for sec in SECTIONS {
        let collapsed = is_collapsed(v, sec.key) || v.columns_collapsed;
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
    // authored at design size; build() scales the whole panel in one place
    let l = Layout::new(1.0);
    let acts: [(&'static str, &str, bool); 4] = [
        ("show", "显示", v.visible),
        ("hide", "隐藏", !v.visible),
        ("stop", "停止", v.permission == 0),
        ("resume", "启用", v.permission > 0),
    ];
    let (w, by) = l.button_row(y, 4);
    for (i, (id, text, on)) in acts.iter().enumerate() {
        let bx = l.pad() + i as i32 * (w + l.gap());
        out.push(
            Hit::new(rect(bx, by, bx + w, by + l.btn_h()), Kind::Button, id)
                .text(*text)
                .active(*on)
                .hover(hov(v, id)),
        );
    }
    y = by + l.btn_h() + l.sc(4);
    // the drag switch sits on its own line, also laid out from the model
    out.push(
        Hit::new(rect(l.pad(), y, l.pad() + l.sc(34), y + l.sc(16)), Kind::Label, "l_drag")
            .text("拖拽"),
    );
    out.push(
        Hit::new(
            rect(l.pad() + l.sc(38), y - l.sc(2), l.pad() + l.sc(130), y + l.sc(22)),
            Kind::Button,
            "drag_toggle",
        )
        .text(if v.drag_enabled { "可用" } else { "禁用" })
        .active(v.drag_enabled)
        .hover(hov(v, "drag_toggle")),
    );
    out.push(
        Hit::new(
            rect(l.pad(), y + l.sc(30), l.pad() + l.sc(120), y + l.sc(56)),
            Kind::Button,
            "stop_all",
        )
        .text("停止全部")
        .hover(hov(v, "stop_all")),
    );
    out.push(
        Hit::new(
            rect(l.pad() + l.sc(128), y + l.sc(35), l.content_right(), y + l.sc(53)),
            Kind::Label,
            "l_stop_all_hint",
        )
        .text("停掉所有 AI 鼠标"),
    );
    out.push(
        Hit::new(
            rect(l.pad(), y + l.sc(30), l.pad() + l.sc(120), y + l.sc(56)),
            Kind::Button,
            "quit_app",
        )
        .text("退出")
        .hover(hov(v, "quit_app")),
    );
    out.push(
        Hit::new(
            rect(l.pad() + l.sc(128), y + l.sc(35), l.content_right(), y + l.sc(53)),
            Kind::Label,
            "l_quit_hint",
        )
        .text("关闭所有 AI 鼠标并退出"),
    );
    out.push(
        Hit::new(
            rect(l.pad(), y + l.sc(62), l.pad() + l.sc(120), y + l.sc(88)),
            Kind::Button,
            "dock_toggle",
        )
        .text("停靠站")
        .hover(hov(v, "dock_toggle")),
    );
    out.push(
        Hit::new(
            rect(l.pad() + l.sc(128), y + l.sc(62), l.pad() + l.sc(248), y + l.sc(88)),
            Kind::Button,
            "map_toggle",
        )
        .text("小地图")
        .hover(hov(v, "map_toggle")),
    );
    out.push(
        Hit::new(
            rect(l.pad() + l.sc(140), y, l.content_right(), y + l.sc(18)),
            Kind::Label,
            "l_drag_hint",
        )
        .text(if v.dragging {
            "正在被用户拖动…"
        } else {
            "按住光标即可用真实鼠标拖动它"
        }),
    );
    y + l.sc(122)
}

/// 菜单主题：**只**描述菜单栏自己长什么样。
///
/// 硬判据：这个分区里没有任何一个控件会改变 AI 光标的像素。它写 `panel_accent`
/// 与 `panel_zoom`，与光标用的 `accent` / `scale` 是两套值、两套存储。
fn body_menu_theme(v: &PanelView, x: i32, mut y: i32, out: &mut Vec<Hit>) -> i32 {
    // 面板强调色（只影响菜单栏的按钮/滑杆/滚动条）
    for (i, (id, rgb)) in MT_SWATCH_IDS.iter().zip(SWATCHES.iter()).enumerate() {
        let bx = x + i as i32 * 36;
        out.push(
            Hit::new(rect(bx, y, bx + 26, y + 26), Kind::Swatch, id)
                .text(hex(*rgb))
                .active(v.panel_accent & 0xFFFFFF == *rgb)
                .hover(hov(v, id)),
        );
    }
    out.push(
        Hit::new(rect(x + 268, y + 4, content_right(), y + 22), Kind::LabelRight, "l_mt_hex")
            .text(hex(v.panel_accent)),
    );
    y += 32;

    // 菜单缩放：和右下角「大小」把手是同一个值
    out.push(Hit::new(rect(x, y + 4, x + 56, y + 20), Kind::Label, "l_mt_zoom").text("菜单缩放"));
    out.push(
        Hit::new(rect(x + 62, y + 2, x + 330, y + 20), Kind::Slider, "panel_zoom")
            .data((((v.zoom - 0.6) / 1.4) * 1000.0).clamp(0.0, 1000.0) as i32)
            .hover(hov(v, "panel_zoom")),
    );
    out.push(
        Hit::new(rect(x + 336, y + 4, content_right(), y + 20), Kind::LabelRight, "l_mt_zoom_v")
            .text(format!("{:.0}%", v.zoom * 100.0)),
    );
    y += 30;

    // 主题预设列表（menu-themes.json，宿主写、面板只读）
    let current = v
        .menu_themes
        .get(v.menu_theme_sel)
        .cloned()
        .unwrap_or_else(|| "未保存".to_string());
    out.push(
        Hit::new(rect(x, y, content_right(), y + 18), Kind::Label, "l_mt_cur")
            .text(format!("主题预设 {} 个 · 当前：{}", v.menu_themes.len(), current)),
    );
    y += 22;
    for (i, name) in v.menu_themes.iter().enumerate().take(6) {
        let id = format!("mt_apply_{i}");
        out.push(
            Hit::new(rect(x, y, content_right(), y + 22), Kind::Button, Box::leak(id.into_boxed_str()))
                .text(name.clone())
                .active(v.menu_theme_sel == i)
                .hover(hov(v, "mt_apply")),
        );
        y += 26;
    }
    out.push(
        Hit::new(rect(x, y + 2, x + 110, y + 26), Kind::Button, "mt_save")
            .text("保存主题")
            .hover(hov(v, "mt_save")),
    );
    out.push(
        Hit::new(rect(x + 118, y + 2, x + 228, y + 26), Kind::Button, "mt_delete")
            .text("删除所选")
            .hover(hov(v, "mt_delete")),
    );
    out.push(
        Hit::new(rect(x + 236, y + 2, x + 340, y + 26), Kind::Button, "mt_reset")
            .text("恢复默认")
            .hover(hov(v, "mt_reset")),
    );
    out.push(
        Hit::new(rect(x, y + 32, content_right(), y + 50), Kind::Label, "l_mt_hint")
            .text("存 menu-themes.json（宿主写）；相同的主题只留一份"),
    );
    y + 56
}

/// 「AI 鼠标设置」：编辑**选中的那一只**光标。
///
/// 第一节是选鼠标的 ‹ 预设名 ›：名字是这只光标当前套用的鼠标预设名，没有名字时
/// 第一只是「默认」，其余依次「自定义1」「自定义2」…（见 `docs/PANEL-REDESIGN.md`）。
///
/// 控件 id 与重构前完全一致（badge_on / opacity / scale / theme_* / sw* / ch_* / wheel），
/// 这样已有的自测、点击脚本和用户的肌肉记忆都还有效。
fn body_mouse(v: &PanelView, x: i32, mut y: i32, out: &mut Vec<Hit>) -> i32 {
    // ---- 选中哪只鼠标 -------------------------------------------------------
    out.push(
        Hit::new(rect(x, y, x + 34, y + 26), Kind::Button, "mouse_prev")
            .text("\u{2039}")
            .hover(hov(v, "mouse_prev")),
    );
    out.push(
        Hit::new(rect(x + 40, y, x + 300, y + 26), Kind::Label, "mouse_sel")
            .text(format!(
                "{}（{}/{}）",
                v.sel_name,
                v.sel_index + 1,
                v.sel_total.max(1)
            )),
    );
    out.push(
        Hit::new(rect(x + 306, y, x + 340, y + 26), Kind::Button, "mouse_next")
            .text("\u{203A}")
            .hover(hov(v, "mouse_next")),
    );
    out.push(
        Hit::new(rect(x + 348, y + 4, content_right(), y + 22), Kind::LabelRight, "l_mouse_swap")
            .text(if v.sel_total > 1 { "左右切换" } else { "只有这一只" }),
    );
    y += 32;

    // ---- 造型 + 大小 --------------------------------------------------------
    let themes: [(&'static str, &str, &str); 3] =
        [("theme_whale", "鲸鱼", "whale"), ("theme_dot", "圆点", "dot"), ("theme_arrow", "箭头", "arrow")];
    for (i, (id, text, key)) in themes.iter().enumerate() {
        let bx = x + i as i32 * 74;
        out.push(
            Hit::new(rect(bx, y, bx + 66, y + 26), Kind::Button, id)
                .text(*text)
                .active(v.sel_settings.theme == *key)
                .hover(hov(v, id)),
        );
    }
    out.push(
        Hit::new(rect(x + 226, y, content_right(), y + 18), Kind::LabelRight, "l_scale")
            .text(format!("大小 {}%", (v.sel_settings.scale * 100.0).round() as i32)),
    );
    out.push(
        Hit::new(rect(x, y + 28, content_right(), y + 46), Kind::Slider, "scale")
            .data(((v.sel_settings.scale - 0.6) / 1.4 * 1000.0).clamp(0.0, 1000.0) as i32)
            .hover(hov(v, "scale")),
    );
    y += 52;

    // ---- 颜色 --------------------------------------------------------------
    for (i, (id, rgb)) in SWATCH_IDS.iter().zip(SWATCHES.iter()).enumerate() {
        let bx = x + i as i32 * 36;
        out.push(
            Hit::new(rect(bx, y, bx + 26, y + 26), Kind::Swatch, id)
                .text(hex(*rgb))
                .active(v.sel_settings.accent & 0xFFFFFF == *rgb)
                .hover(hov(v, id)),
        );
    }
    out.push(
        Hit::new(rect(x + 268, y + 4, content_right(), y + 22), Kind::LabelRight, "l_hex")
            .text(hex(v.sel_settings.accent)),
    );
    y += 30;

    let labels = ["R", "G", "B"];
    let channels = channels_of(v.sel_settings.accent);
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

    // 色环：色相环 + 内部明度/饱和方块
    let d = crate::wheel::D;
    out.push(
        Hit::new(rect(x, y + 6, x + d, y + 6 + d), Kind::Wheel, "wheel").hover(hov(v, "wheel")),
    );
    out.push(
        Hit::new(rect(x + d + 10, y + 12, content_right(), y + 30), Kind::LabelRight, "l_wheel_hex")
            .text(hex(v.sel_settings.accent)),
    );
    out.push(
        Hit::new(rect(x + d + 10, y + 36, content_right(), y + 54), Kind::Label, "l_wheel_hint")
            .text("外环选色相"),
    );
    out.push(
        Hit::new(rect(x + d + 10, y + 54, content_right(), y + 72), Kind::Label, "l_wheel_hint2")
            .text("内块选明度/饱和"),
    );
    y += 6 + d + 8;

    // ---- 透明度 + AI 提示 ---------------------------------------------------
    out.push(
        Hit::new(rect(x, y + 4, x + 56, y + 20), Kind::Label, "l_opacity")
            .text(format!("透明度 {}%", (v.sel_settings.opacity * 100.0).round() as i32)),
    );
    out.push(
        Hit::new(rect(x + 62, y + 2, x + 330, y + 20), Kind::Slider, "opacity")
            .data(((v.sel_settings.opacity - 0.15) / 0.85 * 1000.0).clamp(0.0, 1000.0) as i32)
            .hover(hov(v, "opacity")),
    );
    out.push(
        Hit::new(rect(x + 336, y + 4, content_right(), y + 20), Kind::LabelRight, "l_opacity_v")
            .text(format!("{:.0}%", v.sel_settings.opacity * 100.0)),
    );
    y += 30;
    out.push(Hit::new(rect(x, y + 4, x + 60, y + 20), Kind::Label, "l_badge").text("AI 提示"));
    out.push(
        Hit::new(rect(x + 62, y - 1, x + 112, y + 25), Kind::Button, "badge_on")
            .text("开")
            .active(v.sel_settings.badge)
            .hover(hov(v, "badge_on")),
    );
    out.push(
        Hit::new(rect(x + 120, y - 1, x + 170, y + 25), Kind::Button, "badge_off")
            .text("关")
            .active(!v.sel_settings.badge)
            .hover(hov(v, "badge_off")),
    );
    out.push(
        Hit::new(rect(x + 182, y + 4, content_right(), y + 22), Kind::Label, "l_badge_hint")
            .text("按钮下方显示的文字"),
    );
    y + 32
}

/// 「鼠标预设」：命名保存/应用/删除**选中那只鼠标**的设置。
///
/// 只保留这一个预设分区（旧的 `presets` 分区已移除）：旧文件 `presets.json` 的格式与
/// 工具（`ai_mouse_preset`）继续可用，但面板上不再出现第二个「预设」区块。
fn body_mouse_presets(v: &PanelView, x: i32, mut y: i32, out: &mut Vec<Hit>) -> i32 {
    if v.mouse_presets.is_empty() {
        out.push(
            Hit::new(rect(x, y, content_right(), y + 18), Kind::Label, "l_no_mouse_presets")
                .text("还没有鼠标预设：点下面「保存当前」把选中的鼠标存下来"),
        );
        y += 22;
    }
    for (i, name) in v.mouse_presets.iter().enumerate().take(6) {
        let id = format!("mp_apply_{i}");
        out.push(
            Hit::new(rect(x, y, content_right(), y + 22), Kind::Button, Box::leak(id.into_boxed_str()))
                .text(name.clone())
                .active(v.mouse_preset_sel == i)
                .hover(hov(v, "mp_apply")),
        );
        y += 26;
    }
    out.push(
        Hit::new(rect(x, y + 2, x + 110, y + 26), Kind::Button, "mp_save")
            .text("保存当前")
            .hover(hov(v, "mp_save")),
    );
    out.push(
        Hit::new(rect(x + 118, y + 2, x + 228, y + 26), Kind::Button, "mp_delete")
            .text("删除所选")
            .hover(hov(v, "mp_delete")),
    );
    out.push(
        Hit::new(rect(x + 236, y + 2, x + 336, y + 26), Kind::Button, "mp_export")
            .text("导出")
            .hover(hov(v, "mp_export")),
    );
    out.push(
        Hit::new(rect(x, y + 32, content_right(), y + 50), Kind::Label, "l_mp_hint")
            .text("存 cursor-presets.json；相同设置只留一份"),
    );
    y + 56
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
    // Say it in the UI, not only in the docs: the artwork slots belong to THIS
    // process. The ‹ › selector above switches which mouse is being configured, and
    // the picture editors deliberately do not follow it (see PANEL-REDESIGN §7).
    out.push(
        Hit::new(rect(x, y, content_right(), y + 16), Kind::Label, "l_art_scope")
            .text("仅本面板这只光标：图案不随上面选择的鼠标切换"),
    );
    y += 20;
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

    let rows2: [(&'static str, &str, f64, String); 5] = [
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
            "art_rot_slider",
            "旋转",
            ((slot.rot + 180.0) / 360.0).clamp(0.0, 1.0),
            format!("{:.0}°", slot.rot),
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
