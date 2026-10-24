// Which rth-host journal belongs to THIS app instance.
//
// The app (app/src-tauri/src/main.rs, "rth-host: the one native owner") lays one instance out as
//   <data>/host/<label>/token          the owner token (passed to shell/app.mjs as --token-file,
//                                      and on to DSH as RTH_HOST_TOKEN_FILE)
//   <data>/app/<label>/rth-host.jsonl  the host's --log journal
// so the token file's PATH names the instance's data root and label. This module only parses that
// path string: it never stats, opens or reads the token (or anything else), and it never guesses
// from DSH_HOME, a dev directory or RTH_DATA_DIR. A path that does not have exactly that layout is
// unsupported: the caller leaves the host journal off and shows the reason, never another log.
import { basename, dirname, isAbsolute, join, normalize } from 'node:path'

const LABEL = /^[\w.-]+$/

/**
 * @param {string|undefined} tokenFile the RTH_HOST_TOKEN_FILE value (a path; its contents are never read)
 * @returns {{path: string, data: string, label: string, from: string} | {path: null, why: string}}
 */
export function hostJournalFor(tokenFile) {
  if (typeof tokenFile !== 'string' || !tokenFile.trim()) return { path: null, why: 'RTH_HOST_TOKEN_FILE is not set, so no app instance names its host journal' }
  if (!isAbsolute(tokenFile)) return { path: null, why: `RTH_HOST_TOKEN_FILE is not an absolute path (${tokenFile}); the instance layout cannot be bound` }
  const file = normalize(tokenFile)
  const labelDir = dirname(file)
  const hostDir = dirname(labelDir)
  const label = basename(labelDir)
  if (basename(file) !== 'token' || basename(hostDir) !== 'host' || !LABEL.test(label) || label === '.' || label === '..' || dirname(hostDir) === hostDir) {
    return { path: null, why: `RTH_HOST_TOKEN_FILE (${tokenFile}) is not <data>/host/<label>/token, the app's instance layout; the host journal stays off rather than guess another instance's log` }
  }
  const data = dirname(hostDir)
  return { path: join(data, 'app', label, 'rth-host.jsonl'), data, label, from: tokenFile }
}

/**
 * The host journal Mind should follow, from its config and the environment.
 * An explicit journals.host wins; otherwise journals.hostFromTokenFile binds the app instance.
 * @returns {{path: string|null, detail: string}}
 */
export function resolveHostJournal(journals, env = process.env) {
  if (journals?.host) return { path: journals.host, detail: `${journals.host} (journals.host, set explicitly)` }
  if (!journals?.hostFromTokenFile) return { path: null, detail: 'journals.host not configured: host notes (speech candidates, mute, cancels sent) are not shown' }
  const b = hostJournalFor(env.RTH_HOST_TOKEN_FILE)
  if (!b.path) return { path: null, detail: `host journal binding unsupported: ${b.why}` }
  return { path: b.path, detail: `${b.path} (bound to this app instance: data ${b.data}, label ${b.label}, from the RTH_HOST_TOKEN_FILE path; the token is never read)` }
}
