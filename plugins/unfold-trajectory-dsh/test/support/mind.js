// The Mind stream's model: a bounded list of rows, each one observed event as one sentence, in
// the order Mind observed them (per lane; no clock is ever merged with another), grouped into
// episodes ONLY through explicit ids, plus gap markers wherever the evidence is incomplete.
//
// Lanes and sources: `agentd` rows come from the unfold-events observer feed (or,
// optionally, the agentd journal file); `host` rows from the host journal; both are source
// Unfold. `mind` rows are this plugin's own gap markers, source UI. Brain feeds nothing yet.
//
// Episodes are unions over explicit keys only:
//   r:<response_id>   every reply-scoped event (deltas, audio, done, cancel, playback, signals)
//   c:<command_id>    typed requests (command_submitted / command_admitted / user_text request_id,
//                     host command_* notes); a reply joins it through response_started.request_id
//                     ONLY when that id names a typed request (a voice reply's request_id is the
//                     microphone stream, e.g. mic-1, shared by every turn: never an episode key)
//   d:<decision_id>   decisions, and command_admitted.decision_id
//   u:<utterance_id>  transcripts and replies that carry one
//   e:<event id>      an event's own id; a `cause` joins its parent only when that parent is
//                     itself an episode row (a stream chunk or a connection frame is not)
//   decision.trigger_event `host:<id>` joins c:<id>.
// Decision.episode_id is the attachment label (session-wide): shown as an id, never a key.
//
// Identity: every Mind lifetime has its own random nonce, and every row and
// episode id carries it, so an id saved by one lifetime (in a suggestion, a copied evidence
// record, a browser selection) never names an unrelated event of a later one.
//
// Retention: every index is cleaned with the row window. An episode whose rows are all
// evicted is removed (announced as `episode-removed`) together with its union-find keys; the keys
// leave bounded tombstones, so a late event for it starts a new episode marked partial. A
// coalesced row leaves a bounded continuation note, so a late chunk is shown as a continuation.
//
// Redaction: coalesced text is redacted as a whole (so a credential split across chunks is
// caught) before any sentence is rendered; the raw join is kept only privately, bounded, to
// redact the next join, and is never emitted, served or stored.
//
// The 240-character tail: unfold-events sends a reply's joined text as its last 240
// characters, never its chunks. When a tail no longer starts with the previous one, its start
// was cut: the context of the first fragment ("Bearer ", "key=") may be gone, so that fragment
// is hidden. Every credential value already redacted in a row is remembered (privately, at most
// SECRETS_KEPT per row) and masked wherever it appears again. A redacted value is never shown again.
//
// Bounds: a coalesced row keeps at most ID_SAMPLE chunk ids (the first ones and the latest)
// and counts the rest; union-find keys are counted per retained row and pruned as rows leave, so
// a long-lived episode holds keys only for the rows it still has.
import { createHash, randomBytes } from 'node:crypto'
import { REDACTED, nameText, redact, redactText, secretsIn } from './redact.js'
import { sentence } from './sentences.js'

export const HEARTBEAT_LEASE_MS = 5_000 // agentd heartbeats every 1 s; five missed is stale (unfold-events-dsh LEASE)
const TEXT_KEEP = 4096
// A reply's unfinished last line is held whole, from its first character, up to this many characters
// (#406 Design A); a longer one is hidden from its first character (see feedText).
const PENDING_MAX = 2 * TEXT_KEEP
const OVER_PAD = ' '.repeat(4096)
const NOT_EPISODE = new Set(['hello', 'attach', 'ready', 'heartbeat', 'log', 'rejected', 'user_audio', 'user_audio_end', 'audio_ingress', 'journal_fault', 'context_restore'])
const HOST_NOT_EPISODE = new Set(['started', 'link_up', 'link_down', 'local_vad_onset', 'echo_gate', 'mute_request', 'mute_applied', 'talk', 'quit', 'quit_requested', 'call_request', 'call_devices_opened', 'call_devices_released', 'call_input_closed', 'mic_mode', 'mic_modes_ui', 'outbox_fault', 'say_refused', 'control_without_link', 'stop_without_link'])
const ENVELOPE = ['v', 'id', 'producer', 'seq', 't_ns', 'source', 'cause', 'received', 'observed', 'kind']
const COALESCE = { agent_delta: 'response_id', agent_audio: 'response_id', user_audio: 'stream_id' }
const OUTCOME = { completed: 3, stopped: 3, failed: 3 }
const PRODUCERS_MAX = 256
const ID_SAMPLE = 8 // chunk ids kept per coalesced row: the first ones and the latest
const WINDOW_TAIL = 240 // unfold-events keeps this many characters of a reply's joined text (unfold-events-dsh window.js TEXT_TAIL)
const SECRETS_KEPT = 16
const CUT_FRAGMENT = /^\s*(?:(?:["']\s*)?[:=]\s*["']?)?\S+/
const CUT_HEAD_RUNS = 4 // after a cut, credential-shaped runs among this many leading runs are hidden too

export class MindStream {
  constructor({ limit = 3000, now = () => Date.now() } = {}) {
    this.limit = limit
    this.now = now
    this.rows = [] // retained rows, observation order
    this.byId = new Map() // mind row id -> row
    this.byEvent = new Map() // event id -> mind row id (first, last and journal ids)
    this.children = new Map() // event id -> Set<mind row id> (explicit cause links only)
    this.pendingCause = new Map() // cause event id not yet seen -> Set<mind row id>
    this.episodes = new Map()
    this.uf = new Map() // key -> parent key
    this.keyEpisode = new Map() // root key -> episode id
    this.typed = new Set() // typed request ids seen
    this.requestRefs = new Map() // request_id -> Set<row id> (replies naming it, not yet joined)
    this.lastSeq = new Map() // `${lane}|${producer}` -> highest seq seen
    this.open = new Map() // `${lane}|${kind}:${key}` -> row id (journal lane coalescing)
    this.boots = new Map() // producer -> { utc_ms_at_origin, boot_id }
    this.tombstones = new Map() // episode key of a removed episode -> removed at (FIFO, bounded)
    this.continuations = new Map() // evicted coalesced row: window key or journal open key -> its first envelope (FIFO, bounded)
    this.life = randomBytes(4).toString('hex') // this Mind lifetime's nonce, in every row and episode id
    this.episodeSeq = 0
    this.link = { state: 'unknown', detail: 'no link state from unfold-events yet', since: null, epoch: 0, attaches: 0 }
    this.heartbeat = { at: null, stale: false, since: null }
    this.seq = 0
    this.rev = 0
    this.dropped = 0
    this.listeners = new Set()
  }

  onChange(fn) {
    this.listeners.add(fn)
    return () => this.listeners.delete(fn)
  }

  emit(change, payload) {
    this.rev++
    for (const fn of this.listeners) fn(change, payload, this.rev)
  }

  // ---------------------------------------------------------------- inputs

  /**
   * A row from the unfold-events window (`Catalog.describe` plus window bookkeeping), or a plain
   * protocol envelope from the agentd journal. `origin` says where it came from.
   */
  agentd(input, origin = { via: 'unfold-events observer feed' }) {
    // Stored redacted, whatever the source (a journal envelope arrives raw); PCM is never kept.
    const env = input.wire ? fromWindowRow(input) : redact(stripMedia({ ...input }))
    const kind = env.kind
    // Rows unfold-events still held when Mind loaded are lossy evidence of the sequence (it keeps
    // no heartbeats and folds coalesced parts into seq..seq_last): they seed it silently, and
    // sequence gaps are judged from live frames only.
    if (origin.preload) this.seedSeq(origin.via, env.producer, Math.max(env.seq ?? -1, input.seq_last ?? -1))
    else this.checkSeq(origin.via, env.producer, env.seq, origin)
    if (kind === 'heartbeat') return this.beat(env)
    if (env.id && this.byEvent.has(env.id)) return this.corroborate(this.byEvent.get(env.id), origin)
    if (kind === 'hello' && env.producer) capSet(this.boots, env.producer, { utc_ms_at_origin: env.utc_ms_at_origin ?? null, boot_id: env.boot_id ?? null, journal: typeof env.journal === 'string' ? env.journal : null }, PRODUCERS_MAX)
    const fields = fieldsOf(env)
    const ck = COALESCE[kind] && !input.wire ? `agentd|${kind}:${fields[COALESCE[kind]]}` : null
    if (ck && this.open.has(ck)) return this.grow(this.open.get(ck), { id: env.id, seq: env.seq, t_ns: env.t_ns, text: input.text ?? env.text, audio: audioOf(input.fields ?? env) })
    const continuation = Boolean(ck && this.continuations.delete(ck))
    // Text is taken raw (input.text / the envelope's own text) and redacted by setText, as a whole.
    const extra = input.wire ? { count: input.count ?? 1, audio: input.audio ?? null, text: input.text, textCut: tailCut(input.count, input.text), last_t_ns: input.last_t_ns ?? env.t_ns, ids_omitted: input.ids_omitted ?? 0 } : { count: 1, audio: COALESCE[kind] && kind !== 'agent_delta' ? audioOf(env) : null, text: kind === 'agent_delta' ? (typeof input.text === 'string' ? input.text : undefined) : undefined, last_t_ns: env.t_ns, ids_omitted: 0 }
    const row = this.add({ lane: 'agentd', source: 'Unfold', kind, env, fields, origin, ...extra, continuation })
    if (ck) this.open.set(ck, row.id)
    if (kind === 'agent_done') this.closeOpen('agentd', fields.response_id)
    if (kind === 'user_audio_end') this.open.delete(`agentd|user_audio:${fields.stream_id}`)
    return row
  }

  /** A coalesced row grew (unfold-events `updated` patch). */
  agentdPatch(p) {
    const id = this.byEvent.get(p.key)
    const row = id && this.byId.get(id)
    if (!row) {
      // The row this chunk grows was evicted: show the chunk as a continuation, never drop it.
      const first = this.continuations.get(p.key)
      if (!first) return null
      this.continuations.delete(p.key)
      return this.add({ ...first, count: p.count ?? 1, audio: p.audio ?? null, text: p.text, textCut: tailCut(p.count, p.text), last_t_ns: p.last_t_ns, ids_omitted: p.ids_omitted ?? 0, continuation: true })
    }
    const producer = row.ids.producer
    if (p.seq_last != null) this.checkSeq(row.origin.via, producer, p.seq_last, row.origin)
    if (p.last_id) this.index(p.last_id, row)
    row.count = p.count ?? row.count
    row.last_t_ns = p.last_t_ns ?? row.last_t_ns
    row.ids_omitted = p.ids_omitted ?? row.ids_omitted
    if (p.audio) row.audio = p.audio
    // The window's joined tail, redacted as one string; cut when it no longer starts with the last one.
    if (p.text !== undefined) setText(row, p.text, { cut: row.rawText !== undefined ? !String(p.text).startsWith(rawOf(row)) : tailCut(p.count, p.text) })
    return this.refresh(row)
  }

  /** A record from the host journal (`origin` names the file, offset and line hash). */
  host(rec, origin) {
    const { kind, t_ms, ...fields } = rec
    const env = redact({ ...rec })
    // One host lifetime per host process, established only by its "started" record (a new id
    // each time). Until one is seen the lifetime is unknown (null). Continuity is lost, back to
    // unknown, when the journal is replaced (another file, or an offset that does not move forward)
    // or its t_ms (ms since that process started) goes backwards; neither proves a new process, so
    // neither makes one. Metrics subtract host times only within one known lifetime.
    const file = origin?.file ?? null
    const offset = typeof origin?.offset === 'number' ? origin.offset : null
    const replaced = this.hostAt && (file !== this.hostAt.file || offset == null || this.hostAt.offset == null || offset <= this.hostAt.offset)
    const backwards = typeof t_ms === 'number' && Number.isFinite(t_ms) && Number.isFinite(this.hostLast) && t_ms < this.hostLast
    if (replaced || backwards) this.hostLife = null
    if (kind === 'started') {
      this.hostLives = (this.hostLives ?? 0) + 1
      this.hostLife = `host-${this.hostLives}${Number.isFinite(rec.pid) ? `-pid${rec.pid}` : ''}`
    }
    this.hostAt = { file, offset }
    this.hostLast = typeof t_ms === 'number' && Number.isFinite(t_ms) ? t_ms : this.hostLast
    return this.add({ lane: 'host', source: 'Unfold', kind: String(kind ?? 'unknown'), env, fields: redact(fields), origin, count: 1, t_ms, hostLife: this.hostLife ?? null })
  }

  /** A host journal discontinuity: no host lifetime continues across it (until a new started record). */
  hostBreak(reason) {
    this.hostLife = null
    this.hostAt = null
    this.hostLast = undefined
    this.hostBreaks = (this.hostBreaks ?? 0) + 1
    this.hostBreakWhy = String(reason ?? 'journal discontinuity')
  }

  /** The observer link (unfold-events `link`). Leaving `attached` is a gap; reattaching resumes live only. */
  setLink(state, detail = null) {
    const prev = this.link.state
    const at = this.now()
    if (state === 'attached') {
      const resumed = this.link.attaches > 0
      this.link = { state, detail, since: at, epoch: this.link.epoch + 1, attaches: this.link.attaches + 1 }
      this.heartbeat = { at: null, stale: false, since: null }
      if (resumed) this.marker('link_resumed', `Observer link back (attach ${this.link.attaches}). Live again from here; events emitted while it was down are not replayed and are not shown.`)
    } else {
      const wasLive = prev === 'attached'
      this.link = { ...this.link, state, detail, since: at }
      if (wasLive) {
        const open = this.markOpenEpisodes('the observer link dropped while it was open; its outcome was not observed')
        this.marker('link_lost', `Observer link lost (${state}${detail == null ? '' : `: ${detail}`}). From here until it is back, nothing the agent does is on this feed.${open ? ` ${open} open episode(s) marked unknown.` : ''}`)
      }
    }
    this.emit('link', this.linkView())
  }

  /** Called on a timer by the plugin: a heartbeat silent past its lease is a stall marker. */
  tick() {
    const now = this.now()
    if (this.link.state !== 'attached' || this.heartbeat.stale) return
    const last = this.heartbeat.at ?? this.link.since
    if (last != null && now - last > HEARTBEAT_LEASE_MS) {
      this.heartbeat = { ...this.heartbeat, stale: true, since: now }
      this.marker('heartbeat_stale', `No heartbeat from the agent for ${Math.round((now - last) / 1000)} s while attached: the feed may be stalled. Rows after this may be missing until it resumes.`)
      this.emit('link', this.linkView())
    }
  }

  /** When the heartbeat lease next runs out (for the plugin's one timer), or null. */
  nextDeadline() {
    if (this.link.state !== 'attached' || this.heartbeat.stale) return null
    const last = this.heartbeat.at ?? this.link.since
    return last == null ? null : last + HEARTBEAT_LEASE_MS + 1
  }

  /** A Mind-authored marker (source UI): a gap, a load boundary, a source that is missing. */
  marker(type, text, extra = {}) {
    return this.add({ lane: 'mind', source: 'UI', kind: type, env: null, fields: { type, ...extra }, origin: { via: 'Mind plugin' }, count: 1, sentenceText: text, status: 'gap' })
  }

  // ---------------------------------------------------------------- internals

  beat(env) {
    const now = this.now()
    const wasStale = this.heartbeat.stale
    const last = this.heartbeat.at ?? this.link.since
    this.heartbeat = { at: now, stale: false, since: null, uptime_ms: env.uptime_ms ?? null, attachments: env.attachments ?? null }
    if (wasStale) this.marker('heartbeat_resumed', `Agent heartbeat resumed after ${last == null ? 'an unknown time' : `about ${Math.round((now - last) / 1000)} s`} of silence. A sequence gap marker, if any, says whether frames were lost.`)
    this.emit('heartbeat', { at: now })
    return null
  }

  checkSeq(via, producer, seq, origin) {
    if (typeof producer !== 'string' || !Number.isFinite(seq)) return
    const k = `${via}|${producer}`
    const last = this.lastSeq.get(k)
    if (last !== undefined && seq > last + 1) {
      const n = seq - last - 1
      this.marker('seq_gap', `${n} frame(s) from ${producer} (seq ${last + 1}${n > 1 ? `..${seq - 1}` : ''}) were not observed on ${origin?.via ?? 'this feed'}: lost, skipped for lag, or addressed to another attachment. Incomplete evidence.`, { producer, from: last + 1, to: seq - 1, missing: n })
    }
    if (last === undefined || seq > last) capSet(this.lastSeq, k, seq, PRODUCERS_MAX)
  }

  seedSeq(via, producer, seq) {
    if (typeof producer !== 'string' || !Number.isFinite(seq) || seq < 0) return
    const k = `${via}|${producer}`
    capSet(this.lastSeq, k, Math.max(this.lastSeq.get(k) ?? -1, seq), PRODUCERS_MAX)
  }

  corroborate(id, origin) {
    const row = this.byId.get(id)
    if (!row) return null
    if (!row.origins.some((o) => o.via === origin.via && o.offset === origin.offset)) row.origins.push(origin)
    return this.refresh(row)
  }

  grow(id, chunk) {
    const row = this.byId.get(id)
    if (!row) return null
    if (chunk.id) this.index(chunk.id, row)
    row.count++
    row.last_t_ns = chunk.t_ns ?? row.last_t_ns
    if (row.kind === 'agent_delta') feedText(row, chunk.text)
    else if (row.audio && chunk.audio) row.audio = { chunks: row.audio.chunks + 1, ms: row.audio.ms + chunk.audio.ms }
    return this.refresh(row)
  }

  closeOpen(lane, responseId) {
    for (const k of [...this.open.keys()]) {
      if (!k.startsWith(`${lane}|`) || !k.endsWith(`:${responseId}`)) continue
      // The reply has ended: its unfinished last line has ended with it, and is remembered now.
      const row = this.byId.get(this.open.get(k))
      if (row?.kind === 'agent_delta') endText(row)
      this.open.delete(k)
    }
  }

  add({ lane, source, kind, env, fields, origin, count, audio = null, text, textCut = false, last_t_ns, ids_omitted = 0, t_ms, sentenceText, status, continuation = false, hostLife = null }) {
    const id = `${this.life}-m${++this.seq}`
    const producer = env?.producer ?? null
    const boot = producer ? this.boots.get(producer) : null
    if (lane === 'agentd' && !origin.file) origin = { ...origin, process: 'agentd (guest)', file: null, offset: null, agent_journal: boot?.journal ? `${boot.journal} (guest path named by Hello; this feed carries no offset)` : 'unknown (no Hello observed for this producer)' }
    const row = {
      id,
      lane,
      source, // Unfold | UI (| Brain, when that feed exists)
      training_eligible: trainingOf(lane, source, fields),
      kind,
      env, // redacted, reconstructed where it came from a window row
      fields,
      origin,
      origins: [origin],
      history: Boolean(origin?.history),
      observed_at: this.now(),
      observed_seq: this.seq,
      clock: clockOf(lane, env, t_ms, boot),
      count,
      audio,
      text: undefined, // redacted; set by setText below
      rawText: undefined, // private: never emitted, served or stored
      continuation,
      hostLife,
      last_t_ns: last_t_ns ?? env?.t_ns ?? null,
      ids_omitted,
      ids: idsOf(lane, kind, env, fields),
      cause: env?.cause ?? null,
      episode: null,
      sentenceText,
      fixedStatus: status ?? null,
      eventIds: [],
      chunkIds: { first: null, last: null, total: 0, omitted: 0 },
      keys: [],
      secrets: [], // private: credential values already redacted in this row's text (never emitted)
    }
    if (typeof text === 'string') setText(row, text, { cut: textCut })
    Object.assign(row, this.render(row))
    this.rows.push(row)
    this.byId.set(id, row)
    if (row.ids.event) this.index(row.ids.event, row)
    this.causeLink(row)
    this.group(row)
    this.emit('row', publicRow(row))
    this.trim()
    return row
  }

  index(eventId, row) {
    if (this.byEvent.get(eventId) === row.id) return
    const c = row.chunkIds
    c.total++
    c.first ??= eventId
    c.last = eventId
    if (row.eventIds.length >= ID_SAMPLE) {
      // Keep the first ids and the latest: the previous latest is dropped from the index.
      const dropped = row.eventIds.pop()
      if (dropped !== c.first && this.byEvent.get(dropped) === row.id) this.byEvent.delete(dropped)
      c.omitted++
    }
    row.eventIds.push(eventId)
    this.byEvent.set(eventId, row.id)
  }

  render(row) {
    if (row.lane === 'mind') return { category: 'system', sentence: row.sentenceText, status: row.fixedStatus ?? 'gap', known: true, duration_ms: null, duration_basis: null }
    const s = sentence({ lane: row.lane, kind: row.kind, fields: row.fields, count: row.count, audio: row.audio, text: row.text })
    let { duration_ms = null, duration_basis = null } = s
    if (duration_ms == null && row.lane === 'agentd' && row.count > 1 && Number.isFinite(row.last_t_ns) && Number.isFinite(row.env?.t_ns)) {
      duration_ms = (row.last_t_ns - row.env.t_ns) / 1e6
      duration_basis = `first to last of ${row.count} parts, agentd clock`
    }
    const what = row.kind === 'agent_delta' ? 'text' : 'audio'
    const tail = row.continuation ? ` (continued: the start of this ${what} was evicted from Mind's window)` : ''
    return { category: s.category, sentence: `${s.sentence}${tail}`, status: s.status, known: s.known, duration_ms, duration_basis }
  }

  refresh(row) {
    Object.assign(row, this.render(row))
    this.emit('row-updated', publicRow(row))
    if (row.episode) this.touchEpisode(this.episodes.get(row.episode))
    return row
  }

  causeLink(row) {
    const c = row.cause
    if (c) {
      if (!this.children.has(c)) this.children.set(c, new Set())
      this.children.get(c).add(row.id)
    }
    // A row that arrives after rows naming it as their cause adopts them (explicit link only).
    const waiting = row.ids.event && this.pendingCause.get(row.ids.event)
    if (waiting) {
      this.pendingCause.delete(row.ids.event)
      if (groupable(row)) for (const childId of waiting) {
        const child = this.byId.get(childId)
        if (child && groupable(child)) this.union(`e:${child.ids.event}`, `e:${row.ids.event}`)
      }
    }
  }

  /** The explicit episode keys of one row; empty for stream, connection and system rows. */
  keysOf(row) {
    if (!groupable(row)) return []
    const f = row.fields
    const keys = []
    const add = (p, v) => typeof v === 'string' && v && keys.push(`${p}:${v}`)
    if (row.ids.event) add('e', row.ids.event)
    // A transcript names a reply only with identity evidence (association=provider).
    if (!(row.kind === 'input_transcript' && f.association !== 'provider')) add('r', f.response_id)
    if (Array.isArray(f.cancelled)) for (const r of f.cancelled) add('r', r)
    add('d', f.decision_id)
    add('u', f.utterance_id)
    if (row.kind === 'command_submitted' || row.kind === 'command_admitted' || row.lane === 'host' && /^command_/.test(row.kind)) add('c', f.command_id)
    if (row.kind === 'user_text') add('c', f.request_id)
    if (row.kind === 'decision' && typeof f.trigger_event === 'string' && f.trigger_event.startsWith('host:')) add('c', f.trigger_event.slice(5))
    return keys
  }

  group(row) {
    const keys = this.keysOf(row)
    if (!keys.length) return
    const f = row.fields
    // Typed requests: remember the id; replies that already named it join now.
    for (const k of keys) if (k.startsWith('c:')) {
      const reqId = k.slice(2)
      this.typed.add(reqId)
      for (const rid of this.requestRefs.get(reqId) ?? []) {
        const r = this.byId.get(rid)
        if (r) this.union(keys[0], this.keysOf(r)[0])
      }
      this.requestRefs.delete(reqId)
    }
    if (row.kind === 'response_started' && typeof f.request_id === 'string') {
      if (this.typed.has(f.request_id)) keys.push(`c:${f.request_id}`)
      else if (!/^mic-|^stream/.test(f.request_id)) {
        if (!this.requestRefs.has(f.request_id)) this.requestRefs.set(f.request_id, new Set())
        this.requestRefs.get(f.request_id).add(row.id)
      }
    }
    for (const k of keys) if (!this.uf.has(k)) this.uf.set(k, k)
    if (row.cause) {
      const parentId = this.byEvent.get(row.cause)
      const parent = parentId && this.byId.get(parentId)
      if (parent && groupable(parent)) keys.push(`e:${parent.ids.event}`)
      else if (!parent) {
        if (!this.pendingCause.has(row.cause)) this.pendingCause.set(row.cause, new Set())
        this.pendingCause.get(row.cause).add(row.id)
      }
    }
    for (const k of keys.slice(1)) this.union(keys[0], k)
    const root = this.find(keys[0])
    let epId = this.keyEpisode.get(root)
    let ep = epId && this.episodes.get(epId)
    if (!ep) {
      // A new episode. If one of its keys belonged to an episode that was evicted, it is partial.
      const n = ++this.episodeSeq
      epId = `ep-${this.life}-${n}`
      ep = { id: epId, n, rows: [], keys: new Map(), aliases: [], opened_at: row.observed_at, gap: null, partial: null }
      if (keys.some((k) => this.tombstones.has(k))) ep.partial = "earlier rows of this episode were evicted from Mind's window; it is shown from here on"
      this.keyEpisode.set(root, epId)
      this.episodes.set(epId, ep)
    }
    for (const k of keys) ep.keys.set(k, (ep.keys.get(k) ?? 0) + 1)
    row.keys = keys
    row.episode = epId
    ep.rows.push(row.id)
    this.touchEpisode(ep)
  }

  find(k) {
    let r = k
    while (this.uf.get(r) !== r) {
      if (!this.uf.has(r)) this.uf.set(r, r)
      r = this.uf.get(r)
    }
    this.uf.set(k, r)
    return r
  }

  union(a, b) {
    if (!this.uf.has(a)) this.uf.set(a, a)
    if (!this.uf.has(b)) this.uf.set(b, b)
    const ra = this.find(a)
    const rb = this.find(b)
    if (ra === rb) return
    const ea = this.keyEpisode.get(ra)
    const eb = this.keyEpisode.get(rb)
    // The older episode keeps its id; the newer one's rows move into it.
    const older = (x, y) => this.episodes.get(x).n <= this.episodes.get(y).n
    const keepA = ea && (!eb || older(ea, eb))
    const [keep, gone, keepRoot, goneRoot] = keepA ? [ea, eb, ra, rb] : [eb, ea, rb, ra]
    this.uf.set(goneRoot, keepRoot)
    this.keyEpisode.delete(goneRoot)
    if (keep) this.keyEpisode.set(keepRoot, keep)
    if (keep && gone && keep !== gone) {
      const into = this.episodes.get(keep)
      const from = this.episodes.get(gone)
      for (const rid of from.rows) {
        const r = this.byId.get(rid)
        if (r) r.episode = keep
      }
      into.rows = [...into.rows, ...from.rows].sort((x, y) => this.byId.get(x)?.observed_seq - this.byId.get(y)?.observed_seq)
      into.gap = into.gap ?? from.gap
      into.partial = into.partial ?? from.partial
      for (const [k, n] of from.keys) into.keys.set(k, (into.keys.get(k) ?? 0) + n)
      into.aliases = [...into.aliases, gone, ...from.aliases].slice(-32) // earlier ids, so a saved proposal still finds it
      this.episodes.delete(gone)
      this.emit('merged', { from: gone, into: keep })
      this.touchEpisode(into)
    }
  }

  markOpenEpisodes(why) {
    let n = 0
    for (const ep of this.episodes.values()) {
      const v = episodeView(ep, this)
      if (v.status === 'pending' || v.status === 'attempted') {
        ep.gap = { why, at: this.now(), epoch: this.link.epoch }
        n++
        this.touchEpisode(ep)
      }
    }
    return n
  }

  touchEpisode(ep) {
    if (ep) this.emit('episode', episodeView(ep, this))
  }

  trim() {
    while (this.rows.length > this.limit) {
      const old = this.rows.shift()
      this.dropped++
      this.byId.delete(old.id)
      for (const ev of old.eventIds) {
        if (this.byEvent.get(ev) === old.id) this.byEvent.delete(ev)
        this.children.delete(ev) // links FROM an evicted parent are not needed any more
      }
      dropFromSetMap(this.children, old.cause, old.id)
      dropFromSetMap(this.pendingCause, old.cause, old.id)
      dropFromSetMap(this.requestRefs, old.fields?.request_id, old.id)
      for (const [k, v] of this.open) {
        if (v !== old.id) continue
        this.open.delete(k)
        capSet(this.continuations, k, true, this.limit)
      }
      if (COALESCE[old.kind] && old.lane === 'agentd' && old.ids.event) {
        capSet(this.continuations, old.ids.event, { lane: 'agentd', source: old.source, kind: old.kind, env: old.env, fields: old.fields, origin: old.origin }, this.limit)
      }
      const ep = old.episode && this.episodes.get(old.episode)
      if (ep) {
        ep.rows = ep.rows.filter((r) => r !== old.id)
        ep.evicted = (ep.evicted ?? 0) + 1
        if (ep.rows.length) {
          this.pruneKeys(ep, old.keys)
          this.touchEpisode(ep)
        } else this.removeEpisode(ep)
      }
      this.emit('evicted', { id: old.id })
    }
  }

  /** An evicted row's keys leave a living episode once no retained row carries them; the rest is re-rooted. */
  pruneKeys(ep, keys) {
    let pruned = false
    for (const k of keys ?? []) {
      const n = (ep.keys.get(k) ?? 0) - 1
      if (n > 0) {
        ep.keys.set(k, n)
        continue
      }
      ep.keys.delete(k)
      this.uf.delete(k)
      if (this.keyEpisode.get(k) === ep.id) this.keyEpisode.delete(k)
      if (k.startsWith('c:')) this.typed.delete(k.slice(2))
      capSet(this.tombstones, k, this.now(), 4 * this.limit)
      pruned = true
    }
    if (!pruned) return
    // The episode's keys are one component: flatten it under one remaining key.
    const root = ep.keys.keys().next().value
    if (root === undefined) return
    for (const k of ep.keys.keys()) this.uf.set(k, root)
    for (const [r, id] of this.keyEpisode) if (id === ep.id && r !== root) this.keyEpisode.delete(r)
    this.keyEpisode.set(root, ep.id)
  }

  /** An episode with no retained rows: drop it and every index entry it owned; tombstone its keys. */
  removeEpisode(ep) {
    this.episodes.delete(ep.id)
    for (const k of ep.keys.keys()) {
      if (this.keyEpisode.get(k) === ep.id) this.keyEpisode.delete(k)
      this.uf.delete(k)
      if (k.startsWith('c:')) this.typed.delete(k.slice(2))
      capSet(this.tombstones, k, this.now(), 4 * this.limit)
    }
    for (const [root, id] of this.keyEpisode) if (id === ep.id) this.keyEpisode.delete(root)
    this.emit('episode-removed', { id: ep.id, aliases: ep.aliases })
  }

  /** Provenance for a proposal about `episodeId`, recorded beside it (not part of its content). */
  scopeContext(episodeId) {
    const ep = typeof episodeId === 'string' ? this.episodes.get(episodeId) ?? [...this.episodes.values()].find((e) => e.aliases.includes(episodeId)) : null
    const rows = ep ? ep.rows.map((id) => this.byId.get(id)).filter(Boolean) : []
    return {
      mind_lifetime: this.life,
      episode_retained: Boolean(ep),
      episode_now: ep?.id ?? null,
      event_ids: rows.map((r) => r.ids.event).filter(Boolean).slice(0, 20),
      basis: ep ? 'the episode as Mind held it when the proposal was saved (this lifetime)' : 'no retained episode with this id in this Mind lifetime: recorded as given, linked to nothing',
    }
  }

  /** The ids a proposal saved under `episodeId` may carry: the episode's own and the ids merged into it. */
  episodeIds(episodeId) {
    const ep = this.episodes.get(episodeId)
    return ep ? [ep.id, ...ep.aliases] : [episodeId]
  }

  // ---------------------------------------------------------------- views

  linkView() {
    return { ...this.link, heartbeat: { ...this.heartbeat }, lease_ms: HEARTBEAT_LEASE_MS }
  }

  snapshot() {
    return {
      rev: this.rev,
      at: this.now(),
      rows: this.rows.map(publicRow),
      episodes: [...this.episodes.values()].map((ep) => episodeView(ep, this)),
      link: this.linkView(),
      dropped: this.dropped,
    }
  }

  /** The audit record of one row: everything a person needs to check it, redacted. */
  detail(id, definitions) {
    const row = this.byId.get(id) ?? this.byId.get(this.byEvent.get(id))
    if (!row) return null
    const parents = []
    const seen = new Set()
    let unresolved = null
    for (let c = row.cause; c && !seen.has(c); ) {
      seen.add(c)
      const p = this.byId.get(this.byEvent.get(c))
      if (!p) {
        unresolved = { event_id: c, reason: this.whyMissing(c) }
        break
      }
      parents.push(link(p))
      c = p.cause
    }
    const children = [...(this.children.get(row.ids.event) ?? [])].map((cid) => this.byId.get(cid)).filter(Boolean).map(link)
    const ep = row.episode ? this.episodes.get(row.episode) : null
    const envelope = row.env ? redact(row.env) : null
    const integrity = integrityOf(row)
    const evidence = {
      format: 'unfold.mind-evidence/v1',
      captured_at: new Date(this.now()).toISOString(),
      row: { id: row.id, sentence: row.sentence, sentence_basis: 'Mind plugin rendering (an interpretation of the fields below, not the agent’s words)', category: row.category, status: row.status, source: row.source, lane: row.lane },
      ids: row.ids,
      clock: row.clock,
      observed_at: new Date(row.observed_at).toISOString(),
      origins: row.origins,
      envelope,
      envelope_basis: envelopeBasis(row),
      cause_chain: { parents: parents.map((p) => p.event_id), unresolved, children: children.map((c) => c.event_id) },
      episode: ep ? ep.id : null,
      integrity,
    }
    const canonical = JSON.stringify(evidence)
    return {
      row: publicRow(row),
      envelope,
      envelope_basis: envelopeBasis(row),
      ids: { ...row.ids, chunk_ids: { kept: row.eventIds.length, omitted: row.chunkIds.omitted, first: row.chunkIds.first, last: row.chunkIds.last, basis: `at most ${ID_SAMPLE} chunk ids are kept per row (the first ones and the latest); the rest are counted` } },
      clock: row.clock,
      origins: row.origins,
      chain: { parents, unresolved, children },
      correlated: ep ? episodeView(ep, this).rows.filter((r) => r.id !== row.id).slice(0, 40) : [],
      episode: ep ? episodeView(ep, this) : null,
      integrity,
      definition: definitions ? definitions.define(row.lane, row.kind) : null,
      evidence,
      evidence_sha256: createHash('sha256').update(canonical).digest('hex'),
      evidence_sha256_basis: 'sha256 of the evidence object exactly as JSON-serialized here (computed by Mind)',
    }
  }

  whyMissing(eventId) {
    const s = String(eventId)
    const cut = s.lastIndexOf(':')
    const producer = cut > 0 ? s.slice(0, cut) : null
    if (s.startsWith('host-') || s.startsWith('host:')) return 'a host frame: agentd does not broadcast admitted host frames to observers, so this feed never carries it'
    if (!producer || ![...this.lastSeq.keys()].some((k) => k.endsWith(`|${producer}`))) return 'never observed by Mind (another producer, or before Mind loaded)'
    return 'not retained: evicted, or lost in a gap (see the gap markers)'
  }
}

// ---------------------------------------------------------------- helpers

/**
 * Coalesced text: the raw join kept privately (bounded), only its redaction rendered. Credential
 * values found in the text are remembered (privately, bounded) before anything is cut, so a later
 * tail that lost their context still masks them; a cut tail's first fragment is hidden.
 *
 * The join is a growing reply, held in two parts (#406 Design A): `rawText`, the committed text,
 * which ends at a line terminator (\n, \r\n, or a \r with something after it), and `rawOpen`, the
 * unfinished last line, held whole from its first character and carried across deltas, however
 * many it comes in. A delta with no terminator is only appended to it. At a terminator the lines
 * the delta completed join the committed text, the values in it are remembered, and only then does
 * the 4 KiB bound (keptTail) cut it, so the bound never starts inside the open line and the open
 * line's head is kept whatever the split. The open line is served and remembered as the rules read
 * a growing buffer's last line (`open`): a value still arriving there is hidden, and one that needs
 * a later line (a key line's continued value, an explicit key's, a padded line's) is remembered
 * only once its terminator arrives. An open line longer than PENDING_MAX is withheld from its first
 * character, fail-closed, with no text in its place until its terminator; it is then committed
 * as its first PENDING_MAX characters, a 4 KiB blank run for what was dropped, and its last
 * TEXT_KEEP characters, so the rules read it as a cut-padded line (hidden whole once ended, its key
 * line still naming the value on the lines under it). Every line that long is committed that way,
 * however it arrived, so one delta and many agree.
 */
function setText(row, raw, { cut = false } = {}) {
  row.rawText = ''
  row.rawOpen = ''
  row.rawOver = null
  row.rawFragment = false
  feedText(row, raw, { cut })
}

/** The raw text a row holds: its committed text and its open line (not an oversize one). */
function rawOf(row) {
  return `${row.rawText ?? ''}${row.rawOver ? '' : (row.rawOpen ?? '')}`
}

const LINE_END = /\r\n|\n|\r(?=[\s\S])/g

/** A delta: appended to the open line while it has no terminator, else committed once. */
function feedText(row, add, { cut = false } = {}) {
  const s = String(add ?? '')
  if (row.rawText === undefined) row.rawText = ''
  if (row.rawOpen === undefined) row.rawOpen = ''
  const over = row.rawOver
  // A \r at the end of the open line may be a CRLF's first half: it ends its line only once
  // something other than \n follows it.
  const crEnds = (over ? over.tail.endsWith('\r') : row.rawOpen.endsWith('\r')) && s !== '' && s[0] !== '\n'
  LINE_END.lastIndex = 0
  const first = crEnds ? null : LINE_END.exec(s)
  if (!crEnds && !first) {
    if (over) over.tail = `${over.tail}${s}`.slice(-TEXT_KEEP)
    else {
      row.rawOpen = `${row.rawOpen}${s}`
      if (row.rawOpen.length > PENDING_MAX) overflow(row)
    }
    return renderText(row, cut)
  }
  // The lines this delta completes, from the open line's first character, and the new open line.
  const stop = crEnds ? 0 : first.index + first[0].length
  // An oversize open line: what it dropped stays dropped; the rest of it ends in this delta.
  let committed = over ? `${over.head}${OVER_PAD}${`${over.tail}${s.slice(0, crEnds ? 0 : first.index)}`.slice(-TEXT_KEEP)}${crEnds ? '' : first[0]}` : oversize(`${row.rawOpen}${s.slice(0, stop)}`)
  let pos = stop
  LINE_END.lastIndex = stop
  for (let m; (m = LINE_END.exec(s)); pos = m.index + m[0].length) committed += oversize(s.slice(pos, m.index + m[0].length))
  row.rawOver = null
  const full = nameText(`${row.rawText}${committed}`)
  remember(row, secretsIn(full, true))
  row.rawText = keptTail(full)
  // A plain cut leaves a fragment at the head of what is kept; a key line or an open value's key
  // kept whole (not a tail of the text) starts none.
  row.rawFragment = row.rawText !== full && full.endsWith(row.rawText)
  row.rawOpen = s.slice(pos)
  if (row.rawOpen.length > PENDING_MAX) overflow(row)
  return renderText(row, cut)
}

/** A completed line longer than PENDING_MAX, as an oversize open line is committed. */
function oversize(line) {
  const body = line.replace(/(?:\r\n|\n|\r)$/, '')
  return body.length > PENDING_MAX ? `${body.slice(0, PENDING_MAX)}${OVER_PAD}${body.slice(-TEXT_KEEP)}${line.slice(body.length)}` : line
}

/** The open line passed PENDING_MAX: keep its first characters and a rolling tail of the rest. */
function overflow(row) {
  const line = row.rawOpen
  row.rawOver = { head: line.slice(0, PENDING_MAX), tail: line.slice(-TEXT_KEEP) }
  row.rawOpen = ''
}

function remember(row, values) {
  for (const v of values) if (!row.secrets.includes(v)) row.secrets = [...row.secrets, v].slice(-SECRETS_KEPT)
}

/** The reply ended: its open line has ended too, so the values on it are remembered. */
function endText(row) {
  const over = row.rawOver
  const open = over ? `${over.head}${OVER_PAD}${over.tail}` : row.rawOpen
  if (open) remember(row, secretsIn(nameText(`${row.rawText ?? ''}${open}`)))
}

/** The served text: the committed text and the open line, redacted as a growing buffer, then cut. */
function renderText(row, cut) {
  const full = nameText(rawOf(row))
  // A growing reply: its last line may still be arriving, so a later-line value there is neither
  // hidden nor remembered until its line ends (#406 F-B).
  remember(row, secretsIn(full, true))
  // Redacted whole, then cut: a value whose key falls before the cut is still recognised. The text
  // is cut when its head is a fragment or what is served is longer than the keep.
  const redacted = redactText(full, true)
  const truncated = cut || Boolean(row.rawFragment) || redacted.length > TEXT_KEEP
  let text = redacted.slice(-TEXT_KEEP)
  if (truncated) text = hideAfterCut(text)
  text = redactText(text, true)
  for (const v of row.secrets) text = text.split(v).join(REDACTED)
  row.textCut = row.textCut || truncated
  row.text = text
}

/**
 * The private raw tail kept for the next delta to join: the last TEXT_KEEP characters, never
 * starting inside a token or a quoted credential value (whose spaces are not boundaries), so a
 * line's head survives every delta boundary and what is withheld for it does not depend on where
 * the deltas were split (#406 CLASS). A closed value the cut lands in is dropped with its key; an
 * open one keeps only its key and opening quote, so the rest of the value is still hidden as it
 * arrives. A cut in the blank run that ends a key line still waiting for its value or block
 * indicator keeps that line, as the window's bound does.
 */
function keptTail(full) {
  if (full.length <= TEXT_KEEP) return full
  let at = full.length - TEXT_KEEP
  let span = quotedSpans(full).find((sp) => sp.start < at && at < sp.end)
  // A cut inside a token would slice a credential name into a word that reads as no name, and the
  // value under it would be served (#406 CLASS): the token is kept whole instead (a token longer
  // than the keep, a value, is cut as before). A token that only blanks follow is the head of a
  // line a cut padded: the cut is read from the token's end, so the blank-run branch below keeps
  // that head whole as a key line and drops the pad, whatever the delta size. A cut inside an open
  // quoted value keeps the value's own rule below, so the guard never moves it.
  if (!span && /[^ \t\r\n]/.test(full[at]) && /[^ \t\r\n]/.test(full[at - 1])) {
    let start = at
    while (start > 0 && /[^ \t\r\n]/.test(full[start - 1])) start--
    let end = at
    while (end < full.length && /[^ \t\r\n]/.test(full[end])) end++
    if (!/\S/.test(full.slice(end))) at = end
    else if (at - start <= TEXT_KEEP) at = start
    span = quotedSpans(full).find((sp) => sp.start < at && at < sp.end)
  }
  if (!span && !/\S/.test(full.slice(at))) {
    const line = openKeyLine(full, TEXT_KEEP)
    if (line !== null) return line
    // A pad (#406 F-B): a run of blanks as long as the keep, with no key line before it, is the
    // cut's padding, not text. It is kept (its last TEXT_KEEP chars) with the rest of the line, so
    // that line still reads as the cut's and is hidden whole once it ends.
    const pad = /(?<![ \t])[ \t]{4096,}/.exec(full)
    return pad ? padKept(full, pad) : full.slice(at)
  }
  // The cut is not in the run, but a pad may still reach into the keep, and is kept as it is there:
  // trimming it further would leave too little of it to read the line as the cut's.
  const pad = /(?<![ \t])[ \t]{4096,}/.exec(full)
  if (pad && pad.index + pad[0].length > at && !span) return padKept(full, pad)
  if (!span) return full.slice(at)
  if (span.closed) return full.slice(span.end)
  return span.keep ?? `${full.slice(span.start, span.open)}${span.escaped ? '\\' : ''}`
}

/**
 * A pad kept: its last TEXT_KEEP blanks and the rest of the text. When the text before the pad reads
 * as an open key line (#406 Design A: an oversize line committed whole, its pad and what follows it
 * in one piece), that key line is kept instead, the pad dropped as the cut's padding, as a pad-only
 * open line was kept before: the value after the pad is still read under its key.
 */
function padKept(full, pad) {
  // A pad that fills its line is read through its terminator: the blank line it is keeps no line.
  const filled = pad.index === 0 || /[\r\n]/.test(full[pad.index - 1])
  const padEnd = pad.index + pad[0].length
  const end = !filled ? padEnd : full.startsWith('\r\n', padEnd) ? padEnd + 2 : /[\r\n]/.test(full[padEnd] ?? '') ? padEnd + 1 : padEnd
  const key = openKeyLine(full.slice(0, end), TEXT_KEEP)
  return key === null ? `${pad[0].slice(-TEXT_KEEP)}${full.slice(padEnd)}` : `${key}${full.slice(end)}`
}

// A key line still waiting for its value or block indicator (`password:`, `password: &a`,
// `password: &a !!str`, a comment after them or not), then a blank run on that line, as in
// unfold-events-dsh's window: kept with the run cut to one space, so the indicator and a block's
// lines that arrive next are read under their key.
// A key line whose value comes on a later line (#396) is kept the same way when the cut lands in
// the lines under it, as the window reads them: its blank, comment, anchor and tag lines and a block
// indicator line, then the line the cut is in. The key line, its anchor, tag and indicator lines
// (each indented one more than the key) and the line the cut is in are kept, so the value that
// arrives next is still read under its key. A blank run at the cut is the cut's padding, not text,
// and is not counted against the 4 KiB read.
const KEY_LINE_OPEN = /:[ \t]*(?:[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)*)?(?:[ \t]+#[^\r\n]*)?$/
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

// Where a quoted credential value opens (the quoted forms of redact.js's text rules), as in
// unfold-events-dsh's window.
const QUOTED_SECRET = [
  /\b[A-Z0-9_]+=(?<=(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*=)(["'`])/g,
  /(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)["']?\s*[:=]\s*(?!(?:[&!][^\s"'`]*[ \t]+){3})(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(["'`])/gi, // any prefix
  /(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)["']?\s*[:=]\s*(?!(?:[&!][^\s"'`]*[ \t]+){3})(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(["'`])/g, // camelCase
  /(?:^|(?<=[\r\n]))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*\?[ \t][^\r\n]*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\1[^\r\n#]*?(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:["']?[ \t]*:|["']?[ \t]*(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))\1:)(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\1[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*(?:\r?\n|\r(?!\n))\1[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(["'`])/gi, // on a later line (#396)
  /(?:^|(?<=[\r\n]))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*\?[ \t][^\r\n]*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\1[^\r\n#]*?(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:["']?[ \t]*:|["']?[ \t]*(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))\1:)(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\1[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*(?:\r?\n|\r(?!\n))\1[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(["'`])/g, // on a later line, camelCase
]

// Where a YAML block scalar under a credential name opens and ends, as in redact.js's text rules:
// a cut lands before it or after it, never inside.
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

// Where JSON inside a JSON string opens a credential value (`{\"password\": \"`), as in SECRET_TEXT,
// and where it ends: its escaped closing quote (its own escapes doubled), a bare quote that ends
// the outer string, or the end of the text while it is open.
const ESCAPED_SECRET = [
  /(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)\\"\s*:\s*\\"/gi,
  /(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)\\"\s*:\s*\\"/g,
]
const ESCAPED_END = /(?:[^\\"]|\\[^\\"]|\\\\(?:\\[\s\S]|\\$|[^\\]|$)|\\$)*(\\"|"|$)/y

/**
 * Quoted credential spans and YAML blocks in `raw`: {start, open, end, closed, escaped}, and for a
 * block what to `keep` while it is open; an open one runs to the end.
 */
function quotedSpans(raw) {
  const spans = []
  for (const re of QUOTED_SECRET) {
    for (const m of raw.matchAll(re)) {
      const open = m.index + m[0].length
      const { close, escaped } = closingQuote(raw, open, m[0].slice(-1)) // the match ends at its opening quote
      spans.push({ start: m.index, open, end: close < 0 ? raw.length : close + 1, closed: close >= 0, escaped })
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
      spans.push({ start: m.index, open, end: ESCAPED_END.lastIndex, closed, escaped: false, keep: closed ? undefined : `${raw.slice(m.index, open)}${'\\'.repeat(run % 4)}` })
    }
  }
  if (/:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[&!>|]/.test(raw)) for (const re of YAML_KEY) {
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
      spans.push({ start, end, closed: end < raw.length, keep })
    }
  }
  return spans
}

/** The first UNESCAPED `quote` in `s` from `from` (JSON: `\"` and `\\` do not end a value). */
function closingQuote(s, from, quote) {
  let escaped = false
  for (let i = from; i < s.length; i++) {
    if (escaped) escaped = false
    else if (s[i] === '\\') escaped = true
    else if (s[i] === quote) return { close: i, escaped: false }
  }
  return { close: -1, escaped }
}

/**
 * The start of a cut tail: its first fragment lost its context, so it is hidden; and a
 * credential-shaped run among the next few (a context of several words, "Authorization: Bea|rer
 * TOKEN", may have been cut) is hidden too, even if Mind never saw it whole.
 */
function hideAfterCut(text) {
  const rest = text.replace(CUT_FRAGMENT, '')
  const parts = rest.split(/(\s+)/)
  let runs = 0
  for (let i = 0; i < parts.length && runs < CUT_HEAD_RUNS; i++) {
    if (!parts[i] || /^\s+$/.test(parts[i])) continue
    runs++
    if (credentialShaped(parts[i])) parts[i] = REDACTED
  }
  return `…${parts.join('')}`
}

/** 16+ characters of token alphabet mixing at least two of: upper case, lower case, digits, _ . - ~ + / = */
function credentialShaped(run) {
  const core = run.replace(/^["'(]+|["'),.;:]+$/g, '')
  if (core.length < 16 || !/^[A-Za-z0-9._~+/=-]+$/.test(core)) return false
  return [/[A-Z]/, /[a-z]/, /[0-9]/, /[._~+/=-]/].filter((re) => re.test(core)).length >= 2
}

/** A window tail with no previous one to compare: cut if it is at full length after several parts. */
function tailCut(count, text) {
  return (count ?? 1) > 1 && typeof text === 'string' && text.length >= WINDOW_TAIL
}

function dropFromSetMap(map, key, value) {
  if (key == null) return
  const set = map.get(key)
  if (!set) return
  set.delete(value)
  if (!set.size) map.delete(key)
}

/** Map.set with FIFO eviction past `max` entries (a re-set key moves to the end). */
function capSet(map, key, value, max) {
  map.delete(key)
  map.set(key, value)
  while (map.size > max) map.delete(map.keys().next().value)
}

function groupable(row) {
  if (row.lane === 'mind') return false
  if (row.lane === 'host') return !HOST_NOT_EPISODE.has(row.kind)
  return !NOT_EPISODE.has(row.kind)
}

export function fromWindowRow(d) {
  const env = { v: d.v ?? null, id: d.id, producer: d.producer, seq: d.seq, t_ns: d.t_ns, source: d.source, kind: d.wire }
  if (d.cause) env.cause = d.cause
  if (d.received) env.received = d.received
  if (d.observed) env.observed = d.observed
  return redact({ ...env, ...stripMedia(d.fields ?? {}) })
}

/** The fields of `env`, which both callers have already redacted. */
function fieldsOf(env) {
  const out = {}
  for (const [k, v] of Object.entries(env)) if (!ENVELOPE.includes(k)) out[k] = v
  return stripMedia(out)
}

function stripMedia(fields) {
  if (typeof fields.data === 'string' && fields.data.length) return { ...fields, data: `[${fields.data.length} base64 chars of PCM not kept by Mind]` }
  if ('data' in fields && fields.data === undefined) {
    const { data, ...rest } = fields
    return rest
  }
  return fields
}

function audioOf(env) {
  const b64 = String(env?.data ?? '')
  const m = /^\[(\d+) base64/.exec(b64)
  const len = m ? Number(m[1]) : b64.length
  const bytes = Math.max(0, Math.floor((len * 3) / 4))
  const rate = Number(env?.sample_rate) || 0
  return { chunks: 1, ms: rate ? (bytes / 2 / rate) * 1000 : 0 }
}

function idsOf(lane, kind, env, f) {
  const s = (x) => (typeof x === 'string' || typeof x === 'number' ? x : null)
  return {
    event: s(env?.id),
    cause: s(env?.cause),
    producer: s(env?.producer),
    seq: Number.isFinite(env?.seq) ? env.seq : null,
    correlation: {
      request_id: s(f.request_id),
      response_id: s(f.response_id),
      command_id: s(f.command_id),
      decision_id: s(f.decision_id),
      utterance_id: s(f.utterance_id),
      stream_id: s(f.stream_id),
      trigger_event: s(f.trigger_event),
      agent_episode_id: s(f.episode_id), // decision.episode_id: the attachment label, session-wide
      envelope: s(f.envelope), // host command_written: the host envelope id it sent
    },
    session_id: s(f.session_id ?? f.instance?.session_id),
    boot_id: s(f.boot_id) ?? (typeof env?.producer === 'string' && env.producer.startsWith('guest-') ? env.producer.slice(6) : null),
    epoch: Number.isFinite(f.epoch) ? f.epoch : null,
    generation: Number.isFinite(f.generation) ? f.generation : null,
    attachment_id: s(f.attachment_id),
  }
}

function clockOf(lane, env, t_ms, boot) {
  if (lane === 'agentd') {
    const t = env?.t_ns
    const wall = Number.isFinite(t) && Number.isFinite(boot?.utc_ms_at_origin) ? boot.utc_ms_at_origin + t / 1e6 : null
    return { basis: 'agentd monotonic t_ns since its boot origin', t_ns: Number.isFinite(t) ? t : null, wall_ms_estimate: wall, wall_basis: wall == null ? 'unknown: no Hello with utc_ms_at_origin for this producer was observed' : 'estimate: Hello utc_ms_at_origin + t_ns (agent wall clock at boot; drift not corrected)' }
  }
  if (lane === 'host') return { basis: 'host t_ms since that host process started', t_ms: Number.isFinite(t_ms) ? t_ms : null, wall_ms_estimate: null, wall_basis: 'unknown: the host log carries no wall clock' }
  return { basis: 'Mind wall clock (this marker was written by Mind)', wall_ms_estimate: null, wall_basis: 'see observed_at' }
}

function trainingOf(lane, source, fields) {
  if (typeof fields?.training_eligibility === 'string') return fields.training_eligibility === 'eligible' ? true : fields.training_eligibility === 'ineligible' ? false : null
  return null // unknown: nothing on these feeds says
}

function envelopeBasis(row) {
  if (row.lane === 'mind') return 'none: a Mind marker, not an event'
  if (row.lane === 'host') return 'the host log record as read from the file, redacted'
  const viaWindow = row.origins.some((o) => o.via === 'unfold-events observer feed')
  const parts = [viaWindow ? 'reconstructed from the unfold-events window row (protocol fields as received; unfold-events-dsh adds `observed`)' : 'the agentd journal record as read from the file']
  if (row.count > 1) parts.push(`the first of ${row.count} coalesced envelopes; later parts are counted and their text joined, not kept one by one`)
  parts.push('redacted; audio samples not kept')
  return parts.join('; ')
}

function integrityOf(row) {
  const fromFile = row.origins.filter((o) => o.line_sha256)
  const payload = []
  if (typeof row.fields.sha256 === 'string') payload.push({ field: 'sha256', value: row.fields.sha256, means: row.kind === 'media_stored' ? 'sha256 of the stored PCM blob (stated by the agent)' : 'sha256 of the received audio stream (stated by the agent)' })
  if (typeof row.fields.digest?.sha256 === 'string') payload.push({ field: 'digest.sha256', value: row.fields.digest.sha256, means: 'chunk digest stated by the host' })
  return {
    journal_record_hash: row.lane === 'host'
      ? 'none: host journal lines carry no hash or chain'
      : 'none per record: the agentd journal proves only fsynced prefixes (offset + prefix sha256 at a commit barrier), which this feed does not carry',
    file_lines: fromFile.map((o) => ({ file: o.file, offset: o.offset, length: o.length, line_sha256: o.line_sha256, basis: 'sha256 of the exact line bytes at this offset, computed by Mind when it read them; re-check with the same bytes' })),
    payload_hashes: payload,
    observer_feed: row.origins.some((o) => o.via === 'unfold-events observer feed') ? 'the live feed has no per-frame hash and no offset: unknown' : null,
  }
}

function link(r) {
  return { id: r.id, event_id: r.ids.event, kind: r.kind, sentence: r.sentence, category: r.category, status: r.status }
}

/**
 * The structured, redacted metric projection of a row (no metric parsed from display
 * sentences): only ids, phases, statuses and clock origins, never text. The ids are the explicit
 * joins a metric may use; producer names the agent boot and host_life the host process.
 */
const REF_FIELDS = ['response_id', 'request_id', 'command_id', 'utterance_id', 'stream_id', 'status', 'phase', 'signal', 'decision_id', 'level']
export function refOf(r) {
  const f = r.fields ?? {}
  const ref = {}
  for (const k of REF_FIELDS) if (typeof f[k] === 'string' || typeof f[k] === 'number') ref[k] = f[k]
  if (r.lane === 'agentd') ref.producer = r.ids?.producer ?? null
  // The provider adapter's own notes, classified here so no client parses their text: readiness
  // (with the model id) and usage (a connection generation and a token total; no reply id).
  if (r.kind === 'log' && typeof f.msg === 'string') {
    const ready = /^gemini ready\b.*?\bgen=(\d+)\b.*?\bmodel=([\w.-]+)/.exec(f.msg)
    const usage = /^gemini usage gen=(\d+) total_tokens=(\d+)\b/.exec(f.msg)
    if (ready) Object.assign(ref, { tag: 'provider_ready', gen: Number(ready[1]), model: ready[2] })
    else if (usage) Object.assign(ref, { tag: 'provider_usage', gen: Number(usage[1]), total_tokens: Number(usage[2]) })
  }
  if (r.lane === 'host') ref.host_life = r.hostLife ?? null
  ref.continuation = Boolean(r.continuation)
  return ref
}

export function publicRow(r) {
  return {
    ref: refOf(r),
    id: r.id,
    lane: r.lane,
    source: r.source,
    training_eligible: r.training_eligible,
    kind: r.kind,
    known: r.known,
    category: r.category,
    sentence: r.sentence,
    status: r.status,
    observed_at: r.observed_at,
    history: r.history,
    clock: r.clock,
    duration_ms: r.duration_ms,
    duration_basis: r.duration_basis,
    episode: r.episode,
    count: r.count,
    event_id: r.ids.event,
    cause: r.cause,
  }
}

/** One episode as drawn: a title from its explicit parts, a status, a same-clock duration. */
export function episodeView(ep, mind) {
  const rows = ep.rows.map((id) => mind.byId.get(id)).filter(Boolean)
  const of = (k, lane) => rows.find((r) => r.kind === k && (!lane || r.lane === lane))
  const request = rows.find((r) => ['command_submitted', 'user_text'].includes(r.kind) || (r.kind === 'command_admitted' && r.lane === 'agentd') || r.kind === 'command_written')
  const decision = of('decision')
  const started = of('response_started', 'agentd') ?? of('response_started', 'host')
  const done = of('agent_done') ?? of('response_done')
  const cmdFinal = [...rows].reverse().find((r) => r.kind === 'command_admitted' || r.kind === 'command_finished')
  const f = (r, k) => r?.fields?.[k]
  const parts = []
  if (request) parts.push(`${request.kind === 'user_text' ? 'Typed message' : 'Typed request'} ${f(request, 'command_id') ?? f(request, 'request_id') ?? 'unknown'}`)
  else if (f(started, 'request_id')) parts.push(/^mic-/.test(f(started, 'request_id')) ? `Spoken turn on ${f(started, 'request_id')}` : `Request ${f(started, 'request_id')}`)
  if (decision) parts.push(`decided ${f(decision, 'selected') ?? 'unknown'}`)
  const rid = f(started, 'response_id') ?? f(done, 'response_id') ?? rows.map((r) => f(r, 'response_id')).find(Boolean)
  if (rid) parts.push(`reply ${rid}`)
  if (!parts.length) parts.push(rows[0]?.sentence ?? 'episode')
  // Status: an outcome receipt decides; otherwise the furthest observed step; a gap makes it unknown.
  let status = 'observed'
  let because = 'no request, reply or outcome step recognised'
  const outcome = done ?? (cmdFinal && OUTCOME[cmdFinal.status] ? cmdFinal : null)
  if (outcome && OUTCOME[outcome.status]) [status, because] = [outcome.status, `outcome receipt ${outcome.ids.event ?? outcome.id}: ${outcome.sentence}`]
  else if (started || rows.some((r) => r.status === 'pending')) [status, because] = ['pending', 'started; no outcome receipt observed yet']
  else if (request?.status === 'attempted' || rows.some((r) => r.status === 'attempted')) [status, because] = ['attempted', 'sent; no admission or outcome observed']
  if (ep.gap && !(outcome && OUTCOME[outcome.status] && outcome.observed_at > ep.gap.at)) [status, because] = ['unknown', ep.gap.why]
  const agentRows = rows.filter((r) => r.lane === 'agentd' && Number.isFinite(r.env?.t_ns))
  const sameProducer = agentRows.length > 1 && agentRows.every((r) => r.ids.producer === agentRows[0].ids.producer)
  const last = agentRows.at(-1)
  const duration = sameProducer ? (Math.max(...agentRows.map((r) => r.last_t_ns ?? r.env.t_ns)) - agentRows[0].env.t_ns) / 1e6 : null
  const lanes = [...new Set(rows.map((r) => r.lane))]
  return {
    id: ep.id,
    title: parts.join(' → '),
    status,
    because,
    rows: rows.map((r) => ({ id: r.id, kind: r.kind, lane: r.lane, category: r.category, status: r.status, event_id: r.ids.event })),
    count: rows.length,
    evicted: ep.evicted ?? 0,
    opened_at: ep.opened_at,
    last_at: last?.observed_at ?? rows.at(-1)?.observed_at ?? ep.opened_at,
    duration_ms: duration,
    duration_basis: duration == null ? (lanes.length > 1 ? 'unknown: its rows come from more than one clock' : 'unknown') : 'first to last row, one agentd clock',
    lanes,
    gap: ep.gap,
    partial: ep.partial ?? null,
    aliases: ep.aliases ?? [],
    training_eligible: rows.some((r) => r.training_eligible === false) ? false : rows.length && rows.every((r) => r.training_eligible === true) ? true : null,
    sources: [...new Set(rows.map((r) => r.source))],
  }
}

export { REDACTED }
