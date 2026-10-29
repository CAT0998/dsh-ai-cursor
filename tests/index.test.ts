import { describe, expect, it } from 'vitest'
import { describeStatus } from '../src/index'
import { resolveExePath } from '../src/overlay'
import { LEVELS, denial, levelInfo, levelName, normalizeLevel, permits } from '../src/permissions'

describe('describeStatus', () => {
  it('renders a visible cursor with rounded coordinates', () => {
    const text = describeStatus({
      ok: true,
      visible: true,
      x: 900.4,
      y: 520.6,
      label: 'AI 鼠标',
      theme: 'whale',
      hwnd: '0x6E031E',
      fps: 34.12,
      excluded_from_capture: true,
    })
    expect(text).toContain('visible at (900, 521)')
    expect(text).toContain('theme=whale')
    expect(text).toContain('label=AI 鼠标')
    expect(text).toContain('fps=34.1')
    expect(text).toContain('excluded_from_capture=true')
  })

  it('renders a hidden cursor', () => {
    expect(describeStatus({ ok: true, visible: false, x: 0, y: 0 })).toContain('hidden at (0, 0)')
  })

  it('renders errors without pretending to be a position', () => {
    expect(describeStatus({ ok: false, error: 'no exe' })).toBe('AI 鼠标 error: no exe')
  })

  it('surfaces the appearance and permission state', () => {
    const text = describeStatus({
      ok: true,
      visible: true,
      opacity: 0.55,
      color: '#7C5CFF',
      scale: 1.5,
      badge: false,
      permission: 2,
      panel: true,
      panel_expanded: false,
    })
    expect(text).toContain('opacity=55%')
    expect(text).toContain('color=#7C5CFF')
    expect(text).toContain('scale=150%')
    expect(text).toContain('badge=off')
    expect(text).toContain('permission=操作(2)')
    expect(text).toContain('panel=chip')
  })

  it('defaults to an opaque, badged, admin cursor when the reply is sparse', () => {
    const text = describeStatus({ ok: true, visible: true })
    expect(text).toContain('opacity=100%')
    expect(text).toContain('badge=on')
    expect(text).toContain('scale=100%')
    expect(text).toContain('permission=管理员(3)')
    expect(text).toContain('panel=off')
  })
})

describe('permission ladder', () => {
  it('normalizes anything into a real level', () => {
    expect(normalizeLevel(0)).toBe(0)
    expect(normalizeLevel(2)).toBe(2)
    expect(normalizeLevel(3)).toBe(3)
    expect(normalizeLevel(-4)).toBe(0)
    expect(normalizeLevel(99)).toBe(3)
    expect(normalizeLevel(1.9)).toBe(1)
    expect(normalizeLevel(undefined)).toBe(3)
    expect(normalizeLevel('2')).toBe(2)
    expect(normalizeLevel(undefined, 1)).toBe(1)
  })

  it('only permits what the level covers', () => {
    expect(permits(0, 0)).toBe(true)
    expect(permits(0, 1)).toBe(false)
    expect(permits(1, 1)).toBe(true)
    expect(permits(1, 2)).toBe(false)
    expect(permits(2, 2)).toBe(true)
    expect(permits(2, 3)).toBe(false)
    expect(permits(3, 3)).toBe(true)
  })

  it('names every level in both languages', () => {
    expect(levelName(0)).toBe('停止')
    expect(levelName(1)).toBe('只读')
    expect(levelName(2)).toBe('操作')
    expect(levelName(3)).toBe('管理员')
    expect(levelName(3, 'en')).toBe('admin')
    expect(LEVELS).toHaveLength(4)
    for (const l of LEVELS) expect(l.capability.length).toBeGreaterThan(0)
  })

  it('explains a refusal and who can lift it', () => {
    const text = denial(1, 3, 'ai_mouse_color')
    expect(text).toContain('ai_mouse_color')
    expect(text).toContain('管理员')
    expect(text).toContain('只读')
    expect(text).toContain('Permission denied')
    expect(levelInfo(3).zh).toBe('管理员')
  })
})

describe('resolveExePath', () => {
  it('honours an absolute configured path', () => {
    const exe = 'C:\\tools\\ai-mouse-overlay.exe'
    expect(resolveExePath(exe)).toBe(exe)
  })

  it('refuses a relative configured path', () => {
    expect(() => resolveExePath('bin/ai-mouse-overlay.exe')).toThrow(/must be absolute/)
  })
})
