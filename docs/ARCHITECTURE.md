# 架构与逻辑块

> 先读这份，再改代码。**加功能 = 找到正确的插入点，不是往文件末尾追加。**
> 这份文档存在的理由：之前很多轮改动之所以"插不进去 / 改完没效果"，
> 是因为没有整体结构图，只能靠字符串锚点往后补，锚点一错就静默失败。

---

## 一、运行原理图（进程 / 窗口 / 数据流）

```
┌──────────────────────────────────────────────────────────────────────┐
│ DSH 宿主进程（Node）                                                  │
│   src/index.ts   插件入口：注册工具、权限门、拦截器、光标池             │
│   src/overlay.ts 管理"每个浮层子进程"的 stdin/stdout 与生命周期        │
│   src/presets.ts 预设（含分组/导出导入）  src/dock.ts 站牌数据          │
└───────────┬──────────────────────────────┬───────────────────────────┘
            │ spawn(exe, [])               │ spawn(exe, --dock --slots …)
            │ 每个"光标"一个进程            │ 停靠站一个进程
            ▼                              ▼
┌────────────────────────────┐   ┌──────────────────────────────┐
│ 浮层进程 A（=一只 AI 光标） │   │ 停靠站进程（--dock）          │
│ 窗口: AiMouseOverlay(光标) │   │ 窗口: AiMouseOverlay(条状)    │
│       AiMousePanel(菜单栏) │   │ 读: <exeDir>/logs/berths.json │
│       AiMouseGrab(抓取热区)│   │ 写: dock-selection.json       │
│                            │   │     dock-command.json         │
└────────────────────────────┘   └──────────────────────────────┘
┌────────────────────────────┐   ┌──────────────────────────────┐
│ 浮层进程 B（另一只光标）    │   │ 小地图进程（--map）           │
│ 同上，独立窗口与状态        │   │ 窗口: AiMouseOverlay(方)      │
└────────────────────────────┘   └──────────────────────────────┘

数据通道（只有两条，别再加第三条）：
  1) 下行：宿主 → 子进程 stdin，一行一个 JSON，回包带同一个 id
  2) 文件：跨进程共享状态，位置分两类
       <exeDir>/logs/   浮层/停靠站**能写**（日志、dock-command、dock-selection）
       <assetDir>/      只有**宿主**能写（berths.json、presets.json、素材）
     原因：浮层/停靠站写 assetDir 会被拒（os error 5），这是实测结论。
```

---

## 二、每个区块的职责（不要跨块做事）

| 区块 | 文件 | 只负责 | 绝不负责 |
|---|---|---|---|
| **插件入口** | `src/index.ts` | 工具注册、权限门、配置、光标池、把状态写进文件 | 像素绘制、Win32 |
| **进程管理** | `src/overlay.ts` | spawn/kill 子进程、stdin/stdout 协议、重连 | 业务判断 |
| **预设** | `src/presets.ts` | 预设数据、分组、导入导出 | 与浮层通信 |
| **站牌数据** | `src/dock.ts` | berths/selection 的读写与映射 | 窗口与绘制 |
| **拦截兼容层** | `src/intercept.ts` | 拦别的插件的真实鼠标工具 | 光标渲染 |
| **协议解析** | `overlay/src/main.rs` 的 `handle_line` | 命令分发（一个 match） | 绘制细节 |
| **绘制内核** | `overlay/src/main.rs` 的 `Canvas` | 画图原语、合成、呈现 | 业务状态 |
| **分区布局** | `overlay/src/sections.rs` | 面板各分区的排版与命中框 | 绘制像素 |
| **控件模型** | `overlay/src/widget.rs` | `Hit`/`Kind`/`PanelView` 数据结构 | 逻辑分支 |
| **图案** | `overlay/src/sprite.rs` | 解码、缓存、合成 | 窗口 |
| **GPU 抓屏** | `overlay/src/gpucap.rs` | 小地图的桌面抓取、着色器降采样、能力探测与降级 | 画布绘制、窗口操作 |
| **色环** | `overlay/src/wheel.rs` | 色环缓存与取色 | 面板 |
| **自定义模块** | `overlay/src/custom.rs` | JSON → 按钮/标签 | 执行代码 |
| **日志** | `overlay/src/log.rs` | 轮转、导出 | 业务 |

---

## 三、区块之间的连接（谁能调谁）

```
main() 启动流程（顺序不能乱，这是我踩过坑的地方）：
  1. log::init()                      ← 最早，后面的失败才有记录
  2. 解析 --flag（--dock / --map / --panel / --panel-zoom / --slots）
  3. state() 初始化（St 全部字段）
  4. GetModuleHandleW → hinst
  5. 注册窗口类（循环：AiMouseOverlay / AiMousePanel / AiMouseGrab）
  6. 建窗口（make_overlay / make_panel / make_grab）
  7. 读 stdin 线程启动 → handle_line 分发
  8. 主循环：
       a. 消费 want_* 标志（show/hide/panel/resize/affinity/layer/map/dock）
       b. 读跨进程命令文件（dock-command.json）
       c. 推进动画 → 计算 cur
       d. 需要时 draw_cursor / draw_panel（内部分区由 sections::build 排版）
       e. present（compose → UpdateLayeredWindow）
```

**依赖方向是单向的**：

```
widget.rs  ←── sections.rs ←── main.rs(draw_panel)
   ↑                ↑
   └── PanelView ───┘（main 组装好视图，传给 sections 排版，再回 main 绘制）
```

- `sections.rs` 只依赖 `widget.rs` 和 `PanelView` 的数据，**不碰 Win32**
- `main.rs` 负责把 `St` 转成 `PanelView`，再做绘制与窗口操作
- `Canvas` 不认业务，只认矩形与颜色

---

## 四、插入点分析：新功能该放哪

这是本文档最重要的部分。**每个"能加的地方"都标了它的约束**。

| 想加的东西 | 正确插入点 | 约束 |
|---|---|---|
| 一个新的**工具** | `src/index.ts` 里 `ctx.tools.register(...)`，按**语义区**插入（光标/预设/停靠站/层级），不要堆在末尾 | 需要 `gate(级别)` 与 `TEXT_OUTPUT` |
| 一个新的**协议命令** | `handle_line` 的 `match` 里，按字母/语义区插入 | 必须回 `id`；写文件要用 `<exeDir>/logs` |
| 一个新的**面板分区** | `sections.rs` 的 `SECTIONS` 数组里**按显示顺序插入**，再写一个 `body_xxx` | 分区函数签名固定：`(&PanelView, x, y, &mut Vec<Hit>) -> i32` |
| 一个新的**控件类型** | `widget.rs` 的 `Kind` 枚举 + `sections.rs` 里生成 `Hit` + `main.rs` 的 `draw_panel` match 里画它 | 三处必须同时加，否则 match 不穷尽编译失败 |
| 一个新的**跨进程数据** | 走文件，写 `<exeDir>/logs/<name>.json`；读取方自己算路径 | **不要**用 assetDir（进程写不了） |
| 一个新的**子进程模式** | `--<mode>` 标志 + `main()` 里分支 + 复用 `make_window("AiMouseOverlay")` | **不要**新建窗口类（`CreateWindowExW` 对新类返回 null，实测） |
| 一个新的**外观参数** | `St` 加字段 → `PanelView` 加字段 → `view_of` 传递 → 分区里出控件 | 三处一起改，漏一处就是"改了没效果" |
| 小地图的**桌面画面**（抓屏 + 缩放） | `main.rs` 的 `draw_map_surface` → `map_thumb` → `gpucap::desktop_thumb` | 画布只收成品缩略图（`Canvas::put_thumb`）；GPU 不可用时同一函数回落到 `resample_desktop` |

**反例（我犯过的）**：往 `draw_panel` 的 match 末尾追加一个 `Kind` 分支，但没在 `widget.rs` 加枚举值 → 不穷尽；
往 `sections.rs` 末尾追加分区 → 显示在最后，顺序不对；
用字符串锚点改代码 → 锚点变了一行就静默失败。

---

## 五、两条铁律（血的教训）

1. **改前先想"这是插入还是追加"**：追加之前先问自己"这个位置在数据结构上对吗？"
2. **顺序敏感的三处**：
   - `SECTIONS` 数组 = 面板上分区的**显示顺序**
   - `handle_line` 的 match = 协议命令的**分派顺序**（同名不必，但语义分组要清楚）
   - `main()` 的启动步骤 = **日志 → 参数 → 状态 → 窗口类 → 窗口 → 读线程 → 主循环**

---

## 六、剩余需求落在哪个插入点

| 需求 | 插入点 | 需要动几处 |
|---|---|---|
| 栏顶 `＋` 按钮 | 面板：`sections.rs` 栏头条加一个 `Hit`；点击写 `dock-command.json` 的 `toggle_column`；**宿主需要一个消费点**（下次工具调用时读一次） | 3 |
| 预设编辑器 UI | `sections.rs` 加一个 `presets` 分区（插在 `permission` 之前）；动作走已有协议 + 新增 `preset_export/import` | 2 |
| DSH 侧边栏 | **不在本仓库**：用 DSH 的 Web 侧边栏（浏览器虚拟界面）实现，读 `berths.json` / `presets.json` 渲染 HTML | 新工程 |
| 线程/渲染优化 | `main()` 主循环：把 `sprite.rs` 解码移出、静图不重绘 | 1 处为主 |
| 停靠站圆角黑边 | `Canvas::compose` 与 `present` 的尺寸对照（打印 canvas w/h vs compose w/h） | 1 处 |

---

## 七、面板结构性重构（2026-11-01）后的结构

设计见 [PANEL-REDESIGN.md](PANEL-REDESIGN.md)。**这一节是现在改面板前的必读**，
它替换了旧版「外观/造型/颜色/预设」四节混在一起的布局。

### 7.1 分区（`SECTIONS` 顺序 = 显示顺序）

| 顺序 | key | 标题 | 归属 | 只改什么 |
|---|---|---|---|---|
| 1 | `actions` | 操作 | 全局 | 显示/隐藏/停止/启用、拖拽、停靠站、小地图、停止全部、退出 |
| 2 | `menu_theme` | **菜单主题** | 菜单 | `panel_accent` + 菜单主题预设（`menu-themes.json`，**只有强调色**：`panel_zoom` 已删） |
| 3 | `mouse` | **AI 鼠标设置** | 光标 | 选中光标（roster）、造型、颜色、大小、透明度、徽标 |
| 4 | `mouse_presets` | **鼠标预设** | 光标 | `cursor-presets.json`：保存/应用/删除/导出 |
| 5 | `artwork` | 图案 | **本面板光标** | 三个槽位与参数（不随选择切换，UI 里有明示） |
| 6 | `minimap` | 小地图 | 全局 | 小地图窗口开关 |
| 7 | `permission` | 权限等级 | 全局 | 停止/只读/操作/管理员 |
| 8 | `custom` | 自定义模块（空位） | 全局 | `custom-section.json` |

**硬判据**：`menu_theme` 里没有任何控件会改变光标像素；`mouse` 里没有任何控件会改变
面板像素。两套颜色值是分开的（`panel_accent` vs `accent`），两套存储文件也是分开的。

旧的 `presets` 分区已从面板移除（**只保留一个预设区块**），`presets.json` 的格式与
`ai_mouse_preset` 工具完全不变，仍可用。

### 7.2 数据流（面板 → 宿主 → 目标光标）

```
面板点 ‹ › / 改颜色 / 存主题
   │ 能本地做的直接做（选中光标 == 本进程）
   │ 否则写 <exeDir>/logs/dock-command.json（带 epoch）
   ▼
宿主 drainMailbox()    ← 面板打开时 250ms 定时器 + ai_mouse_dock 各排空一次
   ├─ select_cursor  -> 写 logs/cursor-ui.json，再给所有面板推 panel_roster
   ├─ cursor_patch   -> 翻译成 theme/color/opacity/scale/badge 发给目标光标进程
   ├─ menu_theme_*   -> 写 <assetDir>/menu-themes.json（去重）
   └─ cursor_preset_*-> 写字 <assetDir>/cursor-presets.json（去重）/ 应用
   ▼
panel_roster { self, selected, cursors:[{name,display,theme,color,…}] }
   → 面板据此显示名字（默认 / 自定义N / 已保存预设名）和被选中光标的值
```

**为什么必须绕**：别的光标是独立进程（内存不共享），面板写不了 assetDir，子进程
stdin 的写端只有宿主持有。见 LESSONS 11 / 3d。

### 7.3 新增协议命令（`handle_line` 里按语义区插入）

| 命令 | 方向 | 作用 |
|---|---|---|
| `panel_roster` | 宿主→面板 | 全部光标的名字 + 每只的实时外观；`self` 是本面板所属光标 |
| `panel_identity` | 宿主→面板 | 只报本面板属于哪只（等价于只推 self） |
| `panel_theme` | 宿主→面板 | 直接设菜单强调色（**只有强调色**，尺寸归窗口） |
| `panel_scroll` | 任意→面板 | 设绝对 `y` 或相对 `delta` 纵向滚动（自动化用） |
| `panel_hscroll` | 任意→面板 | 设绝对 `x` 或相对 `delta` 横向滚动；回 `hscroll/max/view_w` |
| `panel_size` | 任意→面板 | 设窗口 `width` / `height`（`auto:true` 交回车高自动）；回窗口、视口与**内容宽**。真机拖边框走同一份几何 |
| （status 诊断） | 任意→面板 | `panel_content_w` / `panel_vbar` / `panel_vthumb` / `panel_hbar` / `panel_hthumb`：内容宽与两条滚动条的矩形，脚本据此抓滑块而不是猜像素 |
| `hits` | 任意→面板 | **诊断**：返回当前所有控件框（id/kind/x/y/w/h/text），点击脚本靠它定位 |

### 7.4 存储一览

| 数据 | 文件 | 谁能写 |
|---|---|---|
| 菜单主题预设 | `<assetDir>/menu-themes.json` | 宿主 |
| 鼠标预设 | `<assetDir>/cursor-presets.json` | 宿主 |
| 旧预设（兼容） | `<assetDir>/presets.json` | 宿主 |
| 面板意图信箱 | `<exeDir>/logs/dock-command.json` | 浮层可写 |
| 选中的光标 | `<exeDir>/logs/cursor-ui.json` | 宿主 |
| 面板列表缓存 | 内存（每 1.5s 重读上面两个 json） | 浮层 |

### 7.5 窗口几何、流体布局、滚动条与改大小（2026-11-05 流体版）

面板**像普通窗口**：一个窗口、可拖边框改大小、**内容随窗口宽度重排**。

- **一份几何**：`main.rs` 的 `panel_geom(&St, content_h) -> PanelGeom` 是唯一算窗口矩形 /
  内容宽 / 视口 / 两个滚动夹紧 / **两条滚动条矩形**的地方。`view_of()` 把结果写进
  `PanelView`（`panel_w/panel_h/content_w/view_w/bars/scroll/hscroll`），所以绘制、命中、
  滚动条、窗口过程、resize、拖滚动条全读同一份数；`pick()` 的视口就是 `panel_w/panel_h`。
- **流体，不是缩放**（用户推翻了"固定 452 + 右侧留白"）：
  `content_w = max(PW, panel_w)`，`PW=452` 从"设计宽"降级成**内容底线**。
  窗宽 ≥ 452 → 内容列跟着变宽，每行重新分配横向空间（按钮/滑杆/色板/分隔线一起变）；
  窗宽 < 452 → 内容仍是 452，多出来的部分横向滚动。
  **字体、行高、控件高度一个都不许乘系数**——那正是被删掉的「菜单缩放」；回归检查是
  `H6d`：452 宽与 700 宽时标题像素逐点一致。
- `sections.rs` 里所有 `content_right()` 都改成 `content_right(v)`（读 `v.content_w`），
  `Layout { content_w }` 承载宽度；`slot_x_to / slot_w` 负责把一行控件均分到当前内容宽。
- **偏移只有一个落点**：`panel_layout()` 在拼完"分区 + 页脚"之后，把整段内容按
  `-v.hscroll` 平移一次；纵向平移仍由 `sections::build(v, 48 - v.scroll)` 的 `y` 承担。
- **裁剪**：内容区 `IntersectClipRect(0, HEADER_BOTTOM, view_w, h)`，画完
  `SelectClipRgn(NULL)` 恢复。**标题栏必须在设裁剪之前画**（顺序反了 = 控件表里有、画面没有）。
- **滚动条**（`vbar_rects / hbar_rects`，画、命中、拖动共用同一矩形）：
  纵向条贴**窗口**右缘（`x = panel_w - 10`，不再是 452-10）；横向条只在内容宽 > 窗宽时存在。
  两条都可以用真鼠标拖（`ScrollBar` + `scroll_drag`，按下位置记在滑块内的偏移，拖动才算位置）；
  绘制 3px 宽，**抓取区向内容侧多 4px**（`bar_grab_rect`），既不瘦到点不中，也不去抢边框热区。
- **改大小**：窗口是 `WS_POPUP + WS_EX_LAYERED`（系统边框会画出不可透明的非客户区），
  边框热区是自己画的**不透明** 6px 带（顶部 3px）。`WM_LBUTTONDOWN` 顺序：
  **滚动条 → 边框 → 控件**；`ResizeDrag` 用按下瞬间的窗口矩形做原点算增量；`WM_SETCURSOR` 给尺寸光标。
- **横向滚动怎么"滚"**：Shift+滚轮、倾斜滚轮（`WM_MOUSEHWHEEL`）、指针放在底部横条上滚，
  或者直接拖横条；纵向仍是裸滚轮，两者互不抢占。
- 画布尺寸 = 窗口尺寸（窗口一变就重建），`compose` 只能按画布自身宽度取样（LESSONS §4）。

### 7.5b 一个面板，多个光标

- 面板归 `default` 光标进程；宿主 `src/overlay.ts` 用 `new Overlay(exe, { panel:false })`
  给第二只之后的光标加 `--no-panel`，那个进程**不创建** `AiMousePanel`（不是隐藏），
  所以 “`EnumWindows` 只有一个面板”成立。
- 所有面板命令走 `index.ts` 的 `sendPanel()`（永远发给面板 owner），不再发给 `active`。
- 面板里 ‹ › 切换编辑对象、`cursor_patch` 远端改别的光标，协议与之前一致。

### 7.6 想加东西时的新插入点

| 想加的东西 | 插入点 |
|---|---|
| 新的面板分区 | `sections.rs` 的 `SECTIONS`（按显示顺序）+ 一个 `body_xxx` |
| 新的面板控件 | `widget.rs` 的 `Kind` + `sections.rs` 生成 `Hit` + `main.rs` 的 `draw_panel` match |
| 新的"面板要宿主做事"的动作 | 1) `sections.rs` 出按钮 2) `on_toggle` 里 `write_mailbox({action,…})` 3) 宿主 `drainMailbox` 加一个分支 4) `panelstate.ts` 的纯函数 + 单测 |
| 新的按光标保存的数据 | 归到 `cursor-presets.json`（`panelstate.ts` 的 `cursorPresetKey` 决定去重） |
| 新的菜单自身外观 | `St.panel_*` 字段 + `menu-themes.json`，**不要**复用 `st.accent` |
| 改窗口大小/位置/滚动范围 | 只改 `panel_geom()`；另写一份坐标就是回归 |
| 标题栏/边框上的东西 | `panel_layout()` 的 header 段 + `is_header_widget()`（否则会被滚动裁剪吃掉） |

