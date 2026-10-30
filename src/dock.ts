/**
 * Berth data for the dock station window.
 *
 * The dock process is a dumb renderer: it re-reads a JSON file on every repaint.
 * Writing a file instead of holding a pipe open is what makes this survive a
 * restart, a crash or a second plugin instance — nothing has to stay connected.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'

export interface Berth {
  agent: string
  preset: string
  color: string
  occupied: boolean
  /** 运行中 / 待机中 / 已停用 / 休整中, as reported by that cursor. */
  state: string
}

export const BERTH_SLOTS = 5

export function emptyBerths(count = BERTH_SLOTS): Berth[] {
  return Array.from({ length: count }, () => ({
    agent: '',
    preset: '',
    color: '',
    occupied: false,
    state: '',
  }))
}

export interface BerthCursor {
  name: string
  color?: string
  preset?: string
  state?: string
}

/**
 * Pack the live cursors into berths.
 *
 * The default cursor is skipped: it is the plugin's own, not a subagent's, and the
 * dock exists to show which subagent holds which cursor. Overflow beyond the
 * available berths is dropped rather than wrapped, so a berth never silently
 * displays someone else's cursor.
 */
export function berthsFrom(cursors: BerthCursor[], count = BERTH_SLOTS): Berth[] {
  const slots = emptyBerths(count)
  let i = 0
  for (const c of cursors) {
    if (c.name === 'default') continue
    if (i >= slots.length) break
    slots[i] = {
      agent: c.name,
      preset: c.preset ?? '',
      color: c.color ?? '#4FC3F7',
      occupied: true,
      state: c.state ?? '',
    }
    i += 1
  }
  return slots
}

export async function writeBerths(file: string, berths: Berth[]): Promise<void> {
  await mkdir(dirname(file), { recursive: true })
  await writeFile(file, `${JSON.stringify(berths, null, 2)}\n`, 'utf8')
}

/** One line per berth, for a quick text summary of the dock. */
export function describeBerths(berths: Berth[]): string {
  return berths
    .map((b, i) =>
      b.occupied
        ? `${i + 1}号位 ${b.agent}${b.preset ? ` / ${b.preset}` : ''} / ${b.color}`
        : `${i + 1}号位 空闲`,
    )
    .join('  |  ')
}

/** The dock publishes the berth indices the user selected. */
export interface DockSelection {
  selected: number[]
}

export async function readSelection(file: string): Promise<number[]> {
  try {
    const raw = await readFile(file, 'utf8')
    const parsed = JSON.parse(raw) as DockSelection
    const list = Array.isArray(parsed.selected) ? parsed.selected : []
    return list
      .map((x) => Number(x))
      .filter((x) => Number.isInteger(x) && x >= 0)
  } catch {
    // no selection file yet, or a half-written one: an empty selection is correct
    return []
  }
}

export async function readBerths(file: string): Promise<Berth[]> {
  try {
    const raw = await readFile(file, 'utf8')
    const parsed = JSON.parse(raw) as unknown
    return Array.isArray(parsed) ? (parsed as Berth[]) : []
  } catch {
    return []
  }
}

/**
 * Map the dock's selected berth indices to the cursors behind them.
 *
 * The dock only knows positions, so the names come from the berth file — the same
 * order it was published in.
 */
export function selectedNames(berths: Berth[], selected: number[]): string[] {
  const names: string[] = []
  for (const i of selected) {
    const b = berths[i]
    if (b && b.occupied && b.agent && !names.includes(b.agent)) names.push(b.agent)
  }
  return names
}
