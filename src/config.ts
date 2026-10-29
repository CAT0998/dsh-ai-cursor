import Schema from '@deepseek-ai/schemastery'
import { defaultAssetDir } from './assets'
import { DEFAULT_MOUSE_TOOLS, INTERCEPT_MODES } from './intercept'
import type { InterceptMode } from './intercept'

/**
 * Tunable configuration must be a Schemastery Schema, never a plain object:
 * the harness validates it at load time and fails loud on invalid values.
 */
export interface Config {
  /** Enable the tool family at all. */
  enabled: boolean
  /** Absolute path to ai-mouse-overlay.exe. Empty = resolve the bundled bin/ folder. */
  exePath: string
  /** Badge text shown under the AI cursor. */
  label: string
  /** Cursor artwork: a little whale (default), a plain dot, or a classic arrow. */
  theme: 'whale' | 'dot' | 'arrow'
  /** Hide the cursor after this many ms without a move (0 = never hide). */
  idleMs: number
  /**
   * Let the AI cursor appear in screen captures. Off by default so the agent's own
   * screenshots stay clean; turn it on for demos, tutorials and recordings.
   */
  captureVisible: boolean
  /** Show the floating control panel the first time the overlay is used. */
  panel: boolean
  /** Open the panel as the full menu bar (true) or as the small chip (false). */
  panelExpanded: boolean
  /** Cursor + badge opacity, 0.15 - 1. */
  opacity: number
  /** Accent colour as #RRGGBB. Drives the cursor, the badge rim and the panel. */
  color: string
  /** Cursor size multiplier, 0.6 - 2. */
  scale: number
  /** Show the "AI 鼠标" badge under the cursor. */
  badge: boolean
  /** Starting permission ceiling for the agent (0 stopped … 3 admin). */
  permission: 0 | 1 | 2 | 3
  /**
   * What to do with other plugins' real-mouse tools.
   * off = leave them alone, refuse = deny, redirect = move the AI cursor there and
   * deny the real call, mirror = let the real call through and follow with the cursor.
   */
  intercept: InterceptMode
  /** Tool names (or `prefix*` patterns) treated as real-mouse tools. */
  interceptTools: string[]
  /** Folder to scan for custom artwork. A bare file name resolves here. */
  assetDir: string
}

export const Config: Schema<Config> = Schema.object({
  enabled: Schema.boolean().default(true).description('Enable the AI cursor overlay tools.'),
  exePath: Schema.string()
    .default('')
    .description('Absolute path to ai-mouse-overlay.exe. Empty resolves the bundled bin/ directory.'),
  label: Schema.string().default('AI 鼠标').description('Badge text under the AI cursor.'),
  theme: Schema.union(['whale', 'dot', 'arrow'] as const)
    .default('whale')
    .description('Cursor artwork: whale (default), dot or arrow.'),
  idleMs: Schema.number()
    .default(60000)
    .description('Hide the cursor after this many ms without a move. 0 never hides.'),
  captureVisible: Schema.boolean()
    .default(false)
    .description('Let the AI cursor appear in screenshots/recordings (off keeps agent captures clean).'),
  panel: Schema.boolean()
    .default(true)
    .description('Show the floating control panel the first time the overlay is used.'),
  panelExpanded: Schema.boolean()
    .default(true)
    .description('Open the panel as the full menu bar; false starts it as the small chip.'),
  opacity: Schema.number()
    .default(1)
    .description('Cursor and badge opacity, 0.15 - 1.'),
  color: Schema.string()
    .default('#4FC3F7')
    .description(
      'Accent colour as #RRGGBB; drives the cursor, the badge rim and the panel highlights.',
    ),
  scale: Schema.number()
    .default(1)
    .description('Cursor size multiplier, 0.6 - 2.'),
  badge: Schema.boolean()
    .default(true)
    .description('Show the "AI 鼠标" badge under the cursor.'),
  intercept: Schema.union(INTERCEPT_MODES)
    .default('off')
    .description(
      'Real-mouse interception: off leaves other plugins alone; refuse denies their mouse '
        + 'tools; redirect moves the AI cursor to the target and denies the real call (the '
        + 'real click does NOT happen); mirror lets the real call through and follows it '
        + 'with the AI cursor.',
    ),
  interceptTools: Schema.array(Schema.string())
    .default([...DEFAULT_MOUSE_TOOLS])
    .description('Tool names treated as real-mouse tools. A trailing * matches a prefix.'),
  assetDir: Schema.string()
    .default(defaultAssetDir())
    .description(
      'Folder scanned for custom artwork. A bare file name in ai_mouse_sprite resolves here; '
        + 'an absolute path is used as-is.',
    ),
  permission: Schema.union([0, 1, 2, 3] as const)
    .default(3)
    .description(
      'Starting permission ceiling for the agent: 0 stopped, 1 read-only, 2 drive, 3 admin. '
        + 'The agent may lower this but never raise it; only the floating panel can raise it.',
    ),
})
