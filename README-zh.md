# dsh-ai-cursor · AI 鼠标

[English](README.md) | 中文 | [Español](README-es.md) | [हिन्दी](README-hi.md) | [Português](README-pt.md)

`dsh-ai-cursor` 给 DeepSeek Harness 的 Agent 在 Windows 桌面上开一只属于它自己的第二光标：一块点击穿透的浮层，带 `AI 鼠标` 标签和一只小鲸鱼；外加一个浮动控制面板，人可以看着它干活、改外观、随时暂停，并限制它能做哪些事。它从不移动你的真实鼠标指针，也从不抢键盘焦点。

> **为什么需要：** 会操作桌面的 Agent 和你共用一只指针。你看不出它下一步要碰哪里，而你随手动一下鼠标，正好落在它点击的中间。给 Agent 一只属于它自己的光标，它就被看见了，而不用抢走你的指针。

## 你能得到什么

一个 Rust 进程（`ai-mouse-overlay.exe`）里的两个 Win32 分层窗口，由 TypeScript / Cordis 插件壳通过 stdin/stdout 上的 JSON 行协议驱动。

| | `AiMouseOverlay`——AI 光标 | `AiMousePanel`——控制面板 |
|---|---|---|
| 扩展样式 | `WS_EX_LAYERED \| WS_EX_TRANSPARENT \| WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE \| WS_EX_TOPMOST` | `WS_EX_LAYERED \| WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE \| WS_EX_TOPMOST` |
| 鼠标 | 点击穿透：点击落到下层窗口；从不抢焦点 | 不穿透：自己命中测试控件；点击不会抢焦点（`WS_EX_NOACTIVATE`） |
| 尺寸 | 360×240 px，光标尖画在 (200, 44) | 展开 452×396 px，收成胶囊时 132×40 px |
| 绘制 | GDI，4 倍超采样 → 下采样 → `UpdateLayeredWindow` | GDI，2 倍超采样，同一条 blit 路径 |
| 截图 | 默认排除（`WDA_EXCLUDEFROMCAPTURE`），可切换 | 同上 |

- `ai_mouse_move` 把光标尖精确放在请求的点上：坐标是绝对物理设备像素，进程按显示器做 DPI 感知（v2）。
- 面板在第一次使用浮层时出现（配置项 `panel`），初始在屏幕右侧、垂直居中，可按住顶栏拖动。
- 面板就是完整的控制台：显示 / 隐藏 / 停止 / 启用、`AI 提示` 标签开关、透明度滑条（15 %–100 %）、八个颜色色块加实时 `#RRGGBB` 读数、造型 鲸鱼 / 圆点 / 箭头、大小滑条（60 %–200 %）、权限等级 停止 / 只读 / 操作 / 管理员，以及一行页脚显示可见性、坐标、fps 和当前权限等级。
- 光标在 `idleMs` 内没有移动就自动隐藏（默认 60 秒，`0` = 永不隐藏）。面板不动。
- 每个 DSH 宿主一个浮层进程：第一次工具调用时启动，插件卸载时退出。

### 浮层 ≠ 输入

本插件不合成点击、不移动系统指针，只负责"显示 AI 在看哪"。真实输入仍然走你的 computer-use 工具——优先用它提供的后台投递方式（Cua Driver 的 `delivery_mode: "background"`），两只光标就不会打架。

## 面板布局

展开态面板，452×396。标签就是浮层实际绘制的中文串：

```text
┌────────────────────────────────────────────┐
│ ◉ AI 鼠标                        ─     ✕   │
├────────────────────────────────────────────┤
│ [ 显示 ][ 隐藏 ][ 停止 ][ 启用 ]            │
├────────────────────────────────────────────┤
│ AI 提示            [开][关]     透明度 70%  │
│ ━━━━━━━━━━━●━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│ 颜色  ● ● ● ● ● ● ● ●            #4FC3F7   │
├────────────────────────────────────────────┤
│ 造型   [鲸鱼][圆点][箭头]       大小 100%   │
│ ━━━━━━━●━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
├────────────────────────────────────────────┤
│ 权限等级 [停止][只读][操作][管理员]         │
├────────────────────────────────────────────┤
│ 可见 · (900,520) · fps 38 · 权限 管理员     │
└────────────────────────────────────────────┘
```

这份草图里的数字（fps、坐标、百分比）只是示意用的示例值，不是实测结果。

- `显示` / `隐藏` —— 显示或隐藏 AI 光标。
- `停止` / `启用` —— 停止把权限等级降到 0 并隐藏光标；启用恢复到上一次的等级，至少是 2。
- `AI 提示` —— 光标下方的标签胶囊，用 `开` / `关` 切换。
- `透明度` —— 光标和标签的不透明度，15% 到 100%。
- `颜色` —— 八个色块加当前 `#RRGGBB`；主色同时驱动光标、标签描边和面板高亮。
- `造型` —— `鲸鱼`（默认）、`圆点` 或 `箭头`。
- `大小` —— 光标尺寸，60% 到 200%。
- `权限等级` —— `停止`(0)、`只读`(1)、`操作`(2)、`管理员`(3)。
- 页脚 —— 可见或隐藏、光标坐标、帧率、当前权限等级。

说明：

- 整条顶栏都是拖动条，面板停在你放下的位置。
- 折叠只用鼠标：点顶栏的 `─` 或 `✕`，面板就收成 132×40 的小胶囊（`≡ AI 鼠标`），点胶囊再展开。键盘操作是有意不支持的。等价的工具调用是 `ai_mouse_panel { expanded: false }`，`expanded: true` 重新展开；面板没有隐藏按钮，隐藏靠 `ai_mouse_panel { visible: false }`。
- 绘制和命中测试用同一份布局结果，点到的就是你看到的。
- 面板从不抢焦点，键盘永远留在前台应用里。

## 权限等级

名字就是面板上印的四个：停止（stopped）、只读（read-only）、操作（drive）、管理员（admin）。

| 等级 | 名称 | AI 可以做什么 |
|---|---|---|
| 0 | 停止 | 除了状态和系统级的截图/空闲管道，什么都不行：`ai_mouse_status` 和 `ai_mouse_capture_visible` 仍然可用。 |
| 1 | 只读 | 显示/隐藏光标和面板。 |
| 2 | 操作 | 等级 1，外加 `ai_mouse_move`。 |
| 3 | 管理员 | 等级 2，外加外观（`label`、`theme`、`badge`、`opacity`、`color`、`scale`）和降低等级。 |

- 人在面板上可以设任意等级。AI 只有一根杠杆 `ai_mouse_permission`，而且只能往下：高于当前等级的请求一律拒绝。
- 双保险：TS 层按工具的最低等级先拦一次，Rust 进程再按命令的 `min_perm` 拦一次。agent 侧的缓存过期也升不了权。
- 停止 = 等级 0 + 隐藏光标。启用 = 恢复到 `max(上一次等级, 2)`。
- `ai_mouse_stop` 是单向门：隐藏光标、把等级降到 0，并显示面板，让人去点启用。
- 拒绝文案会说清动作、需要的等级、当前等级，以及"只有人能提高"——agent 读了就没有理由重试。

## 架构

插件壳注册工具；浮层是另一个小小的 Rust 程序，拥有那两个窗口。两边用 stdin/stdout 通信，一行一个 JSON 对象。

```text
DSH host process (Node / Cordis)
  src/index.ts        registers the ai_mouse_* tools, holds the permission gate
        │
        │  JSON line protocol, one object per line, stdin / stdout
        ▼
ai-mouse-overlay.exe (Rust / Win32)
  AiMouseOverlay      layered, click-through, excluded from screen capture
  AiMousePanel        layered, interactive, collapsible
```

- `src/index.ts` —— Cordis 插件：注册十四个工具、给每次调用过门禁、第一次使用时应用配置的外观。
- `src/overlay.ts` —— 管子进程和行协议：请求 id、3 秒回复超时、5 秒启动超时、卸载时发 `quit`。
- `src/config.ts` —— Schemastery 配置；宿主在加载时校验。
- `src/permissions.ts` —— 等级表和纯函数，不依赖窗口就能单测。
- `overlay/src/main.rs` —— Win32：两个分层窗口、GDI 超采样绘制、面板命中测试、拖动、滑条和色块。

完整设计说明见 [`docs/DESIGN.md`](docs/DESIGN.md)。每条回复都回带请求的 `id`，`status` 返回完整状态。

## 环境要求

| 项 | 要求 |
|---|---|
| 操作系统 | Windows 10 2004 及以上。浮层需要 `WDA_EXCLUDEFROMCAPTURE`；更老的版本不能把窗口排除出捕获。 |
| Node.js | `^22.19.0 \|\| >=24.0.0` |
| pnpm | 11 及以上，只在从源码构建时需要 |
| Rust | 带 MSVC target 的稳定工具链，只在从源码构建浮层时需要 |
| Harness | 带 Cordis 插件加载器的 DeepSeek Harness；支持的 `@deepseek-ai/dsh-tools` 范围写在 `package.json` 里 |

**只支持 Windows。** macOS 与 Linux 没有实现：浮层是 Win32 程序，也没有降级方案，在这两个平台上没有东西能画光标。

## 安装

装已发布的包，或在开发时链接本地仓库。把 `desktop` 换成你的 profile 名。

```sh
dsh plugin --profile desktop add dsh-ai-cursor
dsh plugin --profile desktop add link:/absolute/path/to/dsh-ai-cursor
```

装完要重启 Harness——插件在启动时加载，新插件只有重启后才出现。已发布的包自带 `bin/ai-mouse-overlay.exe`；本地仓库要先跑一次 `pnpm run build:overlay`。

## 构建

```sh
pnpm install
pnpm run build:overlay
pnpm run typecheck
pnpm test
pnpm run build
```

- `pnpm install` —— 装 TypeScript 开发依赖（pnpm 11；`esbuild` 的构建脚本已在 `pnpm-workspace.yaml` 里放行）。
- `pnpm run build:overlay` —— 在 `overlay/` 里跑 `cargo build --release`，再把 `ai-mouse-overlay.exe` 拷进 `bin/`。如果 `cargo` 失败但 `bin/` 里已有二进制，就保留原来那个。
- `pnpm run typecheck` —— `tsc --noEmit`。
- `pnpm test` —— `vitest run`。
- `pnpm run build` —— `tsdown` 打包出 `lib/index.js` 和 `lib/index.d.ts`。

或者在 VS Code 里按 `Ctrl+Shift+B`，跑任务 `all (install → overlay → check → build)`，它会把这五步串起来。构建浮层需要 Rust stable 加 MSVC 生成工具（`x86_64-pc-windows-msvc`）；只做插件壳则需要 Node 22+。

## 配置

所有键都在 `src/config.ts` 里用 Schemastery 声明；宿主在加载时校验，非法值直接报错。

| 键 | 默认 | 含义 |
|---|---|---|
| `enabled` | `true` | 是否注册这组工具。 |
| `exePath` | `""` | `ai-mouse-overlay.exe` 的绝对路径。留空则用包内 `bin/`。 |
| `label` | `"AI 鼠标"` | 光标下方的标签文字。 |
| `theme` | `"whale"` | 造型：`whale`、`dot` 或 `arrow`。 |
| `idleMs` | `60000` | 多久没移动就自动隐藏；`0` 永不隐藏。 |
| `captureVisible` | `false` | 是否让光标出现在截图和录屏里。 |
| `panel` | `true` | 第一次使用浮层时是否显示面板。 |
| `panelExpanded` | `true` | 以完整菜单栏打开；`false` 则以胶囊打开。 |
| `opacity` | `1` | 光标和标签的透明度，0.15–1。 |
| `color` | `"#4FC3F7"` | 主色 `#RRGGBB`，驱动光标、标签描边和面板高亮。 |
| `scale` | `1` | 光标尺寸倍数，0.6–2。 |
| `badge` | `true` | 是否显示 `AI 提示` 标签。 |
| `permission` | `3` | 起始上限：0 停止、1 只读、2 操作、3 管理员。 |

`permission` 只是起点，不是锁：它在第一次工具调用时生效，之后就归面板管。agent 只能降，只有人能升。

## 工具

十四个工具。"最低权限"是允许调用所需的最低等级；`0 · any` 表示光标停止时也能用。

| 工具 | 最低权限 | 作用 |
|---|---|---|
| `ai_mouse_show` | 1 · 只读 | 显示光标，第一次会应用配置的外观。 |
| `ai_mouse_hide` | 1 · 只读 | 隐藏光标。 |
| `ai_mouse_move` | 2 · 操作 | 滑到绝对设备像素坐标（`x`、`y`、`duration_ms?`；默认 320，`0` 为瞬移）。会显示光标。 |
| `ai_mouse_panel` | 1 · 只读 | 显示/隐藏、展开/折叠面板（`visible?`、`expanded?`）。 |
| `ai_mouse_status` | 0 · any | 读回可见性、位置、标签、造型、大小、透明度、颜色、徽章、权限、面板状态和 fps。 |
| `ai_mouse_capture_visible` | 0 · any | 是否让光标出现在截图里（`enabled`）；默认关闭。 |
| `ai_mouse_label` | 3 · 管理员 | 设置标签文字（`text`）；空串即隐藏标签。 |
| `ai_mouse_theme` | 3 · 管理员 | 造型：`whale`（默认）、`dot`、`arrow`。 |
| `ai_mouse_badge` | 3 · 管理员 | 显示或隐藏 `AI 提示` 标签（`visible`）。 |
| `ai_mouse_opacity` | 3 · 管理员 | 光标和标签的透明度，0.15–1。 |
| `ai_mouse_color` | 3 · 管理员 | 主色，`#RRGGBB`。 |
| `ai_mouse_scale` | 3 · 管理员 | 光标尺寸，0.6–2。 |
| `ai_mouse_permission` | 3 · 管理员 | 读取或降低权限等级（`level?`，0–3）。提高会被拒绝。 |
| `ai_mouse_stop` | 3 · 管理员 | 隐藏光标、降到等级 0、显示面板。只有人能重新启用。 |

- 不带 `level` 的 `ai_mouse_permission` 只是读取，但这次调用本身需要等级 3；等级更低时用 `ai_mouse_status` 读。
- 每个工具都返回一行文字；被门禁拦下时返回拒绝文案，什么也不做。
- `ai_mouse_capture_visible` 会一直回读状态，直到重建后的窗口报出新值，所以它返回的才是真实状态。
- 典型流程：`ai_mouse_move` 移到目标，用 computer-use 工具点它，然后 `ai_mouse_hide`——或者留着不管，让空闲超时自动隐藏。

## 行协议

stdin 上一行一个 JSON 对象；stdout 上一行一条回复，带同一个 `id`。`system` 命令是插件壳自己发的管道命令，工具里碰不到。

| 命令 | 所需权限 | 作用 |
|---|---|---|
| `{"cmd":"show"}` / `{"cmd":"hide"}` | ≥ 1 | 显示或隐藏光标。 |
| `{"cmd":"move","x":900,"y":520,"duration_ms":320}` | ≥ 2 | 滑到绝对屏幕坐标。 |
| `{"cmd":"panel","visible":true}` | ≥ 1 | 显示或隐藏面板。 |
| `{"cmd":"panel_expanded","expanded":true}` | ≥ 1 | 完整菜单栏，或小胶囊。 |
| `{"cmd":"opacity","value":0.7}` | ≥ 3 | 光标和标签的透明度，0.15–1。 |
| `{"cmd":"color","hex":"#4FC3F7"}` | ≥ 3 | 主色。 |
| `{"cmd":"scale","value":1.2}` | ≥ 3 | 光标尺寸，0.6–2。 |
| `{"cmd":"badge","visible":true}` | ≥ 3 | `AI 提示` 开或关。 |
| `{"cmd":"theme","theme":"whale"}` | ≥ 3 | `whale`、`dot` 或 `arrow`。 |
| `{"cmd":"label","text":"AI 鼠标"}` | ≥ 3 | 标签文字。 |
| `{"cmd":"permission","level":2}` | ≥ 3，只能降 | 权限等级。 |
| `{"cmd":"pause"}` / `{"cmd":"resume"}` | ≥ 3 | 等价于 停止 / 启用。 |
| `{"cmd":"idle","idle_ms":60000}` | system | 空闲多久自动隐藏；`0` 永不。 |
| `{"cmd":"affinity","capture_visible":false}` | system | 是否进截图；会重建两个窗口。 |
| `{"cmd":"dump","path":"frame.bmp","target":"cursor"}` | system | 把一帧画进 BMP（自测用，不涉及截屏）。 |
| `{"cmd":"status"}` | any | 回读完整状态。 |
| `{"cmd":"quit"}` | system | 退出浮层进程。 |

```json
{"id":"1","cmd":"move","x":900,"y":520,"duration_ms":320}
{"id":"2","cmd":"affinity","capture_visible":true}
{"id":"3","cmd":"theme","theme":"whale"}
{"id":"4","cmd":"dump","path":"frame.bmp","target":"cursor"}
{"cmd":"quit"}
```

每条回复都是 `{"id":"<同一个 id>","ok":true,…}`。被拒绝的命令回 `ok:false` 和一条 `error`，写明它需要哪个等级。`status` 额外返回 `visible`、`x`、`y`、`label`、`theme`、`badge`、`opacity`、`scale`、`color`、`permission`、`panel`、`panel_expanded`、`hwnd`、`panel_hwnd`、`fps`、`capture_visible` 和 `excluded_from_capture`。

## 故障排查

### 光标看不见

- 先看权限：`ai_mouse_status` 会打出 `permission=`。等级 0（停止）时光标是有意隐藏的，得让人在面板上点启用或更高等级。
- 光标在 `idleMs` 内没有移动就自动隐藏（默认 60 秒）。再调一次 `ai_mouse_show` 或 `ai_mouse_move`，或者把 `idleMs` 设成 `0`。
- 如果 `ai_mouse_status` 返回的错误里提到可执行文件，看下一条。
- 隐藏面板不会隐藏光标，隐藏光标也不会隐藏面板——两者互相独立。

### 找不到 `ai-mouse-overlay.exe`

- 先构建一次：`pnpm run build:overlay` 会写出 `bin/ai-mouse-overlay.exe`。
- 不设 `exePath` 时，解析顺序是 `<包目录>/bin/ai-mouse-overlay.exe`，然后是开发构建的 `overlay/target/release/ai-mouse-overlay.exe`。
- `exePath` 必须是绝对路径；相对路径会被拒绝。
- 错误信息里带着它试过的绝对路径——先读它，再猜。

### 录制模式没生效

- Windows 对同一个窗口只允许设置一次 `SetWindowDisplayAffinity`：再切回 `WDA_NONE` 会报 `ERROR_NOT_ENOUGH_MEMORY (0x80070008)`。所以切换录制模式时浮层会重建两个窗口，而且必须在拥有消息泵的线程上重建（换线程创建会让 `ShowWindow` 死锁）。
- `ai_mouse_capture_visible` 会轮询状态，直到浮层报出请求的值；如果回复里还是旧值，说明窗口还没重建完。
- 更老的 Windows 版本没有 `WDA_EXCLUDEFROMCAPTURE`，那里的排除是静默失效的。

### 权限被拒绝

- 这是故意的：工具需要某个等级，浮层当前更低，而只有人能提高。
- 不要换参数重试。工具这一层有门禁，进程里面还会再拦一次。
- 请用户去面板上点「管理员」——如果等级是 0，就点「启用」。
- 如果人刚把等级调上去，重试一次是有效的：缓存的等级拒绝调用时，插件会重新读一次。

### 面板不见了

- `ai_mouse_panel { visible: true }` 把它叫回来；加 `expanded: true` 就是完整菜单栏。
- 收起后的胶囊是 132×40，停在面板最后的位置；两个窗口都是 topmost，所以它压在所有普通窗口之上。
- `panel: false` 只表示第一次使用时不显示；工具调用照样能把它叫回来。

### `ai_mouse_*` 工具不存在

- `enabled: false` 就什么都不注册。
- 插件在启动时加载：装完或更新完要重启 Harness。
- 确认你装进去的 profile 就是你正在跑的 profile。

## 参与贡献

欢迎 issue、翻译和补丁。提 PR 之前请跑 `pnpm run typecheck` 和 `pnpm test`，并保持权限不变量：agent 可以降低自己的等级，只有人能提高。

`overlay/src/main.rs` 里的中文串和 `src/permissions.ts` 里的等级名是产品术语——显示 隐藏 停止 启用 只读 操作 管理员 AI 提示 造型 大小 透明度 颜色 权限等级——请不要改名。五个 README 保持同步：同一节要一起改。

## 许可证

Apache License 2.0 —— 见 [LICENSE](LICENSE)。© 2026 dsh-ai-cursor contributors。
