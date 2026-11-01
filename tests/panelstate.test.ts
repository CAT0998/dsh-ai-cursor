/**
 * The panel redesign's pure rules: naming, and "the same look is stored once".
 *
 * These are the requirements a click cannot prove — requirement 7 is about a file
 * not growing — so they are pinned here rather than in the live panel harness.
 */
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  autoPresetName,
  cursorDisplayName,
  cursorPatchCommands,
  cursorPresetKey,
  handlePanelCommand,
  loadCursorPresets,
  loadMenuThemes,
  menuThemeKey,
  nextThemeName,
  removeCursorPreset,
  resolveTarget,
  saveCursorPresets,
  saveMenuThemes,
  upsertCursorPreset,
  upsertMenuTheme,
} from '../src/panelstate'
import type { CursorPreset, MenuTheme } from '../src/panelstate'

const base: CursorPreset = {
  name: '默认',
  theme: 'whale',
  label: 'AI 鼠标',
  color: '#4FC3F7',
  opacity: 1,
  scale: 1,
  badge: true,
}

describe('鼠标预设：相同设置只留一份 (requirement 7)', () => {
  it('saves the same look twice as ONE entry', () => {
    const first = upsertCursorPreset([], base)
    expect(first.duplicate).toBe(false)
    expect(first.list).toHaveLength(1)

    const second = upsertCursorPreset(first.list, {
      ...base,
      name: '换个名字', // naming must not duplicate the same look
      saved_at: '2026-11-01T00:00:00.000Z',
    })
    expect(second.duplicate).toBe(true)
    expect(second.list).toHaveLength(1)
    // the stored entry keeps the name it already had
    expect(second.name).toBe('默认')
    expect(second.list[0]?.name).toBe('默认')
  })

  it('treats colour case and float noise as the same look', () => {
    const a = cursorPresetKey({ ...base, color: '#4fc3f7' })
    const b = cursorPresetKey({ ...base, color: '4FC3F7', opacity: 0.5500000001 })
    const c = cursorPresetKey({ ...base, color: '#4FC3F7', opacity: 0.55 })
    expect(c).toBe(cursorPresetKey({ ...base, color: '#4FC3F7', opacity: 0.5500001 }))
    expect(a).toBe(cursorPresetKey({ ...base, color: '#4FC3F7' }))
    expect(b).toBe(c) // float noise is not a different preset
    expect(cursorPresetKey({ ...base, opacity: 1 })).not.toBe(c)
  })

  it('keeps genuinely different looks apart', () => {
    // different names AND different looks: three entries, no dedupe
    const byTheme = upsertCursorPreset([base], { ...base, name: '预设-箭头', theme: 'arrow' })
    expect(byTheme.duplicate).toBe(false)
    const byBadge = upsertCursorPreset(byTheme.list, { ...base, name: '预设-无徽标', badge: false })
    expect(byBadge.duplicate).toBe(false)
    expect(byBadge.list).toHaveLength(3)
    const byLabel = upsertCursorPreset(byBadge.list, { ...base, name: '预设-文字', label: '另一个提示' })
    expect(byLabel.duplicate).toBe(false)
    expect(byLabel.list).toHaveLength(4)
  })

  it('replaces by name when the look changed', () => {
    const first = upsertCursorPreset([], base)
    const next = upsertCursorPreset(first.list, { ...base, color: '#FF0044' })
    expect(next.list).toHaveLength(1)
    expect(next.list[0]?.color).toBe('#FF0044')
  })

  it('removes by name, case-insensitively', () => {
    const one = upsertCursorPreset([], base).list
    expect(removeCursorPreset(one, '  默认 ')).toHaveLength(0)
  })
})

describe('菜单主题：同一个主题只留一份', () => {
  const theme: MenuTheme = { name: '主题1', accent: '#4FC3F7', zoom: 1 }

  it('deduplicates on accent + zoom', () => {
    const first = upsertMenuTheme([], theme)
    const again = upsertMenuTheme(first.list, { ...theme, name: '主题2', accent: '#4fc3f7' })
    expect(again.duplicate).toBe(true)
    expect(again.list).toHaveLength(1)
    expect(menuThemeKey({ accent: '#4FC3F7', zoom: 1 })).toBe(menuThemeKey({ accent: '4fc3f7', zoom: 1 }))
  })

  it('names new themes 主题1, 主题2 …', () => {
    expect(nextThemeName([])).toBe('主题1')
    expect(nextThemeName([{ ...theme, name: '主题1' }])).toBe('主题2')
  })
})

describe('命名规则 (requirement 6)', () => {
  it('uses 默认 for the first unnamed cursor', () => {
    expect(cursorDisplayName(0)).toBe('默认')
    expect(cursorDisplayName(0, '  ')).toBe('默认')
  })

  it('uses 自定义1, 自定义2 … for the rest', () => {
    expect(cursorDisplayName(1)).toBe('自定义1')
    expect(cursorDisplayName(2)).toBe('自定义2')
  })

  it('prefers a saved preset name', () => {
    expect(cursorDisplayName(3, '小红')).toBe('小红')
  })

  it('auto-names a save after the label the arrows show', () => {
    expect(autoPresetName(0, [])).toBe('默认')
    expect(autoPresetName(1, [])).toBe('自定义1')
    // a slot that is already taken gets a suffix instead of overwriting it
    expect(autoPresetName(1, [{ ...base, name: '自定义1' }])).toBe('自定义1-2')
  })
})

describe('存储分离 (requirement 4)', () => {
  let dir = ''

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), 'panelstate-'))
  })

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true })
  })

  it('writes menu themes and mouse presets to different files', async () => {
    const themeFile = join(dir, 'menu-themes.json')
    const presetFile = join(dir, 'cursor-presets.json')
    await saveMenuThemes(themeFile, [{ name: '主题1', accent: '#7C5CFF', zoom: 1.15 }])
    await saveCursorPresets(presetFile, [base])

    const themes = await loadMenuThemes(themeFile)
    const presets = await loadCursorPresets(presetFile)
    expect(themes.map((t) => t.name)).toEqual(['主题1'])
    expect(presets.map((p) => p.name)).toEqual(['默认'])
    // a missing file is an empty set, never an error
    expect(await loadMenuThemes(join(dir, 'nope.json'))).toEqual([])
  })

  it('面板命令：同一主题存两次只落一条，且回的是"已存在"', async () => {
    const env = {
      assetDir: dir,
      menuThemesFile: join(dir, 'menu-themes.json'),
      cursorPresetsFile: join(dir, 'cursor-presets.json'),
      status: async () => ({ theme: 'whale', color: '#4FC3F7' }),
    }
    const first = await handlePanelCommand('menu_theme_save', { accent: '#7C5CFF', zoom: 1.2 }, env)
    const second = await handlePanelCommand('menu_theme_save', { accent: '#7c5cff', zoom: 1.2 }, env)
    expect(first?.message).toContain('已保存')
    expect(second?.message).toContain('已存在')
    expect(await loadMenuThemes(env.menuThemesFile)).toHaveLength(1)
  })

  it('面板命令：同一鼠标设置存两次只落一条', async () => {
    const env = {
      assetDir: dir,
      menuThemesFile: join(dir, 'menu-themes.json'),
      cursorPresetsFile: join(dir, 'cursor-presets.json'),
      status: async () => ({
        theme: 'arrow',
        label: 'AI 鼠标',
        color: '#FF0044',
        opacity: 0.8,
        scale: 1.25,
        badge: true,
      }),
    }
    const first = await handlePanelCommand('cursor_preset_save', { index: 0 }, env)
    const second = await handlePanelCommand('cursor_preset_save', { index: 0 }, env)
    expect(first?.message).toContain('已保存')
    expect(second?.message).toContain('没有重复保存')
    const stored = await loadCursorPresets(env.cursorPresetsFile)
    expect(stored).toHaveLength(1)
    expect(stored[0]?.name).toBe('默认')
    expect(stored[0]?.theme).toBe('arrow')
  })

  it('面板命令：应用鼠标预设 -> 返回协议命令，其他动作返回 null', async () => {
    const env = {
      assetDir: dir,
      menuThemesFile: join(dir, 'menu-themes.json'),
      cursorPresetsFile: join(dir, 'cursor-presets.json'),
      status: async () => ({}),
    }
    await handlePanelCommand('cursor_preset_save', { index: 1 }, env)
    const applied = await handlePanelCommand('cursor_preset_apply', { name: '自定义1' }, env)
    expect(applied?.commands?.map(([c]) => c)).toEqual([
      'theme',
      'label',
      'color',
      'opacity',
      'scale',
      'badge',
    ])
    expect(await handlePanelCommand('rest', {}, env)).toBeNull()
  })
})

describe('选中鼠标的路由（面板 -> 宿主 -> 目标光标）', () => {
  it('把一条 patch 翻译成既有协议命令', () => {
    const commands = cursorPatchCommands({ theme: 'dot', color: '#7C5CFF', badge: false })
    expect(commands).toEqual([
      ['theme', { theme: 'dot' }],
      ['color', { hex: '#7C5CFF' }],
      ['badge', { visible: false }],
    ])
    // 不认识的键不会被偷偷翻译成命令
    expect(cursorPatchCommands({ nope: 1 })).toEqual([])
  })

  it('target 支持 self / 名字 / 下标，缺省用当前选中的', () => {
    const names = ['default', 'agent-b', 'agent-c']
    expect(resolveTarget('self', names, 'agent-b', 0)).toBe('agent-b')
    expect(resolveTarget('agent-c', names, 'agent-b', 0)).toBe('agent-c')
    expect(resolveTarget(1, names, 'agent-b', 2)).toBe('agent-b')
    expect(resolveTarget(undefined, names, 'default', 2)).toBe('agent-c')
    expect(resolveTarget('nobody', names, 'default', 0)).toBeUndefined()
  })
})
