import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, isAbsolute, join, resolve } from 'node:path'
import { createInterface, type Interface } from 'node:readline'
import { fileURLToPath } from 'node:url'

export interface OverlayStatus {
  ok: boolean
  visible?: boolean
  x?: number
  y?: number
  target_x?: number
  target_y?: number
  label?: string
  theme?: string
  hwnd?: string
  frames?: number
  fps?: number
  excluded_from_capture?: boolean
  capture_visible?: boolean
  /** Badge under the cursor ("AI 提示"). */
  badge?: boolean
  /** Cursor + badge opacity, 0.15 - 1. */
  opacity?: number
  /** Accent colour, "#RRGGBB". */
  color?: string
  /** Cursor size multiplier, 0.6 - 2. */
  scale?: number
  /** Permission ceiling the agent is held to (0 stopped … 3 admin). */
  permission?: number
  /** True when the permission level is 0 (the overlay is stopped). */
  paused?: boolean
  /** Is the floating control panel on screen? */
  panel?: boolean
  /** Is the panel the full menu bar (true) or the collapsed chip (false)? */
  panel_expanded?: boolean
  panel_hwnd?: string
  /** The human may drag the cursor with their own pointer. */
  drag_enabled?: boolean
  dragging?: boolean
  /** Set when a drag ended: why the agent was stopped. */
  drag_feedback?: string | null
  /** Diagnostics for the grab detection. */
  drag_seen_down?: boolean
  drag_raw?: number
  layer?: string
  layer_hwnd?: string
  layer_title?: string
  layer_topmost?: boolean
  layer_result?: number
  /** Screen rect of the window the cursor is bound to. */
  anchor_rect?: [number, number, number, number]
  /** Cursor position inside that window. */
  anchor_rel?: [number, number]
  anchor_inside?: boolean
  error?: string
}

type Pending = {
  resolve: (value: OverlayStatus) => void
  reject: (error: Error) => void
  timer: ReturnType<typeof setTimeout>
}

const here = dirname(fileURLToPath(import.meta.url))

/**
 * Resolve the overlay executable without hard-coding a machine path:
 * config wins, then `<package>/bin/`, then the dev build under `overlay/target/release`.
 */
export function resolveExePath(configured: string): string {
  if (configured.trim() !== '') {
    if (!isAbsolute(configured)) throw new Error(`exePath must be absolute, got: ${configured}`)
    return configured
  }
  const candidates = [
    join(here, '..', 'bin', 'ai-mouse-overlay.exe'),
    join(here, '..', 'overlay', 'target', 'release', 'ai-mouse-overlay.exe'),
    join(here, '..', '..', 'overlay', 'target', 'release', 'ai-mouse-overlay.exe'),
  ]
  for (const candidate of candidates) {
    const full = resolve(candidate)
    if (existsSync(full)) return full
  }
  return resolve(candidates[0])
}

/**
 * Owns one ai-mouse-overlay child process and speaks its line protocol.
 * A single process paints one cursor per DSH host, which is what a desktop user wants.
 */
export class Overlay {
  private child: ChildProcessWithoutNullStreams | undefined
  private reader: Interface | undefined
  private readonly pending = new Map<string, Pending>()
  private seq = 0
  private starting: Promise<void> | undefined
  private disposed = false
  private spawnCount = 0
  /** The separate minimap process (`--map`); a second instance, its own window. */
  private mapChild: ChildProcessWithoutNullStreams | undefined
  private readonly exe: string

  constructor(exePath: string) {
    this.exe = resolveExePath(exePath)
  }

  get executable(): string {
    return this.exe
  }

  /**
   * Bumped every time the overlay process is spawned. The plugin re-applies its
   * config when this changes: a restarted child starts with defaults, and a
   * stale "already primed" flag would leave the panel pointing at nothing.
   */
  get generation(): number {
    return this.spawnCount
  }

  private async start(): Promise<void> {
    if (this.child) return
    if (this.starting) return this.starting
    if (!existsSync(this.exe)) {
      throw new Error(
        `ai-mouse-overlay.exe not found at ${this.exe}. Build it with ` +
          `"cargo build --release" inside the overlay/ folder, or set exePath in the plugin config.`,
      )
    }

    this.starting = new Promise<void>((resolveStart, rejectStart) => {
      const child = spawn(this.exe, [], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true })
      this.child = child
      this.spawnCount += 1

      const timer = setTimeout(() => {
        rejectStart(new Error(`ai-mouse-overlay did not become ready within 5s (${this.exe})`))
      }, 5000)

      this.reader = createInterface({ input: child.stdout })
      this.reader.on('line', (line) => {
        let msg: OverlayStatus & { id?: string; event?: string }
        try {
          msg = JSON.parse(line) as OverlayStatus & { id?: string; event?: string }
        } catch {
          return
        }
        if (msg.event === 'ready') {
          clearTimeout(timer)
          resolveStart()
          return
        }
        const id = msg.id === undefined ? undefined : String(msg.id)
        if (id === undefined) return
        const pending = this.pending.get(id)
        if (!pending) return
        this.pending.delete(id)
        clearTimeout(pending.timer)
        pending.resolve(msg)
      })

      child.stderr.on('data', (chunk: Buffer) => {
        // stderr carries only diagnostics; never let it break a tool call.
        if (process.env.DSH_AI_CURSOR_DEBUG) process.stderr.write(`[ai-mouse] ${chunk}`)
      })

      child.on('error', (error) => {
        clearTimeout(timer)
        rejectStart(error instanceof Error ? error : new Error(String(error)))
      })

      child.on('exit', (code) => {
        this.child = undefined
        const failure = new Error(`ai-mouse-overlay exited (code ${String(code)})`)
        for (const [id, pending] of this.pending) {
          clearTimeout(pending.timer)
          pending.reject(failure)
          this.pending.delete(id)
        }
      })
    }).finally(() => {
      this.starting = undefined
    })

    return this.starting
  }

  /** Send one command and wait for its matching reply. */
  async request(
    cmd: string,
    payload: Record<string, unknown> = {},
    timeoutMs = 3000,
  ): Promise<OverlayStatus> {
    if (this.disposed) throw new Error('ai-mouse-overlay host is disposed')
    await this.start()
    const child = this.child
    if (!child) throw new Error('ai-mouse-overlay is not running')

    const id = String(++this.seq)
    return new Promise<OverlayStatus>((resolveReply, rejectReply) => {
      const timer = setTimeout(() => {
        this.pending.delete(id)
        rejectReply(new Error(`ai-mouse-overlay did not answer "${cmd}" within ${timeoutMs}ms`))
      }, timeoutMs)
      this.pending.set(id, { resolve: resolveReply, reject: rejectReply, timer })
      child.stdin.write(`${JSON.stringify({ id, cmd, ...payload })}\n`)
    })
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    const child = this.child
    if (!child) return
    try {
      child.stdin.write(`${JSON.stringify({ cmd: 'quit' })}\n`)
    } catch {
      /* already gone */
    }
    const killTimer = setTimeout(() => {
      if (this.child === child) child.kill()
    }, 500)
    killTimer.unref?.()
    this.reader?.close()
    this.reader = undefined
    this.child = undefined
  }

  /**
   * Start or stop the separate minimap process.
   *
   * It is the SAME binary with `--map`, so it reuses the window class that already
   * works instead of needing a new one. Two processes is what keeps the minimap
   * and the menu bar in separate blocks.
   */
  async setMap(visible: boolean): Promise<boolean> {
    if (visible) {
      if (this.mapChild) return true
      const child = spawn(this.exe, ['--map'], {
        stdio: ['pipe', 'pipe', 'pipe'],
        windowsHide: true,
      })
      this.mapChild = child
      child.on('exit', () => {
        if (this.mapChild === child) this.mapChild = undefined
      })
      child.on('error', () => {
        if (this.mapChild === child) this.mapChild = undefined
      })
      // drain so the child never blocks on a full pipe
      child.stdout.resume()
      child.stderr.resume()
      return true
    }
    const child = this.mapChild
    this.mapChild = undefined
    if (child) {
      child.kill()
      return true
    }
    return false
  }

  /** Is the separate minimap window running? */
  get mapRunning(): boolean {
    return this.mapChild !== undefined
  }
}
