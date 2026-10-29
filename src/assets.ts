/**
 * Where custom artwork comes from.
 *
 * Two entry points, as asked for: a bare file name resolves inside the asset
 * folder (so dropping a GIF into `~/.dsh/ai-mouse-assets` is the whole workflow),
 * and an absolute path is taken as-is (that is what the panel's file picker will
 * hand back). Built-in sentinels keep the vector artwork reachable.
 */
import { existsSync, readdirSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { isAbsolute, join, resolve } from 'node:path'

/** `builtin` = the vector artwork for the current theme; `none`/`` = slot off. */
export const BUILTIN_SOURCES = ['builtin', 'theme'] as const
export const OFF_SOURCES = ['', 'none', 'off'] as const

export const IMAGE_EXTENSIONS = [
  'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'ico', 'tif', 'tiff', 'svg',
] as const

export interface ResolvedSource {
  /** What to send over the wire: a sentinel or an absolute path. */
  wire: string
  kind: 'builtin' | 'off' | 'file' | 'missing'
  /** Human-readable explanation when kind is `missing`. */
  error?: string
}

export function defaultAssetDir(home: string = homedir()): string {
  return join(home, '.dsh', 'ai-mouse-assets')
}

export function hasImageExtension(name: string): boolean {
  const dot = name.lastIndexOf('.')
  if (dot < 0) return false
  const ext = name.slice(dot + 1).toLowerCase()
  return (IMAGE_EXTENSIONS as readonly string[]).includes(ext)
}

/**
 * Turn whatever the caller passed into something the overlay understands.
 * `exists` is injected so this stays a pure decision in tests.
 */
export function resolveSource(
  source: string | undefined,
  assetDir: string,
  exists: (path: string) => boolean = existsSync,
): ResolvedSource {
  const raw = (source ?? '').trim()
  if (raw === '') return { wire: '', kind: 'off' }
  const lower = raw.toLowerCase()
  if ((BUILTIN_SOURCES as readonly string[]).includes(lower)) {
    return { wire: 'builtin', kind: 'builtin' }
  }
  if ((OFF_SOURCES as readonly string[]).includes(lower)) return { wire: '', kind: 'off' }

  const path = isAbsolute(raw) ? resolve(raw) : resolve(assetDir, raw)
  if (exists(path)) return { wire: path, kind: 'file' }
  return {
    wire: path,
    kind: 'missing',
    error: isAbsolute(raw)
      ? `文件不存在：${path}`
      : `在素材目录里找不到「${raw}」（${assetDir}）。支持 ${IMAGE_EXTENSIONS.join(' / ')}。`,
  }
}

export interface AssetEntry {
  name: string
  size: number
  modified: number
}

/** Every picture in the asset folder, newest first. Unreadable folders read as empty. */
export function listAssets(
  assetDir: string,
  deps: { readdir: (p: string) => string[]; stat: (p: string) => { size: number; mtimeMs: number } } = {
    readdir: (p) => readdirSync(p),
    stat: (p) => {
      const s = statSync(p)
      return { size: s.size, mtimeMs: s.mtimeMs }
    },
  },
): AssetEntry[] {
  let names: string[]
  try {
    names = deps.readdir(assetDir)
  } catch {
    return []
  }
  const out: AssetEntry[] = []
  for (const name of names) {
    if (!hasImageExtension(name)) continue
    try {
      const s = deps.stat(join(assetDir, name))
      out.push({ name, size: s.size, modified: s.mtimeMs })
    } catch {
      /* vanished between readdir and stat */
    }
  }
  return out.sort((a, b) => b.modified - a.modified)
}
