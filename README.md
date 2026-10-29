# dsh-ai-cursor · AI 鼠标

English | [中文](README-zh.md) | [Español](README-es.md) | [हिन्दी](README-hi.md) | [Português](README-pt.md)

`dsh-ai-cursor` gives a DeepSeek Harness agent its own second cursor on the Windows desktop: a click-through overlay with an `AI 鼠标` badge and a small whale, plus a floating control panel where a human can watch it, change how it looks, pause it and cap what it is allowed to do. It never moves your real mouse pointer and never takes keyboard focus.

> **Why:** an agent that drives your desktop shares one pointer with you. You cannot see what it is about to touch, and your own mouse movement lands in the middle of its click. A second, agent-owned cursor makes the agent visible without hijacking your pointer.

## What you get

Two Win32 layered windows in one Rust process (`ai-mouse-overlay.exe`), driven by a TypeScript/Cordis plugin shell over a JSON line protocol on stdin/stdout.

| | `AiMouseOverlay` — the AI cursor | `AiMousePanel` — the control panel |
|---|---|---|
| Extended styles | `WS_EX_LAYERED \| WS_EX_TRANSPARENT \| WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE \| WS_EX_TOPMOST` | `WS_EX_LAYERED \| WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE \| WS_EX_TOPMOST` |
| Mouse | Click-through: clicks reach the window underneath; never takes focus | Not click-through: hit-tests its own widgets; clicking it never steals focus (`WS_EX_NOACTIVATE`) |
| Size | 360×240 px, cursor tip drawn at (200, 44) | 452×396 px expanded, 132×40 px as the chip |
| Painting | GDI, 4× supersampled, downsampled, blitted with `UpdateLayeredWindow` | GDI, 2× supersampled, same blit path |
| Screen capture | Excluded by default (`WDA_EXCLUDEFROMCAPTURE`), switchable | Same |

- `ai_mouse_move` puts the tip exactly on the requested point: coordinates are absolute physical device pixels, and the process is per-monitor DPI aware (v2).
- The panel appears the first time the overlay is used (config `panel`), opens near the right edge of the screen, vertically centred, and is draggable by its header.
- The panel holds the whole control surface: 显示 / 隐藏 / 停止 / 启用, the `AI 提示` badge switch, an opacity slider (15 %–100 %), eight colour swatches with a live `#RRGGBB` readout, the artwork 鲸鱼 / 圆点 / 箭头, a size slider (60 %–200 %), the permission ladder 停止 / 只读 / 操作 / 管理员, and a footer with visibility, coordinates, fps and the current level.
- The cursor auto-hides after `idleMs` without a move (default 60 s, `0` = never). The panel stays where it is.
- One overlay process per DSH host: it starts on the first tool call and is shut down when the plugin unloads.

### Overlay ≠ input

This plugin never synthesises clicks and never moves the OS pointer; it only shows where the agent is looking. Real input still goes through your computer-use tool — prefer a background delivery mode where the tool has one (Cua Driver's `delivery_mode: "background"`), and the two cursors stop fighting.

## Panel layout

Expanded panel, 452×396. The labels are the Chinese strings the overlay actually paints:

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

The numbers in this sketch (fps, coordinates, percentages) are illustrative sample values, not measurements.

- `显示` / `隐藏` — show or hide the AI cursor.
- `停止` / `启用` — 停止 drops the permission level to 0 and hides the cursor; 启用 restores the previous level, at least 2.
- `AI 提示` — the badge under the cursor, switched with `开` / `关`.
- `透明度` — cursor and badge opacity, 15 % to 100 %.
- `颜色` — eight swatches plus the current `#RRGGBB`; the accent drives the cursor, the badge rim and the panel highlights.
- `造型` — `鲸鱼` (whale, the default), `圆点` (dot) or `箭头` (arrow).
- `大小` — cursor size, 60 % to 200 %.
- `权限等级` — `停止` (0), `只读` (1), `操作` (2), `管理员` (3).
- Footer — visible or hidden, cursor coordinates, frames per second, current permission level.

Notes:

- The whole header bar is the drag handle, and the panel keeps the position you drop it at.
- Collapsing is mouse-only: click `─` or `✕` in the header and the panel shrinks to the 132×40 chip (`≡ AI 鼠标`); click the chip to expand it again. Keyboard activation is intentionally not supported. The tool equivalent is `ai_mouse_panel { expanded: false }`, and `expanded: true` reopens it; there is no hide button — hiding is `ai_mouse_panel { visible: false }`.
- One layout function produces both the drawn rectangles and the hit list, so a click lands on what you see.
- The panel never takes focus: your keyboard stays in the foreground application.

## Permission ladder

The names are the ones printed on the panel: 停止 (stopped), 只读 (read-only), 操作 (drive), 管理员 (admin).

| Level | Name | What the agent may do |
|---|---|---|
| 0 | 停止 | Nothing except status and the system-level capture/idle plumbing: `ai_mouse_status` and `ai_mouse_capture_visible` still work. |
| 1 | 只读 | Show or hide the cursor and the panel. |
| 2 | 操作 | Level 1 plus `ai_mouse_move`. |
| 3 | 管理员 | Level 2 plus appearance (`label`, `theme`, `badge`, `opacity`, `color`, `scale`) and lowering the level. |

- A human can set any level in the panel. The agent has exactly one lever, `ai_mouse_permission`, and it only goes down: a level above the current one is refused.
- Two independent gates: the TypeScript layer checks each tool's minimum level, and the Rust process re-checks `min_perm` for every command. A stale cache on the agent side cannot escalate anything.
- 停止 means level 0 plus a hidden cursor. 启用 restores `max(previous level, 2)`.
- `ai_mouse_stop` is a one-way door: it hides the cursor, drops the level to 0 and shows the panel so the human can click 启用.
- A refusal names the action, the level it needs, the current level, and says that only a human can raise it — an agent that reads it has no reason to retry.

## Architecture

The plugin shell registers the tools; the overlay is a separate small Rust program that owns the two windows. They talk over stdin/stdout, one JSON object per line.

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

- `src/index.ts` — Cordis plugin: registers the fourteen tools, gates every call, applies the configured look on first use.
- `src/overlay.ts` — owns the child process and the line protocol: request ids, a 3 s reply timeout, a 5 s startup timeout, `quit` on dispose.
- `src/config.ts` — the Schemastery schema; the harness validates it at load time.
- `src/permissions.ts` — the level table and the pure helpers, unit-tested without a window.
- `overlay/src/main.rs` — Win32: two layered windows, GDI supersampled painting, panel hit-testing, dragging, sliders and swatches.

The full design notes (in Chinese) are in [`docs/DESIGN.md`](docs/DESIGN.md). Every reply echoes the request `id`, and `status` returns the complete state.

## Requirements

| Item | Requirement |
|---|---|
| OS | Windows 10 version 2004 or newer. The overlay needs `WDA_EXCLUDEFROMCAPTURE`; older builds cannot exclude a window from capture. |
| Node.js | `^22.19.0 \|\| >=24.0.0` |
| pnpm | 11 or newer, only to build from source |
| Rust | A stable toolchain with the MSVC target, only to build the overlay from source |
| Harness | A DeepSeek Harness build with the Cordis plugin loader; the supported `@deepseek-ai/dsh-tools` range is declared in `package.json` |

**Windows only.** macOS and Linux are not implemented: the overlay is a Win32 program with no fallback, so on those platforms there is nothing to draw a cursor with.

## Install

Install the published package, or link a local checkout while developing. Replace `desktop` with your profile name.

```sh
dsh plugin --profile desktop add dsh-ai-cursor
dsh plugin --profile desktop add link:/absolute/path/to/dsh-ai-cursor
```

Restart the harness afterwards — plugins are loaded at boot, so a new one only appears after a restart. Published packages ship `bin/ai-mouse-overlay.exe`; a local checkout has to build it once with `pnpm run build:overlay`.

## Build

```sh
pnpm install
pnpm run build:overlay
pnpm run typecheck
pnpm test
pnpm run build
```

- `pnpm install` — TypeScript dev dependencies (pnpm 11; the `esbuild` build script is allowed in `pnpm-workspace.yaml`).
- `pnpm run build:overlay` — `cargo build --release` in `overlay/`, then copies `ai-mouse-overlay.exe` into `bin/`. If `cargo` fails and a binary already exists in `bin/`, that binary is kept.
- `pnpm run typecheck` — `tsc --noEmit`.
- `pnpm test` — `vitest run`.
- `pnpm run build` — `tsdown` bundles `lib/index.js` and `lib/index.d.ts`.

Or press `Ctrl+Shift+B` in VS Code and run the task `all (install → overlay → check → build)`, which chains the same five steps. Building the overlay needs Rust stable with the MSVC build tools (`x86_64-pc-windows-msvc`); the plugin shell alone needs Node 22+.

## Configuration

All keys are Schemastery-declared in `src/config.ts`; the harness validates them at load time and rejects invalid values.

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `true` | Register the tool family. |
| `exePath` | `""` | Absolute path to `ai-mouse-overlay.exe`. Empty resolves the bundled `bin/`. |
| `label` | `"AI 鼠标"` | Badge text under the cursor. |
| `theme` | `"whale"` | Artwork: `whale`, `dot` or `arrow`. |
| `idleMs` | `60000` | Hide the cursor after this many ms without a move; `0` never hides. |
| `captureVisible` | `false` | Let the cursor appear in screenshots and recordings. |
| `panel` | `true` | Show the floating panel the first time the overlay is used. |
| `panelExpanded` | `true` | Open as the full menu bar; `false` starts as the chip. |
| `opacity` | `1` | Cursor and badge opacity, 0.15–1. |
| `color` | `"#4FC3F7"` | Accent `#RRGGBB` for the cursor, the badge rim and the panel highlights. |
| `scale` | `1` | Cursor size multiplier, 0.6–2. |
| `badge` | `true` | Show the `AI 提示` badge. |
| `permission` | `3` | Starting ceiling: 0 停止, 1 只读, 2 操作, 3 管理员. |

`permission` is a starting point, not a lock: it is applied on the first tool call, and from then on the panel owns it. An agent can lower it; only a human can raise it.

## Tools

Fourteen tools. "Min permission" is the lowest level at which the call is allowed; `0 · any` also works while the cursor is stopped.

| Tool | Min permission | What it does |
|---|---|---|
| `ai_mouse_show` | 1 · 只读 | Show the cursor, applying the configured look the first time. |
| `ai_mouse_hide` | 1 · 只读 | Hide the cursor. |
| `ai_mouse_move` | 2 · 操作 | Glide to absolute device-pixel coordinates (`x`, `y`, `duration_ms?`; default 320, `0` jumps). Shows the cursor. |
| `ai_mouse_panel` | 1 · 只读 | Show or hide, expand or collapse the panel (`visible?`, `expanded?`). |
| `ai_mouse_status` | 0 · any | Read visibility, position, label, theme, size, opacity, colour, badge, permission, panel state and fps. |
| `ai_mouse_capture_visible` | 0 · any | Let the cursor appear in captures (`enabled`); off by default. |
| `ai_mouse_label` | 3 · 管理员 | Set the badge text (`text`); an empty string hides the badge. |
| `ai_mouse_theme` | 3 · 管理员 | Artwork: `whale` (default), `dot`, `arrow`. |
| `ai_mouse_badge` | 3 · 管理员 | Show or hide the `AI 提示` badge (`visible`). |
| `ai_mouse_opacity` | 3 · 管理员 | Cursor and badge opacity, 0.15–1. |
| `ai_mouse_color` | 3 · 管理员 | Accent colour as `#RRGGBB`. |
| `ai_mouse_scale` | 3 · 管理员 | Cursor size, 0.6–2. |
| `ai_mouse_permission` | 3 · 管理员 | Read or lower the level (`level?`, 0–3). Raising is refused. |
| `ai_mouse_stop` | 3 · 管理员 | Hide the cursor, drop to level 0, show the panel. Only a human can start it again. |

- `ai_mouse_permission` without `level` only reads, but the call itself needs level 3; from below, read the level with `ai_mouse_status`.
- Every tool returns one line of text; a gated call returns the refusal sentence instead of doing anything.
- `ai_mouse_capture_visible` re-reads the status until the recreated windows report the new value, so its reply reflects the real state.
- Typical flow: `ai_mouse_move` to the target, click it through the computer-use tool, then `ai_mouse_hide` — or leave the cursor visible and let the idle timeout hide it.

## Line protocol

One JSON object per line on stdin; one reply per line on stdout carrying the same `id`. `system` commands are plumbing that the plugin shell sends itself and are not reachable through a tool.

| Command | Required permission | Effect |
|---|---|---|
| `{"cmd":"show"}` / `{"cmd":"hide"}` | ≥ 1 | Show or hide the cursor. |
| `{"cmd":"move","x":900,"y":520,"duration_ms":320}` | ≥ 2 | Glide to absolute screen coordinates. |
| `{"cmd":"panel","visible":true}` | ≥ 1 | Show or hide the panel. |
| `{"cmd":"panel_expanded","expanded":true}` | ≥ 1 | Full menu bar, or the chip. |
| `{"cmd":"opacity","value":0.7}` | ≥ 3 | Cursor and badge opacity, 0.15–1. |
| `{"cmd":"color","hex":"#4FC3F7"}` | ≥ 3 | Accent colour. |
| `{"cmd":"scale","value":1.2}` | ≥ 3 | Cursor size, 0.6–2. |
| `{"cmd":"badge","visible":true}` | ≥ 3 | `AI 提示` on or off. |
| `{"cmd":"theme","theme":"whale"}` | ≥ 3 | `whale`, `dot` or `arrow`. |
| `{"cmd":"label","text":"AI 鼠标"}` | ≥ 3 | Badge text. |
| `{"cmd":"permission","level":2}` | ≥ 3, lower only | Permission level. |
| `{"cmd":"pause"}` / `{"cmd":"resume"}` | ≥ 3 | Same as 停止 / 启用. |
| `{"cmd":"idle","idle_ms":60000}` | system | Auto-hide after this much idle time; `0` never. |
| `{"cmd":"affinity","capture_visible":false}` | system | Capture exclusion; recreates both windows. |
| `{"cmd":"dump","path":"frame.bmp","target":"cursor"}` | system | Paint one frame to a BMP (self-test, no screen capture involved). |
| `{"cmd":"status"}` | any | Read back the full state. |
| `{"cmd":"quit"}` | system | Exit the overlay process. |

```json
{"id":"1","cmd":"move","x":900,"y":520,"duration_ms":320}
{"id":"2","cmd":"affinity","capture_visible":true}
{"id":"3","cmd":"theme","theme":"whale"}
{"id":"4","cmd":"dump","path":"frame.bmp","target":"cursor"}
{"cmd":"quit"}
```

Every reply is `{"id":"<same id>","ok":true,…}`. A refused command replies with `ok:false` and an `error` naming the level it needed. `status` adds `visible`, `x`, `y`, `label`, `theme`, `badge`, `opacity`, `scale`, `color`, `permission`, `panel`, `panel_expanded`, `hwnd`, `panel_hwnd`, `fps`, `capture_visible` and `excluded_from_capture`.

## Troubleshooting

### The cursor is invisible

- Check the level: `ai_mouse_status` prints `permission=`. At level 0 (停止) the cursor is hidden on purpose; a human has to click 启用 or a higher level in the panel.
- The cursor auto-hides after `idleMs` without a move (default 60 s). Call `ai_mouse_show` or `ai_mouse_move` again, or set `idleMs: 0`.
- If `ai_mouse_status` returns an error mentioning the executable, see the next item.
- Hiding the panel does not hide the cursor, and hiding the cursor does not hide the panel — the two are independent.

### `ai-mouse-overlay.exe` not found

- Build it once: `pnpm run build:overlay` writes `bin/ai-mouse-overlay.exe`.
- Without `exePath`, the resolver tries `<package>/bin/ai-mouse-overlay.exe`, then the dev build at `overlay/target/release/ai-mouse-overlay.exe`.
- `exePath` must be absolute; a relative path is rejected.
- The error message contains the resolved absolute path it tried — read it before guessing.

### Capture mode did not take effect

- Windows allows `SetWindowDisplayAffinity` once per window: switching back to `WDA_NONE` fails with `ERROR_NOT_ENOUGH_MEMORY (0x80070008)`. The overlay therefore recreates both windows whenever capture mode changes, and that has to happen on the thread that owns the message pump (creating them elsewhere deadlocks `ShowWindow`).
- `ai_mouse_capture_visible` polls the status until the overlay reports the requested value; if the reply still shows the old value, the windows are not recreated yet.
- Older Windows builds have no `WDA_EXCLUDEFROMCAPTURE`, so the exclusion silently does nothing there.

### Permission refused

- The refusal is deliberate: the tool needs one level, the overlay sits at a lower one, and only a human can raise it.
- Do not retry with other arguments. The tool is gated, and the command is gated again inside the process.
- Ask the user to click 管理员 in the panel — or 启用, if the level is 0.
- If a human has just raised the level, one retry does work: the plugin re-reads the level when its cached value refuses a call.

### The panel is missing

- `ai_mouse_panel { visible: true }` shows it; add `expanded: true` for the full menu bar.
- The collapsed chip is 132×40 and stays where the panel last was; both windows are topmost, so it sits above normal windows.
- With `panel: false` the panel is simply not shown on first use; a tool call still brings it back.

### The `ai_mouse_*` tools do not exist

- `enabled: false` registers nothing.
- Plugins are loaded at boot: restart the harness after installing or updating.
- Make sure the profile you installed into is the profile you are running.

## Contributing

Issues, translations and patches are welcome. Before opening a pull request, run `pnpm run typecheck` and `pnpm test`, and keep the permission invariant intact: an agent may lower its own level, only a human raises it.

The Chinese strings in `overlay/src/main.rs` and the level names in `src/permissions.ts` are product terms — 显示 隐藏 停止 启用 只读 操作 管理员 AI 提示 造型 大小 透明度 颜色 权限等级 — please do not rename them. The five READMEs are kept in sync: change the same section in all of them.

## Licence

Apache License 2.0 — see [LICENSE](LICENSE). © 2026 dsh-ai-cursor contributors.
