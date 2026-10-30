import { describe, expect, it, beforeEach, afterEach } from 'vitest'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  capturePreset,
  describePreset,
  findPreset,
  loadPresets,
  presetCommands,
  removePreset,
  savePresets,
  upsertPreset,
  type Preset,
} from '../src/presets'

const NOW = new Date('2026-01-02T03:04:05.000Z')

describe('capturePreset', () => {
  it('copies only the fields that are present', () => {
    const p = capturePreset('夜间', { theme: 'arrow', opacity: 0.5, badge: false }, NOW)
    expect(p.name).toBe('夜间')
    expect(p.theme).toBe('arrow')
    expect(p.opacity).toBe(0.5)
    expect(p.badge).toBe(false)
    // never invented
    expect('label' in p).toBe(false)
    expect('scale' in p).toBe(false)
    expect(p.saved_at).toBe(NOW.toISOString())
  })

  it('keeps zero and false rather than treating them as absent', () => {
    const p = capturePreset('x', { opacity: 0, badge: false }, NOW)
    expect(p.opacity).toBe(0)
    expect(p.badge).toBe(false)
  })
})

describe('presetCommands', () => {
  it('maps every set field to exactly one protocol call', () => {
    const cmds = presetCommands({
      name: 'p',
      theme: 'dot',
      label: 'hi',
      color: '#112233',
      opacity: 0.4,
      scale: 1.5,
      badge: true,
      capture_visible: false,
    })
    expect(cmds.map(([c]) => c)).toEqual([
      'theme',
      'label',
      'color',
      'opacity',
      'scale',
      'badge',
      'affinity',
    ])
    expect(cmds[6][1]).toEqual({ capture_visible: false })
  })

  it('does not emit anything for an empty preset', () => {
    expect(presetCommands({ name: 'empty' })).toEqual([])
  })
})

describe('list helpers', () => {
  const a: Preset = { name: 'Alpha', theme: 'whale' }
  const b: Preset = { name: 'beta', theme: 'dot' }

  it('finds case-insensitively', () => {
    expect(findPreset([a, b], 'ALPHA')).toBe(a)
    expect(findPreset([a, b], ' nope ')).toBeUndefined()
  })

  it('replaces an existing name instead of duplicating it', () => {
    const next = upsertPreset([a, b], { name: 'alpha', theme: 'arrow' })
    expect(next).toHaveLength(2)
    expect(findPreset(next, 'alpha')?.theme).toBe('arrow')
  })

  it('keeps the list sorted', () => {
    expect(upsertPreset([b, a], { name: 'Gamma' }).map((p) => p.name)).toEqual([
      'Alpha',
      'beta',
      'Gamma',
    ])
  })

  it('removes case-insensitively and tolerates a miss', () => {
    expect(removePreset([a, b], 'BETA').map((p) => p.name)).toEqual(['Alpha'])
    expect(removePreset([a, b], 'zzz')).toHaveLength(2)
  })
})

describe('persistence', () => {
  let dir = ''
  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), 'presets-'))
  })
  afterEach(async () => {
    await rm(dir, { recursive: true, force: true })
  })

  it('round-trips through a file', async () => {
    const file = join(dir, 'nested', 'presets.json')
    const list = [capturePreset('one', { theme: 'whale', opacity: 0.8 }, NOW)]
    await savePresets(file, list)
    expect(await loadPresets(file)).toEqual(list)
  })

  it('treats a missing file as an empty set', async () => {
    expect(await loadPresets(join(dir, 'nope.json'))).toEqual([])
  })

  it('drops entries without a name', async () => {
    const file = join(dir, 'bad.json')
    await savePresets(file, [])
    await savePresets(file, [{ theme: 'x' } as unknown as Preset, { name: 'ok' }])
    expect((await loadPresets(file)).map((p) => p.name)).toEqual(['ok'])
  })
})

describe('describePreset', () => {
  it('lists the set fields and marks an empty preset', () => {
    expect(describePreset({ name: 'e' })).toContain('空')
    expect(describePreset({ name: 'n', theme: 'dot', opacity: 0.5 })).toMatch(/theme=dot/)
  })
})
