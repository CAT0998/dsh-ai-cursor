# 路线图

已完成与待办的分期。每期都要：编译 → 协议自测 → 真实鼠标点一遍 → 截图，通过了才进下一期。

## 已完成

| 期 | 内容 | 验证 |
|---|---|---|
| P0 | **兼容拦截层**：`off / refuse / redirect / mirror` 四模式拦其他插件的真实鼠标工具 | 390 次采样证明 redirect 下真实指针不动；mirror/refuse 各自语义正确 |
| P1a | **图案管线**：三槽位（光标 / 提示 / 附属）、PNG·JPG·GIF(动图)·WebP·BMP·ICO·TIFF·SVG、真 alpha、热重载 | 9/9 浮层自测；GIF 6 帧在真实桌面循环 |
| P1b(框架) | **面板分区框架**：`widget.rs` + `sections.rs` + 动态高度 + 折叠 | 553→607 实测伸缩 |
| P1b(部分) | 图案分区：三槽位、缩略图（动图会动）、`‹ ›` 循环素材、**选中槽位后的大小 / 横向 / 纵向 / 透明度 / 翻转 / 重置** | 见下 |
| P2 | **颜色**：8 色板 + 256 级 RGB 三通道条 + **色环（色相环 + 明度/饱和度方块）** | 点外环 `#4FC3F7→#F74F4F`；点内块 `→#2C2222` |
| P5(部分) | **自定义模块**：`custom-section.json` 驱动，标题可改，按钮/标签，解析错误定位到行 | 三个示例按钮逐个点过并回读状态 |

## 待办（按顺序）

### 用户可拖拽 AI 光标（✅ 已实测通过）
- 真实指针按住光标附近（46px 内）→ 进入拖拽，跟随指针移动
- 松手 → `permission = 0`（AI 自动停止），并在 `status.drag_feedback` 留下说明
- 面板「操作」分区新增 `拖拽 可用/禁用` 开关（`drag_toggle`）
- **最终解法（第五轮，成功）**：不再轮询、不依赖全局钩子——放一个 **40×40 的抓取热区窗口**
  （类名 `AiMouseGrab`，不穿透）在光标尖端；拖拽模式开启时它跟随光标显示，
  按下去就是一条普通的 `WM_LBUTTONDOWN`。实测：按下瞬间 `permission` 3→0（先停），
  拖动把光标从 (900,520) 拉到 (1100,584)，松手落点精确。
  代价：拖拽模式开启时，这个 40×40 区域内的点击会落到热区而不是下层程序。
- **走过的弯路（保留记录）**：
- **诊断结果（已定位到根因方向）**：加了 `drag_raw / drag_seen_down / drag_near` 三个
  诊断字段后，用独立浮层进程实测合成拖拽：
  `held  raw=0 seen=False near=True dragging=False`
  → **几何命中是对的（`near=True`）**，但 `GetAsyncKeyState(VK_LBUTTON)` 在按键确实
  被按住期间返回 **0**，所以永远进不了拖拽。
- **低级鼠标钩子也试了（2026-10-29 第三轮）**：装了 `SetWindowsHookExW(WH_MOUSE_LL)`，
  实测 `hook_events=0`、`hook_down=False` —— **钩子一次都没被调用**。最可能是
  `SetWindowsHookExW` 安装就失败了，而我用 `.ok()` 把错误吞掉了（和之前丢
  `SetWindowPos` 返回值是同一个毛病）。
  **已把错误码记下来（第四轮）**：`hook_error=0`、`hook_handle=783422651` → **钩子安装成功**，
  但 `hook_events` 仍是 **0**，说明装上了却**一次都没被系统回调**。
  所以不是安装失败，是派发没到我们线程。下一步排查顺序：
  1. 主循环用的是 `PeekMessage` + `sleep(8)`；低级钩子要求安装线程持续泵消息，
     可能需要在每次泵之前/之后调用一次 `MsgWaitForMultipleObjectsEx` 或改用 `GetMessage`；
  2. 本机 DSH 与测试进程都是 **管理员权限（提权）**，高完整性进程的 LL 钩子在某些配置下
     收不到低完整性进程的输入；可尝试用普通权限进程复测；
  3. 确认钩子线程就是创建窗口的那个线程（目前看是）。
- 备选结论：`GetAsyncKeyState` 看不到这次合成按键。下一步二选一：
  1. 改用 **`SetWindowsHookEx(WH_MOUSE_LL)`** 低级鼠标钩子——它能收到注入输入，是最稳的做法；
  2. 或者先让真人在光标上按住一次，确认 `drag_raw` 会不会变成非 0（能变说明只是合成输入
     不被 `GetAsyncKeyState` 认，真人可用）。

### P1b 收尾
- 图案槽位的**文件选择框**（Win32 打开对话框）。目前可以：素材目录按文件名引用、绝对路径、`‹ ›` 循环、工具 `ai_mouse_sprite`。

### P3 层级锚点 —— ⚠️ 协议已实现，**z 序未生效，待修**
让 AI 鼠标在**各应用窗口之间自由切换**，且**绝不打扰其他窗口**（不改 z 序、不移动、不缩放、不抢焦点）。
- 锚点模式：`topmost` / `above:<窗口>` / `below:<窗口>` / `desktop`
- 面板显示当前锚点窗口的标题、类名、进程、z 序号
- `SetWinEventHook` 跟随锚点窗口移动/最小化
- **只改自己窗口的 z 序**，对其他窗口只读不写

**2026-10-29 实测状态（第二轮，仍失败）**：又加了两步仍无效：
1. 每次锚定后回读 `GWL_EXSTYLE` 写进 `status.layer_topmost`；
2. 低频率看门狗（400ms）重复锚定，防重绘把窗口顶回去；
3. 先显式 `SetWindowPos(HWND_NOTOPMOST)` 降级，再插入到锚点位置。
结果：`below:0x2C1660` 之后 `EnumWindows` 里我们的窗口仍是 index 2、锚点 index 3，
**依旧在锚点之前**。下一步要看 `status.layer_topmost` 的真值，以及 `SetWindowPos`
的返回值/GetLastError——现在的代码把返回值丢掉了（`let _ =`），这是最该改的地方。

**第一轮记录**：`ai_mouse_layer` 能收下命令、能用 `GetClassNameW` 解析锚点、
`ai_mouse_windows_at` 能正确列出窗口栈（修复了跨进程读标题导致浮层挂死的问题）。
但设成 `below:<窗口>` 之后，`WS_EX_TOPMOST` 仍然留在我们的两个窗口上，枚举出来的
z 序里光标依旧排在锚点**之前**——也就是说 **`apply_layer` 的效果没有落地**。

下一步排查方向（按可能性排序）：
1. `SetWindowPos(hwnd, anchor, …)` 是否真的被执行（在 apply 处加一行回读
   `GetWindowLongW(GWL_EXSTYLE)` 并写进 `status`，直接看 topmost 位有没有被清掉）；
2. 是否有别的路径在之后又把窗口推回 topmost（`make_window` 创建时设了
   `HWND_TOPMOST`；`p_resize`/`p_move` 也传了 `HWND_TOPMOST`）；
3. 确认 `want_layer` 是否被主循环及时取走（怀疑 `p_layer` 的 drain 位置在
   `want_layer` 被设置之前的那一轮就已经跑过）。

### P4 两个小地图
- 真实缩略图（屏幕实拍缩小）＋ 层级示意图（屏幕/窗口矩形 + 光点）
- 都可缩放、调半透明度、隐藏

### P6 多智能体：每个子代理一只自己的 AI 鼠标
> 新需求（2026-10-29）：让 AI 的**子代理也能使用这个插件**——每个代理智能体都有自己的 AI 鼠标，各自用不同的配置，并且可以**自定义预设**。

设计要点（待细化）：
- 现在是一进程一只光标、一套状态。要变成**多光标**：一个浮层进程管理 N 个光标窗口，每个带 `agentId`。
- 工具按调用者的 agent 身份路由：`ai_mouse_*` 作用于**调用它的那个 agent 的光标**，而不是全局那只。
- 每只光标一套配置（造型 / 颜色 / 尺寸 / 透明度 / 徽章 / 层级 / 图案槽位），来源优先级：**预设 < 按 agent 覆盖 < 运行时工具调用**。
- **预设（preset）**：命名的一组配置，可保存/加载/复制；新子代理按预设自动分配外观（例如按 agent 名 hash 出颜色，避免两只光标撞色）。
- 面板需要从"这只光标"变成"光标列表 + 当前选中"，或按 agent 分页。
- 权限等级要**按 agent 独立**，不能一个子代理拉低所有光标。

## 已知环境限制（不是代码问题）

- 本机 `github.com:443` 不通，`git push` 用不了；提交走 `python scripts/push-via-api.py`（REST Git Data API）。
- `.github/workflows/ci.yml` 未推上去：token 缺 `workflow` scope（GitHub 对这种写入故意回 404）。
- 插件市场 PR 受「仓库创建满 1 天」自动检查限制，条目已备在 fork 分支 `add-dsh-ai-cursor`。
