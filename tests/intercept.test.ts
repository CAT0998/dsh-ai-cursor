import { describe, expect, it } from 'vitest'
import {
  DEFAULT_MOUSE_TOOLS,
  INTERCEPT_MODES,
  extractPoint,
  interceptReason,
  matchesTool,
  normalizeMode,
} from '../src/intercept'

describe('matchesTool', () => {
  it('matches exact names', () => {
    expect(matchesTool('computer_click', DEFAULT_MOUSE_TOOLS)).toBe(true)
    expect(matchesTool('cua_driver_native__drag', DEFAULT_MOUSE_TOOLS)).toBe(true)
  })

  it('leaves unrelated tools alone', () => {
    expect(matchesTool('computer_screenshot', DEFAULT_MOUSE_TOOLS)).toBe(false)
    expect(matchesTool('computer_ui_action', DEFAULT_MOUSE_TOOLS)).toBe(false)
    expect(matchesTool('ai_mouse_move', DEFAULT_MOUSE_TOOLS)).toBe(false)
    expect(matchesTool('read', DEFAULT_MOUSE_TOOLS)).toBe(false)
  })

  it('honours a trailing * as a prefix pattern', () => {
    expect(matchesTool('cua_driver_native__click', ['cua_driver_native__*'])).toBe(true)
    expect(matchesTool('computer_click', ['cua_driver_native__*'])).toBe(false)
  })
})

describe('extractPoint', () => {
  it('reads a flat x/y pair', () => {
    expect(extractPoint({ x: 10, y: 20 })).toEqual({ x: 10, y: 20 })
  })

  it('reads drag corners', () => {
    expect(extractPoint({ from_x: 5, from_y: 6, to_x: 7, to_y: 8 })).toEqual({ x: 5, y: 6 })
    expect(extractPoint({ fromX: 1, fromY: 2, toX: 3, toY: 4 })).toEqual({ x: 1, y: 2 })
  })

  it('reads an [x, y] pair', () => {
    expect(extractPoint({ at: [300, 400] })).toEqual({ x: 300, y: 400 })
    expect(extractPoint({ position: [11, 12] })).toEqual({ x: 11, y: 12 })
  })

  it('reads a nested object', () => {
    expect(extractPoint({ position: { x: 21, y: 22 } })).toEqual({ x: 21, y: 22 })
  })

  it('falls through computer_actions steps to the first located step', () => {
    const args = { steps: [{ action: 'key', keys: 'enter' }, { action: 'click', x: 640, y: 480 }] }
    expect(extractPoint(args)).toEqual({ x: 640, y: 480 })
  })

  it('returns undefined when there is no screen point', () => {
    expect(extractPoint({})).toBeUndefined()
    expect(extractPoint({ text: 'hello' })).toBeUndefined()
    expect(extractPoint(null)).toBeUndefined()
    expect(extractPoint({ x: 'nope', y: 3 })).toBeUndefined()
    expect(extractPoint({ x: Number.NaN, y: 3 })).toBeUndefined()
  })
})

describe('interceptReason', () => {
  it('says plainly that the real action did not happen in redirect mode', () => {
    const text = interceptReason({ mode: 'redirect', tool: 'computer_click', point: { x: 1.4, y: 2.6 } })
    expect(text).toContain('computer_click')
    expect(text).toContain('(1, 3)')
    expect(text).toContain('真实操作未执行')
    expect(text).toContain('mirror')
    expect(text).toContain('off')
  })

  it('does not claim the action failed in mirror mode', () => {
    const text = interceptReason({ mode: 'mirror', tool: 'computer_click', point: { x: 1, y: 2 } })
    expect(text).toContain('真实操作已照常执行')
    expect(text).not.toContain('未执行')
  })

  it('offers the ai_mouse_* alternative in refuse mode', () => {
    const text = interceptReason({ mode: 'refuse', tool: 'cua_driver_native__click' })
    expect(text).toContain('cua_driver_native__click')
    expect(text).toContain('未知坐标')
    expect(text).toContain('ai_mouse_*')
  })
})

describe('normalizeMode', () => {
  it('accepts every real mode', () => {
    for (const mode of INTERCEPT_MODES) expect(normalizeMode(mode)).toBe(mode)
  })

  it('falls back on anything else', () => {
    expect(normalizeMode('nope')).toBe('off')
    expect(normalizeMode(undefined)).toBe('off')
    expect(normalizeMode('nope', 'refuse')).toBe('refuse')
  })
})
