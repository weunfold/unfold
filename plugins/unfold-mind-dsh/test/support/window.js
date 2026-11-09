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
// A reply's unfinished last line is held whole, from its first character, up to LINE_MAX characters
// (#406 Design A), twice the bound as Mind holds twice its keep; a longer one is withheld from its
// first character and kept as its head, a pad for what was dropped and its last LINE_TAIL characters.
const LINE_MAX = 2 * RAW_MAX
const LINE_TAIL = 4 * 1024
const OVER_PAD = ' '.repeat(4096)
const LINE_END = /\r\n|\n|\r(?=[\s\S])/g
// A line longer than LINE_MAX: a run of characters with no line end in it. A delta without one
// has nothing for `oversize` to cut, so `feed` keeps its lines as written. A single pass: an
// unanchored `{LINE_MAX+1}` search is quadratic on a long line that is merely short (it retries
// the whole run from every character), which the 80k timing rows caught.
function longLine(s) {
  let last = -1
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i)
    if (c === 10 || c === 13) last = i
    else if (i - last > LINE_MAX) return true
  }
  return false
}
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
    this.orphan = new Map() // row key -> what is still arriving after a dropped head: {token} | {quote} | {line}
    this.line = new Map() // row key -> its unfinished last line, held whole: {open} | {head, tail} once oversize (private)
  }

  onChange(listener) {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  push(env) {
    // Every field this window emits (rows, patches, status, hence the /api/unfold-events snapshot
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
      const text = row.text
      this.raw.set(row.key, '')
      this.feed(row, text)
      // The emitted row carries no unredacted reply text either: its other fields key-aware, its
      // first delta redacted, and its summary the redacted tail (never a clipped raw prefix). The
      // text is redacted once, as a growing buffer: the key-aware pass skips it (blanked in place,
      // so the fields keep their order), since its result would be replaced.
      row.fields = { ...redact({ ...row.fields, text: '' }), text: redactText(String(row.fields.text ?? ''), true) }
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
    if (row.text !== undefined) this.take(row, String(d.fields.text ?? ''))
  }

  /** A text delta: what a dropped head still withholds is discarded, the rest fed to the row. */
  take(row, add) {
    const pending = this.orphan.get(row.key)
    if (pending?.quote) {
      // Inside a quoted credential value whose key was dropped: withhold everything up to and
      // including its UNESCAPED closing quote, however many deltas that takes; a backslash that
      // ended the previous delta escapes this delta's first character.
      const { close, escaped } = closingQuote(add, 0, pending.quote, pending.escaped)
      if (close >= 0) this.orphan.delete(row.key)
      else this.orphan.set(row.key, { quote: pending.quote, escaped })
      add = close < 0 ? '' : add.slice(close + 1)
    } else if (pending?.line) {
      // The dropped head was a name's first part (a colon followed it): the rest of its line is a
      // value it could have named, withheld to the line's end, however many deltas that takes.
      const breakAt = add.search(/[\r\n]/)
      if (breakAt < 0) add = ''
      else {
        this.orphan.delete(row.key)
        add = add.slice(breakAt)
      }
    } else if (pending) {
      // A run whose context was dropped is still arriving: discard any leading blanks and the
      // next whole token (it may span deltas), so its value can never be shown orphaned.
      const head = add.match(/^\s*\S*/)[0]
      const rest = add.slice(head.length)
      if (/^\s*:/.test(add) || head.endsWith(':')) {
        // A colon right after the dropped token makes it a name's first part (#406 F-B): its line
        // holds that name's value, so the rest of the line is withheld to its end.
        const breakAt = rest.search(/[\r\n]/)
        if (breakAt < 0) {
          this.orphan.set(row.key, { line: true })
          add = ''
        } else {
          this.orphan.delete(row.key)
          add = rest.slice(breakAt)
        }
      } else if (!rest) add = '' // the token is still arriving
      else {
        this.orphan.delete(row.key) // the token ended (whitespace follows): the rest is prose
        add = rest
      }
    }
    this.feed(row, add)
  }

  /**
   * The reply's text is held in two parts (#406 Design A): `raw`, the committed text, which ends at
   * a line terminator (\n, \r\n, or a \r with something after it), and `line`, the unfinished last
   * line, held whole from its first character and carried across deltas, however many it comes in.
   * A delta with no terminator is only appended to it; at a terminator the lines it completed join
   * the committed text and only then does the 16 KiB bound cut it, so the bound never starts inside
   * the open line and what is kept depends on the text alone, not on where the deltas were split.
   * An open line longer than LINE_MAX is withheld from its first character, fail-closed, with no text
   * in its place until its terminator; it is then committed as its first LINE_MAX characters, a
   * 4 KiB blank run for what was dropped and its last LINE_TAIL characters, so the rules read it as
   * a cut-padded line. Every line that long is committed that way, however it arrived.
   */
  feed(row, add) {
    const held = this.line.get(row.key) ?? { open: '' }
    const over = held.head !== undefined
    // A \r at the end of the open line may be a CRLF's first half: it ends its line only once
    // something other than \n follows it.
    const crEnds = (over ? held.tail : held.open).endsWith('\r') && add !== '' && add[0] !== '\n'
    LINE_END.lastIndex = 0
    const first = crEnds ? null : LINE_END.exec(add)
    if (!crEnds && !first) {
      if (over) held.tail = `${held.tail}${add}`.slice(-LINE_TAIL)
      else {
        held.open = `${held.open}${add}`
        if (held.open.length > LINE_MAX) Object.assign(held, { head: held.open.slice(0, LINE_MAX), tail: held.open.slice(-LINE_TAIL), open: '' })
      }
      this.line.set(row.key, held)
      return this.show(row)
    }
    const stop = crEnds ? 0 : first.index + first[0].length
    let done = over ? `${held.head}${OVER_PAD}${`${held.tail}${add.slice(0, crEnds ? 0 : first.index)}`.slice(-LINE_TAIL)}${crEnds ? '' : first[0]}` : oversize(`${held.open}${add.slice(0, stop)}`)
    let pos = stop
    // A delta of many short lines is one slice, not one per line: with the first line short and
    // no line longer than LINE_MAX after it, every line is kept as written, so the last line end
    // is all `feed` needs (#406's fast budget: 40k blank lines cost one scan).
    if (!over && !crEnds && held.open.length + first.index <= LINE_MAX && !longLine(add)) {
      pos = lastLineEnd(add, stop)
      done += add.slice(stop, pos)
    } else {
      LINE_END.lastIndex = stop
      for (let m; (m = LINE_END.exec(add)); pos = m.index + m[0].length) done += oversize(add.slice(pos, m.index + m[0].length))
    }
    const { raw, orphan } = boundRaw(`${this.raw.get(row.key) ?? ''}${done}`)
    this.raw.set(row.key, raw)
    this.line.set(row.key, { open: '' })
    // The rest of the delta is the new open line; when the cut dropped a head whose value is still
    // arriving, the rest is read as what it withholds first.
    if (orphan) {
      this.orphan.set(row.key, orphan)
      return this.take(row, add.slice(pos))
    }
    return this.feed(row, add.slice(pos))
  }

  /** The shown text: the committed text and the open line; an oversize one is withheld, no text. */
  show(row) {
    const held = this.line.get(row.key)
    const raw = this.raw.get(row.key) ?? ''
    row.text = shownTail(held?.head !== undefined ? raw : `${raw}${held?.open ?? ''}`)
    row.summary = row.text
  }

  closeMessage(responseId) {
    for (const [key, row] of this.open) {
      if (row.fields.response_id !== responseId) continue
      this.open.delete(key)
      this.raw.delete(row.key)
      this.orphan.delete(row.key)
      this.line.delete(row.key)
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
    this.line.delete(old.key)
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

// Credential patterns: the same as src/client/redact.js `redactText` (the masking the mind and
// settings plugins carry too); keep them in step. Copied, not imported: each plugin installs on its
// own and must not depend on another.
const REDACTED = '[redacted]'
// Standalone sk- credentials, with no key,
// assignment or scheme around them. The family: sk-<body>, sk-proj-, sk-svcacct-, sk-admin-,
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
// The replacement for the later-line rules in SECRET_TEXT (#396): the value on a later line hidden,
// or a block indicator's lines hidden under it, or, for a sibling or the end of the text, an anchor
// or tag left with no value on the key line hidden. Everything else is kept as written.
// A text that is a growing buffer (a reply streamed into the window or Mind) ends where the next
// delta will go on: its last line may still be arriving. There a bare value on a later line is not
// yet a scalar or a child key (it may yet become `password:`), so it is neither hidden nor
// remembered until its line ends; until then it is withheld from the served text, the one place it
// is not shown (#406). Everywhere else (a whole payload) the end of the text ends its last line.
let OPEN_END = false
function laterLine(m, nl, indent, before, name, sep, props, tail, run, br, indicator, body, prefix, q, close, offset, string) {
  const head = `${nl}${indent}${before}${name}${sep}`
  if (indicator !== undefined) return `${head}${props}${tail}${run}${br}${indicator}${/\S/.test(body) ? `${/^\r?\n|^\r/.exec(body)[0]}${/[\r\n]([ \t]*)\S/.exec(body)[1]}${REDACTED}` : body}`
  if (prefix !== undefined) {
    if (OPEN_END && q === undefined && !/[\r\n]/.test(string.slice(offset + m.length))) return `${head}${props}${tail}${run}${br}${prefix}`
    return `${head}${props}${tail}${run}${br}${prefix}${q ? `${q}${REDACTED}${close}` : REDACTED}`
  }
  return `${head}${props.replace(/^([ \t]+)[\s\S]+$/, `$1${REDACTED}`)}${tail}${run}`
}
// Each rule is [pattern, replacement, gate]: a rule with a gate runs only on a text the gate finds
// something in, a literal its pattern cannot match without, so a text skips the rules it cannot meet.
const SECRET_TEXT = [
  // Whole-value priority: a credential key's value is secret as ONE unit, whatever it
  // starts with ("Bearer ", "Basic ", even an already-redacted marker, which is untrusted input),
  // so the key rules run first and no inner pattern can split a value they recognise.
  // A quoted value runs to its UNESCAPED closing quote (JSON: `\"` and `\\` do not end it; a
  // trailing lone `\` is an escape still to come), or to the end while it is still open (a
  // streamed `KEY="…` is hidden until its quote arrives). The `=` is found first and the name
  // looked for behind it, so a long word of names is one pass, with or without a value after it.
  [/\b([A-Z0-9_]+)=(?<=(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*=)("(?:[^"\\]|\\[\s\S]|\\$)*(?:"|$)|'(?:[^'\\]|\\[\s\S]|\\$)*(?:'|$)|`(?:[^`\\]|\\[\s\S]|\\$)*(?:`|$)|\S+)/g, (_, name) => `${name}=${REDACTED}`, /=/],
  // A block indicator with an anchor or tag before it (`password: &a |`, `!!str >-`,
  // `!<tag:yaml.org,2002:str> |`, one or two of them, each followed by a space or tab) hides
  // that property with the block: it serves as `password: |` and its block, the shape of the
  // plain form. The plain indicator, and every key line the scan below reads more broadly
  // (quoted keys, invisible characters, explicit keys past blanks), is left to `[redactYamlBlocks]`.
  // YAML allows at most one anchor and one tag on a node, so a line with three or more is
  // invalid and no rule here or below matches it: it is left as written and nothing on it is
  // remembered (#396).
  [/(^|\n)(?=[^\n]*:[ \t]*(?:[&!][^\s"'`]*[ \t]+){1,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*\r?(?:\n|$))(?=([ \t]*))\2([^\n]*?(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))([ \t]*:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2}([>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*\r?)(?=\n|$)((?:\n(?:\2[ \t]+[^\n]*|[ \t]*\r?(?=\n|$)))*)/gi, (_, nl, indent, before, name, sep, indicator, block) => `${nl}${indent}${before}${name}${sep}${indicator}${/\S/.test(block) ? `\n${indent}  ${REDACTED}${block.endsWith('\r') ? '\r' : ''}` : block}`, /:[ \t]*[&!>|]/],
  [/(^|\n)(?=[^\n]*:[ \t]*(?:[&!][^\s"'`]*[ \t]+){1,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*\r?(?:\n|$))(?=([ \t]*))\2([^\n]*?(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))([ \t]*:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2}([>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*\r?)(?=\n|$)((?:\n(?:\2[ \t]+[^\n]*|[ \t]*\r?(?=\n|$)))*)/g, (_, nl, indent, before, name, sep, indicator, block) => `${nl}${indent}${before}${name}${sep}${indicator}${/\S/.test(block) ? `\n${indent}  ${REDACTED}${block.endsWith('\r') ? '\r' : ''}` : block}`, /:[ \t]*[&!>|]/],

  // A YAML block scalar under a name (`password: |` or `>`, then the lines under it) is hidden
  // whole, its key line kept, text before the name on that line too: YAML_KEY finds its key line
  // and yamlBlockEnd where the block ends. It runs before the name rules, so they leave its `|`
  // or `>` alone.
  [redactYamlBlocks, null, /:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[&!>|]/],
  [redactExplicitKeyValues, null, /(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t]/],
  // A credential key line that ends at its colon, or at up to two anchors and tags (`&a`, `!!str`),
  // a comment after them or not, continues on a later line (#396, #410). Blank lines and comment lines
  // are skipped, and so are lines of anchors and tags alone indented under the key. Then the first
  // line strictly more indented than the key line is its value, hidden with the plain lines that
  // continue it (a quoted one up to its closing quote), or a block indicator (`|`, `>-`), whose
  // lines are hidden. A more indented mapping line (a name and a colon, quoted or not) is a child
  // key, never the value: it is left to the name rules, which hide only a credential child's value.
  // A line at the key's indent or less is a sibling: nothing is hidden or remembered from it, and an
  // anchor or tag with no value on the key line is hidden, as at the end of the text. A comment is
  // never a value, a comment line that names a credential starts nothing, and the key line and each
  // blank, comment, anchor and tag line stay as written. A line ends at \n, \r\n or a lone \r. Each
  // line is read forward from its start, its indent taken whole (a lookahead and its backreference),
  // so a key line costs one pass over it and the lines under it. An explicit key (#406), a `? name`
  // line (a list item's `- ? name` too, a trailing comment after the name, a quoted name taken
  // whole with any colon in it) then, past any blank or comment lines, a `:` line at its indent
  // with nothing after the colon but blanks, anchors, tags or a comment, is a key line too: its
  // `:` line ends it, and the lines under it are read from there. A `? name` line followed by a
  // `:` line with a block indicator (`: |`, `: >-`, anchors and tags too) is the previous rules'
  // block form.
  [/(^|(?:\r?\n|\r(?!\n)))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\2([^\r\n#]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(["']?[ \t]*:|["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))\2[ \t]*:)((?:[ \t]+[&!][^\s"'`]*){0,2})((?:[ \t]+#[^\r\n]*)?[ \t]*)((?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\2[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*)(?:((?:\r?\n|\r(?!\n)))(?:(\2[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*)(?=[\r\n]|$)((?:(?:\r?\n|\r(?!\n))(?:\2[ \t]+[^\r\n]*|[ \t]*(?=[\r\n]|$)))*)|(\2[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(?:(?:Bearer|Basic)[ \t]+(?=[A-Za-z0-9._~+/=-]))?)(?:(["'`])(?:(?!\13)[^\\]|\\[\s\S]|\\$)*(\13|$)|(?:[^ \t\r\n]|[ \t]+(?![ \t#\r\n]|$))+(?:(?:(?:\r?\n|\r(?!\n))[ \t]*(?=[\r\n]))*(?:\r?\n|\r(?!\n))\2[ \t]+(?![ \t#\r\n])[^\r\n]*)*))|(?=(?:\r?\n|\r(?!\n))(?!\2[ \t])[ \t]*[^\s#]|$))/gi, laterLine, /:[ \t]*(?:[&!#][^\r\n]*)?(?:[\r\n]|$)/],
  [/(^|(?:\r?\n|\r(?!\n)))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\2([^\r\n#]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(["']?[ \t]*:|["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))\2[ \t]*:)((?:[ \t]+[&!][^\s"'`]*){0,2})((?:[ \t]+#[^\r\n]*)?[ \t]*)((?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\2[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*)(?:((?:\r?\n|\r(?!\n)))(?:(\2[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*)(?=[\r\n]|$)((?:(?:\r?\n|\r(?!\n))(?:\2[ \t]+[^\r\n]*|[ \t]*(?=[\r\n]|$)))*)|(\2[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(?:(?:Bearer|bearer|BEARER|Basic|basic|BASIC)[ \t]+(?=[A-Za-z0-9._~+/=-]))?)(?:(["'`])(?:(?!\13)[^\\]|\\[\s\S]|\\$)*(\13|$)|(?:[^ \t\r\n]|[ \t]+(?![ \t#\r\n]|$))+(?:(?:(?:\r?\n|\r(?!\n))[ \t]*(?=[\r\n]))*(?:\r?\n|\r(?!\n))\2[ \t]+(?![ \t#\r\n])[^\r\n]*)*))|(?=(?:\r?\n|\r(?!\n))(?!\2[ \t])[ \t]*[^\s#]|$))/g, laterLine, /:[ \t]*(?:[&!#][^\r\n]*)?(?:[\r\n]|$)/],
  // `name: value` / `name=value`, in JSON too. A quoted value ("…", '…' or `…`; spaces and escapes
  // included) runs to its unescaped closing quote, or to the end while open. After a name, an
  // authorization scheme in any case (`authorization: bearer x`) keeps its word and hides what
  // follows; a scheme with no name is the case-sensitive rule below, so "basic steps" stays prose.
  // A name is passphrase, password, passwd, pass, secret, secret key, secret access key, token, api
  // key, private key or authorization, in any case, as a whole token with any prefix joined by _ or
  // - (db_password, x-api-key, aws_secret_access_key), also right after a - or -- (--api-key=x):
  // `bypass` and `tokenizer` are words, not names.
  // After a name and its colon, up to two YAML anchors and tags (`&a`, `!!str`, `!secret`, in either
  // order, each followed by a space or tab, on the colon's line) come before the value: they are
  // hidden with it, so `password: &a x` serves as `password: [redacted]`, the shape of the plain
  // scalar. The skip never crosses a line break, so `password: &a` then `username: bob` hides only
  // `&a` and the next line's key is never read, or remembered, as the value. Anchors and tags
  // then a block indicator (and a comment) at the end of the line are a block scalar's key line
  // (the rules above), never a value, so neither an anchor nor a tag is read as one. YAML allows
  // at most one anchor and one tag on a node: a line with three or more before its value or its
  // indicator is invalid YAML, and these rules do not match it, so it is left as written, no anchor
  // or tag is hidden as its value and nothing on it is remembered; the block rules do not read it
  // either.
  // A key line that ends at its colon, or at its anchors and tags, or at a comment, is the later-line
  // rule's above, never these rules': here a name with no value on its line reads no later line, and
  // a comment is not a value. A quoted name with nothing after its colon (`"password":`) still reads
  // the next line, as JSON may break there. A quoted name is taken whole, a colon inside it
  // included. An explicit key's `? name` line whose value sits on its `:` line, past a trailing
  // comment or a gap, is the gap rules below: they run after these, so a comment's own text is read
  // by these rules first (#500).
  // An explicit key's name is one token to these rules and to the gap rules below: the first token
  // of a `? name` line (a list item's too, anchors and tags allowed before it) is taken whole,
  // quoted or not, so a colon in the name is never read as a name and a value and no part of the
  // name is remembered (#509). A quoted name is recognised while its quote closes on its line; an
  // unquoted one is the token right after `? `, and its colon has no blank after it (with one it is
  // the name's value on its own line). The guard sits at the name/separator boundary: it reads the
  // line's end, any gap, and the `:` line, so a `? name` line with no `:` line of its own is left
  // to these rules as before. Its bound is 240 characters of name and tail; past it, or with a
  // quote left open on its line, the line is read as text here and the value on the `:` line is
  // hidden by the gap rules' opaque-name branch -- over-redaction (a name's tail with it), never
  // service. A trailing comment is still read by these rules (#500).
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}"[^"\r\n]{0,240})(?=[^"\r\n]{0,240}"|(?![^"\r\n]*:[ \t])[^"\r\n]*[\r\n]))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}'[^'\r\n]{0,240})(?=[^'\r\n]{0,240}'|(?![^'\r\n]*:[ \t])[^'\r\n]*[\r\n]))(?!(?=:(?![ \t\r\n])(?:(?!:[ \t])[^ \t"'#\r\n]){0,240}[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:)(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[A-Za-z0-9_-]{1,240}))((?:["']?\s*=|\s*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))|["']\s*:(?!(?=[ \t]+[&!#])(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))\s*)(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:(["'`])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+|(?![>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?!Bearer\b|Basic\b|\[redacted\])[^\s"',}]+)/gi, (_, name, sep, q, close, scheme) => (q ? `${name}${sep}${q}${REDACTED}${close}` : scheme ? `${name}${sep}${scheme} ${REDACTED}` : `${name}${sep}${REDACTED}`), /[:=]/],
  // The same names in camelCase or PascalCase (accessToken, ClientSecret, dbPassword, DbPass,
  // apiKey, authToken), case sensitive and whole-token, so accessLevel, MaxTokens, passRate and
  // PassengerCount stay words.
  // A name is a key only with its value, so a firstPass in prose stays a word.
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}"[^"\r\n]{0,240})(?=[^"\r\n]{0,240}"|(?![^"\r\n]*:[ \t])[^"\r\n]*[\r\n]))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}'[^'\r\n]{0,240})(?=[^'\r\n]{0,240}'|(?![^'\r\n]*:[ \t])[^'\r\n]*[\r\n]))(?!(?=:(?![ \t\r\n])(?:(?!:[ \t])[^ \t"'#\r\n]){0,240}[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:)(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[A-Za-z0-9_-]{1,240}))((?:["']?\s*=|\s*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))|["']\s*:(?!(?=[ \t]+[&!#])(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))\s*)(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:(["'`])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(Bearer|bearer|BEARER|Basic|basic|BASIC)\s+[A-Za-z0-9._~+/=-]+|(?![>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?!(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\b|\[redacted\])[^\s"',}]+)/g, (_, name, sep, q, close, scheme) => (q ? `${name}${sep}${q}${REDACTED}${close}` : scheme ? `${name}${sep}${scheme} ${REDACTED}` : `${name}${sep}${REDACTED}`), /Token|Secret|Pass|Key|Authorization/],
  // The explicit key's gap form (#496, #500): a `? name` line whose value sits on its `:` line,
  // past a trailing comment and any blank or comment lines. It runs AFTER the two rules above,
  // so the comment's own text is read by them first: a match spans the comment, the replacement
  // writes it back, and the scan continues after the value, so text a match swallows is never
  // read again (#500 served a credential in the comment that way). Both the trailing comment and
  // each gap line end at their own line end, so each matches whole and cannot back up into a
  // colon inside it (which once made a comment line the value, #500), and the `:` that ends the
  // gap must be the first non-blank character of its own line: a colon mid-line is not a
  // separator, and a `? name` line with nothing after it is not a key line here. The name may be
  // unquoted with colons in it (`? password:primary`: one token, a colon with no blank after it),
  // and a quoted one whose quote closes past the bound or not at all is opaque: both are taken
  // whole, so the value under the `:` line is hidden and remembered and no part of the name is (a
  // bound is 240 characters; an opaque name is 241 or more, or unclosed on its line).
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))((?:["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))[ \t]*)(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:(["'`])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+|(?![>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?!Bearer\b|Basic\b|\[redacted\])[^\s"',}]+)/gi, (_, name, sep, q, close, scheme) => (q ? `${name}${sep}${q}${REDACTED}${close}` : scheme ? `${name}${sep}${scheme} ${REDACTED}` : `${name}${sep}${REDACTED}`), /[:=]/],
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))((?:["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))[ \t]*)(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:(["'`])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(Bearer|bearer|BEARER|Basic|basic|BASIC)\s+[A-Za-z0-9._~+/=-]+|(?![>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?!(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\b|\[redacted\])[^\s"',}]+)/g, (_, name, sep, q, close, scheme) => (q ? `${name}${sep}${q}${REDACTED}${close}` : scheme ? `${name}${sep}${scheme} ${REDACTED}` : `${name}${sep}${REDACTED}`), /Token|Secret|Pass|Key|Authorization/],
  // An unclosed quoted explicit key with a no-space colon on its line (`? "password:hunter2`):
  // the tail reads as the value; mask it to the line end here, text only. The clean-close skip
  // uses the opening quote only, so a mismatched quote cannot pass. A quote of the other kind
  // inside the name is part of it (`? "user's_password:hunter2`, #530): the scan before the
  // credential and the `:` may cross it, or the line would serve its value.
  [/(^|[\r\n])((?:[ \t]*(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]+){0,2})(["'])(?=(?:(?!\3)[^\r\n:])*?(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(?:(?!\3)[^\r\n:])*:(?![ \t])(?![^"'\r\n]*(?<!\\)\3[ \t]*(?::[^\r\n]*|#[^\r\n]*)?(?=[\r\n]|$)))([^\r\n]+)(?=[\r\n]|$)/g, (_, nl, prefix) => `${nl}${prefix}${REDACTED}`, /:/],
  [/(^|[\r\n])((?:[ \t]*(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]+){0,2})(["'])(?=(?:(?!\3)[^\r\n:])*?(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(?:(?!\3)[^\r\n:])*:(?![ \t])(?![^"'\r\n]*(?<!\\)\3[ \t]*(?::[^\r\n]*|#[^\r\n]*)?(?=[\r\n]|$)))([^\r\n]+)(?=[\r\n]|$)/g, (_, nl, prefix) => `${nl}${prefix}${REDACTED}`, /:/],
  // JSON inside a JSON string (`{\"password\": \"…\"}`): the value runs to its escaped closing
  // quote, its own escapes doubled, or to a bare quote that ends the outer string.
  [/(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(\\"\s*:\s*)\\"((?:[^\\"]|\\[^\\"]|\\\\(?:\\[\s\S]|\\$|[^\\]|$)|\\$)*)(\\"|"|$)/gi, (_, name, sep, value, close) => `${name}${sep}\\"${REDACTED}${close}`, /\\"/],
  [/(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(\\"\s*:\s*)\\"((?:[^\\"]|\\[^\\"]|\\\\(?:\\[\s\S]|\\$|[^\\]|$)|\\$)*)(\\"|"|$)/g, (_, name, sep, value, close) => `${name}${sep}\\"${REDACTED}${close}`, /\\"/],
  [/\b(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+/g, (_, scheme) => `${scheme} ${REDACTED}`],
  // A scheme in another case with no name before it ("bearer x9…") hides only a token-shaped word:
  // 16+ characters of the token alphabet with a digit, so "bearer bonds" and "basic steps" stay
  // prose. A run still open at the end of the text is hidden until it closes, so a token streamed
  // in parts shows no prefix.
  [/\b(bearer|basic)\s+(?=[A-Za-z0-9._~+/=-]*[0-9])[A-Za-z0-9._~+/=-]{16,}/gi, (_, scheme) => `${scheme} ${REDACTED}`],
  [/\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]+$/i, (_, scheme) => `${scheme} ${REDACTED}`],
  [/\bAIza[0-9A-Za-z_-]{20,}/g, () => REDACTED],
  // Standalone sk- credentials (the contract above): a whole key, then a still-open run at the end.
  [/(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))sk-(?=[A-Za-z0-9_-]*[A-Z])(?=[A-Za-z0-9_-]*[a-z])(?=[A-Za-z0-9_-]*[0-9])[A-Za-z0-9_-]{20,}/g, () => REDACTED],
  [/(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))sk-[A-Za-z0-9_-]+$/, () => REDACTED],
  // A line a cut padded with blanks (#406 F-B): a run of blanks as long as the keep is the cut's
  // padding, not text. Its head is gone — the name on it cannot be read (a child key arrives as
  // "pass" and "word:") — so once the line has ended it is hidden whole, and while it is still
  // arriving (a growing buffer's last line, OPEN_END) its rest is withheld, as the value that
  // completes it is not remembered yet (#406's last frame). It runs LAST, on purpose: a padded line
  // that is a key line still names the value on the lines under it, and the rules above hide that
  // value first; hiding this line first would destroy the name and leave the value orphaned and
  // served, however the deltas were split (#406 CLASS). A pad-only line is the blank line the cut
  // padded: it holds nothing to hide, and the rules above read its blanks as the blank line it is
  // (a key line skips it to its value).
  [/^[^\n]*$/gm, (line, at, all) => {
    // A pad is read from its run's first blank only, so a run just short of the keep is scanned once,
    // not again from each of its blanks (it was quadratic in a growing buffer's padded last line).
    const pad = /(?<![ \t])[ \t]{4096,}/.exec(line)
    if (!pad) return line
    if (!/[^ \t]/.test(line)) return ' '
    // While the line is still arriving (a growing buffer's last line) everything after the pad is
    // withheld: the head is cut, so the line cannot be read, and no rule above has served it.
    return OPEN_END && at + line.length === all.length ? line.slice(0, pad.index + pad[0].length) : REDACTED
  }, /(?<![ \t])[ \t]{4096,}/],
]

// A YAML key line under a credential name, read as untrusted text, not validated YAML: from the
// line end before it (group 1) and its indent (2), the name anywhere on the line as a whole token,
// quoted or not, then its `:`, an anchor (&a) and a tag (!!str) in either order, the `|` or `>`
// with its indicators and a comment, up to its own line end. A character that prints nothing
// (NUL, DEL, zero-width, BOM) counts as a space between them. A quoted name is read whole, a colon
// inside it included. The last two read an explicit key (`? password`, its `: |` on a later line,
// past any blank or comment lines): their indent is the key line's when the `:` line starts with
// it, otherwise none, so the block is hidden whole either way. Each is one pass: a blank or comment
// line skips no gap as a key line, so a gap is read once.
const YAML_KEY = [
  /(^|\r\n|\r(?!\n)|\n)(?=[^\r\n]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[&!>|])(?=([ \t]*))\2([^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)))((?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*["'])?[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))/gi,
  /(^|\r\n|\r(?!\n)|\n)(?=[^\r\n]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[&!>|])(?=([ \t]*))\2([^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)))((?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*["'])?[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))/g,
  /(^|\r\n|\r(?!\n)|\n)(?=(?:(?![\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#|[\r\n]|$))[^\r\n]*(?:\r\n|\r(?!\n)|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*|[^\r\n]*(?:\r\n|\r|\n))[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))(?=((?:(?=([ \t]*))\3(?=[^\r\n]*(?:\r\n|\r|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*\3[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:))?))([^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n]))))((?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*["'])?[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))/gi,
  /(^|\r\n|\r(?!\n)|\n)(?=(?:(?![\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#|[\r\n]|$))[^\r\n]*(?:\r\n|\r(?!\n)|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*|[^\r\n]*(?:\r\n|\r|\n))[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))(?=((?:(?=([ \t]*))\3(?=[^\r\n]*(?:\r\n|\r|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*\3[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:))?))([^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n]))))((?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*["'])?[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))/g,
]

/**
 * Where the block under a YAML key line ends: `at` is the key line's own line end, `indent` its
 * indent. A line scanner. The key line's line end (\n, \r\n or a lone \r, each one line end; an
 * LF or CR key's \r\n too) ends a block line, and any other \r or \n is a stray inside the value.
 * A line ends the block only when its first other character comes after spaces alone, no more of
 * them than its key's indent (a prefix of it). Any other line is value: a blank one, a deeper one,
 * one with a tab, a stray or a character that prints nothing (NUL, DEL, zero-width) before its
 * first other character. The block ends before the line end of that line, the less-indented line
 * after it, shown.
 */
function yamlBlockEnd(s, at, indent) {
  const eol = s.startsWith('\r\n', at) ? '\r\n' : s.charAt(at)
  let end = at
  while (eol && s.startsWith(eol, end)) {
    let from = end + eol.length
    if (eol === '\r' && s[from] === '\n') from++
    let next = s.indexOf(eol, from)
    if (next < 0) next = s.length
    const lead = /^[\s\x00-\x08\x0E-\x1F\x7F-\x9F\xAD\u061C\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064]*/.exec(s.slice(from, next))[0]
    if (from + lead.length < next && /^ *$/.test(lead) && indent.startsWith(lead)) break
    end = next
  }
  return end
}

// The YAML rule of SECRET_TEXT: a block with more than whitespace becomes one line under its key,
// the key line's own line end and indent then `  [redacted]`.
function redactYamlBlocks(s) {
  let out = s
  for (const re of YAML_KEY) {
    let done = ''
    let from = 0
    re.lastIndex = 0
    for (let m = re.exec(out); m; m = re.exec(out)) {
      const at = m.index + m[0].length
      const end = yamlBlockEnd(out, at, m[2])
      const block = out.slice(at, end)
      done += `${out.slice(from, at)}${/\S/.test(block) ? `${block.match(/^(?:\r\n?|\n)/)[0]}${m[2]}  ${REDACTED}` : block}`
      from = re.lastIndex = end
    }
    out = `${done}${out.slice(from)}`
  }
  return out
}

// #590: the explicit-key guard in the name rules scans a bounded quote tail at every name. On a
// text with no `? name` line the guard can never fail, so dropping it is exact; the rule is
// compiled with and without it and chosen per text, keeping an unclosed quote run linear (#172).
const EXPLICIT_KEY = /(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t]/
const GUARD_START = '(?!(?<=(?:^|[\\r\\n])[ \\t]{0,160}'
const KEY_TAIL_START = String.raw`|(?<=(?:^|[\r\n])[ \t]{0,256}`
const QUOTE_TAIL = String.raw`|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}`
  + String.raw`|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])`
  + String.raw`|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n]))`
function withoutGuard(source) {
  let out = source
  for (let at = out.indexOf(GUARD_START); at >= 0; at = out.indexOf(GUARD_START)) {
    let depth = 0
    let end = at
    for (; end < out.length; end++) {
      if (out[end] === '\\') { end++; continue }
      if (out[end] === '(') depth++
      else if (out[end] === ')' && --depth === 0) { end++; break }
    }
    out = out.slice(0, at) + out.slice(end)
  }
  for (let at = out.indexOf(KEY_TAIL_START); at >= 0; at = out.indexOf(KEY_TAIL_START)) {
    const next = out.indexOf(QUOTE_TAIL, at)
    if (next < 0) break
    out = out.slice(0, at) + out.slice(next)
  }
  return out.split(QUOTE_TAIL).join('')
}
function nameRule(pattern, replace) {
  const plain = new RegExp(withoutGuard(pattern.source), pattern.flags)
  return (text) => (EXPLICIT_KEY.test(text)
    ? text.replace(pattern, replace)
    : text.replace(plain, replace))
}
for (const rule of SECRET_TEXT) {
  if (typeof rule[0] !== 'function' &&
    (rule[0].source.includes(GUARD_START) || rule[0].source.includes(QUOTE_TAIL))) {
    rule[0] = nameRule(rule[0], rule[1])
    rule[1] = null
  }
}
// #507: a value on the indented lines under an explicit key. A `? name` line whose name holds a
// credential stem (quoted or not, a list item, anchors and tags before it, a colon inside the
// name) claims the more-indented lines under it as a value: a scalar line and a child key's value
// are hidden there, and once such a line sits in the gap its `:` line's value is hidden too (an
// inline value, the lines under a block indicator, or the lines under an empty colon). A clean
// `? name` ... `:` shape is left to the gap and block rules. Read as untrusted text, not as
// validated YAML: over-hiding a value block is the cost, never service.
const KEY_NAME_LOWER = /(?:(?<![A-Za-z0-9_-])|(?<=[_-]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-])/i
const KEY_NAME_CAMEL = /(?:(?<![A-Za-z0-9_-])|(?<=[_-]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-])/
const KEY_PROPS = /^(?:[&!][^\s"'`]*[ \t]+){0,2}/
const KEY_BLOCK = /^[>|][-+0-9]*[ \t]*(?:#[^\r\n]*)?$/

/**
 * The name on an explicit key line: a quoted token whole (any colon in it), or the first token. A
 * quoted name ends only at its real closing quote: an escaped \" inside double quotes and a doubled
 * '' inside single quotes are part of the name. With no closing quote, the rest of the line is it.
 */
function explicitKeyName(text) {
  const tail = text.slice(KEY_PROPS.exec(text)[0].length)
  const quote = tail[0]
  if (quote === '"' || quote === "'") {
    for (let i = 1; i < tail.length; i++) {
      if (quote === '"' && tail[i] === '\\') i++
      else if (tail[i] === quote && quote === "'" && tail[i + 1] === "'") i++
      else if (tail[i] === quote) return tail.slice(1, i)
    }
    return tail.slice(1)
  }
  return /^\S*/.exec(tail)[0]
}

function credentialName(name) {
  // An escaped explicit name is opaque, so its value is hidden.
  if (name.includes('\\')) return true
  const plain = INVISIBLE_GATE.test(name) ? name.replace(INVISIBLE, '') : name
  return KEY_NAME_LOWER.test(plain) || KEY_NAME_CAMEL.test(plain)
}

/**
 * The value a hidden line holds, for `secretsIn`: a child key's value, else the line's first word.
 * A list item's `- ` marks are not the value: the item after them is.
 */
function explicitLineValue(content) {
  const item = content.replace(/^(?:-[ \t]+)+/, '')
  const child = /^[^ \t"'][^:]*?:[ \t]+(\S.*)$/.exec(item)
  const text = child ? child[1] : item
  const quoted = /^(["'`])((?:\\.|(?!\1)[^\\])*?)\1/.exec(text)
  if (quoted) return quoted[2]
  const first = /^(?:[&!][^\s"'`]*[ \t]+)*([^\s"'`]+)/.exec(text)
  return first ? first[1] : ''
}

/** The `:` line's value: an inline one hidden in place, or the lines under it hidden whole. */
function explicitColonHits(s, line, start, lineEnd) {
  const at = line.indexOf(':')
  const blanks = /^[ \t]*/.exec(line.slice(at + 1))[0]
  const props = KEY_PROPS.exec(line.slice(at + 1 + blanks.length))[0]
  const rest = line.slice(at + 1 + blanks.length + props.length)
  const hits = []
  if (rest && rest[0] !== '#' && !KEY_BLOCK.test(rest)) {
    hits.push({ start, end: lineEnd, text: `${line.slice(0, at + 1)}${blanks}${REDACTED}`, value: explicitLineValue(rest), ended: lineEnd })
    return hits
  }
  // A block indicator or an empty colon: the lines under it, more indented than it, are its value.
  let cursor = lineEnd
  if (s.startsWith('\r\n', cursor)) cursor += 2
  else if (s[cursor] === '\r' || s[cursor] === '\n') cursor += 1
  else return hits
  const indent = /^[ \t]*/.exec(line)[0].length
  let first = true
  while (cursor < s.length) {
    let end = cursor
    while (end < s.length && s[end] !== '\r' && s[end] !== '\n') end++
    const body = s.slice(cursor, end)
    const lead = /^[ \t]*/.exec(body)[0]
    const content = body.slice(lead.length)
    if (!content || content[0] === '#') { /* a blank or comment line stays */ }
    else if (lead.length > indent) {
      hits.push({ start: cursor, end, text: `${lead}${REDACTED}`, value: first ? explicitLineValue(content) : '', ended: end })
      first = false
    } else break
    if (s.startsWith('\r\n', end)) cursor = end + 2
    else cursor = end + 1
  }
  return hits
}

/**
 * The lines under an explicit credential key that hide a value (#507), as replacements. The `?`
 * line itself stays as written. A key line already covered by an outer key's block is skipped, so
 * nested keys cost one pass.
 */
function explicitKeyHits(s) {
  if (!s.includes('?')) return []
  const hits = []
  const keyLine = /(^|[\r\n])([ \t]*)(?:-[ \t]+)?\?[ \t]([^\r\n]*)/g
  let covered = 0
  for (let m = keyLine.exec(s); m; m = keyLine.exec(s)) {
    if (m.index < covered) continue
    if (!credentialName(explicitKeyName(m[3]))) {
      // A plain name of several words (`? db password`): the rest of the line, minus a comment.
      const rest = m[3].slice(KEY_PROPS.exec(m[3])[0].length).replace(/(?:^|[ \t])#.*$/, '')
      if (!credentialName(rest)) continue
    }
    // #530: a plain name of several words (`? password db`) is more than the one token the gap
    // rules read, so its `:` line right under it is taken here even with nothing in the gap.
    const plain = m[3].slice(KEY_PROPS.exec(m[3])[0].length).replace(/(?:^|[ \t])#.*$/, '')
    const several = !/^["']/.test(plain) && /\S[ \t]+\S/.test(plain)
    const opaque = explicitKeyName(m[3]).includes('\\')
    const keyIndent = m[2].length
    let cursor = m.index + m[0].length
    if (s.startsWith('\r\n', cursor)) cursor += 2
    else if (s[cursor] === '\r' || s[cursor] === '\n') cursor += 1
    else continue
    let seen = false
    while (cursor < s.length) {
      let end = cursor
      while (end < s.length && s[end] !== '\r' && s[end] !== '\n') end++
      const line = s.slice(cursor, end)
      const lead = /^[ \t]*/.exec(line)[0]
      const content = line.slice(lead.length)
      if (content && content[0] === ':') {
        if (seen || several || opaque || lead.length < m[0].length - m[1].length - m[3].length - 2) hits.push(...explicitColonHits(s, line, cursor, end))
        break
      }
      if (content && content[0] !== '#' && lead.length > keyIndent) {
        hits.push({ start: cursor, end, text: `${lead}${REDACTED}`, value: explicitLineValue(content), ended: end })
        seen = true
      } else if (content && content[0] !== '#' && lead.length <= keyIndent) break
      if (s.startsWith('\r\n', end)) cursor = end + 2
      else cursor = end + 1
    }
    if (hits.length) covered = Math.max(covered, hits[hits.length - 1].end)
  }
  return hits
}

function redactExplicitKeyValues(s) {
  const hits = explicitKeyHits(s)
  if (!hits.length) return s
  hits.sort((a, b) => a.start - b.start)
  let out = ''
  let from = 0
  for (const hit of hits) {
    if (hit.start < from) continue
    out += `${s.slice(from, hit.start)}${hit.text}`
    from = hit.end
  }
  return out + s.slice(from)
}

// A closed quoted explicit name followed by more text is malformed YAML. Read its whole
// line as an opaque name and hide any indented value and following colon value (#785).
function redactMalformedExplicitKeys(s) {
  if (!/\?[ \t]/.test(s)) return s
  const keys = /(^|\r\n|\r(?!\n)|\n)([ \t]*)(-[ \t]+)?\?[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}("(?:[^"\\\r\n]|\\[^\r\n])*"|'(?:[^'\r\n]|'')*')([^\r\n]*)/g
  let done = ''
  let from = 0
  for (let m = keys.exec(s); m; m = keys.exec(s)) {
    if (/^[ \t]*(?:#[^\r\n]*|:[^\r\n]*)?$/.test(m[5]) || !NAME_STEM.test(m[4] + m[5])) continue
    const indent = m[2] + (m[3] ? ' '.repeat(m[3].length) : '')
    const at = m.index + m[0].length
    let end = yamlBlockEnd(s, at, indent)
    const colon = /^(?:(?:\r\n|\r(?!\n)|\n)[ \t]*(?:#[^\r\n]*)?)*(?:\r\n|\r(?!\n)|\n)([ \t]*):[^\r\n]*/.exec(s.slice(end))
    if (colon && colon[1] === indent) {
      end += colon[0].length
      end = yamlBlockEnd(s, end, indent)
    }
    const block = s.slice(at, end)
    done += s.slice(from, at) + (/\S/.test(block) ? `${block.match(/^(?:\r\n|\r|\n)/)[0]}${indent}  ${REDACTED}` : block)
    from = keys.lastIndex = end
  }
  return done + s.slice(from)
}

export function redactText(s, open = false) {
  const was = OPEN_END
  OPEN_END = open
  try {
    let out = redactMalformedExplicitKeys(nameText(s))
    for (const [re, fn, gate] of SECRET_TEXT) if (!gate || gate.test(out)) out = typeof re === 'function' ? re(out) : out.replace(re, fn)
    return out
  } finally {
    OPEN_END = was
  }
}

// Key-aware masking: the same as src/client/redact.js `SECRET_KEY` and `redact`; keep them in step. A value under a
// secret-looking key is replaced whatever it is; strings elsewhere get the text patterns; objects
// and arrays are walked with their keys.
const SECRET_KEY = /(token|secret|passw(or)?d|passphrase|(?:^|[_-])pass$|api[_-]?key|private[_-]?key|authorization|bearer|cookie|credential)/i
// Invisible characters inside a credential name (`pass\u200bword: x`, `{"pass\u0000word": …}`): NUL and
// the other C0 controls but tab and line ends, DEL, and the Unicode format characters (Cf: zero-width
// space and joiners, bidi marks, word joiner, BOM, soft hyphen), raw or as a JSON \u escape. A name
// read with them is the same name. `redact` tests a key without them and keeps the key as written.
// `nameText` (the first step of `redactText`) drops them from a run of name characters that a
// separator ends (`:`, `=`, a quoted or escaped JSON key's `":` or `\":`) and that holds a credential
// stem, and right after that separator, so `pass\u200bword: x` serves as `password: [redacted]`.
// Nothing else changes: an emoji's joiner, a ZWNJ in Persian text, and an invisible inside a word
// with no separator after it, or in a key with no credential stem, stay. A run starts only after a
// character that is not a name character, an invisible or a backslash (a \n \r \t escape excepted),
// so each run is read once.
const INVISIBLE_GATE = /[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\u/u
const INVISIBLE = /[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff])/gu
const INVISIBLE_NAME = /(?<![A-Za-z0-9_-]|[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff]))(?:(?<!\\)|(?=[nrt]))((?:[A-Za-z0-9_-]|[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff]))+)((?:\\?["'])?[ \t]*[:=][ \t]*)((?:[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff]))*)/gu
const NAME_STEM = /pass|secret|token|key|authorization/i
// The name on a YAML explicit key line: `? name`, quoted or not, after a list item's `- ` and any
// anchors or tags, then spaces or a comment to its line end (or a colon and its value). A quoted
// name is read whole, a colon inside it included. Its `:` line is what makes it a key line; the
// name is read without its invisible characters here, before the rules look for it, so the value
// under it is found. Only the name changes; a line with no credential stem stays.
const INVISIBLE_EXPLICIT = /(^|\r\n|\r(?!\n)|\n)([ \t]*(?:-[ \t]+)?\?[ \t][^\r\n#]*?["']?)((?:[A-Za-z0-9_:-]|[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff]))+)(["']?[ \t]*(?:#[^\r\n]*)?(?=\r\n|\r(?!\n)|\n|$)|["']?[ \t]*:[^\r\n]*)/gu

export function nameText(s) {
  if (!INVISIBLE_GATE.test(s)) return s
  return s
    .replace(INVISIBLE_NAME, (m, run, sep, after) => {
      const name = run.replace(INVISIBLE, '')
      return (name !== run || after) && NAME_STEM.test(name) ? `${name}${sep}` : m
    })
    .replace(INVISIBLE_EXPLICIT, (m, nl, head, run, tail) => {
      const name = run.replace(INVISIBLE, '')
      return name !== run && NAME_STEM.test(name) ? `${nl}${head}${name}${tail}` : m
    })
}

export function redact(value, key = '') {
  if (key && SECRET_KEY.test(key.replace(INVISIBLE, '')) && value !== null && value !== undefined && typeof value !== 'boolean') return REDACTED
  if (typeof value === 'string') return redactText(value)
  if (Array.isArray(value)) return value.map((v) => redact(v))
  if (value && typeof value === 'object') {
    const out = {}
    for (const [k, v] of Object.entries(value)) out[k] = redact(v, k)
    return out
  }
  return value
}

// The envelope's own stamps (catalog.js ENVELOPE_KEYS): protocol identities, never redacted.
const ENVELOPE = new Set(['v', 'id', 'producer', 'seq', 't_ns', 'source', 'cause', 'received', 'observed', 'kind'])

/** An envelope with every payload field redacted (key-aware), its stamps kept. A media chunk's
 * base64 `data` is kept as it is: it is only counted (audioOf) and never held or emitted. */
export function redactEnvelope(env, entry) {
  const out = {}
  for (const [k, v] of Object.entries(env)) out[k] = ENVELOPE.has(k) || (entry?.media && k === 'data') ? v : redact(v, k)
  return out
}

/** The shown text: the redaction of the whole joined (still growing) text, THEN its tail. */
function shownTail(raw) {
  return redactText(raw, true).slice(-TEXT_TAIL)
}

// A credential keyword or assignment right before a cut would leave its value orphaned.
// Tested on the text before the cut with its trailing whitespace removed, so a long blank run
// between a keyword and its value cannot hide the keyword. Anchors and tags after a colon count on
// the colon's line only, as in SECRET_TEXT. A key line whose value comes on a later line (#396),
// then the blank, comment, anchor and tag lines or the block indicator under it, counts too: when
// the text before the cut ends on such a line, the cut is read as openKeyLine reads one.
const CONTEXT_BEFORE = /(?:\b(?:Bearer|Basic)|[:=]["']?|:[ \t]*(?:[&!][^\s"'`]*[ \t]+)?[&!][^\s"'`]*)$/i
const LATER_LINE_BEFORE = /(?:^|[\r\n])[ \t]*[#&!>|][^\r\n]*$|:(?:[ \t]+[&!][^\s"'`]*){0,2}[ \t]+#[^\r\n]*$/
function contextBefore(raw, cut) {
  const before = raw.slice(0, cut).trimEnd()
  return CONTEXT_BEFORE.test(before.slice(-64)) || (LATER_LINE_BEFORE.test(before.slice(-256)) && openKeyLine(`${before}\n`, RAW_MAX) !== null)
}

// A key line still waiting for its value or block indicator (`password:`, `password: &a`,
// `password: &a !!str`, a comment after them or not), then a blank run on that line. When a cut
// lands in the run, the line is kept with the run cut to one space, so the indicator and a block's
// lines that arrive next are read under their key; the bare last word (`&a`) would lose it.
// A key line whose value comes on a later line (#396) is kept the same way when the cut lands in
// the lines under it: blank lines, comment lines, lines of anchors and tags alone and a block
// indicator line, the last two more indented than the key, then the line the cut is in (blank, a
// comment, anchors and tags or an indicator, then a blank run). The key line is kept, then its
// anchor, tag and indicator lines, each indented one more than the key; the blank and comment lines
// between are dropped, as they never hold the value, and the line the cut is in is kept as it reads
// (a blank run cut to the key's indent and one more), so the value that arrives next is still read
// under its key. A line ends at \n, \r\n or a lone \r; up to 64 lines and 4 KiB above it are read,
// a blank run at the cut not counted against the 4 KiB (it is the cut's padding, not text).
const KEY_LINE_OPEN = /:[ \t]*(?:[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)*)?(?:[ \t]+#[^\r\n]*)?$/
// An explicit key's colon line with a block indicator on it (`: |`, `: &a >-`, a comment after them).
const EX_COLON = /^[ \t]*:[ \t]*(?:[&!][^\s"'`]*[ \t]+){0,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*$/
const PROPERTY_LINE = /^[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?$/
const INDICATOR_LINE = /^[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?$/
const SKIPPED_LINE = /^[ \t]*(?:#[^\r\n]*)?$/
function openKeyLine(raw, max) {
  let end = raw.length
  while (end > 0 && raw[end - 1] !== '\n' && raw[end - 1] !== '\r') end--
  const last = raw.slice(end)
  const line = last.trimEnd()
  // An explicit key (#406): a `? name` line is a key line, and a `:` line at its indent right under
  // it is kept with it, so the value that arrives under the `:` line is still read under its name.
  // A list item's `- ? name` keeps the same way: its marker reaches the `:` line's column.
  const keyOf = (colon, key) => {
    const indent = /^[ \t]*/.exec(colon)[0]
    const list = /^[ \t]*-[ \t]+$/.test(key?.slice(0, indent.length) ?? '')
    return colon[indent.length] === ':' && key?.[indent.length] === '?' && (list || key.startsWith(indent)) && /^[ \t]/.test(key.slice(indent.length + 1)) && key.length + colon.length < max / 2 ? `${key}\n` : ''
  }
  const lineAbove = () => {
    const from = Math.max(0, end - 4096)
    const ls = raw.slice(from, end).split(/\r\n|\r|\n/)
    ls.pop()
    return ls.length > 1 || (ls.length === 1 && from === 0) ? ls.at(-1).trimEnd() : undefined
  }
  if (line.length < max && (KEY_LINE_OPEN.test(line.slice(-256)) || EX_COLON.test(line) || /^[ \t]*(?:-[ \t]+)?\?[ \t]/.test(line))) return `${end > 0 ? keyOf(line, lineAbove()) : ''}${line} `
  const under = (l) => PROPERTY_LINE.test(l.slice(-256)) || INDICATOR_LINE.test(l.slice(-256))
  if (end === 0 || (line && !SKIPPED_LINE.test(line) && !under(line))) return null
  // A blank run at the cut is the cut's padding, not text: the read starts 4 KiB back from the last
  // text, not from the run, so a key or question line before a pad longer than the read is still read.
  let blank = 0
  while (end - blank > 0 && (raw[end - blank - 1] === ' ' || raw[end - blank - 1] === '\t' || raw[end - blank - 1] === '\r' || raw[end - blank - 1] === '\n')) blank++
  const start = Math.max(0, end - blank - 4096)
  const lines = raw.slice(start, end).split(/\r\n|\r|\n/)
  lines.pop() // the empty text after the last line break
  const kept = []
  for (let i = lines.length - 1; i >= 0 && lines.length - i <= 64; i--) {
    const above = lines[i].trimEnd()
    if (SKIPPED_LINE.test(above)) continue
    if (under(above)) {
      kept.unshift(above)
      continue
    }
    if (i === 0 && start > 0) return null // cut by the 4 KiB read: not a whole line
    const indent = /^[ \t]*/.exec(above)[0]
    // A `? name` line (#406 FAIL 3) is a key line too: a pad between it and its `:` line drops the
    // `:` line with the rest of the text, and the question line is what the value arrives under.
    if (above.length >= max / 2 || !(KEY_LINE_OPEN.test(above.slice(-256)) || EX_COLON.test(above.slice(-256)) || /^[ \t]*(?:-[ \t]+)?\?[ \t]/.test(above))) return null
    const deeper = (l) => l.startsWith(indent) && /^[ \t]/.test(l.slice(indent.length))
    if (!kept.every(deeper) || (line && !SKIPPED_LINE.test(line) && !deeper(line))) return null
    const cutLine = !line ? last.slice(0, indent.length + 1) : SKIPPED_LINE.test(line) ? `${line} ` : `${indent} ${line.trimStart()} `
    const key = keyOf(above, i > 1 || (i === 1 && start === 0) ? lines[i - 1].trimEnd() : undefined)
    const out = [`${key}${above}`, ...kept.map((l) => `${indent} ${l.trimStart()}`), cutLine].join('\n')
    return out.length < max ? out : null
  }
  return null
}

// Where a quoted credential value opens (the quoted forms of SECRET_TEXT): a trim never cuts
// inside one, since whitespace within quotes is not a boundary, and only an unescaped
// quote ends one. Each match ends at its opening quote; a value on a later line (#396) is matched
// from the start of its key line.
const QUOTED_SECRET = [
  /\b[A-Z0-9_]+=(?<=(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*=)(["'`])/g,
  /(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)["']?\s*[:=]\s*(?!(?:[&!][^\s"'`]*[ \t]+){3})(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(["'`])/gi, // any prefix
  /(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)["']?\s*[:=]\s*(?!(?:[&!][^\s"'`]*[ \t]+){3})(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(["'`])/g, // camelCase
  /(?:^|(?<=[\r\n]))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*\?[ \t][^\r\n]*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\1[^\r\n#]*?(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:["']?[ \t]*:|["']?[ \t]*(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))\1:)(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\1[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*(?:\r?\n|\r(?!\n))\1[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(["'`])/gi, // on a later line (#396)
  /(?:^|(?<=[\r\n]))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*\?[ \t][^\r\n]*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\1[^\r\n#]*?(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:["']?[ \t]*:|["']?[ \t]*(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))\1:)(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\1[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*(?:\r?\n|\r(?!\n))\1[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(["'`])/g, // on a later line, camelCase
]


// Where JSON inside a JSON string opens a credential value (`{\"password\": \"`), as in SECRET_TEXT,
// and where it ends: its escaped closing quote (its own escapes doubled), a bare quote that ends
// the outer string, or the end of the text while it is open.
const ESCAPED_SECRET = [
  /(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)\\"\s*:\s*\\"/gi,
  /(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)\\"\s*:\s*\\"/g,
]
const ESCAPED_END = /(?:[^\\"]|\\[^\\"]|\\\\(?:\\[\s\S]|\\$|[^\\]|$)|\\$)*(\\"|"|$)/y

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

/**
 * A `:` at the start of a line (past blanks that are never a line end): the only place the last
 * two YAML key rules can find the colon their lookahead requires, so a text without one skips
 * them (in `[quotedSpans]`).
 */
const COLON_LINE = /(?:^|[\r\n])[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:/
// Any quote a quoted credential value can open with: the gate of the quoted-span scan.
const QUOTE = /["'`]/

/**
 * Quoted credential spans and YAML blocks in `raw`: {start, end, closed, quote, escaped}, and for a
 * block what to `keep` while it is open; an open one runs to the end.
 */
function quotedSpans(raw) {
  const spans = []
  // Every quoted rule's match ends at its opening quote, so a text without one skips all four:
  // a block of plain lines has nothing for them to find, and the scan is the whole cost there.
  if (QUOTE.test(raw)) for (const re of QUOTED_SECRET) {
    for (const m of raw.matchAll(re)) {
      const quote = m[0].slice(-1) // the match ends at its opening quote
      const { close, escaped } = closingQuote(raw, m.index + m[0].length, quote)
      spans.push({ start: m.index, end: close < 0 ? raw.length : close + 1, closed: close >= 0, quote, escaped })
    }
  }
  if (raw.includes('\\"')) for (const re of ESCAPED_SECRET) {
    for (const m of raw.matchAll(re)) {
      const open = m.index + m[0].length
      ESCAPED_END.lastIndex = open
      const closed = ESCAPED_END.exec(raw)[1] !== ''
      // An open one keeps its key and opening quote, and the escape it ends inside.
      let run = 0
      while (!closed && raw.length - run > open && raw[raw.length - 1 - run] === '\\') run++
      spans.push({ start: m.index, open, end: ESCAPED_END.lastIndex, closed, quote: '"', escaped: false, keep: closed ? undefined : `${raw.slice(m.index, open)}${'\\'.repeat(run % 4)}` })
    }
  }
  if (/:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[&!>|]/.test(raw)) for (const [rule, re] of YAML_KEY.entries()) {
    // The last two rules read an explicit key (`? name`, then its `:` line): their lookahead
    // finds a `:` at the start of a line (past blanks) or matches nothing, so a text without one
    // skips them. Each looks ahead from every line start it can (a lone \r with text after it
    // too), and a block key line's blank lines are what it walks, so the skip is what keeps a
    // 40k-blank-line block's scan off the per-line cost (#406's fast budget).
    if (rule > 1 && !COLON_LINE.test(raw)) continue
    re.lastIndex = 0
    for (let m = re.exec(raw); m; m = re.exec(raw)) {
      const start = m.index + m[1].length
      const at = m.index + m[0].length
      const end = (re.lastIndex = yamlBlockEnd(raw, at, m[2]))
      // An open block keeps its key line (for an indicator on a later line, the lines down to it), its
      // blank run dropped, and a fresh block line, so the rest arrives under its key, after the
      // block's own line end. A text that ends mid-line (on a stray \r or \n too) keeps an indented
      // line, and one that ends on a CRLF's \r keeps the \r, so either next delta stays in it.
      const eol = end === at ? '\n' : raw.startsWith('\r\n', at) ? '\r\n' : raw[at]
      const keep = `${raw.slice(start, at).trimEnd()}${eol}${raw.endsWith(eol) ? '' : `${m[2]} ${eol === '\r\n' && raw.endsWith('\r') ? '\r' : ''}`}`
      spans.push({ start, end, closed: end < raw.length, quote: null, escaped: false, keep })
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
/**
 * Where a pad's reading stops: its end, or past its line's terminator when the pad fills its line
 * (a blank line the cut padded, read as the blank line it is, so no blank line is kept for it).
 */
function padLineEnd(raw, pad) {
  const end = pad.index + pad[0].length
  if (pad.index > 0 && !/[\r\n]/.test(raw[pad.index - 1])) return end
  return raw.startsWith('\r\n', end) ? end + 2 : /[\r\n]/.test(raw[end] ?? '') ? end + 1 : end
}

/** A completed line longer than LINE_MAX, as an oversize open line is committed. */
function oversize(line) {
  const body = line.replace(/(?:\r\n|\n|\r)$/, '')
  return body.length > LINE_MAX ? `${body.slice(0, LINE_MAX)}${OVER_PAD}${body.slice(-LINE_TAIL)}${line.slice(body.length)}` : line
}

/**
 * The end of the last line terminator in `s` at or after `from`, read backwards: a \n, or a \r
 * with something after it (a \r\n pair ends after its \n). A lone \r at the very end of `s` is a
 * line still arriving, not one, so it is skipped.
 */
function lastLineEnd(s, from) {
  for (let i = s.length - 1; i >= from; i--) {
    const c = s.charCodeAt(i)
    if (c === 10) return i + 1
    if (c === 13 && i + 1 < s.length) return s.charCodeAt(i + 1) === 10 ? i + 2 : i + 1
  }
  return from
}

function boundRaw(text) {
  // Names read without their invisible characters (#348), as redactText reads them, so the spans
  // and the context before a cut are found under `pass\u200bword: ` too.
  const raw = nameText(text)
  if (raw.length <= RAW_MAX) return { raw, orphan: null }
  const spans = quotedSpans(raw)
  let cut = raw.length - RAW_MAX
  // A cut before or inside a pad (a blank run as long as the keep) keeps the open key line before it
  // and drops the pad, as a cut in the blank run that ends a key line does below (#406 Design A):
  // a key line padded past the bound (`? name`, `- ? name`, `name:` and a pad) still names the
  // value that arrives on the lines under it, whatever the split of the deltas that crossed the
  // bound (a 1-character delta first cuts inside the `? name` itself).
  for (const pad of raw.matchAll(/(?<![ \t])[ \t]{4096,}/g)) {
    if (cut >= pad.index + pad[0].length) continue
    const end = padLineEnd(raw, pad)
    const key = openKeyLine(raw.slice(0, end), RAW_MAX)
    const kept = key === null ? null : `${key}${raw.slice(end)}`
    if (kept !== null && kept.length <= RAW_MAX) return { raw: kept, orphan: null }
    // A cut on the pad's own line that keeps no key line: the line came whole (an open line is
    // held to its terminator) and is a cut-padded line now, hidden whole as the rules hide one, so
    // the rest of it goes with the pad and the lines after it are read as written. A quoted value
    // that runs on past the line's end is left to the span rules below, which withhold it.
    const lineStart = Math.max(raw.lastIndexOf('\n', pad.index - 1), raw.lastIndexOf('\r', pad.index - 1)) + 1
    if (cut >= lineStart) {
      const next = raw.slice(pad.index + pad[0].length).search(/\r\n|\n|\r/)
      const after = next < 0 ? raw.length : pad.index + pad[0].length + next
      if (!spans.some((sp) => sp.start < after && (!sp.closed || sp.end > after))) {
        return { raw: next < 0 ? '' : raw.slice(raw.startsWith('\r\n', after) ? after + 2 : after + 1), orphan: null }
      }
    }
    break
  }
  for (let i = 0; i < 8; i++) {
    const inside = spans.find((sp) => sp.start < cut && cut < sp.end)
    if (inside && !inside.closed) return inside.keep !== undefined ? { raw: inside.keep, orphan: null } : { raw: '', orphan: { quote: inside.quote, escaped: inside.escaped } }
    if (inside) cut = inside.end // past the closed value, whose key is dropped with it
    const ws = raw.slice(cut).search(/\s/)
    if (ws < 0) return { raw: '', orphan: { token: true } }
    cut += ws
    const next = raw.slice(cut).search(/\S/)
    if (next < 0) {
      // Only whitespace is left after the cut: keep an open key line, or else the last word, so
      // a key or a keyword such as "Bearer" still precedes what the next delta brings.
      const line = openKeyLine(raw, RAW_MAX)
      if (line !== null) return { raw: line, orphan: null }
      const lastAt = raw.search(/(?<!\S)\S+\s*$/)
      const last = lastAt < 0 ? '' : raw.slice(lastAt)
      // A word inside a quoted value the cut skipped is that value, dropped with its key.
      if (lastAt >= 0 && spans.some((sp) => sp.start <= lastAt && lastAt < sp.end)) return { raw: '', orphan: null }
      return last.length <= RAW_MAX ? { raw: last, orphan: null } : { raw: '', orphan: { token: true } }
    }
    cut += next
    if (!contextBefore(raw, cut) && !spans.some((sp) => sp.start < cut && cut < sp.end)) return { raw: raw.slice(cut), orphan: null }
  }
  return { raw: '', orphan: { token: true } }
}
