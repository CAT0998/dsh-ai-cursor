# 上架 DSH 插件市场

本插件通过 [awesome-dsh-plugin](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin)
收录，它同时是 `dshmarket`（DSH 内可视化市场）的索引源：
索引数据在 `data/plugins/<owner>__<repo>.yml`，两个 README 由脚本生成，**不要手工改**。

## 一、收录要求（官方 contributing）

| 要求 | 本仓库 |
|---|---|
| `package.json` 声明 `dsh.bundle`（可被 `dsh plugin add` 安装） | ✅ `dsh.bundle.patch → ./cordis.patch.yml` |
| 仓库有真实可用代码（非占位） | ✅ 1400+ 行 Rust + TypeScript |
| 仓库创建满 **1 天**（CI 自动检查） | ⏳ 2026-10-29 创建 |
| 仓库加 `dsh-plugin` topic | ✅ 已加 |
| 描述只说功能、不带营销词、且必须与代码一致 | ✅ |
| 官方包用 `peerDependencies` 而非 `dependencies` | ✅ |

预构建安装（推荐）：发 npm 包，或把 `pnpm pack` 的 `.tgz` 挂到 GitHub Release，
在条目里用 `tarball:` 指过去——免掉用户的 `allowBuilds` 构建授权。

## 二、提交的条目文件

路径：`data/plugins/CAT0998__dsh-ai-cursor.yml`

```yaml
url: https://github.com/CAT0998/dsh-ai-cursor
name: CAT0998/dsh-ai-cursor
category: tools
description:
  en: A second, agent-owned cursor with a floating control panel for opacity, colour, size, badge and a permission ladder only a human can raise. It never moves the real pointer.
  zh: 给 AI 的第二只光标加一个浮动控制面板，可调透明度、颜色、大小与提示显隐，并按只能由人提高的权限等级限制它；从不移动你的真实鼠标指针。
tarball: https://github.com/CAT0998/dsh-ai-cursor/releases/latest/download/dsh-ai-cursor.tgz
```

- `category` 取值见官方列表：`agi ui usage theme model identity session memory tools wsl browser vision voice docs skill workflow git notify dev security remote market fun`。
  本插件核心是桌面光标/面板（视觉浮层）+ Agent 工具，取 `tools`。
- `tarball` 资产名**不带版本号**，否则下一次发版就会 404（官方明确提醒过这个坑）。
- 描述以 `。` / `.` 结尾；含 `: ` 时必须给 YAML 加引号（本条目没有）。

## 三、提交步骤（可复现）

```sh
# 1. fork 并克隆
gh repo fork awesome-dsh-plugin/awesome-dsh-plugin --clone
cd awesome-dsh-plugin
git checkout -b add-dsh-ai-cursor

# 2. 只加一个文件
mkdir -p data/plugins
#    把上面第二节的 YAML 存成 data/plugins/CAT0998__dsh-ai-cursor.yml

# 3. 可选：本地预览生成出来的那一行
npm ci && node scripts/generate-readme.mjs

# 4. 提交（只提交自己的条目文件）
git add data/plugins/CAT0998__dsh-ai-cursor.yml
git commit -m "Add CAT0998/dsh-ai-cursor"
git push -u origin add-dsh-ai-cursor
gh pr create --repo awesome-dsh-plugin/awesome-dsh-plugin \
  --title "Add CAT0998/dsh-ai-cursor" \
  --body "AI cursor + floating control panel for DeepSeek Harness (Windows). Adds 14 ai_mouse_* tools, a collapsible panel, and a permission ladder the agent can only lower."
```

## 四、发布前自检清单

- [x] `pnpm run typecheck` / `pnpm test` / `pnpm run build` / `pnpm run test:overlay` 全绿
- [x] `pnpm pack` 产物里 `bin/ai-mouse-overlay.exe` 存在（用户不会装 Rust）
- [x] `locale/{en,zh}.json` 有 `meta.title` / `meta.description`
- [x] 导出 `./icon`（`icon.svg`，≤256 KiB，realpath 在包内）
- [x] 仓库 topic `dsh-plugin`
- [ ] 仓库满 1 天后再提 PR
