/**
 * Panel-side stores: **menu themes** and **mouse (per-cursor) presets**.
 *
 * They are two separate files on purpose — requirement "鼠标设置和主题设置分开保存":
 *
 *   `<assetDir>/menu-themes.json`     the menu bar's own look: its accent only
 *   `<assetDir>/cursor-presets.json`  a named snapshot of ONE AI cursor
 *
 * `presets.json` keeps working exactly as before as the legacy mixed store
 * (`ai_mouse_preset` and every old preset still resolve against it), so nothing
 * that existed stops working.
 *
 * The overlay cannot write the asset directory (os error 5, see LESSONS 3d), so a
 * save from the panel is a mailbox command consumed by the host; the overlay only
 * READS these files.
 */
import { readFile, mkdir, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'

/**
 * One named look for the menu bar itself.
 *
 * It is an accent and nothing else. Menu themes used to carry a `zoom` as well; that
 * field went out with the 「菜单缩放」 control, which the user had removed because it
 * duplicated the concept of resizing the window (see docs/TASKS.md, decision table).
 * Old files that still contain `zoom` keep loading — the extra key is simply ignored.
 */
export interface MenuTheme {
  name: string
  accent: string
  saved_at?: string
}

/** One named snapshot of a single AI cursor's settings. */
export interface CursorPreset {
  name: string
  theme?: string
  label?: string
  color?: string
  opacity?: number
  scale?: number
  badge?: boolean
  saved_at?: string
}

const round3 = (value: number | undefined, fallback: number): string => {
  const n = typeof value === 'number' && Number.isFinite(value) ? value : fallback
  return String(Number(n.toFixed(3)))
}

const normColor = (value: string | undefined, fallback = '#4FC3F7'): string =>
  `#${(value ?? fallback).replace(/^#/, '').toUpperCase()}`

const normText = (value: string | undefined): string => (value ?? '').trim()

/**
 * The comparability key of a mouse preset: **the same settings give the same key**,
 * so saving the same look twice stores one entry instead of growing the file.
 *
 * Fields that are not part of the look (`name`, `saved_at`) are excluded on purpose:
 * naming the same look differently must not duplicate it.
 */
export function cursorPresetKey(
  preset: Pick<CursorPreset, 'theme' | 'label' | 'color' | 'opacity' | 'scale' | 'badge'>,
): string {
  return [
    normText(preset.theme).toLowerCase() || 'whale',
    normText(preset.label),
    normColor(preset.color),
    round3(preset.opacity, 1),
    round3(preset.scale, 1),
    preset.badge === false ? '0' : '1',
  ].join('|')
}

/** Same idea for a menu theme: the accent IS the whole look now. */
export function menuThemeKey(theme: Pick<MenuTheme, 'accent'>): string {
  return normColor(theme.accent)
}

export interface UpsertResult<T> {
  list: T[]
  /** The name that ended up holding these settings. */
  name: string
  /** True when nothing new was written (the same look was already there). */
  duplicate: boolean
}

/**
 * Insert or replace by name, **and never store the same settings twice**.
 *
 * - An existing entry with the same settings key wins: the list keeps its length
 *   (only `saved_at` is refreshed) and `duplicate: true` reports that.
 * - Otherwise the entry is added under its own name, kept sorted by name.
 */
export function upsertCursorPreset(
  list: CursorPreset[],
  preset: CursorPreset,
): UpsertResult<CursorPreset> {
  const key = cursorPresetKey(preset)
  const same = list.find((p) => cursorPresetKey(p) === key)
  if (same) {
    const next = list.map((p) =>
      cursorPresetKey(p) === key ? { ...p, saved_at: preset.saved_at ?? p.saved_at } : p,
    )
    return { list: next, name: same.name, duplicate: true }
  }
  const next = list.filter((p) => p.name.trim().toLowerCase() !== preset.name.trim().toLowerCase())
  next.push(preset)
  next.sort((a, b) => a.name.localeCompare(b.name))
  return { list: next, name: preset.name, duplicate: false }
}

export function removeCursorPreset(list: CursorPreset[], name: string): CursorPreset[] {
  const wanted = name.trim().toLowerCase()
  return list.filter((p) => p.name.trim().toLowerCase() !== wanted)
}

export function upsertMenuTheme(list: MenuTheme[], theme: MenuTheme): UpsertResult<MenuTheme> {
  const key = menuThemeKey(theme)
  const same = list.find((p) => menuThemeKey(p) === key)
  if (same) {
    const next = list.map((p) =>
      menuThemeKey(p) === key ? { ...p, saved_at: theme.saved_at ?? p.saved_at } : p,
    )
    return { list: next, name: same.name, duplicate: true }
  }
  const next = list.filter((p) => p.name.trim().toLowerCase() !== theme.name.trim().toLowerCase())
  next.push(theme)
  next.sort((a, b) => a.name.localeCompare(b.name))
  return { list: next, name: theme.name, duplicate: false }
}

export function removeMenuTheme(list: MenuTheme[], name: string): MenuTheme[] {
  const wanted = name.trim().toLowerCase()
  return list.filter((p) => p.name.trim().toLowerCase() !== wanted)
}

/**
 * The name shown next to the ‹ › arrows for the cursor at `index`.
 *
 * Its saved preset name wins; an unnamed first cursor is 默认, the rest are
 * 自定义1, 自定义2 … (1-based, so it reads like "the second mouse").
 */
export function cursorDisplayName(index: number, presetName?: string): string {
  const name = (presetName ?? '').trim()
  if (name) return name
  return index <= 0 ? '默认' : `自定义${index}`
}

/** The name a save-without-a-name picks: the same label the arrows show. */
export function autoPresetName(index: number, existing: CursorPreset[]): string {
  const base = cursorDisplayName(index, '')
  if (!existing.some((p) => p.name === base)) return base
  for (let n = 2; n < 200; n += 1) {
    const candidate = `${base}-${n}`
    if (!existing.some((p) => p.name === candidate)) return candidate
  }
  return `${base}-${existing.length + 1}`
}

/** The auto name for an unnamed menu theme: 主题1, 主题2 … */
export function nextThemeName(existing: MenuTheme[]): string {
  for (let n = 1; n < 200; n += 1) {
    const candidate = `主题${n}`
    if (!existing.some((t) => t.name === candidate)) return candidate
  }
  return `主题${existing.length + 1}`
}

export async function loadMenuThemes(file: string): Promise<MenuTheme[]> {
  return (await loadArray(file)).filter(
    (t): t is MenuTheme =>
      typeof t === 'object' &&
      t !== null &&
      typeof (t as MenuTheme).name === 'string' &&
      typeof (t as MenuTheme).accent === 'string',
  )
}

export async function saveMenuThemes(file: string, list: MenuTheme[]): Promise<void> {
  await writeArray(file, list)
}

export async function loadCursorPresets(file: string): Promise<CursorPreset[]> {
  return (await loadArray(file)).filter(
    (p): p is CursorPreset =>
      typeof p === 'object' && p !== null && typeof (p as CursorPreset).name === 'string',
  )
}

export async function saveCursorPresets(file: string, list: CursorPreset[]): Promise<void> {
  await writeArray(file, list)
}

async function loadArray(file: string): Promise<unknown[]> {
  try {
    const parsed = JSON.parse(await readFile(file, 'utf8')) as unknown
    return Array.isArray(parsed) ? parsed : []
  } catch {
    // A missing or unreadable file is an empty set, never an error: the panel must
    // still open on a fresh machine.
    return []
  }
}

async function writeArray(file: string, list: unknown[]): Promise<void> {
  await mkdir(dirname(file), { recursive: true })
  await writeFile(file, `${JSON.stringify(list, null, 2)}\n`, 'utf8')
}

/** What one panel command resolved to. */
export interface PanelCommandResult {
  /** Human sentence for the log and the tool reply. */
  message: string
  /**
   * Protocol commands the host still has to send to the cursor (applying a preset).
   * Empty for pure file actions.
   */
  commands?: Array<[string, Record<string, unknown>]>
}

/** Everything a panel command needs, injected so it can be tested without a host. */
export interface PanelCommandEnv {
  assetDir: string
  menuThemesFile: string
  cursorPresetsFile: string
  /** The cursor's live state, for a save. */
  status: () => Promise<Record<string, unknown>>
}

/**
 * Handle one mailbox action that belongs to the panel redesign.
 *
 * Returns `null` for actions this module does not own (the dock's `rest`, the legacy
 * presets, `toggle_column`, …) so the caller can keep handling those its own way.
 * Every branch is a file write that the overlay process cannot do itself, or an
 * "apply this preset" that has to be turned into protocol commands.
 */
export async function handlePanelCommand(
  action: string,
  cmd: Record<string, unknown>,
  env: PanelCommandEnv,
): Promise<PanelCommandResult | null> {
  if (!action.startsWith('menu_theme_') && !action.startsWith('cursor_preset_')) return null
  if (!env.assetDir) return { message: '素材目录未配置，面板的保存/删除无法落盘。' }

  if (action === 'menu_theme_save') {
    const list = await loadMenuThemes(env.menuThemesFile)
    const theme: MenuTheme = {
      name: nextThemeName(list),
      accent: String(cmd.accent ?? '#4FC3F7'),
      saved_at: new Date().toISOString(),
    }
    const result = upsertMenuTheme(list, theme)
    if (result.duplicate) return { message: `菜单主题「${result.name}」已存在，没有重复保存` }
    await saveMenuThemes(env.menuThemesFile, result.list)
    return { message: `已保存菜单主题「${result.name}」` }
  }

  if (action === 'menu_theme_delete') {
    const name = String(cmd.name ?? '').trim()
    if (!name) return { message: '没有选中要删除的菜单主题' }
    const list = await loadMenuThemes(env.menuThemesFile)
    const next = removeMenuTheme(list, name)
    if (next.length === list.length) return { message: `没有名为 ${name} 的菜单主题` }
    await saveMenuThemes(env.menuThemesFile, next)
    return { message: `已删除菜单主题「${name}」` }
  }

  if (action === 'cursor_preset_save') {
    const list = await loadCursorPresets(env.cursorPresetsFile)
    const st = await env.status()
    const index = Number(cmd.index ?? 0)
    const preset: CursorPreset = {
      name: String(cmd.name ?? '').trim() || autoPresetName(index, list),
      theme: String(st.theme ?? 'whale'),
      label: String(st.label ?? ''),
      color: String(st.color ?? '#4FC3F7'),
      opacity: Number(st.opacity ?? 1),
      scale: Number(st.scale ?? 1),
      badge: Boolean(st.badge),
      saved_at: new Date().toISOString(),
    }
    const result = upsertCursorPreset(list, preset)
    if (result.duplicate) return { message: `相同的鼠标设置已存为「${result.name}」，没有重复保存` }
    await saveCursorPresets(env.cursorPresetsFile, result.list)
    return { message: `已保存鼠标预设「${result.name}」` }
  }

  if (action === 'cursor_preset_delete') {
    const name = String(cmd.name ?? '').trim()
    if (!name) return { message: '没有选中要删除的鼠标预设' }
    const list = await loadCursorPresets(env.cursorPresetsFile)
    const next = removeCursorPreset(list, name)
    if (next.length === list.length) return { message: `没有名为 ${name} 的鼠标预设` }
    await saveCursorPresets(env.cursorPresetsFile, next)
    return { message: `已删除鼠标预设「${name}」` }
  }

  if (action === 'cursor_preset_apply') {
    const name = String(cmd.name ?? '').trim()
    const list = await loadCursorPresets(env.cursorPresetsFile)
    const preset = list.find((p) => p.name === name)
    if (!preset) return { message: `没有名为 ${name} 的鼠标预设` }
    return {
      message: `已应用鼠标预设「${preset.name}」`,
      commands: cursorPresetCommands(preset),
    }
  }

  return null
}

/**
 * The protocol calls that reproduce a mouse preset.
 *
 * Same rule as the legacy presets: an absent field means "leave it alone", never
 * "reset to default".
 */
export function cursorPresetCommands(
  preset: CursorPreset,
): Array<[string, Record<string, unknown>]> {
  const out: Array<[string, Record<string, unknown>]> = []
  if (preset.theme !== undefined) out.push(['theme', { theme: preset.theme }])
  if (preset.label !== undefined) out.push(['label', { text: preset.label }])
  if (preset.color !== undefined) out.push(['color', { hex: preset.color }])
  if (preset.opacity !== undefined) out.push(['opacity', { value: preset.opacity }])
  if (preset.scale !== undefined) out.push(['scale', { value: preset.scale }])
  if (preset.badge !== undefined) out.push(['badge', { visible: preset.badge }])
  return out
}

/** One entry of the roster a panel shows. */
export interface RosterEntry {
  /** Stable cursor name in the host (`default`, `agent-b`, …). */
  name: string
  /** What the ‹ › arrows show. */
  display: string
  theme: string
  label: string
  color: string
  opacity: number
  scale: number
  badge: boolean
}

/**
 * One panel's view of every cursor.
 *
 * `self` is the cursor that owns the panel process, so it can tell "edit me" from
 * "forward this to someone else". `selected` is the cursor the arrows point at.
 */
export interface PanelRoster {
  self: string
  selected: number
  cursors: RosterEntry[]
}

/**
 * Turn one `cursor_patch` patch into protocol commands for the target cursor.
 *
 * The panel writes intents; this is where an intent becomes the same commands the
 * `ai_mouse_*` tools already use, so a panel edit can never do anything a tool call
 * could not.
 */
export function cursorPatchCommands(
  patch: Record<string, unknown>,
): Array<[string, Record<string, unknown>]> {
  const out: Array<[string, Record<string, unknown>]> = []
  if (typeof patch.theme === 'string') out.push(['theme', { theme: patch.theme }])
  if (typeof patch.label === 'string') out.push(['label', { text: patch.label }])
  if (typeof patch.color === 'string') out.push(['color', { hex: patch.color }])
  if (typeof patch.opacity === 'number') out.push(['opacity', { value: patch.opacity }])
  if (typeof patch.scale === 'number') out.push(['scale', { value: patch.scale }])
  if (typeof patch.badge === 'boolean') out.push(['badge', { visible: patch.badge }])
  return out
}

/** Resolve a mailbox target (a name, `self`, or an index) to a roster name. */
export function resolveTarget(
  target: unknown,
  names: string[],
  selfName: string,
  selectedIndex: number,
): string | undefined {
  if (typeof target === 'string') {
    const wanted = target.trim()
    if (wanted === '' || wanted === 'self') return selfName
    if (names.includes(wanted)) return wanted
    return undefined
  }
  if (typeof target === 'number' && Number.isInteger(target)) {
    return names[target]
  }
  return names[selectedIndex] ?? names[0]
}
