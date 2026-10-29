/**
 * Interception of other plugins' real-mouse tools.
 *
 * The harness offers exactly one honest seam here: `tools/pre-execute` is a
 * waterfall whose decision is `allow | deny | cancel | ask`. It **cannot rewrite
 * the call**, so there is no way to make `computer_click` secretly act on the AI
 * cursor. What we can do is:
 *
 *   redirect — move the AI cursor to the coordinates the tool was about to use,
 *              then deny the real call and say so. The real click does NOT
 *              happen; the agent is told, not tricked.
 *   refuse   — deny and point the agent at the ai_mouse_* tools.
 *   mirror   — let the real call through and also walk the AI cursor there, so a
 *              human can see where the real pointer is about to go.
 *
 * Everything here is a pure function so it can be unit-tested without a desktop.
 */

export type InterceptMode = 'off' | 'refuse' | 'redirect' | 'mirror'

export const INTERCEPT_MODES: readonly InterceptMode[] = ['off', 'refuse', 'redirect', 'mirror']

/**
 * Tools that move the real pointer, taken from the tools this profile actually
 * exposes. `computer_ui_action` is deliberately absent: it is usually UI
 * Automation and never touches the pointer — add it via `interceptTools` if your
 * setup needs it.
 */
export const DEFAULT_MOUSE_TOOLS: readonly string[] = [
  'computer_click',
  'computer_drag',
  'computer_scroll',
  'computer_actions',
  'cua_driver_native__click',
  'cua_driver_native__double_click',
  'cua_driver_native__right_click',
  'cua_driver_native__drag',
  'cua_driver_native__scroll',
  'cua_driver_native__move_cursor',
]

export interface Point {
  x: number
  y: number
}

/** A tool name matches when listed exactly, or when it starts with `<prefix>_`/`<prefix>__`. */
export function matchesTool(name: string, patterns: readonly string[]): boolean {
  for (const pattern of patterns) {
    if (pattern === name) return true
    if (pattern.endsWith('*') && name.startsWith(pattern.slice(0, -1))) return true
  }
  return false
}

function num(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

function fromObject(value: unknown): Point | undefined {
  if (value === null || typeof value !== 'object') return undefined
  const rec = value as Record<string, unknown>
  const x = num(rec.x) ?? num(rec.from_x) ?? num(rec.to_x) ?? num(rec.fromX) ?? num(rec.toX)
  const y = num(rec.y) ?? num(rec.from_y) ?? num(rec.to_y) ?? num(rec.fromY) ?? num(rec.toY)
  if (x !== undefined && y !== undefined) return { x, y }

  // [x, y] pairs: `at`, `coords`, `position: [x, y]`
  for (const key of ['at', 'position', 'coords', 'point']) {
    const arr = rec[key]
    if (Array.isArray(arr) && arr.length >= 2) {
      const ax = num(arr[0])
      const ay = num(arr[1])
      if (ax !== undefined && ay !== undefined) return { x: ax, y: ay }
    }
  }
  // nested { position: { x, y } } / { target: { x, y } }
  for (const key of ['position', 'target', 'point', 'location']) {
    const nested = fromObject(rec[key])
    if (nested) return nested
  }
  // computer_actions: the first step that carries coordinates
  if (Array.isArray(rec.steps)) {
    for (const step of rec.steps) {
      const found = fromObject(step)
      if (found) return found
    }
  }
  return undefined
}

/**
 * Best-effort extraction of the screen point a mouse tool is about to use.
 * Every shape it accepts is one this profile really exposes; anything else is
 * left alone (no point = deny without moving).
 */
export function extractPoint(args: unknown): Point | undefined {
  if (Array.isArray(args)) {
    for (const item of args) {
      const found = fromObject(item)
      if (found) return found
    }
    return undefined
  }
  return fromObject(args)
}

export interface InterceptContext {
  mode: InterceptMode
  tool: string
  point?: Point
}

/**
 * The text the agent sees instead of the real-mouse result. It must state three
 * things plainly: what was blocked, whether the real action happened, and how a
 * human can lift the block — an agent that cannot tell these apart invents
 * workarounds or reports success it did not achieve.
 */
export function interceptReason(ctx: InterceptContext): string {
  const { mode, tool, point } = ctx
  const at = point ? `(${Math.round(point.x)}, ${Math.round(point.y)})` : '未知坐标'
  const head = `AI 鼠标拦截：${tool} 被拦截（模式 ${mode}），目标 ${at}。`
  if (mode === 'mirror') {
    return `${head}真实操作已照常执行；AI 光标同步移动到该点，仅用于显示。`
  }
  const tail =
    ' 如需真实点击，请让用户把 `ai_mouse_intercept` 切到 `mirror`（真点 + 光标跟随）或 `off`（完全放行）。'
  if (mode === 'refuse') {
    return `${head}真实操作未执行。请改用 ai_mouse_* 工具，或${tail}`
  }
  return (
    `${head}AI 光标已移动到该点，**真实操作未执行**（没有真实点击、没有真实拖拽）。` +
    ` 如果这次必须真的点在那个窗口上，请改用 ai_mouse_* 工具，或${tail}`
  )
}

/** Validate a mode coming from config or a tool argument. */
export function normalizeMode(value: unknown, fallback: InterceptMode = 'off'): InterceptMode {
  return INTERCEPT_MODES.includes(value as InterceptMode) ? (value as InterceptMode) : fallback
}
