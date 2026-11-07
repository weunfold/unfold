// Where Unfold finds the bridge: one small JSON file, in this plugin's own state folder,
// readable only by the person who runs DSH. It holds the port and the key of this start;
// Unfold reads it and hands the key to its agent only. It is removed when the bridge stops.
import { chmodSync, lstatSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { randomBytes } from 'node:crypto'
import { homedir } from 'node:os'
import { dirname, isAbsolute, join } from 'node:path'

// Unfold's own DSH names the folder in UNFOLD_BRIDGE_STATE_DIR, so its bridge never writes the
// file another DSH on this computer uses. It is taken out of the env at the read: tools DSH
// spawns later build their env from process.env and never see it. That hides the path; it is
// no boundary, since DSH's launch snapshot keeps the value. Only an absolute folder is used:
// undefined when unset, null when set but not absolute (an empty one too).
export function takeStateDir(env = process.env) {
  if (!Object.hasOwn(env, 'UNFOLD_BRIDGE_STATE_DIR')) return undefined
  const dir = env.UNFOLD_BRIDGE_STATE_DIR
  delete env.UNFOLD_BRIDGE_STATE_DIR
  return dir && isAbsolute(dir) ? dir : null
}

// `<stateDir>/endpoint.json` when Unfold named a folder (writeEndpoint makes it 0700). Else
// XDG_STATE_HOME when it is absolute (the spec ignores a relative one), else ~/.local/state.
export function endpointFile(env = process.env, home = homedir(), stateDir = null) {
  if (stateDir) return join(stateDir, 'endpoint.json')
  const state = env.XDG_STATE_HOME && isAbsolute(env.XDG_STATE_HOME) ? env.XDG_STATE_HOME : join(home, '.local', 'state')
  return join(state, 'unfold-bridge', 'endpoint.json')
}

// Written whole or not at all: a temporary file beside it, then a rename. A folder that is a
// symlink is refused, so the file and the 0700 never land wherever the link points.
export function writeEndpoint(file, { port, key, pid, started }) {
  const dir = dirname(file)
  mkdirSync(dir, { recursive: true, mode: 0o700 })
  if (lstatSync(dir).isSymbolicLink()) throw Object.assign(new Error(`the state folder is a symlink (${dir})`), { code: 'symlinked_state_dir' })
  chmodSync(dir, 0o700)
  const temp = join(dir, `.endpoint.${randomBytes(6).toString('hex')}.tmp`)
  try {
    writeFileSync(temp, `${JSON.stringify({ v: 1, port, key, pid, started })}\n`, { mode: 0o600, flag: 'wx' })
    renameSync(temp, file)
  } catch (error) {
    rmSync(temp, { force: true })
    throw error
  }
}

// Only this start's file goes: a newer start (another DSH, or this one restarted) keeps its own.
export function removeEndpoint(file, key) {
  let current
  try { current = JSON.parse(readFileSync(file, 'utf8')) } catch { return }
  if (current?.key === key) rmSync(file, { force: true })
}
