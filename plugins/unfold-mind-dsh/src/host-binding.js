// Which host journal belongs to THIS app instance.
//
// The app will set UNFOLD_HOST_JOURNAL on its instance child to the journal under its own dir,
//   <data>/app/<label>/host.jsonl
// (where its other per-run journals live, never in the instance's DSH area). It is part of the
// channel contract: the launcher preload reads and deletes it and hands it to plugins as
// `hostJournal` in its registry. The name is reserved and unset until the app writes that journal,
// so the host lane stays off until then. This module only
// parses that path string: it never stats, opens or reads it (or anything else), and it never
// guesses from DSH_HOME or a dev directory. A path that does not have exactly that layout is
// unsupported: the caller leaves the host journal off and shows the reason, never another log.
import { basename, dirname, isAbsolute, join, normalize, sep } from 'node:path'

export const HOST_JOURNAL_ENV = 'UNFOLD_HOST_JOURNAL'
const FILE = 'host.jsonl'
// The app's own label rule: 1-64 letters, digits, '.', '_' or '-', not starting with '.'.
const LABEL = /^[A-Za-z0-9_-][A-Za-z0-9._-]{0,63}$/

let launch // the value seen at the first load, kept for later loads of the plugin

/**
 * The instance's host journal path, read once: when the launcher preload's registry exists, from
 * its `hostJournal` key only (no key: the preload had none, and the env is not consulted); with no
 * registry, from the env. The variable is deleted either way, so tools DSH spawns later never
 * inherit it. A later load (a config change re-applies the plugin) keeps the launch value.
 * @returns {string|null}
 */
export function takeHostJournal(env = process.env, channel = globalThis[Symbol.for('unfold.channel')]) {
  const value = channel ? channel.hostJournal : env[HOST_JOURNAL_ENV]
  delete env[HOST_JOURNAL_ENV]
  if (typeof value === 'string') launch = value
  return launch ?? null
}

/**
 * @param {string|null|undefined} file the UNFOLD_HOST_JOURNAL value (a path; never opened here)
 * @returns {{path: string, data: string, label: string, from: string} | {path: null, why: string}}
 */
export function hostJournalFor(file) {
  if (typeof file !== 'string' || !file.trim()) return { path: null, why: `${HOST_JOURNAL_ENV} is not set, so no app instance names its host journal` }
  if (!isAbsolute(file)) return { path: null, why: `${HOST_JOURNAL_ENV} is not an absolute path (${file}); the instance layout cannot be bound` }
  if (file.split(sep).includes('..')) return { path: null, why: `${HOST_JOURNAL_ENV} (${file}) has a '..' segment, which is refused rather than resolved; the host journal stays off` }
  const path = normalize(file)
  const labelDir = dirname(path)
  const appDir = dirname(labelDir)
  const label = basename(labelDir)
  if (basename(path) !== FILE || basename(appDir) !== 'app' || !LABEL.test(label) || dirname(appDir) === appDir) {
    return { path: null, why: `${HOST_JOURNAL_ENV} (${file}) is not <data>/app/<label>/${FILE}, the app's instance layout; the host journal stays off rather than guess another instance's log` }
  }
  const data = dirname(appDir)
  return { path: join(data, 'app', label, FILE), data, label, from: file }
}

/**
 * The host journal Mind should follow, from its config and the instance's value.
 * An explicit journals.host wins; otherwise journals.hostFromInstance binds the app instance.
 * @returns {{path: string|null, detail: string, data?: string}} data, when bound: the instance's
 *   data dir, below which the journal tail refuses a symbolic link at every component
 */
export function resolveHostJournal(journals, value) {
  if (journals?.host) return { path: journals.host, detail: `${journals.host} (journals.host, set explicitly)` }
  if (!journals?.hostFromInstance) return { path: null, detail: 'journals.host not configured: host notes (speech candidates, mute, cancels sent) are not shown' }
  const b = hostJournalFor(value)
  if (!b.path) return { path: null, detail: `host journal binding unsupported: ${b.why}` }
  return { path: b.path, data: b.data, detail: `${b.path} (bound to this app instance: data ${b.data}, label ${b.label}, from ${HOST_JOURNAL_ENV})` }
}
