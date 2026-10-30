/**
 * Named configuration presets.
 *
 * A preset is just the visible state of one cursor: theme, label, colour,
 * opacity, scale, badge and capture visibility. Saving captures the live state;
 * loading replays it as ordinary protocol commands, so a preset can never do
 * anything an individual tool call could not.
 *
 * This is the groundwork for one cursor per subagent: the same shape can later be
 * assigned per agent, and a new agent can pick a preset by name.
 */
import { readFile, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'
import { mkdir } from 'node:fs/promises'

export interface Preset {
  name: string
  theme?: string
  label?: string
  color?: string
  opacity?: number
  scale?: number
  badge?: boolean
  capture_visible?: boolean
  /** ISO timestamp of the last save. */
  saved_at?: string
}

/** The fields a preset captures, in the order they are applied. */
export const PRESET_FIELDS = [
  'theme',
  'label',
  'color',
  'opacity',
  'scale',
  'badge',
  'capture_visible',
] as const

/** Everything a preset can be built from. */
export interface PresetSource {
  theme?: string
  label?: string
  color?: string
  opacity?: number
  scale?: number
  badge?: boolean
  capture_visible?: boolean
}

export function capturePreset(name: string, source: PresetSource, now = new Date()): Preset {
  const preset: Preset = { name, saved_at: now.toISOString() }
  for (const key of PRESET_FIELDS) {
    const value = source[key]
    if (value !== undefined) {
      ;(preset as unknown as Record<string, unknown>)[key] = value
    }
  }
  return preset
}

/**
 * The protocol calls that reproduce a preset.
 *
 * Values are filtered for the same reason `capturePreset` is: an absent field
 * means "do not touch", never "reset to default".
 */
export function presetCommands(preset: Preset): Array<[string, Record<string, unknown>]> {
  const out: Array<[string, Record<string, unknown>]> = []
  if (preset.theme !== undefined) out.push(['theme', { theme: preset.theme }])
  if (preset.label !== undefined) out.push(['label', { text: preset.label }])
  if (preset.color !== undefined) out.push(['color', { hex: preset.color }])
  if (preset.opacity !== undefined) out.push(['opacity', { value: preset.opacity }])
  if (preset.scale !== undefined) out.push(['scale', { value: preset.scale }])
  if (preset.badge !== undefined) out.push(['badge', { visible: preset.badge }])
  if (preset.capture_visible !== undefined) {
    out.push(['affinity', { capture_visible: preset.capture_visible }])
  }
  return out
}

export function findPreset(list: Preset[], name: string): Preset | undefined {
  const wanted = name.trim().toLowerCase()
  return list.find((p) => p.name.trim().toLowerCase() === wanted)
}

/** Insert or replace by name (case-insensitive), keeping the list ordered. */
export function upsertPreset(list: Preset[], preset: Preset): Preset[] {
  const next = list.filter((p) => p.name.trim().toLowerCase() !== preset.name.trim().toLowerCase())
  next.push(preset)
  return next.sort((a, b) => a.name.localeCompare(b.name))
}

export function removePreset(list: Preset[], name: string): Preset[] {
  const wanted = name.trim().toLowerCase()
  return list.filter((p) => p.name.trim().toLowerCase() !== wanted)
}

export async function loadPresets(file: string): Promise<Preset[]> {
  try {
    const raw = await readFile(file, 'utf8')
    const parsed = JSON.parse(raw) as unknown
    if (!Array.isArray(parsed)) return []
    return parsed.filter(
      (p): p is Preset => typeof p === 'object' && p !== null && typeof (p as Preset).name === 'string',
    )
  } catch {
    // A missing or unreadable file is an empty set, not an error: presets are
    // optional and must never block the plugin from starting.
    return []
  }
}

export async function savePresets(file: string, list: Preset[]): Promise<void> {
  await mkdir(dirname(file), { recursive: true })
  await writeFile(file, `${JSON.stringify(list, null, 2)}\n`, 'utf8')
}

export function describePreset(preset: Preset): string {
  const bits = PRESET_FIELDS.filter((k) => preset[k] !== undefined).map((k) => {
    const v = preset[k]
    return `${k}=${typeof v === 'number' ? Number(v.toFixed?.(2) ?? v) : String(v)}`
  })
  return `${preset.name}: ${bits.join(' ') || '（空）'}`
}
