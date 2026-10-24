// What the working tree says, and how the panel checks it against the running agent.
//
// The agent never reports its system instruction or its context bounds. Two things are on a
// wire: the `gemini ready` log line (model, knobs, and `setup_id`, the SHA-256 prefix of the
// whole setup message without its resumption handle; crates/rth-agentd/src/gemini.rs
// `setup_identity`), and the Hello / context_restore context report. So the panel reads the
// policy text from the working tree and shows it as the effective prompt ONLY when a replica of
// `setup_message` over that text and the line's knobs reproduces the line's `setup_id`. The
// match also proves `sessionResumption` and `contextWindowCompression` (they are in the hash,
// not in the line). Anything the replica does not model makes the text unverifiable, never
// assumed.
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'

export const DEFAULT_GEMINI_SOURCE = new URL('../../../crates/rth-agentd/src/gemini.rs', import.meta.url)
export const DEFAULT_CONTEXT_SOURCE = new URL('../../../crates/rth-agentd/src/context.rs', import.meta.url)

const sha256 = (text) => createHash('sha256').update(text).digest('hex')
const where = (path) => (path instanceof URL ? path.pathname : String(path))

/** A Rust "…" literal body, unescaped (\n \t \r \0 \\ \" \' \u{…}, and `\` line continuation). */
export function rustString(body) {
  return body.replace(/\\(\n\s*|u\{([0-9a-fA-F]+)\}|.)/g, (_, esc, hex) => {
    if (hex) return String.fromCodePoint(parseInt(hex, 16))
    if (esc[0] === '\n') return ''
    return { n: '\n', t: '\t', r: '\r', 0: '\0', '\\': '\\', '"': '"', "'": "'" }[esc] ?? `\\${esc}`
  })
}

/** Every `pub const POLICY_<NAME>: &str = "…";` in gemini.rs, by its `[rth-voice-policy <id>]` tag. */
export function readPolicies(path = DEFAULT_GEMINI_SOURCE) {
  let src
  try {
    src = readFileSync(path, 'utf8')
  } catch (e) {
    return { path: where(path), error: `unreadable: ${e.code ?? e.message}`, policies: {} }
  }
  const policies = {}
  for (const m of src.matchAll(/pub const (POLICY_[A-Z0-9_]+): &str = "((?:[^"\\]|\\[\s\S])*)";/g)) {
    const text = rustString(m[2])
    const id = /^\[rth-voice-policy ([^\]]+)\]/.exec(text)?.[1]
    if (id) policies[id] = { constant: m[1], text, sha256: sha256(text) }
  }
  return { path: where(path), file_sha256: sha256(src), policies, read_at: Date.now() }
}

/** `pub const MAX_TURNS: usize = 128;` and friends; a value is a product of integer literals. */
export function readContextBounds(path = DEFAULT_CONTEXT_SOURCE) {
  let src
  try {
    src = readFileSync(path, 'utf8')
  } catch (e) {
    return { path: where(path), error: `unreadable: ${e.code ?? e.message}`, bounds: {} }
  }
  const bounds = {}
  for (const name of ['MAX_TURNS', 'MAX_CONTEXT_BYTES', 'MAX_TURN_BYTES']) {
    const m = new RegExp(`pub const ${name}: usize = ([0-9_ *]+);`).exec(src)
    if (!m) continue
    const factors = m[1].split('*').map((f) => f.trim().replaceAll('_', ''))
    if (factors.every((f) => /^\d+$/.test(f))) bounds[name] = factors.reduce((a, f) => a * Number(f), 1)
  }
  return { path: where(path), file_sha256: sha256(src), bounds, read_at: Date.now() }
}

/** `gemini ready gen=1 model=… resumed=false vad=server … setup_id=…` as fields. */
export function parseReady(msg) {
  if (typeof msg !== 'string' || !msg.startsWith('gemini ready ')) return null
  const out = {}
  for (const tok of msg.slice('gemini ready '.length).split(' ')) {
    const i = tok.indexOf('=')
    if (i > 0) out[tok.slice(0, i)] = tok.slice(i + 1)
  }
  return out
}

// gemini.rs `vad_config`: any other knob value sends no realtimeInputConfig at all.
const VAD = {
  'silence-v1': { automaticActivityDetection: { disabled: false, silenceDurationMs: 900 } },
  'low-v1': { automaticActivityDetection: { disabled: false, startOfSpeechSensitivity: 'START_SENSITIVITY_LOW', prefixPaddingMs: 300, silenceDurationMs: 900 } },
}
// The knobs the replica models; a ready line naming any other knob is not reproduced.
const MODELED = new Set(['gen', 'model', 'ws_connect_ms', 'setup_ms', 'setup_to_ready_ms', 'resumed', 'vad', 'policy', 'proactive', 'transcription_langs', 'system_extra', 'setup_id'])

/** gemini.rs `setup_message(cfg, None)`, for a known instruction text (or none). */
export function setupMessage({ model, vad, proactive, langs, system, resumption, compression }) {
  const setup = {
    model: `models/${model}`,
    generationConfig: { responseModalities: ['AUDIO'] },
    outputAudioTranscription: {},
    inputAudioTranscription: langs.length ? { languageCodes: langs } : {},
  }
  if (VAD[vad]) setup.realtimeInputConfig = VAD[vad]
  if (proactive) setup.proactivity = { proactiveAudio: true }
  if (resumption) setup.sessionResumption = {}
  if (compression) setup.contextWindowCompression = { slidingWindow: {} }
  if (system !== null) setup.systemInstruction = { parts: [{ text: system }] }
  return { setup }
}

// serde_json without `preserve_order` (Cargo.lock: no indexmap) writes object keys sorted.
const sorted = (v) => (Array.isArray(v) ? v.map(sorted) : v && typeof v === 'object' ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, sorted(v[k])])) : v)

/** gemini.rs `setup_identity`: the first 8 bytes of SHA-256 over the compact setup JSON, as hex. */
export function setupIdentity(knobs) {
  return sha256(JSON.stringify(sorted(setupMessage(knobs)))).slice(0, 16)
}

/**
 * Check a ready line against the working-tree policies. Returns
 * `{ status: 'verified', text, policy, resumption, compression }` when exactly the line's
 * setup_id is reproduced, else `{ status: 'mismatch' | 'unverifiable', reason }`.
 */
export function verifyPrompt(ready, policies) {
  if (!ready) return { status: 'unverifiable', reason: 'no gemini ready line observed for this boot' }
  const extra = Object.keys(ready).filter((k) => !MODELED.has(k))
  if (extra.length) return { status: 'unverifiable', reason: `the ready line names knobs this panel does not model: ${extra.join(', ')}` }
  if (!ready.setup_id || !ready.model || !ready.policy) return { status: 'unverifiable', reason: 'the ready line has no setup_id, model or policy' }
  if (ready.system_extra !== 'false') {
    return { status: 'unverifiable', reason: 'system_extra is set: an instruction from RTH_GEMINI_SYSTEM or the build is appended and is not reported anywhere' }
  }
  // A policy name the agent does not know sends no instruction (gemini.rs `_ => None`), so
  // "no instruction" is always a candidate; the known text is the other.
  const known = policies?.[ready.policy]
  const candidates = known ? [known.text, null] : [null]
  const langs = ready.transcription_langs && ready.transcription_langs !== '-' ? ready.transcription_langs.split(',').filter(Boolean) : []
  const base = { model: ready.model, vad: ready.vad, proactive: ready.proactive === 'true', langs }
  for (const system of candidates) {
    for (const resumption of [true, false]) {
      for (const compression of [true, false]) {
        if (setupIdentity({ ...base, system, resumption, compression }) === ready.setup_id) {
          return { status: 'verified', text: system, policy: ready.policy, resumption, compression }
        }
      }
    }
  }
  return {
    status: 'mismatch',
    reason: known
      ? 'no combination of the working-tree text and the line\'s knobs reproduces its setup_id: the running artifact sends a different setup'
      : `policy ${ready.policy} is not in the working-tree source, and the setup is not the no-instruction one`,
  }
}
