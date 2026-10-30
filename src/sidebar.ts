/**
 * A self-contained HTML sidebar for the AI cursors.
 *
 * The data is embedded at generation time rather than fetched, so the file works
 * from file:// with no server and no CORS problems: open it once and it shows the
 * berths, their state and the preset list. Regenerate it to refresh.
 *
 * This is the "browser virtual interface" route for the side panel: no native
 * window, no extra process, nothing to install.
 */
import { readFile } from 'node:fs/promises'

export interface SidebarBerth {
  agent: string
  preset: string
  color: string
  occupied: boolean
  state: string
}

export interface SidebarPreset {
  name: string
  theme?: string
  color?: string
  opacity?: number
  scale?: number
  saved_at?: string
}

export async function readJsonArray<T>(file: string): Promise<T[]> {
  try {
    const raw = await readFile(file, 'utf8')
    const parsed = JSON.parse(raw) as unknown
    return Array.isArray(parsed) ? (parsed as T[]) : []
  } catch {
    return []
  }
}

const STATE_COLOR: Record<string, string> = {
  运行中: '#37c26a',
  待机中: '#4FC3F7',
  已停用: '#7c8798',
  休整中: '#e0a44a',
}

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] as string,
  )
}

export function renderSidebar(berths: SidebarBerth[], presets: SidebarPreset[], now = new Date()): string {
  const shown = berths.filter((b) => b.occupied)
  const berthCards = shown.length
    ? shown
        .map((b, i) => {
          const colour = STATE_COLOR[b.state] ?? '#7c8798'
          return `<div class="berth" style="--c:${esc(b.color || '#4FC3F7')}">
  <div class="dot"></div>
  <div class="meta">
    <div class="name">${esc(b.agent)}</div>
    <div class="line">${i + 1}号位 · ${esc(b.preset || '无预设')}</div>
    <div class="state" style="color:${colour}">${esc(b.state || '未知')}</div>
  </div>
</div>`
        })
        .join('')
    : '<p class="empty">停靠站里没有已占用的光标</p>'

  const presetRows = presets.length
    ? presets
        .map(
          (p) =>
            `<li><span class="pname">${esc(p.name)}</span><span class="pmeta">${esc(
              [p.theme, p.color, p.opacity !== undefined ? `${Math.round(p.opacity * 100)}%` : '']
                .filter(Boolean)
                .join(' · '),
            )}</span></li>`,
        )
        .join('')
    : '<li class="empty">还没有预设</li>'

  return `<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<title>AI 鼠标 · 侧边栏</title>
<style>
  :root { color-scheme: dark; }
  body { margin:0; font:13px/1.5 "Microsoft YaHei",system-ui,sans-serif;
         background:#0b1016; color:#c7d3e0; width:280px; }
  header { padding:14px 14px 8px; font-weight:600; font-size:15px; color:#e6eef7; }
  h2 { font-size:12px; letter-spacing:.08em; text-transform:uppercase;
       color:#6e8199; margin:14px 14px 6px; font-weight:600; }
  .berth { display:flex; gap:10px; align-items:center; margin:6px 14px;
           padding:8px 10px; border-radius:10px; background:#141c26;
           border:1px solid #24313f; border-left:3px solid var(--c); }
  .dot { width:14px; height:14px; border-radius:50%; background:var(--c);
         box-shadow:0 0 0 2px rgba(255,255,255,.15) inset; flex:0 0 auto; }
  .name { color:#dbe7f3; font-weight:600; }
  .line { color:#7d8ea3; font-size:11px; }
  .state { font-size:11px; }
  ul { list-style:none; margin:0 14px; padding:0; }
  li { display:flex; justify-content:space-between; gap:8px; padding:6px 8px;
       border-radius:8px; background:#141c26; margin-bottom:4px; }
  .pname { color:#dbe7f3; }
  .pmeta { color:#6e8199; font-size:11px; }
  .empty { color:#5a6b7e; font-size:12px; margin:6px 14px; }
  footer { color:#46586b; font-size:11px; padding:12px 14px 18px; }
  details > summary { cursor:pointer; list-style:none; }
  details > summary::-webkit-details-marker { display:none; }
</style></head>
<body>
<header>AI 鼠标</header>
<details open><summary><h2>停靠站（${shown.length}）</h2></summary>${berthCards}</details>
<details open><summary><h2>预设（${presets.length}）</h2></summary><ul>${presetRows}</ul></details>
<footer>生成于 ${esc(now.toLocaleString('zh-CN'))} · 重新生成可刷新</footer>
</body></html>
`
}

export async function writeSidebar(
  path: string,
  berthsFile: string,
  presetsFile: string,
): Promise<{ berths: number; presets: number }> {
  const berths = await readJsonArray<SidebarBerth>(berthsFile)
  const presets = await readJsonArray<SidebarPreset>(presetsFile)
  const html = renderSidebar(berths, presets)
  const { writeFile, mkdir } = await import('node:fs/promises')
  const { dirname } = await import('node:path')
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, html, 'utf8')
  return { berths: berths.filter((b) => b.occupied).length, presets: presets.length }
}
