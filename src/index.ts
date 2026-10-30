import type { Context } from '@deepseek-ai/cordis'
import { defineTool } from '@deepseek-ai/dsh-tools'
import { Config } from './config'
import type { Config as ConfigShape } from './config'
import { Overlay, type OverlayStatus } from './overlay'
import { denial, levelInfo, levelName, normalizeLevel, permits } from './permissions'
import {
  DEFAULT_MOUSE_TOOLS,
  extractPoint,
  interceptReason,
  matchesTool,
  normalizeMode,
} from './intercept'
import type { InterceptMode } from './intercept'
import { defaultAssetDir, listAssets, resolveSource } from './assets'
import type { AssetEntry } from './assets'

export const name = 'dsh-ai-cursor'
export const inject = ['tools']

// Re-export the schema so the loader can validate this plugin's config.
export { Config }
export { Overlay, resolveExePath } from './overlay'
export { LEVELS, levelInfo, levelName, normalizeLevel, permits, denial } from './permissions'
export {
  DEFAULT_MOUSE_TOOLS,
  INTERCEPT_MODES,
  extractPoint,
  interceptReason,
  matchesTool,
  normalizeMode,
} from './intercept'
import {
  capturePreset,
  describePreset,
  findPreset,
  loadPresets,
  presetCommands,
  removePreset,
  savePresets,
  upsertPreset,
} from './presets'
export type { InterceptMode } from './intercept'
export type { OverlayStatus } from './overlay'

/** One-line human/machine readable rendering of an overlay reply (pure, testable). */
export function describeStatus(status: OverlayStatus): string {
  if (!status.ok) return `AI 鼠标 error: ${status.error ?? 'unknown'}`
  const x = status.x ?? 0
  const y = status.y ?? 0
  const parts = [
    `AI 鼠标 ${status.visible ? 'visible' : 'hidden'} at (${Math.round(x)}, ${Math.round(y)})`,
    `theme=${status.theme ?? '?'}`,
    `label=${status.label ?? '?'}`,
    `hwnd=${status.hwnd ?? '?'}`,
    `fps=${(status.fps ?? 0).toFixed(1)}`,
    `opacity=${Math.round((status.opacity ?? 1) * 100)}%`,
    `color=${status.color ?? '?'}`,
    `scale=${Math.round((status.scale ?? 1) * 100)}%`,
    `badge=${status.badge === false ? 'off' : 'on'}`,
    `permission=${levelName(status.permission ?? 3)}(${normalizeLevel(status.permission, 3)})`,
    `panel=${status.panel ? (status.panel_expanded === false ? 'chip' : 'full') : 'off'}`,
    `drag=${status.drag_enabled === false ? 'off' : 'on'}${status.dragging ? '/dragging' : ''}`,
    `press=${status.drag_seen_down ? 'seen' : 'no'}`,
  ]
  if (status.anchor_rect) {
    parts.push(
      `anchor=${status.anchor_rect[2]}x${status.anchor_rect[3]}@(${status.anchor_rect[0]},${status.anchor_rect[1]})`,
      `in_window=${status.anchor_rel ? `(${status.anchor_rel[0]},${status.anchor_rel[1]})` : '?'}${status.anchor_inside ? '' : ' outside'}`,
    )
  }
  if (status.drag_feedback) parts.push(`note=${status.drag_feedback}`)
  if (status.excluded_from_capture) parts.push('excluded_from_capture=true')
  return parts.join(' ')
}

const TEXT_OUTPUT = {
  schema: { type: 'string' as const },
  render: (_args: unknown, value: string) => [{ type: 'text' as const, text: value }],
}

export function apply(ctx: Context, config: ConfigShape): void {
  if (!config.enabled) return

  const overlay = new Overlay(config.exePath)
  ctx.effect(() => () => overlay.dispose())

  /**
   * Last permission level the overlay reported. Every reply carries it, so this
   * stays fresh without polling; `gate()` refreshes it explicitly when the cached
   * value is too low, because a human may have raised the ceiling in the panel.
   */
  const state = {
    permission: normalizeLevel(config.permission, 3),
    intercept: normalizeMode(config.intercept, 'off') as InterceptMode,
    interceptTools: [...config.interceptTools],
  }

  const send = async (
    cmd: string,
    payload: Record<string, unknown> = {},
    timeoutMs?: number,
  ): Promise<OverlayStatus> => {
    const reply = await overlay.request(cmd, payload, timeoutMs)
    if (typeof reply.permission === 'number') {
      state.permission = normalizeLevel(reply.permission, 3)
    }
    return reply
  }

  // ---------------------------------------------------------------- intercept
  //
  // Other plugins drive the real pointer. `tools/pre-execute` is the only honest
  // seam: it can allow/deny, never rewrite. So "redirect" moves the AI cursor and
  // then denies the real call — the agent is told the real action did not happen,
  // instead of being left to believe it clicked something.
  ctx.on('tools/pre-execute', async (exec, next) => {
    if (state.intercept === 'off') return next()
    const name = exec.name
    if (name.startsWith('ai_mouse_')) return next()
    if (!matchesTool(name, state.interceptTools)) return next()
    if (exec.signal?.aborted) return next()

    const point = extractPoint(exec.arguments)

    if (state.intercept === 'mirror') {
      if (point) {
        void send('move', { x: point.x, y: point.y, duration_ms: 200 }).catch(() => {})
      }
      return next()
    }

    if (state.intercept === 'redirect' && point) {
      // Walk the AI cursor there first so the human sees where the blocked call
      // was aimed; a failure to draw must not change the decision.
      try {
        await send('move', { x: point.x, y: point.y, duration_ms: 200 })
      } catch {
        /* the overlay being down does not make the real call acceptable */
      }
    }
    return {
      kind: 'deny',
      reason: interceptReason({ mode: state.intercept, tool: name, point }),
      info: { name: 'ai-mouse-intercept', code: 'REAL_MOUSE_BLOCKED' },
    }
  })

  const refreshLevel = async (): Promise<number> => {
    try {
      const reply = await overlay.request('status')
      if (typeof reply.permission === 'number') {
        state.permission = normalizeLevel(reply.permission, 3)
      }
    } catch {
      /* keep the cached value; the overlay enforces the rule itself either way */
    }
    return state.permission
  }

  /** Returns the refusal text when the agent is not allowed to do this, else undefined. */
  const gate = async (need: number, action: string): Promise<string | undefined> => {
    // Fast path: the cached level already allows it. The overlay re-checks the
    // rule on its side, so a stale cache cannot escalate anything.
    if (state.permission >= need) return undefined
    const now = await refreshLevel()
    return permits(now, need) ? undefined : denial(now, need, action)
  }

  // First touch applies the configured look so a plain move already looks right.
  // `permission` is applied last: lowering it earlier would block the rest.
  let primedGeneration = -1
  const prime = async (): Promise<void> => {
    // Re-apply whenever the child process is replaced: a fresh process starts
    // with defaults, so the old "primed" state must not short-circuit.
    if (primedGeneration === overlay.generation) return
    primedGeneration = overlay.generation
    await send('idle', { idle_ms: config.idleMs })
    await send('affinity', { capture_visible: config.captureVisible })
    await send('label', { text: config.label })
    await send('theme', { theme: config.theme })
    await send('badge', { visible: config.badge })
    await send('opacity', { value: config.opacity })
    await send('color', { hex: config.color })
    await send('scale', { value: config.scale })
    await send('assets', { dir: config.assetDir })
    await send('panel_expanded', { expanded: config.panelExpanded })
    await send('panel', { visible: config.panel })
    await send('permission', { level: config.permission })
  }

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_show',
      description:
        'Show the AI cursor overlay: a second, agent-owned cursor drawn on top of the desktop with its own badge. It never moves the real mouse pointer and never takes focus.',
      parameters: {},
      output: TEXT_OUTPUT,
      async execute() {
        const refused = await gate(1, 'ai_mouse_show')
        if (refused) return refused
        await prime()
        return describeStatus(await send('show'))
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_hide',
      description: 'Hide the AI cursor overlay.',
      parameters: {},
      output: TEXT_OUTPUT,
      async execute() {
        const refused = await gate(1, 'ai_mouse_hide')
        if (refused) return refused
        return describeStatus(await send('hide'))
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_move',
      description:
        'Move the AI cursor overlay to absolute screen coordinates (device pixels). The overlay glides there; the real mouse pointer is not touched.',
      parameters: {
        x: { type: 'number', required: true, description: 'Absolute screen X in device pixels.' },
        y: { type: 'number', required: true, description: 'Absolute screen Y in device pixels.' },
        duration_ms: {
          type: 'number',
          description: 'Glide duration in milliseconds (default 320). 0 jumps.',
        },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        const refused = await gate(2, 'ai_mouse_move')
        if (refused) return refused
        await prime()
        return describeStatus(
          await send('move', {
            x: args.x,
            y: args.y,
            duration_ms: args.duration_ms ?? 320,
          }),
        )
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_label',
      description: 'Change the badge text shown under the AI cursor (default "AI 鼠标").',
      parameters: {
        text: {
          type: 'string',
          required: true,
          description: 'Badge text, e.g. "AI 鼠标" or a session name. Empty hides the badge.',
        },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        const refused = await gate(3, 'ai_mouse_label')
        if (refused) return refused
        return describeStatus(await send('label', { text: args.text }))
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_theme',
      description:
        'Switch the AI cursor artwork: a whale (default), a plain dot, or a classic arrow pointer.',
      parameters: {
        theme: { type: 'string', required: true, description: 'whale | dot | arrow' },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        const refused = await gate(3, 'ai_mouse_theme')
        if (refused) return refused
        const theme = args.theme === 'dot' || args.theme === 'arrow' ? args.theme : 'whale'
        return describeStatus(await send('theme', { theme }))
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_capture_visible',
      description:
        "Let the AI cursor appear in screenshots and recordings (off by default so the agent's own captures stay clean). Useful for demos and tutorials.",
      parameters: {
        enabled: { type: 'boolean', required: true, description: 'true = visible in captures.' },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        await send('affinity', { capture_visible: args.enabled })
        // Both windows are recreated on a capture-mode change, so read back until
        // the state actually matches instead of reporting a hopeful reply.
        for (let i = 0; i < 40; i++) {
          const status = await send('status')
          if ((status.capture_visible ?? false) === args.enabled) return describeStatus(status)
          await new Promise((r) => setTimeout(r, 25))
        }
        return describeStatus(await send('status'))
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_status',
      description:
        'Read back the AI cursor state: visibility, position, label, theme, size, opacity, colour, badge, permission level, panel state, frame rate.',
      parameters: {},
      output: TEXT_OUTPUT,
      async execute() {
        return describeStatus(await send('status'))
      },
    }),
  )

  // ---------------------------------------------------------------- panel ---

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_panel',
      description:
        'Show, hide, expand or collapse the floating control panel on the desktop. The panel is where a human sets the permission level, opacity, colour, cursor style, and can stop or start the AI cursor. Use it to hand control back to the user.',
      parameters: {
        visible: { type: 'boolean', description: 'Show (true) or hide (false) the panel.' },
        expanded: {
          type: 'boolean',
          description: 'Full menu bar (true) or the small chip collapsed at the screen edge (false).',
        },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        const refused = await gate(1, 'ai_mouse_panel')
        if (refused) return refused
        await prime()
        if (args.expanded !== undefined) await send('panel_expanded', { expanded: args.expanded })
        if (args.visible !== undefined) await send('panel', { visible: args.visible })
        return describeStatus(await send('status'))
      },
    }),
  )

  // ----------------------------------------------------------- appearance ---

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_opacity',
      description:
        'Set the AI cursor and badge opacity from 0.15 (nearly invisible) to 1 (fully opaque).',
      parameters: {
        value: { type: 'number', required: true, description: 'Opacity, 0.15 - 1.' },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        const refused = await gate(3, 'ai_mouse_opacity')
        if (refused) return refused
        return describeStatus(await send('opacity', { value: args.value }))
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_color',
      description:
        'Set the accent colour of the AI cursor, its badge rim and the panel highlights, as #RRGGBB.',
      parameters: {
        hex: { type: 'string', required: true, description: 'Colour as #RRGGBB, e.g. #4FC3F7.' },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        const refused = await gate(3, 'ai_mouse_color')
        if (refused) return refused
        return describeStatus(await send('color', { hex: args.hex }))
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_scale',
      description: 'Scale the AI cursor artwork between 0.6x and 2x.',
      parameters: {
        value: { type: 'number', required: true, description: 'Size multiplier, 0.6 - 2.' },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        const refused = await gate(3, 'ai_mouse_scale')
        if (refused) return refused
        return describeStatus(await send('scale', { value: args.value }))
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_badge',
      description: 'Show or hide the "AI 鼠标" badge under the AI cursor ("AI 提示").',
      parameters: {
        visible: { type: 'boolean', required: true, description: 'true = show the badge.' },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        const refused = await gate(3, 'ai_mouse_badge')
        if (refused) return refused
        return describeStatus(await send('badge', { visible: args.visible }))
      },
    }),
  )

  // ----------------------------------------------------------- permission ---

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_permission',
      description:
        'Read or lower the permission level that caps what this agent may do with the AI cursor: 0 stopped, 1 read-only, 2 drive, 3 admin. The agent can only lower it — a human has to raise it again in the floating panel.',
      parameters: {
        level: {
          type: 'number',
          description: 'Target level 0-3. Omit to just read the current level.',
        },
      },
      output: TEXT_OUTPUT,
      async execute(args) {
        const refused = await gate(3, 'ai_mouse_permission')
        if (refused) return refused
        if (args.level === undefined) return describeStatus(await send('status'))
        const target = normalizeLevel(args.level, 3)
        const current = await refreshLevel()
        if (target > current) {
          return denial(current, target, 'ai_mouse_permission (raising the level)')
        }
        const reply = await send('permission', { level: target })
        const info = levelInfo(target)
        return `${describeStatus(reply)}\n权限已降为「${info.zh}」(${info.level})：${info.capability}。只有人能在浮动面板上把它调回去。`
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_intercept',
      description:
        "Read or set what happens to other plugins' real-mouse tools (computer_click, cua_driver_native__click, ...). off leaves them alone; refuse denies them; redirect moves the AI cursor to the target and denies the real call (the real click does NOT happen); mirror lets the real call through and follows it with the AI cursor. Report which of the configured tool names actually exist with `describe: true`.",
      parameters: {
        mode: {
          type: 'string',
          description: 'off | refuse | redirect | mirror. Omit to just read the current mode.',
        },
        add_tools: {
          type: 'array',
          items: { type: 'string' },
          description: 'Extra tool names (or `prefix*` patterns) to treat as real-mouse tools.',
        },
        describe: {
          type: 'boolean',
          description: 'Also report which configured names are currently registered.',
        },
      },
      output: TEXT_OUTPUT,
      async execute(args: { mode?: string; add_tools?: string[]; describe?: boolean }) {
        if (args.mode !== undefined) {
          const refused = await gate(3, 'ai_mouse_intercept')
          if (refused) return refused
          state.intercept = normalizeMode(args.mode, state.intercept)
        }
        if (args.add_tools !== undefined) {
          const refused = await gate(3, 'ai_mouse_intercept')
          if (refused) return refused
          for (const extra of args.add_tools) {
            if (typeof extra === 'string' && extra.trim() !== '' && !state.interceptTools.includes(extra.trim())) {
              state.interceptTools.push(extra.trim())
            }
          }
        }
        const lines = [
          `拦截模式 = ${state.intercept}`,
          `监视的鼠标工具（${state.interceptTools.length}）: ${state.interceptTools.join(', ')}`,
        ]
        if (args.describe) {
          const missing = state.interceptTools.filter((name) => ctx.tools.get(name) === undefined)
          lines.push(
            missing.length === 0
              ? '全部已注册 ✓'
              : `未注册（不会命中）: ${missing.join(', ')}`,
          )
        }
        if (state.intercept === 'off') {
          lines.push('当前放行：其他插件的真实鼠标操作不会被拦截。')
        } else if (state.intercept === 'mirror') {
          lines.push('当前 mirror：真实操作照常执行，AI 光标同步跟随（只用于显示）。')
        } else {
          lines.push(
            `当前 ${state.intercept}：命中的调用会被拒绝，真实点击/拖拽不会发生；` +
              'redirect 会先把 AI 光标移到目标点。',
          )
        }
        return lines.join('\n')
      },
    }),
  )

  // ------------------------------------------------------------- artwork ---

  const SLOT_NAMES = ['cursor', 'hint', 'ornament'] as const

  const slotSummary = (status: OverlayStatus): string => {
    const slots = (status as unknown as { slots?: Record<string, Record<string, unknown>> }).slots
    if (!slots) return '（浮层未返回图案状态）'
    return SLOT_NAMES.map((name) => {
      const s = slots[name]
      if (!s) return `${name}=?`
      const src = String(s.source ?? '')
      const shown = src === 'builtin' ? 'vector' : src === '' ? 'off' : src.split(/[\\/]/).pop()
      const frames = typeof s.frames === 'number' ? `/${s.frames}f` : ''
      const err = s.error ? ` ERROR(${String(s.error)})` : ''
      return `${name}=${shown}${frames}${err}`
    }).join('  ')
  }

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_sprite',
      description:
        'Set the artwork of one picture slot: `cursor` (the pointer itself), `hint` (the "AI 鼠标" badge) or `ornament` (a decoration attached to the cursor). `source` accepts an absolute image path, a bare file name inside the asset folder, "builtin" for the vector artwork, or "none" to switch the slot off. Formats: png / jpg / gif (animated) / webp / bmp / ico / tif / tiff / svg. Transparency is preserved per pixel.',
      parameters: {
        slot: { type: 'string', required: true, description: 'cursor | hint | ornament' },
        source: {
          type: 'string',
          description: 'Absolute path, file name in the asset folder, "builtin", or "none".',
        },
        size: { type: 'number', description: 'Target width in device pixels; 0 keeps the natural size.' },
        offset: {
          type: 'array',
          items: { type: 'number' },
          description: 'Where the hotspot sits relative to the cursor tip, [x, y] device px.',
        },
        hotspot: {
          type: 'array',
          items: { type: 'number' },
          description: 'Which point of the image lands on the offset, normalised [0..1, 0..1].',
        },
        opacity: { type: 'number', description: 'Slot opacity 0..1, multiplied by the global opacity.' },
        flip: { type: 'boolean', description: 'Mirror the artwork horizontally.' },
        visible: { type: 'boolean', description: 'Show or hide this slot.' },
      },
      output: TEXT_OUTPUT,
      async execute(args: {
        slot: string
        source?: string
        size?: number
        offset?: number[]
        hotspot?: number[]
        opacity?: number
        flip?: boolean
        visible?: boolean
      }) {
        const refused = await gate(3, 'ai_mouse_sprite')
        if (refused) return refused
        if (!(SLOT_NAMES as readonly string[]).includes(args.slot)) {
          return `slot 必须是 ${SLOT_NAMES.join(' | ')}，收到「${args.slot}」。`
        }
        const payload: Record<string, unknown> = { slot: args.slot }
        if (args.source !== undefined) {
          const resolved = resolveSource(args.source, config.assetDir)
          if (resolved.kind === 'missing') return `加载失败：${resolved.error}`
          payload.source = resolved.wire
        }
        if (args.size !== undefined) payload.size = args.size
        if (args.opacity !== undefined) payload.opacity = args.opacity
        if (args.flip !== undefined) payload.flip = args.flip
        if (args.visible !== undefined) payload.visible = args.visible
        if (Array.isArray(args.offset) && args.offset.length >= 2) payload.offset = args.offset.slice(0, 2)
        if (Array.isArray(args.hotspot) && args.hotspot.length >= 2) payload.hotspot = args.hotspot.slice(0, 2)

        await prime()
        const reply = await send('sprite', payload)
        const status = await send('status')
        return `${describeStatus(reply)}\n图案: ${slotSummary(status)}`
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_assets',
      description:
        'List the pictures available in the asset folder (the folder ai_mouse_sprite resolves bare file names against). Newest first.',
      parameters: {
        limit: { type: 'number', description: 'Maximum entries to list (default 30).' },
      },
      output: TEXT_OUTPUT,
      async execute(args: { limit?: number }) {
        const dir = config.assetDir || defaultAssetDir()
        // Keep the panel's slot cyclers in step with what the tool reports.
        await send('assets', { dir }).catch(() => undefined)
        const entries: AssetEntry[] = listAssets(dir)
        if (entries.length === 0) {
          return (
            `素材目录为空或不存在：${dir}\n` +
            '把 png / gif / svg / webp 等图片放进去，然后用 ai_mouse_sprite 按文件名引用即可。'
          )
        }
        const limit = Math.max(1, Math.min(args.limit ?? 30, 200))
        const lines = entries
          .slice(0, limit)
          .map((e) => `  ${e.name}  (${(e.size / 1024).toFixed(1)} KB)`)
        return `素材目录：${dir}（共 ${entries.length} 个）\n${lines.join('\n')}`
      },
    }),
  )

  // --------------------------------------------------------------- layers ---

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_preset',
      description:
        'Save or load a named configuration preset for the cursor (theme, label, colour, opacity, size, badge, capture visibility). Saves capture the LIVE state; loading replays it as ordinary commands, so a preset can never do anything a single tool call could not. Presets are stored as JSON next to the assets and are the building block for giving each subagent its own look.',
      parameters: {
        action: {
          type: 'string',
          description: 'save | load | list | show | delete',
        },
        name: { type: 'string', description: 'Preset name (case-insensitive).' },
      },
      output: TEXT_OUTPUT,
      async execute(args: { action?: string; name?: string }) {
        const dir = (config.assetDir || '').replace(/[\/]+$/, '')
        if (!dir) return '素材目录未配置，无法读写预设。'
        const file = `${dir}/presets.json`
        const list = await loadPresets(file)
        const action = (args.action ?? 'list').toLowerCase()
        const name = (args.name ?? '').trim()

        if (action === 'list') {
          if (list.length === 0) {
            return '还没有预设。用 ai_mouse_preset { action: "save", name: "..." } 保存当前外观。'
          }
          return `共 ${list.length} 个预设：` + list.map((p) => describePreset(p)).join('  |  ')
        }
        if (action === 'show') {
          const p = findPreset(list, name)
          return p ? describePreset(p) : `没有名为「${name}」的预设。`
        }
        if (!name) return `${action} 需要 name。`

        if (action === 'save') {
          const refused = await gate(2, 'ai_mouse_preset')
          if (refused) return refused
          const st = await send('status')
          const preset = capturePreset(name, st as never)
          await savePresets(file, upsertPreset(list, preset))
          return `已保存预设「${name}」：${describePreset(preset).slice(name.length + 2)}`
        }
        if (action === 'load') {
          const refused = await gate(2, 'ai_mouse_preset')
          if (refused) return refused
          const p = findPreset(list, name)
          if (!p) return `没有名为「${name}」的预设。可用：${list.map((x) => x.name).join(', ') || '空'}`
          const cmds = presetCommands(p)
          if (cmds.length === 0) return `预设「${p.name}」是空的，没有可应用的项。`
          for (const [cmd, payload] of cmds) await send(cmd, payload)
          return `已应用预设「${p.name}」（${cmds.length} 项）。`
        }
        if (action === 'delete') {
          const refused = await gate(2, 'ai_mouse_preset')
          if (refused) return refused
          if (!findPreset(list, name)) return `没有名为「${name}」的预设。`
          await savePresets(file, removePreset(list, name))
          return `已删除预设「${name}」。`
        }
        return `未知 action：${action}（可用 save / load / list / show / delete）`
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_map',
      description:
        'Start or stop the separate minimap window: a schematic of the screen with the top windows and a dot for this AI cursor. It runs as its own process, so it is a separate block from the menu bar and can be moved, resized and minimised like any window.',
      parameters: {
        visible: { type: 'boolean', description: 'true = show, false = close. Omit to read.' },
      },
      output: TEXT_OUTPUT,
      async execute(args: { visible?: boolean }) {
        if (args.visible !== undefined) {
          const refused = await gate(2, 'ai_mouse_map')
          if (refused) return refused
          await overlay.setMap(args.visible)
          return args.visible
            ? '小地图窗口已启动（独立进程，可拖动/缩放/最小化）。'
            : '小地图窗口已关闭。'
        }
        return overlay.mapRunning ? '小地图窗口：开' : '小地图窗口：关'
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_layer',
      description:
        'Put the AI cursor into the window stack: `topmost` (above everything, the default), `above`/`below` a specific window (needs `window`), or `desktop` (behind normal windows). Only the AI cursor\'s own window moves in the z-order — the anchored window and every other window keep their position, size, focus and stacking.',
      parameters: {
        mode: { type: 'string', description: 'topmost | above | below | desktop. Omit to just read.' },
        window: {
          type: 'string',
          description: 'Target window handle, e.g. "0x1A0B2C". Required for above/below.',
        },
      },
      output: TEXT_OUTPUT,
      async execute(args: { mode?: string; window?: string }) {
        if (args.mode !== undefined) {
          const refused = await gate(2, 'ai_mouse_layer')
          if (refused) return refused
          const reply = await send('layer', {
            mode: args.mode,
            ...(args.window ? { hwnd: args.window } : {}),
          })
          if (!reply.ok) return `设置层级失败：${reply.error ?? 'unknown'}`
        }
        const st = await send('status')
        const extra = st as unknown as { layer?: string; layer_title?: string; layer_hwnd?: string }
        return `${describeStatus(st)}\n层级=${extra.layer ?? '?'}${
          extra.layer_title ? ` 锚定「${extra.layer_title}」(${extra.layer_hwnd})` : ''
        }`
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_windows_at',
      description:
        'List the windows stacked under a screen point, front to back (title, class, process, rect). Read-only. Use it to pick a window for ai_mouse_layer.',
      parameters: {
        x: { type: 'number', description: 'Screen X (defaults to the cursor position).' },
        y: { type: 'number', description: 'Screen Y (defaults to the cursor position).' },
      },
      output: TEXT_OUTPUT,
      async execute(args: { x?: number; y?: number }) {
        const refused = await gate(1, 'ai_mouse_windows_at')
        if (refused) return refused
        const reply = (await send('layer_probe', {
          ...(args.x !== undefined ? { x: args.x } : {}),
          ...(args.y !== undefined ? { y: args.y } : {}),
        })) as unknown as {
          windows?: Array<{ hwnd: string; title: string; class: string; pid: number; rect: number[] }>
        }
        const list = reply.windows ?? []
        if (list.length === 0) return '该位置没有可见窗口。'
        return list
          .map(
            (w, i) =>
              `${i + 1}. ${w.hwnd}  ${w.title || '（无标题）'}  [${w.class}]  pid=${w.pid}  ` +
              `${w.rect[2]}×${w.rect[3]} @ (${w.rect[0]},${w.rect[1]})`,
          )
          .join('\n')
      },
    }),
  )

  ctx.tools.register(
    defineTool({
      name: 'ai_mouse_stop',
      description:
        'Stop the AI cursor: hides it and drops the permission level to 0, so this agent can neither drive nor restart it. Shows the floating panel afterwards so the user can click 启用 to hand control back.',
      parameters: {},
      output: TEXT_OUTPUT,
      async execute() {
        const refused = await gate(3, 'ai_mouse_stop')
        if (refused) return refused
        await send('pause')
        await send('panel', { visible: true })
        const reply = await send('status')
        return `${describeStatus(reply)}\nAI 鼠标已停止，面板已显示。恢复只能由人点面板上的「启用」或「管理员」。`
      },
    }),
  )
}
