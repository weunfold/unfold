// The event pane's model: a bounded window of rows over the live stream. Text and audio
// packets of one message coalesce into one row that grows in place, so the pane shows facts,
// never raw samples. Closing the pane drops listeners only; the window keeps ingesting.
//
// Every collection is bounded on its own: rows by `limit`; a coalesced row keeps counters,
// first/last ids and a sample of `idSample` ids, never every chunk id; `byId` holds only the
// sampled and last ids of retained rows; `effects` lists row keys (not chunk ids), at most
// `effectsPerCause` per cause and only for retained rows, and is pruned on eviction.
//
// Reply text is redacted before it is cut to its tail: the raw joined text of a growing text row
// is kept privately (`raw`, never on the row, never emitted), bounded by RAW_MAX and cut only at
// a safe boundary, and the shown tail is taken from its redaction. A credential split across
// deltas, or by the tail cut, is never shown in part.

const TEXT_TAIL = 240
const RAW_MAX = 16 * 1024
const CAUSES_PER_ROW = 4
const PRODUCERS_MAX = 256
const UNKNOWN_MAX = 32

export class EventWindow {
  constructor(catalog, { limit = 2000, idSample = 8, effectsPerCause = 64 } = {}) {
    this.catalog = catalog
    this.limit = limit
    this.idSample = idSample
    this.effectsPerCause = effectsPerCause
    this.rows = []
    this.byId = new Map() // event id -> row (first, sampled and last ids of retained rows)
    this.effects = new Map() // cause id -> { keys: Set<row key>, omitted }
    this.open = new Map() // `${name}:${coalesce}` -> retained row still growing
    this.producers = new Map() // producer -> highest evicted seq, to explain unresolved causes
    this.status = null // latest ephemeral status report (heartbeat)
    this.dropped = 0
    this.unknown = new Set()
    this.unknownOmitted = 0
    this.rev = 0
    this.listeners = new Set()
    this.raw = new Map() // row key -> raw joined text, open text rows only (private)
    this.orphan = new Map() // row key -> what is still arriving after a dropped head: {token} | {quote}
  }

  onChange(listener) {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  push(env) {
    // Every field this window emits (rows, patches, status, hence the /api/rth-events snapshot
    // and SSE frames) is redacted at intake, BEFORE the envelope is described: key-aware (a
    // secret-named key is masked wherever it nests) and with the shared credential patterns, so
    // the summary is rendered and clipped from already-redacted inputs and no clip can cut a
    // credential into an unrecognised fragment. A reply's text
    // deltas are the exception: their raw join is kept privately and redacted whole (see `raw`),
    // so a credential split across deltas is caught.
    const reply = this.catalog.entry(env.kind)?.name === 'message_text_added'
    const d = this.catalog.describe(reply ? env : redactEnvelope(env, this.catalog.entry(env.kind)))
    if (!d.known && !this.unknown.has(d.wire)) {
      if (this.unknown.size < UNKNOWN_MAX) this.unknown.add(d.wire)
      else this.unknownOmitted++
    }
    if (this.catalog.entry(d.wire)?.ephemeral) {
      this.status = d
      return this.emit('status', d)
    }
    this.seen(d.producer)
    const openKey = d.coalesce ? `${d.name}:${d.coalesce}` : null
    const growing = openKey ? this.open.get(openKey) : null
    if (growing) {
      this.absorb(growing, d)
      this.link(growing, d.cause)
      return this.emit('updated', patch(growing))
    }
    const row = newRow(d)
    if (row.text !== undefined) {
      const { raw, orphan } = boundRaw(row.text)
      if (orphan) this.orphan.set(row.key, orphan)
      this.raw.set(row.key, raw)
      row.text = shownTail(raw)
      // The emitted row carries no unredacted reply text either: its other fields key-aware, its
      // first delta redacted, and its summary the redacted tail (never a clipped raw prefix).
      row.fields = { ...redact(row.fields), text: redactText(String(row.fields.text ?? '')) }
      row.summary = row.text
    }
    this.rows.push(row)
    this.byId.set(d.id, row)
    this.link(row, d.cause)
    if (openKey) this.open.set(openKey, row)
    if (d.name === 'message_finished') this.closeMessage(d.fields.response_id)
    const closes = this.catalog.entry(d.wire)?.closes // an end event closes its stream's growing row
    if (closes) this.open.delete(`${this.catalog.entry(closes).name}:${d.coalesce ?? d.fields.stream_id}`)
    this.emit('added', row)
    this.trim()
    return row
  }

  /**
   * Rows from `id` back through its causes, as far as the window reaches. `unresolved` names
   * the first ancestor the window cannot show and why: evicted from the window, missing from
   * a producer it has seen (lost or skipped), or never observed at all (the live observer
   * stream omits admitted host frames, so a guest event's trigger is often one of these).
   */
  chain(id) {
    const rows = []
    const seen = new Set()
    let next = id
    for (let row = this.byId.get(next); row && !seen.has(row.key); row = this.byId.get(next)) {
      seen.add(row.key)
      rows.push(row)
      next = row.cause
    }
    const unresolved = next && !this.byId.has(next) ? { id: next, reason: this.gap(next) } : null
    return { rows, unresolved }
  }

  /** Retained rows caused directly by `id` (`effects.get(id).omitted` counts the capped rest). */
  causedBy(id) {
    const entry = this.effects.get(id)
    return entry ? [...entry.keys].map((key) => this.byId.get(key)).filter(Boolean) : []
  }

  gap(id) {
    const s = String(id)
    const cut = s.lastIndexOf(':')
    const producer = cut > 0 ? s.slice(0, cut) : null
    const seq = Number(s.slice(cut + 1))
    if (!producer || !this.producers.has(producer)) return 'not_observed'
    return seq <= this.producers.get(producer) ? 'evicted' : 'missing'
  }

  seen(producer) {
    if (typeof producer !== 'string') return
    if (!this.producers.has(producer)) this.producers.set(producer, -1)
    if (this.producers.size > PRODUCERS_MAX) this.producers.delete(this.producers.keys().next().value)
  }

  link(row, cause) {
    if (!cause || row.causes.includes(cause)) return
    if (row.causes.length >= CAUSES_PER_ROW) return void row.causes_omitted++
    const entry = this.effects.get(cause) ?? { keys: new Set(), omitted: 0 }
    if (entry.keys.size >= this.effectsPerCause) {
      entry.omitted++
      row.causes_omitted++
    } else {
      entry.keys.add(row.key)
      row.causes.push(cause)
    }
    this.effects.set(cause, entry)
  }

  absorb(row, d) {
    if (row.ids.length < this.idSample) {
      row.ids.push(d.id)
    } else {
      row.ids_omitted++
      if (row.last_id !== row.key && !row.ids.includes(row.last_id)) this.byId.delete(row.last_id)
    }
    this.byId.set(d.id, row)
    row.last_id = d.id
    row.count++
    row.last_t_ns = d.t_ns
    row.seq_last = d.seq
    if (row.audio) {
      addAudio(row.audio, d.fields)
      row.summary = `${row.audio.chunks} audio chunks, ${Math.round(row.audio.ms)} ms`
    }
    if (row.text !== undefined) {
      let add = String(d.fields.text ?? '')
      const pending = this.orphan.get(row.key)
      if (pending?.quote) {
        // Inside a quoted credential value whose key was dropped: withhold everything up to and
        // including its UNESCAPED closing quote, however many deltas that takes; a backslash that
        // ended the previous delta escapes this delta's first character.
        const { close, escaped } = closingQuote(add, 0, pending.quote, pending.escaped)
        if (close >= 0) this.orphan.delete(row.key)
        else this.orphan.set(row.key, { quote: pending.quote, escaped })
        add = close < 0 ? '' : add.slice(close + 1)
      } else if (pending) {
        // A run whose context was dropped is still arriving: discard any leading blanks and the
        // next whole token (it may span deltas), so its value can never be shown orphaned.
        const head = add.match(/^\s*\S*/)[0]
        const rest = add.slice(head.length)
        if (rest) this.orphan.delete(row.key) // the token ended (whitespace follows)
        add = rest
      }
      const { raw, orphan } = boundRaw((this.raw.get(row.key) ?? row.text) + add)
      if (orphan) this.orphan.set(row.key, orphan)
      this.raw.set(row.key, raw)
      row.text = shownTail(raw)
      row.summary = row.text
    }
  }

  closeMessage(responseId) {
    for (const [key, row] of this.open) {
      if (row.fields.response_id !== responseId) continue
      this.open.delete(key)
      this.raw.delete(row.key)
      this.orphan.delete(row.key)
    }
  }

  trim() {
    while (this.rows.length > this.limit) this.evict(this.rows.shift())
  }

  evict(old) {
    this.dropped++
    for (const id of [old.key, ...old.ids, old.last_id]) if (this.byId.get(id) === old) this.byId.delete(id)
    for (const [key, row] of this.open) if (row === old) this.open.delete(key)
    this.raw.delete(old.key)
    this.orphan.delete(old.key)
    for (const cause of old.causes) {
      const entry = this.effects.get(cause)
      entry?.keys.delete(old.key)
      if (entry && !entry.keys.size) this.effects.delete(cause)
    }
    if (typeof old.producer === 'string' && Number.isFinite(old.seq_last)) {
      this.producers.set(old.producer, Math.max(this.producers.get(old.producer) ?? -1, old.seq_last))
    }
    this.emit('evicted', { key: old.key })
  }

  emit(change, payload) {
    this.rev++
    for (const listener of this.listeners) listener(change, payload, this.rev)
    return payload
  }
}

function newRow(d) {
  const row = { ...d, key: d.id, ids: [d.id], ids_omitted: 0, last_id: d.id, count: 1, last_t_ns: d.t_ns, seq_last: d.seq, causes: [], causes_omitted: 0 }
  if (d.media) {
    row.audio = { ...audioOf(d.fields), first_chunk: num(d.fields.chunk), last_chunk: num(d.fields.chunk), sample_start: num(d.fields.sample_offset), sample_end: null, gaps: 0 }
    row.audio.sample_end = row.audio.sample_start === null ? null : row.audio.sample_start + row.audio.bytes / 2
    row.fields = { ...d.fields, data: undefined } // the pane counts samples; it never holds them
  }
  if (d.name === 'message_text_added') row.text = String(d.fields.text ?? '')
  return row
}

function addAudio(a, fields) {
  const chunk = audioOf(fields)
  const offset = num(fields.sample_offset)
  const index = num(fields.chunk)
  if ((offset !== null && a.sample_end !== null && offset !== a.sample_end) || (index !== null && a.last_chunk !== null && index !== a.last_chunk + 1)) a.gaps++
  a.chunks++
  a.bytes += chunk.bytes
  a.ms += chunk.ms
  if (index !== null) a.last_chunk = index
  if (offset !== null) a.sample_end = offset + chunk.bytes / 2
}

/** What changed in a coalesced row: a bounded delta, so a long reply never re-sends the row. */
export function patch(row) {
  const out = { key: row.key, count: row.count, last_id: row.last_id, last_t_ns: row.last_t_ns, seq_last: row.seq_last, summary: row.summary, ids_omitted: row.ids_omitted }
  if (row.audio) out.audio = row.audio
  if (row.text !== undefined) out.text = row.text
  return out
}

export function audioOf(fields) {
  const b64 = String(fields.data ?? '')
  const pad = b64.endsWith('==') ? 2 : b64.endsWith('=') ? 1 : 0
  const bytes = Math.max(0, Math.floor((b64.length * 3) / 4) - pad)
  const rate = Number(fields.sample_rate) || 0
  return { chunks: 1, bytes, ms: rate ? (bytes / 2 / rate) * 1000 : 0, sample_rate: rate }
}

const num = (x) => (Number.isFinite(x) ? x : null)

// Credential patterns: a copy of plugins/dsh-mind/src/redact.mjs `redactText` (itself a copy of
// plugins/dsh-settings/src/redact.mjs); keep the three in step. Copied, not imported: each plugin
// is installed on its own and dsh-events must not depend on another plugin.
const REDACTED = '[redacted]'
// Standalone sk- credentials, with no key, assignment or scheme around them. The family:
// sk-<body>, sk-proj-, sk-svcacct-, sk-admin-,
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

// Key-aware masking: a copy of plugins/dsh-mind/src/redact.mjs `SECRET_KEY` and `redact`
// (identical in plugins/dsh-settings/src/redact.mjs); keep the three in step. A value under a
// secret-looking key is replaced whatever it is; strings elsewhere get the text patterns; objects
// and arrays are walked with their keys.
const SECRET_KEY = /(token|secret|passw(or)?d|api[_-]?key|private[_-]?key|authorization|bearer|cookie|credential)/i

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

// The envelope's own stamps (catalog.mjs ENVELOPE_KEYS): protocol identities, never redacted.
const ENVELOPE = new Set(['v', 'id', 'producer', 'seq', 't_ns', 'source', 'cause', 'received', 'observed', 'kind'])

/** An envelope with every payload field redacted (key-aware), its stamps kept. A media chunk's
 * base64 `data` is kept as it is: it is only counted (audioOf) and never held or emitted. */
function redactEnvelope(env, entry) {
  const out = {}
  for (const [k, v] of Object.entries(env)) out[k] = ENVELOPE.has(k) || (entry?.media && k === 'data') ? v : redact(v, k)
  return out
}

/** The shown text: the redaction of the whole joined text, THEN its tail. */
function shownTail(raw) {
  return redactText(raw).slice(-TEXT_TAIL)
}

// A credential keyword or assignment right before a cut would leave its value orphaned.
// Tested on the text before the cut with its trailing whitespace removed, so a long blank run
// between a keyword and its value cannot hide the keyword.
const CONTEXT_BEFORE = /(?:\b(?:Bearer|Basic)|[:=]["']?)$/i
const contextBefore = (raw, cut) => CONTEXT_BEFORE.test(raw.slice(0, cut).trimEnd().slice(-64))

// Where a quoted credential value opens (the quoted forms of SECRET_TEXT): a trim never cuts
// inside one, since whitespace within quotes is not a boundary, and only an unescaped
// quote ends one.
const QUOTED_SECRET = [
  /\b[A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*=(["'])/g,
  /\b(?:x-goog-api-key|authorization|api[_-]?key|token)["']?\s*[:=]\s*(["'])/gi, // any prefix
]

/**
 * The first UNESCAPED `quote` in `s` from `from` (JSON escapes: `\"` and `\\` do not end a
 * value). `escaped`: the character at `from` is escaped by a backslash that came before it (one
 * that ended the previous delta). Returns the index (-1 if none) and whether `s` ends mid-escape.
 */
function closingQuote(s, from, quote, escaped = false) {
  for (let i = from; i < s.length; i++) {
    if (escaped) escaped = false
    else if (s[i] === '\\') escaped = true
    else if (s[i] === quote) return { close: i, escaped: false }
  }
  return { close: -1, escaped }
}

/** Quoted credential spans in `raw`: {start, end, closed, quote, escaped}; an open one runs to the end. */
function quotedSpans(raw) {
  const spans = []
  for (const re of QUOTED_SECRET) {
    for (const m of raw.matchAll(re)) {
      const quote = m[1]
      const { close, escaped } = closingQuote(raw, m.index + m[0].length, quote)
      spans.push({ start: m.index, end: close < 0 ? raw.length : close + 1, closed: close >= 0, quote, escaped })
    }
  }
  return spans
}

/**
 * Bound the private raw text to RAW_MAX by dropping its head, cutting only after whitespace,
 * never inside a quoted credential value and never between a credential keyword and its value,
 * so what is kept starts with a whole token whose context is kept too. When no such cut exists,
 * nothing of the head is kept and `orphan` tells the caller what to withhold as it keeps arriving:
 * `{quote}` the rest of an open quoted value up to its closing quote, `{token}` the rest of an
 * unbroken run (never an orphaned value).
 */
function boundRaw(raw) {
  if (raw.length <= RAW_MAX) return { raw, orphan: null }
  const spans = quotedSpans(raw)
  let cut = raw.length - RAW_MAX
  for (let i = 0; i < 8; i++) {
    const inside = spans.find((sp) => sp.start < cut && cut < sp.end)
    if (inside && !inside.closed) return { raw: '', orphan: { quote: inside.quote, escaped: inside.escaped } }
    if (inside) cut = inside.end // past the closed value, whose key is dropped with it
    const ws = raw.slice(cut).search(/\s/)
    if (ws < 0) return { raw: '', orphan: { token: true } }
    cut += ws
    const next = raw.slice(cut).search(/\S/)
    if (next < 0) {
      // Only whitespace is left after the cut: keep the last word, so a keyword such as
      // "Bearer" still precedes the value that the next delta brings.
      const last = raw.match(/\S+\s*$/)?.[0] ?? ''
      return last.length <= RAW_MAX ? { raw: last, orphan: null } : { raw: '', orphan: { token: true } }
    }
    cut += next
    if (!contextBefore(raw, cut) && !spans.some((sp) => sp.start < cut && cut < sp.end)) return { raw: raw.slice(cut), orphan: null }
  }
  return { raw: '', orphan: { token: true } }
}
