# 面板结构性重构设计（菜单主题 / 鼠标设置 / 滚动）

> 需求来源：用户 2026-11-01 的 8 条要求（见文末「原始需求」）。
> 本文是**动手前的设计**，遵守 `docs/ARCHITECTURE.md` 的插入点表和 `docs/LESSONS.md` 的铁律。
> 读完本文再看代码：先看「一、区块划分」，再看「三、数据流」，最后看「六、分阶段」。

---

## 零、先厘清三个概念（后面全部按这个用词）

| 概念 | 含义 | 归属进程 | 现存位置 |
|---|---|---|---|
| **菜单主题** | 菜单栏（= 面板窗口 `AiMousePanel`）**自己**长什么样：面板强调色、面板缩放 | 面板进程 | 现在和光标共用 `st.accent`（这就是"混在一起"） |
| **AI 鼠标设置** | 一只 AI 光标长什么样：造型（鲸鱼/圆点/箭头）、颜色、大小、透明度、徽标、标签 | 光标进程（每只一个） | `St.theme/accent/scale/opacity/badge/label` |
| **鼠标预设** | 上述「AI 鼠标设置」的命名快照 | 宿主写的 JSON | 现在混在 `presets.json`（主题+光标字段都在一起） |

**关键事实（决定了整个方案）**：面板是 `--panel` 进程，**每只 AI 光标各起一个**。
所以"在面板里选中别的鼠标"**不可能**靠面板自己改，必须：
面板写意图 → 文件 → 宿主消费 → 宿主给目标光标进程下命令 → 宿主把结果广播回所有面板。

---

## 一、区块划分（SECTIONS 数组的最终顺序）

`overlay/src/sections.rs` 的 `SECTIONS` 数组顺序 = 显示顺序。重构后：

| # | key | 标题 | 归属 | 内容 | 来源 |
|---|---|---|---|---|---|
| 1 | `actions` | 操作 | 全局 | 显示/隐藏/停止/启用、拖拽开关、停靠站、小地图、停止全部、退出 | 现有 `body_actions`（保留） |
| 2 | `menu_theme` | **菜单主题** | 菜单 | `‹ 主题名 ›` 切换；应用/保存/删除；**菜单强调色**色板；**菜单缩放**滑杆 | **新** `body_menu_theme` |
| 3 | `mouse` | **AI 鼠标设置** | 光标 | 第一行 `‹ 预设名 ›`（选中哪只鼠标）；造型（鲸鱼/圆点/箭头）、颜色色板+RGB+色环、大小、透明度、徽标、标签 | **新** `body_mouse`（吞并原 外观/造型/颜色 三节） |
| 4 | `mouse_presets` | 鼠标预设 | 光标 | 预设名列表（点击应用到选中鼠标）、保存当前、删除所选 | **新** `body_mouse_presets` |
| 5 | `artwork` | 图案（自定义鼠标 / 提示 / 附属） | 光标 | 三个槽位 + 参数编辑（**只对本面板光标**，见「七、已知限制」） | 现有 `body_artwork`（保留） |
| 6 | `minimap` | 小地图 | 全局 | 小地图窗口开关 | 现有（保留） |
| 7 | `presets` | 旧预设（兼容） | 全局 | 现有 `presets.json`：列出/导出/保存/删除 | 现有 `body_presets`（**保留不删**，满足"保留原有预设功能"） |
| 8 | `permission` | 权限等级 | 全局 | 停止/只读/操作/管理员 | 现有（保留） |
| 9 | `custom` | 自定义模块（空位） | 全局 | `custom-section.json` | 现有（保留） |

删除的三节：原 `appearance`（徽标+透明度）、`style`（造型+大小）、`colors`（颜色）——**它们的内容整体搬进 `mouse`**，
不是删功能：`body_mouse` 复用同样的控件 id（`badge_on/badge_off/opacity/scale/theme_*/ch_r/ch_g/ch_b/wheel/sw*`），
只是**加了"选中鼠标"这一层**（编辑对象由 `sel` 决定，而不是无脑改本地 St）。

**为什么这样分**：`menu_theme` 里没有任何一个控件会改变 AI 光标的像素；`mouse` 里没有任何一个控件会改变面板的像素，
除了「菜单强调色」——它被**新建的 `panel_accent`** 接管（光标继续用 `accent`）。这是"分开"的硬判据。

---

## 二、存储位置（两个文件，互不覆盖）

| 数据 | 文件 | 谁能写 | 说明 |
|---|---|---|---|
| 菜单主题预设 | `<assetDir>/menu-themes.json` | **只有宿主**（浮层写 assetDir 会 os error 5） | `[{name, accent, zoom, saved_at}]` |
| 鼠标预设 | `<assetDir>/cursor-presets.json` | **只有宿主** | `[{name, theme, label, color, opacity, scale, badge, saved_at}]` |
| 旧预设（兼容） | `<assetDir>/presets.json` | 只有宿主 | 原样不动，原工具/旧分区继续用 |
| 面板↔宿主意图 | `<exeDir>/logs/dock-command.json` | 浮层可写 | 现有信箱，**所有**动作都带 `epoch` |
| 选中哪只鼠标（宿主权威副本） | `<exeDir>/logs/cursor-ui.json` | 只有宿主 | `{selected: 2}`，重启后能恢复 |

浮层只**读**前三个（读没问题），要写就写信箱。这条是 `LESSONS.md` 3d 的结论，不再试探。

---

## 三、数据流：左右箭头切换选中鼠标

```
用户点 ‹ / ›
   │  panel_proc WM_LBUTTONDOWN (id = "mouse_prev" / "mouse_next")
   ▼
面板进程：st.sel = clamp(sel ± 1)          ← 只改"选中谁"，立刻重绘（乐观 UI）
          st.ui = roster[sel] 的设置快照   ← 编辑面板显示的值来自这里，不是本地 St
          写 <exeDir>/logs/dock-command.json
            {"action":"select_cursor","index":N,"epoch":now_ms()}
   │
   ▼  文件通道（唯一允许的跨进程通道之一）
宿主插件：setInterval(250ms) → drainMailbox()   ← 新：不再只在 ai_mouse_dock 里消费
          读命令 → 校验 epoch > 上次处理过的 epoch
          selected = N；写 <exeDir>/logs/cursor-ui.json
          给第 N 只光标的子进程发协议：status（拿最新设置）
          向**所有**面板进程推：panel_roster { self, selected, cursors:[...] }
   │
   ▼  stdin JSON（宿主 → 子进程，架构文档规定的下行通道）
每个面板进程：st.roster / st.sel / st.ui 更新 → panel_dirty → 重绘
```

**为什么不能在面板里直接切**：
1. 每只 AI 光标是**独立进程**，面板拿到的是自己进程的 `St`；别的光标的状态根本不在这块内存里，
   也没有任何 Win32 句柄可以"改一下它的窗口"。
2. 面板进程写不了 `assetDir`（os error 5），写不了别人的配置。
3. 面板**能**给别的进程发消息的唯一途径就是文件；而**唯一能收文件的**是宿主（宿主才是子进程的父进程，
   只有它持有 stdin 写端）。所以"宿主消费"不是设计选择，是通道约束。

**编辑转发（同一套流向）**：`mouse` 分区里任何控件被改时——
- 若 `sel` 指向**本进程自己的光标**：直接改本地 `St`（即时、无延迟，和今天一样）；
- 否则：写 `{"action":"cursor_patch","target":"<名字>","patch":{...},"epoch":…}`，
  本地 `st.ui` 先乐观更新（画面上数值立刻动），宿主排空后给目标光标发 `theme/color/opacity/scale/badge/label`，
  再把新 roster 推回来收敛。**不重复应用**：只有 `sel != self` 才转发，因此不会出现"本地改一次 + 宿主再改一次"。

**协议新增**（`handle_line` 的 match 里按语义区插入，不是文件末尾）：

| 命令 | 方向 | 载荷 | 作用 |
|---|---|---|---|
| `panel_roster` | 宿主→面板 | `{self, selected, cursors:[{name, preset, display, theme, color, opacity, scale, badge, label}]}` | 面板渲染选择器 + 编辑面板的数值来源 |
| `panel_theme` | 宿主→面板 | `{accent, zoom}` | 应用菜单主题（只影响面板像素） |
| `panel_scroll` | 宿主→面板 | `{y}` 或 `{delta}` | 自测/自动化验证滚动 |
| `panel_identity` | 宿主→面板 | `{name}` | 本面板属于哪只光标（用于 `sel == self` 判定） |

---

## 四、命名规则

**显示名（选择器里的那个字符串）**，对 roster 里第 `i` 只：

```
display(i) = preset_name            // 该光标最近一次"保存/应用"过的鼠标预设名，非空时优先
           | i == 0 ? "默认"        // 第一只（default 光标）未命名
           | "自定义" + (i)         // i >= 1 且未命名：第 2 只=自定义1、第 3 只=自定义2（1 基）
```
`i` 是**该光标在 roster 里的下标**（0 基），显示时 +1 → 与"第一只默认叫默认、其余依次自定义1/2…"一致。
空名、纯空白名一律视为未命名。

**保存时的自动命名**：面板「保存当前」没给名字 → 用 `display(i)` 当名字（默认 / 自定义N）。
同名保存 = 覆盖（沿用旧 `upsertPreset` 的语义），**但内容相同则什么都不写**（见下）。

**去重判定（req 7）**：把预设的**可比较字段规范化后拼成一个键**，键相同即"同一个预设"：

```
key(p) = [ theme.toLowerCase().trim(),
           label.trim(),                    // 大小写敏感：徽标文字是给人看的
           color.toUpperCase(),             // #4fc3f7 == #4FC3F7
           round(opacity, 3),               // 0.5500001 == 0.55
           round(scale, 3),
           badge ? 1 : 0
         ].join("|")
```
保存流程（宿主侧）：
1. 现算出 `key(new)`；
2. 在 `cursor-presets.json` 里找 **同键** 的已有项：
   - 找到 → **不追加**，只把该项 `saved_at` 更新（并且若请求带了新名字，**不**改名，返回已有名字），
     结果集大小不变（这就是 req 7 的"只留一份"）；
   - 没找到 → 追加。
3. 单测（`tests/presets.test.ts` 追加）：
   - 同设置保存两次 → 长度 1、第二次返回"已存在"；
   - `#4fc3f7` 与 `#4FC3F7`、`0.55` 与 `0.5500000001` 视为相同；
   - `badge` 不同 / `theme` 不同 → 视为不同；
   - 菜单主题同规则（`accent|zoom`）。

---

## 五、滚动方案（req 8，同时修 1080p「点不到」缺陷）

**结论：面板内滚轮 + 视口裁剪 + 命中框平移。** 不采用"窗口跟着内容一起变高"（会和任务栏打架），
也不采用"每个分区内部各自滚动"（没有统一滚动条、滚轮归谁不明确）。

具体：

1. **视口**：`viewport_h = min(content_h, work_h - 16)`，`work_h` 来自 `GetMonitorInfoW` 的 `rcWork`
   （排除任务栏）。面板窗口高度 = `viewport_h`；`content_h` 由 `sections::build` 算出（现有 `panel_height`）。
2. **滚动量**：`st.scroll_y ∈ [0, max(0, content_h - viewport_h)]`。`sections::build(v, 48 - scroll_y, …)`
   —— 布局整体上移，**所有 Hit 矩形跟着平移**（这就是"命中框平移"：`pick()` 拿到的坐标天然对齐，
   不需要第二套坐标换算）。
3. **裁剪**：
   - 命中：`panel_layout()` 产出后过滤 `h.r.bottom > HEADER_BOTTOM(41) && h.r.top < viewport_h`，
     再把 `r.top/right/bottom` 夹进视口 —— 保证**任务栏下方 / 头部条下面的控件点不到**。
   - 绘制：`draw_panel` 里对内容区 `IntersectClipRect(hdc_big, 0, 41, PW*ss, h*ss)`，画完恢复；
     否则上滚的内容会盖住标题栏。
4. **滚轮输入**：
   - `panel_proc` 增加 `0x020A`（`WM_MOUSEWHEEL`）分支，**先按 LESSONS 规矩打印原始 `wparam`**，
     再取 `hi_signed/120 * 3` 行 → 改 `scroll_y` → `panel_dirty` + 需要时 `want_panel_resize`；
   - 兜底：现有 `WH_MOUSE_LL` 钩子里同一消息也记一份（`MSLLHOOKSTRUCT.mouseData` 高 16 位就是 delta），
     存进 `HOOK_WHEEL` 原子量；主循环发现"钩子看到的滚轮没被 `panel_proc` 消费"且指针在面板矩形内 → 手动滚。
     这一条是为了防 `LESSONS.md` 里记录的"dock 滚轮 `wParam` 整段是 0"重演；
   - 顶部 clamp 到 0，底部 clamp 到 `content_h - viewport_h`，两端都有 4px 回弹留白（视觉上"到头了"）。
5. **可见性**：右边缘画一条 3px 滚动条（`track` + `thumb`），`Kind::Scrollbar` 不参与点击（纯指示）。
6. **1080p 缺陷的根治**：`viewport_h` 一旦被工作区夹住，「预设」分区就不再落在任务栏下面——
   要么在视口内，要么滚一格就到。**不再需要 `PH_MAX=1200` 当"唯一裁决者"**：
   `PH_MAX` 保留为画布分配上限（防止 DIB 无限大），但窗口高度 = 视口高度。

---

## 六、分阶段顺序（每阶段都要编译 + 35/35 + 真机验证）

| 阶段 | 内容 | 涉及文件 | 验证 |
|---|---|---|---|
| **A. 滚动** | 视口/裁剪/命中平移/滚轮/滚动条/工作区取高；顺带修 1080p | `main.rs`、`widget.rs`(+`scroll` 字段)、`sections.rs`(build 用平移，加 `Kind::Scrollbar` 绘制分支) | 自测 `python scripts/selftest-overlay.py` 35/35；真机：滚轮在面板上滚，截图前后对比内容上移；`dump` 出图确认「预设」可见 |
| **B. 分区拆分 + 存储分离** | 新增 `menu_theme`/`mouse`/`mouse_presets`；删三节；新增 `panel_accent`；`menu-themes.json` 读写（宿主）；旧预设保留 | `sections.rs`、`widget.rs`、`main.rs`、`src/index.ts`、`src/panelstate.ts`(新)、`tests/` | 编译 + 35/35；真机点「菜单主题」色板 → 只有面板变色、光标不变；`logs/dock-command.json` 出现 `menu_theme_save`；`menu-themes.json` 生成 |
| **C. 选中鼠标** | roster 协议、身份、左右箭头、名称显示、路由 patch、宿主 250ms 排空 | `main.rs`、`sections.rs`、`widget.rs`、`src/index.ts`、`src/panelstate.ts` | 双光标真机：点 › 名字变「自定义1」，编辑颜色后目标光标变色，另一只不变；`cursor-ui.json` 落盘 |
| **D. 鼠标预设** | 保存/应用/删除 + 去重 + 自动命名；`cursor-presets.json` | `src/panelstate.ts`、`src/index.ts`、`main.rs`(命令与列表渲染) | `pnpm exec vitest run tests/`（含去重单测）+ `tsc --noEmit`；真机：同一设置存两次 → 列表只有一条 |
| **E. 收尾** | 文档、`ARCHITECTURE.md`/`LESSONS.md` 增补、全量回归 | docs | 全量：35/35 + Rust 单测 + TS 单测 + tsc + 真机逐项 |

编译前固定动作（否则 cargo 假成功）：
```bash
export CARGO_HOME="E:/rust/cargo" RUSTUP_HOME="E:/rust/rustup"
touch -d "2030-01-01 00:00:00" overlay/src/*.rs
cargo build --release --manifest-path overlay/Cargo.toml
cp -f overlay/target/release/ai-mouse-overlay.exe bin/ai-mouse-overlay.exe
md5sum overlay/target/release/ai-mouse-overlay.exe bin/ai-mouse-overlay.exe   # 必须一致
```

---

## 七、已知限制 / 故意不做

1. **图案分区（artwork）永远只编辑"本面板的那只光标"**：图片路径/偏移在别的进程里，
   要转发得把整个槽位状态走一遍信箱；本期只转发预设字段（theme/label/color/opacity/scale/badge）。
   当 `sel != self` 时该分区顶部显示一行提示「图案只对本面板光标生效」。
2. 滚动不做平滑动画（直接跟手 3 行/格），避免和 8ms 主循环抢帧。
3. 菜单主题只含 `accent + zoom`（可扩展），不做整套配色方案。
4. 不改 `presets.json` 的格式（旧文件旧工具照常工作），避免破坏"保留原有预设功能"。
5. 所有跨进程写入仍只有两条通道；不加 socket / 不加命名管道。

---

## 八、原始需求 → 设计落点对照

| # | 需求 | 落在哪 |
|---|---|---|
| 1 | 菜单主题和 AI 鼠标设置分开 | 一、`menu_theme` / `mouse` 两节 + 新建 `panel_accent` |
| 2 | 鼠标设置里选中每一只鼠标 | 三、roster + `sel` + `cursor_patch` 路由 |
| 3 | 每只鼠标命名保存；保留原预设 | 四、命名 + `cursor-presets.json`；旧 `presets` 分区保留 |
| 4 | 鼠标设置与主题设置分开保存 | 二、`cursor-presets.json` vs `menu-themes.json` |
| 5 | 左右箭头切换选中鼠标 | 一、`mouse` 第一节 `mouse_prev`/`mouse_next` |
| 6 | 箭头旁显示预设名（默认/自定义N） | 四、`display(i)` |
| 7 | 相同预设不重复保存 | 四、规范化 key + 单测 |
| 8 | 菜单栏滚轮翻动 | 五、视口裁剪 + 滚轮 + 滚动条 |

---

## 九、实施状态（2026-11-01 完成）

### 已完成并验证

| 阶段 | 内容 | 证据 |
|---|---|---|
| A 滚动 | 视口 = `min(内容, 工作区-16)`、命中框平移、内容区裁剪、`pick` 视口守卫、`WM_MOUSEWHEEL`、右缘滚动条；顺带修掉 1080p「预设压在任务栏下」 | `live-panel-check.py` A1–A11：真滚轮 0→60→196（max）、点滚出视口的「预设」表头能展开、点不可见的那一半无反应 |
| B 拆分 + 存储 | `menu_theme` / `mouse` / `mouse_presets` 三节；`panel_accent` 与 `accent` 彻底分开；旧 `presets` 分区移除（文件与工具保留）；`menu-themes.json` + `cursor-presets.json` | B1–B12：改菜单色不动光标色（反之亦然）、应用主题改 accent+zoom、保存写对信箱；单测验证两文件分离 |
| C 选中鼠标 | `panel_roster` / `panel_identity` / `panel_theme` 协议；‹ › 切换 + 名字；编辑路由（本人本地、别人写信箱）；`cursor_patch` → 协议命令 | C1–C8：切到「自定义1」显示其颜色、改色写 `cursor_patch{target:agent-b}` 且本地不变、切回本地立即生效且不再写信箱 |
| D 鼠标预设 | 保存/应用/删除 + 自动命名（默认 / 自定义N / 已保存名）+ 规范化去重；`cursor-ui.json` 记住选中项 | D1–D2（面板写信箱）；`node scripts/live-host-check.mjs` E4–E14：端到端落盘、同主题/同设置存两次只留一条、`cursor_patch` 真改到光标、两只光标名字为 `默认` / `自定义1` |
| E 收尾 | `ARCHITECTURE.md` 第七节（新插入点）、`LESSONS.md` 第 12 条（五条教训） | — |

一次性脚本（都可重复跑）：

```bash
export CARGO_HOME="E:/rust/cargo" RUSTUP_HOME="E:/rust/rustup"
touch -d "2030-01-01 00:00:00" overlay/src/*.rs && (cd overlay && cargo build --release)
cp -f overlay/target/release/ai-mouse-overlay.exe bin/ai-mouse-overlay.exe

PYTHONIOENCODING=utf-8 python scripts/selftest-overlay.py bin/ai-mouse-overlay.exe artifacts  # 35/35
PYTHONIOENCODING=utf-8 python scripts/live-panel-check.py bin/ai-mouse-overlay.exe artifacts   # 35/35 真输入
pnpm run build && node scripts/live-host-check.mjs                                            # 14/14 宿主端到端
pnpm exec tsc --noEmit && pnpm exec vitest run tests/                                         # 65/65
```

`live-host-check.mjs` 用假 Cordis 上下文装载**构建后的插件**（`lib/index.js`），跑的是真的
`apply()`：真起浮层进程、真跑 250ms 信箱消费、真写文件。这样不需要重启 DSH 就能验证宿主
半边；面板半边由 `live-panel-check.py` 用真鼠标注入验证。两半用**同一套 mailbox 载荷**对接。

### 未做 / 已知边界（不当成已完成）

1. **图案（artwork）分区只编辑本面板那只光标**：UI 顶部已明说「仅本面板这只光标」。跨进程
   转发整套槽位状态太重，本期只转发预设字段（theme/label/color/opacity/scale/badge）。
2. **面板不再列出旧 `presets.json`**（`ai_mouse_preset` 工具与文件格式不变）。要在面板上应用
   旧预设，需要回到工具；这是"只保留一个预设区块"的取舍。
3. 菜单主题只含 `accent + zoom`，不是整套配色方案。
4. 视口高度按**主显示器**工作区算；面板被拖到副屏时不会跟着副屏 `rcWork` 变。
5. 选中鼠标是宿主侧概念（`selectedCursor`），**不改变** `ai_mouse_*` 工具默认作用的光标
   （那仍是 `ai_mouse_cursor use` 的 active）。面板改的是"选中那只"的外观，不是"agent 用哪只"。

