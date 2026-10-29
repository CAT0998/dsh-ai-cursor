// Build the Rust overlay and drop it into bin/, so the plugin shell can find it.
// Used by `pnpm run build:overlay` and by prepack.
import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const crate = join(root, 'overlay')
const built = join(crate, 'target', 'release', 'ai-mouse-overlay.exe')
const dest = join(root, 'bin', 'ai-mouse-overlay.exe')
// `prepack` passes --if-missing so an npm publish on a machine without Rust can
// still ship a prebuilt bin/ai-mouse-overlay.exe. Every other caller must fail
// loud: silently keeping a stale exe is how a "successful" build ships old code.
const ifMissing = process.argv.includes('--if-missing')

const upToDate = () => {
  if (!existsSync(built) || !existsSync(dest)) return false
  return statSync(dest).mtimeMs >= statSync(built).mtimeMs
}

if (ifMissing && upToDate()) {
  console.log('[build:overlay] bin/ai-mouse-overlay.exe is up to date, skipping cargo build')
  process.exit(0)
}

/** rustup installs are often not on PATH under this shell; find one anyway. */
function resolveCargo() {
  if (process.env.CARGO) return { cmd: process.env.CARGO, env: {} }
  const home = process.env.USERPROFILE ?? process.env.HOME ?? ''
  const candidates = [
    join(home, '.cargo', 'bin', 'cargo.exe'),
    join(home, '.cargo', 'bin', 'cargo'),
    'E:\\rust\\cargo\\bin\\cargo.exe',
    'D:\\rust\\cargo\\bin\\cargo.exe',
  ]
  for (const c of candidates) {
    if (c && existsSync(c)) {
      // A rustup proxy needs RUSTUP_HOME to know which toolchain to run, and the
      // standard layout puts the registry next to it. Fill them in beside the exe.
      const cargoHome = resolve(dirname(dirname(c)))
      const rustupHome = join(dirname(cargoHome), 'rustup')
      const env = {}
      if (!process.env.CARGO_HOME && existsSync(join(cargoHome, 'registry'))) {
        env.CARGO_HOME = cargoHome
      }
      if (!process.env.RUSTUP_HOME && existsSync(join(rustupHome, 'toolchains'))) {
        env.RUSTUP_HOME = rustupHome
      }
      return { cmd: c, env }
    }
  }
  return { cmd: 'cargo', env: {} }
}

const { cmd: cargo, env: extraEnv } = resolveCargo()
console.log(`[build:overlay] ${cargo} build --release  (cwd: overlay/)`)
if (Object.keys(extraEnv).length > 0) {
  console.log(
    `[build:overlay] using ${Object.entries(extraEnv)
      .map(([k, v]) => `${k}=${v}`)
      .join(' ')}`,
  )
}
const result = spawnSync(cargo, ['build', '--release'], {
  cwd: crate,
  stdio: 'inherit',
  shell: process.platform === 'win32',
  env: { ...process.env, ...extraEnv },
})

if (result.error || result.status !== 0) {
  if (ifMissing && existsSync(dest)) {
    console.warn('[build:overlay] cargo failed; --if-missing keeps the prebuilt bin/ai-mouse-overlay.exe')
    process.exit(0)
  }
  console.error('[build:overlay] cargo build FAILED — bin/ai-mouse-overlay.exe was not updated.')
  console.error(`[build:overlay] tried: ${cargo}`)
  console.error('[build:overlay] need Rust (https://rustup.rs) plus the MSVC build tools, and')
  console.error('[build:overlay] cargo on PATH, or set CARGO=<abs path to cargo.exe>.')
  if (result.error) console.error(result.error)
  process.exit(typeof result.status === 'number' && result.status > 0 ? result.status : 1)
}

if (!existsSync(built)) {
  console.error(`[build:overlay] cargo reported success but ${built} is missing.`)
  process.exit(1)
}

mkdirSync(dirname(dest), { recursive: true })
copyFileSync(built, dest)
console.log(`[build:overlay] copied -> ${dest}`)
