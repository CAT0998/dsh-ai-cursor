/**
 * The permission ladder shared by the panel (where a human sets it) and the
 * tools (where the agent is held to it).
 *
 * The rule that makes this worth having: **the agent may lower its own ceiling
 * but never raise it.** Only the floating panel — i.e. a human clicking — can
 * hand control back once the overlay has been stopped.
 */
export type Level = 0 | 1 | 2 | 3

export interface LevelInfo {
  level: Level
  /** Stable id, used in logs and tests. */
  id: 'stopped' | 'readonly' | 'drive' | 'admin'
  zh: string
  en: string
  /** What an agent at this level is allowed to do, in one line. */
  capability: string
}

export const LEVELS: readonly LevelInfo[] = [
  {
    level: 0,
    id: 'stopped',
    zh: '停止',
    en: 'stopped',
    capability: 'nothing except reading status',
  },
  {
    level: 1,
    id: 'readonly',
    zh: '只读',
    en: 'read-only',
    capability: 'status, show, hide, show/hide the panel',
  },
  {
    level: 2,
    id: 'drive',
    zh: '操作',
    en: 'drive',
    capability: 'level 1 plus moving the cursor',
  },
  {
    level: 3,
    id: 'admin',
    zh: '管理员',
    en: 'admin',
    capability: 'level 2 plus appearance (label, theme, badge, opacity, colour, size) and lowering the level',
  },
] as const

/** Clamp anything (a config value, a tool argument) into a real level. */
export function normalizeLevel(value: unknown, fallback: Level = 3): Level {
  const n = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(n)) return fallback
  const i = Math.trunc(n)
  if (i <= 0) return 0
  if (i >= 3) return 3
  return i as Level
}

export function levelInfo(level: number): LevelInfo {
  return LEVELS[normalizeLevel(level)]!
}

export function levelName(level: number, locale: 'zh' | 'en' = 'zh'): string {
  const info = levelInfo(level)
  return locale === 'en' ? info.en : info.zh
}

/** Is an agent at `current` allowed to do something that needs `need`? */
export function permits(current: number, need: number): boolean {
  return normalizeLevel(current, 0) >= normalizeLevel(need, 0)
}

/**
 * The refusal an agent sees. It must say what happened, what the level is and
 * who can change it — an agent that cannot tell why it was blocked will retry
 * forever or start guessing at workarounds.
 */
export function denial(current: number, need: number, action: string): string {
  const cur = levelInfo(current)
  const wanted = levelInfo(need)
  return (
    `AI 鼠标 权限不足：「${action}」需要「${wanted.zh}」(${wanted.level})，当前是「${cur.zh}」(${cur.level})。` +
    ` 权限等级只能由人在浮动面板上提高，agent 只能降低它 —— 请让用户点面板上的「${wanted.zh}」或「管理员」。` +
    ` Permission denied for "${action}": needs level ${wanted.level} (${wanted.en}), current is ${cur.level} (${cur.en}).` +
    ` Only a human can raise it in the floating panel.`
  )
}
