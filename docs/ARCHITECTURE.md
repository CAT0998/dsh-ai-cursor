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
