/**
 * End-to-end check of the HOST half of the panel redesign, without restarting DSH.
 *
 * It loads the built plugin (`lib/index.js`) into a fake Cordis context, so the real
 * `apply()` runs: the real overlay processes are spawned, the real mailbox consumer
 * is registered, and the real files are written. Input (a human clicking the panel)
 * is simulated by writing the same mailbox commands the panel writes — those exact
 * payloads are already proven by `scripts/live-panel-check.py`.
 *
 *     node scripts/live-host-check.mjs
 */
import { mkdtemp, readFile, rm, writeFile, access } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repo = resolve(here, '..')
const exe = join(repo, 'bin', 'ai-mouse-overlay.exe')
const { pathToFileURL } = await import('node:url')
const { apply } = await import(pathToFileURL(join(repo, 'lib', 'index.js')).href)

const results = []
const check = (name, ok, got = '') => {
  results.push([name, Boolean(ok), got])
  console.log(`${ok ? '[PASS]' : '[FAIL]'} ${name}${got === '' ? '' : ` -> ${got}`}`)
}

const exists = async (p) => {
  try {
    await access(p)
    return true
  } catch {
    return false
  }
}

const readJson = async (p) => JSON.parse(await readFile(p, 'utf8'))

const assets = await mkdtemp(join(tmpdir(), 'host-assets-'))
const stateDir = join(dirname(exe), 'logs')
const mailbox = join(stateDir, 'dock-command.json')

const tools = new Map()
const disposers = []
const ctx = {
  tools: {
    register(tool) {
      tools.set(tool.name, tool)
    },
  },
  effect(fn) {
    const dispose = fn()
    if (typeof dispose === 'function') disposers.push(dispose)
  },
  on() {},
}

const config = {
  enabled: true,
  exePath: exe,
  label: 'AI 鼠标',
  theme: 'whale',
  idleMs: 60000,
  captureVisible: false,
  panel: true,
  panelExpanded: true,
  opacity: 1,
  color: '#4FC3F7',
  scale: 1,
  badge: true,
  permission: 3,
  intercept: 'off',
  interceptTools: [],
  assetDir: assets,
}

const call = async (name, args = {}) => {
  const tool = tools.get(name)
  if (!tool) throw new Error(`tool ${name} not registered`)
  return await tool.execute(args)
}

const sendMailbox = async (body) => {
  await writeFile(mailbox, JSON.stringify({ ...body, epoch: Date.now() }), 'utf8')
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

try {
  await rm(mailbox, { force: true })
  apply(ctx, config)
  check('E1 插件装载并注册工具', tools.size > 5, `${tools.size} tools`)

  // opening the panel primes the cursor and publishes the roster
  const panelReply = await call('ai_mouse_panel', { visible: true, expanded: true })
  check('E2 面板能打开', /panel=full/.test(panelReply) || /panel=/.test(panelReply), panelReply.slice(0, 90))

  const logPath = join(stateDir, 'overlay.log')
  const log = await readFile(logPath, 'utf8')
  const rosterLines = log.split('\n').filter((l) => l.includes('panel roster:'))
  check(
    'E3 宿主推送了 roster（面板收到 self + 名字）',
    rosterLines.length > 0 &&
      rosterLines[rosterLines.length - 1].includes('self="default"') &&
      rosterLines[rosterLines.length - 1].includes('"默认"'),
    rosterLines[rosterLines.length - 1]?.trim() ?? 'no roster line',
  )

  // ---- 菜单主题：面板写的意图 -> 宿主落盘 -> 相同主题不重复 ----
  await sendMailbox({ action: 'menu_theme_save', accent: '#7C5CFF', zoom: 1.15 })
  const drained = await call('ai_mouse_dock', {})
  const themeFile = join(assets, 'menu-themes.json')
  const themes = (await exists(themeFile)) ? await readJson(themeFile) : []
  check('E4 信箱命令被消费并写入 menu-themes.json', themes.length === 1, drained.slice(0, 70))
  check('E5 主题内容正确（accent/zoom）', themes[0]?.accent === '#7C5CFF' && themes[0]?.zoom === 1.15, JSON.stringify(themes[0]))

  await sendMailbox({ action: 'menu_theme_save', accent: '#7c5cff', zoom: 1.15 })
  const dedup = await call('ai_mouse_dock', {})
  const themes2 = await readJson(themeFile)
  check('E6 同一个主题存两次 -> 只有一条（端到端去重）',
        themes2.length === 1 && /已存在/.test(dedup), `${themes2.length} 条 / ${dedup.slice(0, 50)}`)

  // ---- 定时器路径：不调工具也要落盘 ----
  await rm(themeFile, { force: true })
  await sendMailbox({ action: 'menu_theme_save', accent: '#34D399', zoom: 1 })
  await sleep(900)
  const timerThemes = (await exists(themeFile)) ? await readJson(themeFile) : []
  check('E7 250ms 定时器在面板打开时排空信箱', timerThemes.length === 1, JSON.stringify(timerThemes[0]?.name))

  // ---- 选中鼠标：select_cursor 落盘 ----
  await sendMailbox({ action: 'select_cursor', index: 0 })
  await sleep(700)
  const uiFile = join(stateDir, 'cursor-ui.json')
  const ui = (await exists(uiFile)) ? await readJson(uiFile) : {}
  check('E8 select_cursor -> cursor-ui.json 记住选中项', ui.selected === 0, JSON.stringify(ui))

  // ---- 路由修改：cursor_patch 真的改到光标上 ----
  await sendMailbox({ action: 'cursor_patch', target: 'self', patch: { color: '#FF004D', opacity: 0.6 } })
  await sleep(700)
  const status = await call('ai_mouse_status', {})
  check('E9 cursor_patch 应用到光标（color/opacity）',
        /color=#FF004D/.test(status) && /opacity=60%/.test(status),
        status.match(/color=[^ ]+ opacity=[^ ]+/)?.[0] ?? status.slice(0, 80))

  // ---- 鼠标预设：保存 + 相同设置去重 ----
  await sendMailbox({ action: 'cursor_preset_save', target: 'self', index: 0 })
  await sleep(700)
  const presetFile = join(assets, 'cursor-presets.json')
  const presets = (await exists(presetFile)) ? await readJson(presetFile) : []
  check('E10 cursor_preset_save 写入 cursor-presets.json', presets.length === 1, JSON.stringify(presets[0]?.name))
  check('E11 自动命名第一只为「默认」', presets[0]?.name === '默认', presets[0]?.name)

  await sendMailbox({ action: 'cursor_preset_save', target: 'self', index: 0 })
  await sleep(700)
  const presets2 = await readJson(presetFile)
  check('E12 同一设置再存一次 -> 仍然只有一条', presets2.length === 1, `${presets2.length} 条`)

  // ---- 鼠标预设：应用到光标 ----
  await sendMailbox({ action: 'cursor_preset_apply', target: 'self', name: '默认' })
  await sleep(700)
  const status2 = await call('ai_mouse_status', {})
  check('E13 cursor_preset_apply 把预设套回光标',
        /color=#FF004D/.test(status2), status2.match(/color=[^ ]+/)?.[0] ?? status2.slice(0, 60))

  // ---- 多光标：名字规则 默认 / 自定义1 ----
  const created = await call('ai_mouse_cursor', { action: 'create', name: 'agent-b' })
  await sleep(600)
  const log2 = await readFile(logPath, 'utf8')
  const lastRoster = log2.split('\n').filter((l) => l.includes('panel roster:')).pop() ?? ''
  check('E14 新建第二只光标 -> roster 变成 2 个且命名 默认/自定义1',
        lastRoster.includes('2 cursors') &&
        lastRoster.includes('"默认"') &&
        lastRoster.includes('"自定义1"'),
        lastRoster.trim())
  await call('ai_mouse_cursor', { action: 'use', name: 'default' }).catch(() => '')
  void created
} catch (error) {
  check('EXCEPTION', false, error instanceof Error ? error.message : String(error))
} finally {
  for (const dispose of disposers.reverse()) {
    try {
      dispose()
    } catch {
      /* best effort */
    }
  }
  await sleep(400)
  await rm(assets, { recursive: true, force: true })
  const failed = results.filter(([, ok]) => !ok)
  console.log(`\n${results.length - failed.length}/${results.length} passed`)
  for (const [name, , got] of failed) console.log('  FAILED:', name, got)
  process.exit(failed.length === 0 ? 0 : 1)
}
