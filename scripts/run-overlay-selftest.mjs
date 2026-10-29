// Run the Win32 overlay's own self-test: it talks to the real ai-mouse-overlay.exe
// over the line protocol, so a green run means the windows exist and painted.
// Needs a built overlay (pnpm run build:overlay) and Python 3 with Pillow.
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const exe = join(root, 'bin', 'ai-mouse-overlay.exe')
const script = join(root, 'scripts', 'selftest-overlay.py')

if (process.platform !== 'win32') {
  console.error('[test:overlay] the overlay is Windows-only (WDA_EXCLUDEFROMCAPTURE).')
  process.exit(1)
}
if (!existsSync(exe)) {
  console.error(`[test:overlay] ${exe} is missing — run "pnpm run build:overlay" first.`)
  process.exit(1)
}

const python = process.env.PYTHON || (process.platform === 'win32' ? 'python' : 'python3')
const result = spawnSync(python, [script, exe, join(root, 'artifacts')], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
})
if (result.error) {
  console.error('[test:overlay] could not start python:', result.error.message)
  process.exit(1)
}
process.exit(result.status ?? 1)
