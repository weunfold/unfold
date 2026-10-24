// A copy of plugins/dsh-settings/src/redact.mjs, not an import, so each plugin stands alone.
// Same contract.
// Secret values never leave this plugin. Everything served passes through `redact`: a value
// under a secret-looking key is replaced, whatever it is, and a string that carries a bearer
// credential, a Google API key, a standalone sk- key or a KEY=value assignment has that part replaced. Hashes and
// ids (artifact_id, *_sha256, boot ids) are not secrets and are kept: no length or hex rule.
export const REDACTED = '[redacted]'

const SECRET_KEY = /(token|secret|passw(or)?d|api[_-]?key|private[_-]?key|authorization|bearer|cookie|credential)/i
// Standalone sk- credentials, with no key, assignment or scheme around them. The family: sk-<body>, sk-proj-, sk-svcacct-, sk-admin-,
// sk-ant-api03-, sk-ant-admin01-, sk-ant-oat01- (each sub-prefix is in the body alphabet, so one rule
// covers them all). A START is a lower-case "sk-" not preceded by a letter, digit, "_" or "-" ("risk-",
// "desk-", "task-sk-" are not starts), or preceded by a serialized JSON escape \n \r \t. A KEY is a
// start then 20+ characters of [A-Za-z0-9_-] mixing upper case, lower case AND digits; the whole run
// is replaced. Kept (not credentials to this rule): runs without all three classes ("sk-learn",
// "sk-learn-pipeline-v2"), runs under 20, other families (sk_live_, rk_, ...). There is no length or
// entropy rule anywhere else, so hashes, ids and prose keep. STREAMING: a text that ENDS inside a run
// after a start (1+ body characters) is hidden whole, since the next delta may complete a key. The
// cost: a benign run at the very end of a text ("...uses sk-learn") is hidden until text follows it,
// and in a static string for good.
const SECRET_TEXT = [
  // Whole-value priority: a credential key's value is secret as ONE unit, whatever it
  // starts with ("Bearer ", "Basic ", even an already-redacted marker, which is untrusted input),
  // so the key rules run first and no inner pattern can split a value they recognise.
  // A quoted value runs to its UNESCAPED closing quote (JSON: `\"` and `\\` do not end it; a
  // trailing lone `\` is an escape still to come), or to the end while it is still open (a
  // streamed `KEY="…` is hidden until its quote arrives).
  [/\b([A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*)=("(?:[^"\\]|\\[\s\S]|\\$)*(?:"|$)|'(?:[^'\\]|\\[\s\S]|\\$)*(?:'|$)|\S+)/g, (_, name) => `${name}=${REDACTED}`],
  // `name: value` / `name=value`. A quoted value (any prefix, spaces and escapes included) runs to
  // its unescaped closing quote, or to the end while open. A bare value that is an authorization
  // scheme (`Authorization: Bearer x`) is left to the scheme rule below.
  [/\b(x-goog-api-key|authorization|api[_-]?key|token)(["']?\s*[:=]\s*)(?:(["'])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(?!Bearer\b|Basic\b|\[redacted\])[^\s"',}]+)/gi, (_, name, sep, q, close) => (q ? `${name}${sep}${q}${REDACTED}${close}` : `${name}${sep}${REDACTED}`)],
  [/\b(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+/g, (_, scheme) => `${scheme} ${REDACTED}`],
  [/\bAIza[0-9A-Za-z_-]{20,}/g, () => REDACTED],
  // Standalone sk- credentials (the contract above): a whole key, then a still-open run at the end.
  [/(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))sk-(?=[A-Za-z0-9_-]*[A-Z])(?=[A-Za-z0-9_-]*[a-z])(?=[A-Za-z0-9_-]*[0-9])[A-Za-z0-9_-]{20,}/g, () => REDACTED],
  [/(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))sk-[A-Za-z0-9_-]+$/, () => REDACTED],
]

export function redactText(s) {
  let out = s
  for (const [re, fn] of SECRET_TEXT) out = out.replace(re, fn)
  return out
}

export function redact(value, key = '') {
  if (key && SECRET_KEY.test(key) && value !== null && value !== undefined && typeof value !== 'boolean') return REDACTED
  if (typeof value === 'string') return redactText(value)
  if (Array.isArray(value)) return value.map((v) => redact(v))
  if (value && typeof value === 'object') {
    const out = {}
    for (const [k, v] of Object.entries(value)) out[k] = redact(v, k)
    return out
  }
  return value
}

// The credential VALUES the text rules above would replace, so a caller that later sees the same
// text without its context (a tail cut away "Bearer ", "key=" or "Authorization: ") can still
// mask them. The values are returned to the caller only, never stored by this module.
const SECRET_VALUES = [
  /\b(?:Bearer|Basic)\s+([A-Za-z0-9._~+/=-]+)/g,
  /\b(AIza[0-9A-Za-z_-]{20,})/g,
  /(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))(sk-(?=[A-Za-z0-9_-]*[A-Z])(?=[A-Za-z0-9_-]*[a-z])(?=[A-Za-z0-9_-]*[0-9])[A-Za-z0-9_-]{20,})(?=[^A-Za-z0-9_-])/g, // a whole, CLOSED sk- key only: a run still open at the end of the text is never remembered, so a key streamed in parts cannot fill SECRETS_KEPT with its prefixes
  /\b[A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*=("(?:[^"\\]|\\[\s\S]|\\$)*(?:"|$)|'(?:[^'\\]|\\[\s\S]|\\$)*(?:'|$)|\S+)/g,
  /\b(?:x-goog-api-key|authorization|api[_-]?key|token)["']?\s*[:=]\s*(?:"((?:[^"\\]|\\[\s\S]|\\$)+)|'((?:[^'\\]|\\[\s\S]|\\$)+)|(?!Bearer\b|Basic\b|\[redacted\])([^\s"',}]+))/gi,
]

export function secretsIn(s) {
  const out = new Set()
  for (const re of SECRET_VALUES) {
    for (const m of String(s).matchAll(re)) {
      const v = m.slice(1).find((x) => x !== undefined) // the quoted or the bare value
      if (v && v.length >= 4 && v !== REDACTED) out.add(v)
    }
  }
  return [...out]
}
