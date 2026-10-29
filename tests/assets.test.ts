import { describe, expect, it } from 'vitest'
import {
  IMAGE_EXTENSIONS,
  defaultAssetDir,
  hasImageExtension,
  listAssets,
  resolveSource,
} from '../src/assets'

const ASSETS = 'C:\\Users\\me\\.dsh\\ai-mouse-assets'
const never = () => false
const always = () => true

describe('hasImageExtension', () => {
  it('accepts every advertised format regardless of case', () => {
    for (const ext of IMAGE_EXTENSIONS) {
      expect(hasImageExtension(`cursor.${ext}`)).toBe(true)
      expect(hasImageExtension(`cursor.${ext.toUpperCase()}`)).toBe(true)
    }
  })

  it('rejects files it cannot decode', () => {
    expect(hasImageExtension('notes.txt')).toBe(false)
    expect(hasImageExtension('noextension')).toBe(false)
    expect(hasImageExtension('archive.png.zip')).toBe(false)
  })
})

describe('resolveSource', () => {
  it('maps the builtin sentinels to the vector artwork', () => {
    expect(resolveSource('builtin', ASSETS, never)).toEqual({ wire: 'builtin', kind: 'builtin' })
    expect(resolveSource('THEME', ASSETS, never)).toEqual({ wire: 'builtin', kind: 'builtin' })
  })

  it('turns the slot off for the off sentinels', () => {
    for (const value of ['', 'none', 'off', '  ']) {
      expect(resolveSource(value, ASSETS, never)).toEqual({ wire: '', kind: 'off' })
    }
    expect(resolveSource(undefined, ASSETS, never)).toEqual({ wire: '', kind: 'off' })
  })

  it('resolves a bare file name inside the asset folder', () => {
    const r = resolveSource('spin.gif', ASSETS, always)
    expect(r.kind).toBe('file')
    expect(r.wire).toBe(`${ASSETS}\\spin.gif`)
  })

  it('uses an absolute path as-is', () => {
    const p = 'D:\\art\\cursor.png'
    expect(resolveSource(p, ASSETS, always)).toEqual({ wire: p, kind: 'file' })
  })

  it('explains a miss instead of silently doing nothing', () => {
    const bare = resolveSource('missing.gif', ASSETS, never)
    expect(bare.kind).toBe('missing')
    expect(bare.error).toContain('素材目录')
    expect(bare.error).toContain('missing.gif')

    const absolute = resolveSource('D:\\art\\gone.png', ASSETS, never)
    expect(absolute.kind).toBe('missing')
    expect(absolute.error).toContain('文件不存在')
  })
})

describe('listAssets', () => {
  const deps = (names: string[]) => ({
    readdir: () => names,
    stat: () => ({ size: 2048, mtimeMs: 1000 }),
  })

  it('lists only pictures', () => {
    const out = listAssets(ASSETS, deps(['a.png', 'notes.txt', 'b.gif', 'c.txt']))
    expect(out.map((e) => e.name).sort()).toEqual(['a.png', 'b.gif'])
  })

  it('returns an empty list when the folder is unreadable', () => {
    expect(
      listAssets(ASSETS, {
        readdir: () => {
          throw new Error('ENOENT')
        },
        stat: () => ({ size: 0, mtimeMs: 0 }),
      }),
    ).toEqual([])
  })

  it('sorts newest first and skips entries that vanished', () => {
    const out = listAssets(ASSETS, {
      readdir: () => ['old.png', 'new.png', 'gone.png'],
      stat: (p) => {
        if (p.endsWith('gone.png')) throw new Error('ENOENT')
        return { size: 1, mtimeMs: p.endsWith('new.png') ? 2000 : 1000 }
      },
    })
    expect(out.map((e) => e.name)).toEqual(['new.png', 'old.png'])
  })
})

describe('defaultAssetDir', () => {
  it('lives under .dsh', () => {
    expect(defaultAssetDir('C:\\Users\\me')).toContain('ai-mouse-assets')
    expect(defaultAssetDir('C:\\Users\\me')).toContain('.dsh')
  })
})
