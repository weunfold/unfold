// What the provider HEARD (speech_transcribed / wire `input_transcript`), kept apart from what
// the agent SAID. Projection only: nothing here sends anything anywhere, and a transcript is
// never turned into a command.
//
// Pairing contract: a transcript belongs to a spoken reply ONLY through explicit ids, never by
// arrival order:
//   - the transcript names the reply: `response_id` with `association: "provider"`, or
//   - the transcript and the reply's Decision or `response_started` share an `utterance_id`.
// Everything else is shown as a separate "heard" note, marked "association unknown". Old
// journals carry neither field, so every transcript in them is unpaired. Gemini Live gives no
// ordering between input transcription and model turns: its adapter emits association "unknown"
// and no utterance_id on its Decisions, so for Gemini every transcript is a note and every reply
// stands alone. The id pairing is kept for a backend that can prove it.
//
// Arrival order: until this backend has shown it can pair (an utterance_id on a Decision or a
// response, or an association "provider"), nothing waits for a pairing that cannot come: a
// final transcript is shown at once. After that, a final one waits `pairGraceMs` for its reply.
//
// Coalescing: `text` is a DELTA; an utterance's text is its parts joined in `utterance_seq` order
// (the envelope's own `seq` is not used), and a part seen again with the same
// `(utterance_id, utterance_seq)` is a duplicate (replay, reconnect) and ignored.
// `utterance_id` is minted by the agent (`<boot>-u<n>`), not by the provider. A transcript is
// final only when a part carried `final: true` (the provider's end flag was parsed); nothing
// else implies finality. A part with `association: "unknown"` pairs only by its utterance_id.
//
// Shown is shown: DSH's log is append-only, so a transcript that changes after it was shown
// (a later part, a filled gap, the end flag) is shown again whole as an UPDATE note, at most
// MAX_UPDATES times per utterance; nothing is silently dropped while it is remembered. Missing
// `utterance_seq` positions (counted from 0, the agent's numbering) are shown as
// "[missing part]", and a gapped transcript is never final, even with the end flag.
//
// Memory is bounded: a fixed number of utterances, a fixed text size per utterance, and fixed
// rings of seen envelope ids and forgotten utterance ids (replay and reconnect dedupe).
//
// ADJACENCY: a deliberate exception to the pairing contract, for a backend that cannot pair.
// Until the backend has shown it can pair (canPair), a transcript is grouped by when it arrived,
// not by its utterance_id: Gemini's adapter keeps one utterance_id for a whole call (it closes
// one only on the provider's `finished` flag, which Gemini does not send), so grouping by that id
// would merge every utterance into one. Rules:
//   - a segment collects the transcript parts that arrive between two boundaries (each part once,
//     deduplicated by envelope id and by (utterance_id, utterance_seq));
//   - a provider `voice_activity_start` begins a new segment (the one before it is closed), and so
//     do a part with another utterance_id and the part after one flagged `final` (the provider's own
//     end of an utterance; Gemini sends neither, its adapter one id per call);
//   - the next spoken reply that OPENS takes the latest closed or open segment with text as its user
//     bubble (bind), and closes it; segments before that one were not answered and are shown on
//     their own first, in order;
//   - a part that arrives after its segment was taken by a reply starts a new segment, so it is
//     shown after that reply (never written into its bubble);
//   - a segment no reply takes is shown on its own adjacentHoldMs after its last part;
//   - once a spoken reply opens with nothing heard, the origin of every later uncorrelated
//     transcript is unknown (it may be that reply's late words), so no later reply takes words by
//     adjacency in this ledger's lifetime: they are shown on their own, never attached to the
//     wrong reply. Explicit ids still pair. With OpenAI, which opens every reply before its words,
//     the words are therefore always notes after the reply, correctly labelled;
//   - a reply that takes, or releases, a segment that is not final leaves that utterance fenced:
//     its later parts are shown on their own, never another reply's input (bounded; on overflow,
//     or for words with no utterance_id, the ledger turns unresolved, as above). The fence lasts
//     for good, for every backend (Gemini's input transcription is independent of its model turn,
//     with no ordering guarantee, so a reply's open is not the end of its words). Gemini, which
//     sends no final flags and keeps one utterance_id per call, therefore shows its words as notes
//     after its first reply that took words; explicit ids still pair;
//   - finality is the provider's, and only for complete text: a segment is final only if a part
//     said so AND no part is missing between its first and last sequence numbers; missing parts
//     are shown as "[missing part]" and counted, for bubbles and notes alike.
// The chat shows the words only; how they were associated stays in Trajectory and Mind's audit.
// Adjacency is a DISPLAY grouping (a heuristic), never a causal id: it is never an input for
// anything downstream. Explicit evidence wins: the first reply or transcript that names an
// utterance or its reply switches the ledger to id pairing BEFORE any adjacency binding, and the
// segments still buffered become id-tracked utterances (same text, no duplicate note), so a
// linked reply takes its own words or none, never a neighbour's.
import { randomBytes } from 'node:crypto'

export const VOICE_LIMITS = { utterances: 32, chars: 4000, seen: 512, pins: 64 }

/** A small insertion-ordered set that forgets its oldest entries. */
export class Ring {
  constructor(max) {
    this.max = max
    this.set = new Set()
  }

  has(k) {
    return this.set.has(k)
  }

  add(k) {
    if (this.set.has(k)) return false
    this.set.add(k)
    if (this.set.size > this.max) this.set.delete(this.set.values().next().value)
    return true
  }
}

export const MISSING = ' [missing part] '
// After this many update notes for one utterance, later changes are only counted.
const MAX_UPDATES = 8

/**
 * What an utterance says so far: its parts in `utterance_seq` order, with a marker for every
 * position not received (from 0, or from the first part of a resumed utterance). A transcript
 * with a gap is never final, whatever the provider's flag said.
 */
function display(u) {
  const seqs = [...u.pieces.keys()].sort((a, b) => a - b)
  let text = ''
  let missing = 0
  let expect = u.resumed ? seqs[0] : 0
  for (const q of seqs) {
    if (q > expect) {
      missing += q - expect
      text += MISSING
    }
    text += u.pieces.get(q)
    expect = q + 1
  }
  return { text, missing, final: u.final && missing === 0 }
}

export class VoiceLedger {
  /**
   * @param onNote called with `{key, text, final, missing, association, utteranceId,
   *   responseId?, late?, update?}` for each transcript shown on its own: unpaired, completed
   *   after its reply, or changed after it was shown (an update note, never an edit).
   * @param timing `pairGraceMs`: how long a final unpaired transcript waits for a reply that
   *   names it (only once the backend has shown it can); `holdMs`: how long a non-final or
   *   gapped one waits after its last part; `retainMs`: how long a settled one is remembered.
   */
  constructor({ onNote = () => {}, timing = {}, limits = {}, now = () => Date.now(), adjacency = true, epoch = randomBytes(4).toString('hex') } = {}) {
    this.onNote = onNote
    this.timing = { pairGraceMs: 2000, holdMs: 6000, retainMs: 30_000, adjacentHoldMs: 4000, ...timing }
    this.limits = { ...VOICE_LIMITS, ...limits }
    this.now = now
    this.utterances = new Map()
    this.seen = new Ring(this.limits.seen)
    this.forgotten = new Ring(this.limits.seen)
    this.pinByUtterance = new Map()
    this.pinnedResponses = new Ring(this.limits.pins)
    this.committedResponses = new Ring(this.limits.pins)
    this.stats = { transcripts: 0, duplicates: 0, notes: 0, late_notes: 0, update_notes: 0, updates_after_note: 0, updates_dropped: 0, truncated: 0, evicted: 0, segments: 0, bound: 0, adjacent_notes: 0 }
    // Adjacency (above): the open segment, closed ones not yet taken or shown, and the ones a reply took.
    // Segment keys become note request ids (`rth-voice-h-<key>`), and DSH treats an id already in
    // rth-root's durable log as accepted. A counter alone restarts at a1 in every process (and a
    // repeated key swallows a note), so each ledger keys its segments with its own random epoch.
    this.epoch = epoch
    this.segment = null
    this.closed = []
    // Set once a spoken reply opened with nothing heard (above).
    this.unresolved = false
    // Utterances whose incomplete words a reply took or released; their later parts are notes.
    this.fenced = new Ring(this.limits.seen)
    this.bound = new Map()
    this.parts = new Ring(this.limits.seen)
    this.segmentTimer = null
    this.timer = null
    // Set once the backend has shown it can name the reply a transcript belongs to.
    this.canPair = false
    // Group what cannot be paired by adjacency; false keeps the strict id contract for it (each
    // transcript its own note at once), which the id-contract tests pin.
    this.adjacency = adjacency
  }

  stop() {
    clearTimeout(this.timer)
    this.timer = null
    clearTimeout(this.segmentTimer)
    this.segmentTimer = null
  }

  /** The words of the utterance the last transcript part went into (the live view), or null. */
  heardText() {
    return this.lastHeard ?? null
  }

  /** Grouped by adjacency: only while the backend cannot pair, and never a part that names its reply. */
  adjacent(env) {
    return this.adjacency && !this.canPair && env.association !== 'provider'
  }

  /** A transcript part into the open segment (a new one when none is open). */
  addPart(env) {
    const part = env.utterance_id && Number.isInteger(env.utterance_seq) ? `${env.utterance_id}#${env.utterance_seq}` : null
    if (part && !this.parts.add(part)) {
      this.stats.duplicates++
      return false
    }
    const uid = typeof env.utterance_id === 'string' && env.utterance_id ? env.utterance_id : null
    if (this.segment?.parts.length && uid && this.segment.utteranceId && uid !== this.segment.utteranceId) this.closeSegment()
    if (!this.segment) {
      this.stats.segments++
      this.segment = { key: `${this.epoch}-a${this.stats.segments}`, utteranceId: uid, parts: [], chars: 0, truncated: false, lastAt: 0, ineligible: !!uid && this.fenced.has(uid) }
    }
    const sg = this.segment
    // Bounded: what fits of a part is kept and the segment is marked cut, so nothing heard is lost
    // without a sign. A segment holds one utterance_id at most, so its parts go in utterance_seq order.
    const room = this.limits.chars - sg.chars
    const text = env.text.length > room ? env.text.slice(0, Math.max(0, room)) : env.text
    if (text.length < env.text.length) {
      this.stats.truncated++
      sg.truncated = true
    }
    if (text) {
      sg.parts.push({ seq: Number.isInteger(env.utterance_seq) ? env.utterance_seq : null, i: sg.parts.length, text })
      sg.chars += text.length
    }
    sg.lastAt = this.now()
    // Taken before a final part closes the segment, so the live view still gets these words.
    this.lastHeard = this.segmentText(sg)
    if (env.final === true) {
      sg.final = true
      this.closeSegment()
    }
    this.scheduleSegments()
    return true
  }

  /**
   * The backend has shown it can pair (an explicit utterance id on a reply, or a transcript that
   * names its reply): buffered segments become id-tracked utterances, so the id contract decides
   * from here on. A segment without an utterance_id can never be paired and is shown on its own.
   */
  becomePairable() {
    if (this.canPair) return
    this.closeSegment()
    clearTimeout(this.segmentTimer)
    this.segmentTimer = null
    // Explicit evidence wins over an adjacency binding not yet committed: those words go
    // back to the id contract too, so the reply that names them takes them.
    const uncommitted = [...this.bound.values()]
    this.bound.clear()
    for (const sg of [...uncommitted, ...this.closed.splice(0)]) {
      if (!sg.utteranceId || this.utterances.has(sg.utteranceId)) {
        this.showSegment(sg)
        continue
      }
      const pieces = new Map()
      sg.parts.forEach((p, i) => pieces.set(p.seq ?? i, p.text))
      const first = Math.min(...pieces.keys())
      this.utterances.set(sg.utteranceId, { key: sg.utteranceId, utteranceId: sg.utteranceId, pieces, chars: sg.chars, final: !!sg.final, providerResponse: null, paired: null, shown: null, committed: false, updates: 0, resumed: first > 0, lastAt: sg.lastAt, truncated: sg.truncated })
      // A part already in the ring was migrated, not new: its later replay stays a duplicate.
      this.stats.migrated = (this.stats.migrated ?? 0) + 1
    }
    this.canPair = true
    this.evict()
    this.schedule()
  }

  /** What the open segment has heard so far (the live listening view), or null. */
  openText() {
    return this.segment?.parts.length ? this.segmentText(this.segment) : null
  }

  /** The words a reply took by adjacency (for its queued placeholder), without committing them. */
  peek(responseId) {
    const sg = this.bound.get(responseId)
    return sg ? this.segmentText(sg) : null
  }

  /** Provider voice_activity_start: the open segment (if it heard anything) is closed. */
  voiceStart() {
    if (this.segment?.parts.length) this.closeSegment()
    else this.segment = null
  }

  closeSegment() {
    if (!this.segment) return
    if (this.segment.parts.length || this.segment.truncated) this.closed.push(this.segment)
    this.segment = null
    while (this.closed.length > this.limits.utterances) this.showSegment(this.closed.shift())
    this.scheduleSegments()
  }

  /**
   * A reply took (or released) this segment. If it is not final, the rest of its utterance
   * may still arrive: that utterance is fenced. Words with no utterance_id, or a full fence ring,
   * cannot be fenced precisely, so the ledger turns unresolved (fail closed).
   */
  fence(sg) {
    if (this.display(sg).final) return
    if (!sg.utteranceId) {
      this.unresolved = true
      return
    }
    if (this.fenced.set.size >= this.fenced.max) {
      this.unresolved = true
      return
    }
    this.fenced.add(sg.utteranceId)
  }

  /** One display for bubbles and notes: the words in order, internal gaps shown and counted. */
  display(sg) {
    const known = sg.parts.every((p) => p.seq !== null)
    const ordered = [...sg.parts].sort((a, b) => (known ? a.seq - b.seq : a.i - b.i))
    let missing = 0
    const pieces = []
    for (const [k, p] of ordered.entries()) {
      if (known && k > 0) {
        const gap = p.seq - ordered[k - 1].seq - 1
        if (gap > 0) {
          missing += gap
          pieces.push(MISSING)
        }
      }
      pieces.push(p.text)
    }
    const text = `${pieces.join('').replace(/\s+/g, ' ').trim()}${sg.truncated ? ' […]' : ''}`
    return { text, missing, final: sg.final === true && missing === 0 }
  }

  segmentText(sg) {
    return this.display(sg).text
  }

  showSegment(sg) {
    this.stats.adjacent_notes++
    const d = this.display(sg)
    this.note({ key: `${sg.key}`, text: d.text, final: d.final, missing: d.missing, association: 'adjacent', utteranceId: null, adjacent: true })
  }

  /** A reply that will write no bubble: what it took is shown on its own at once, never taken again. */
  release(responseId) {
    const sg = this.bound.get(responseId)
    if (!sg) return
    this.bound.delete(responseId)
    this.fence(sg)
    this.showSegment(sg)
  }

  /** A reply opened: it takes the latest segment that heard something; earlier ones are shown first. */
  bindSegment(responseId) {
    this.closeSegment()
    if (this.unresolved) {
      // An earlier reply opened with nothing heard: these words may be its late ones.
      for (const old of this.closed.splice(0)) this.showSegment(old)
      this.scheduleSegments()
      return
    }
    // The latest segment, unless it continues an utterance a reply already took or released.
    const latest = this.closed.pop()
    const sg = latest && !latest.ineligible ? latest : null
    for (const old of [...this.closed.splice(0), ...(latest && !sg ? [latest] : [])]) this.showSegment(old)
    if (!sg) {
      this.unresolved = true
      this.stats.unresolved_replies = (this.stats.unresolved_replies ?? 0) + 1
      return
    }
    this.fence(sg)
    this.stats.bound++
    this.bound.set(responseId, sg)
    while (this.bound.size > this.limits.pins) this.bound.delete(this.bound.keys().next().value)
    this.scheduleSegments()
  }

  /** Segments no reply took are shown adjacentHoldMs after their last part. One timer. */
  scheduleSegments() {
    clearTimeout(this.segmentTimer)
    this.segmentTimer = null
    const waiting = [...this.closed, ...(this.segment?.parts.length ? [this.segment] : [])]
    if (!waiting.length) return
    const next = Math.min(...waiting.map((sg) => sg.lastAt + this.timing.adjacentHoldMs))
    this.segmentTimer = setTimeout(() => this.sweepSegments(), Math.max(0, next - this.now()))
    this.segmentTimer.unref?.()
  }

  sweepSegments() {
    const now = this.now()
    if (this.segment?.parts.length && this.segment.lastAt + this.timing.adjacentHoldMs <= now) this.closeSegment()
    while (this.closed.length && this.closed[0].lastAt + this.timing.adjacentHoldMs <= now) this.showSegment(this.closed.shift())
    this.scheduleSegments()
  }

  /** One `input_transcript` envelope (old or new shape). */
  /** One `input_transcript` envelope; true when it added something new (not a duplicate). */
  transcript(env) {
    if (typeof env.text !== 'string') return false
    if (env.id && !this.seen.add(env.id)) {
      this.stats.duplicates++
      return false
    }
    this.stats.transcripts++
    if (this.adjacent(env)) return this.addPart(env)
    // A transcript that names its reply: explicit evidence, decided before anything else.
    if (this.adjacency && !this.canPair && env.association === 'provider' && typeof env.response_id === 'string') this.becomePairable()
    const utteranceId = typeof env.utterance_id === 'string' && env.utterance_id ? env.utterance_id : null
    const providerResponse = env.association === 'provider' && typeof env.response_id === 'string' ? env.response_id : null
    if (!utteranceId && !providerResponse) {
      // Old shape, or an explicit "unknown" association without an utterance: nothing can ever
      // pair it with a reply. Shown at once, on its own.
      this.lastHeard = env.text.trim() || null
      this.note({ key: `e-${env.id ?? `${this.stats.transcripts}`}`, text: env.text, final: env.final === true, missing: 0, association: 'unknown', utteranceId: null })
      return true
    }
    const key = utteranceId ?? `r-${providerResponse}`
    let u = this.utterances.get(key)
    if (!u) {
      // `resumed`: this utterance was shown and forgotten before; a part arriving now continues
      // it, and the positions before it were shown then, not missing.
      u = { key, utteranceId, pieces: new Map(), chars: 0, final: false, providerResponse: null, paired: null, shown: null, committed: false, updates: 0, resumed: this.forgotten.has(key), lastAt: 0 }
      if (u.resumed) u.updates = 1
      this.utterances.set(key, u)
      this.evict()
    }
    if (providerResponse) {
      u.providerResponse = providerResponse
      this.canPair = true
    }
    const seq = Number.isInteger(env.utterance_seq) ? env.utterance_seq : (u.pieces.size ? Math.max(...u.pieces.keys()) + 1 : 0)
    if (u.pieces.has(seq)) {
      this.stats.duplicates++
      return false
    }
    if (u.chars + env.text.length > this.limits.chars) {
      this.stats.truncated++
      u.truncated = true
      u.pieces.set(seq, '')
    } else {
      u.pieces.set(seq, env.text)
      u.chars += env.text.length
    }
    if (env.final === true) u.final = true
    u.lastAt = this.now()
    this.lastHeard = display(u).text.trim() || null
    if (u.shown) this.stats.updates_after_note++
    // Pairing only before anything was shown: a transcript already shown as unpaired stays a
    // note (its updates too), so it is never shown twice, as a note and as a prompt.
    if (!u.paired && !u.shown) {
      u.paired = this.pinFor(u)
      // Its reply's bubble is already written (the transcription came late): never into that
      // bubble; it is shown on its own once it settles.
      if (u.paired && this.committedResponses.has(u.paired) && !u.committed) {
        u.committed = true
        u.shown = { text: '', final: false }
      }
    }
    // Synchronously: a note that is due now is offered before any later event is ingested.
    this.sweep()
    return true
  }

  pinFor(u) {
    if (u.providerResponse && this.pinnedResponses.has(u.providerResponse)) return u.providerResponse
    if (u.utteranceId && this.pinByUtterance.has(u.utteranceId)) return this.pinByUtterance.get(u.utteranceId)
    return null
  }

  /** A spoken reply opened: transcripts that name it (now or later) are its own. */
  pin({ responseId, utteranceId }) {
    this.pinnedResponses.add(responseId)
    // Explicit evidence first: a reply that names its utterance switches to id pairing
    // before any adjacency binding; only a reply with no id at all takes the adjacent words.
    if (this.adjacency && !this.canPair) {
      if (utteranceId) this.becomePairable()
      else this.bindSegment(responseId)
    }
    if (utteranceId) {
      this.canPair = true
      this.pinByUtterance.set(utteranceId, responseId)
      if (this.pinByUtterance.size > this.limits.pins) this.pinByUtterance.delete(this.pinByUtterance.keys().next().value)
    }
    for (const u of this.utterances.values()) {
      if (u.paired || u.shown) continue
      if (u.providerResponse === responseId || (utteranceId && u.utteranceId === utteranceId)) u.paired = responseId
    }
  }

  /**
   * The reply's user bubble is being written: hand over what was heard for it so far, and
   * remember that, so a later part is never written into (or relabels) that bubble.
   * @returns `{text, final, missing, utteranceId, noted, truncated}` or null when nothing is paired.
   */
  commit({ responseId }) {
    this.committedResponses.add(responseId)
    const sg = this.bound.get(responseId)
    if (sg) {
      this.bound.delete(responseId)
      const d = this.display(sg)
      return { text: d.text, final: d.final, missing: d.missing, utteranceId: null, noted: false, truncated: sg.truncated, adjacent: true }
    }
    let u = null
    for (const c of this.utterances.values()) if (c.paired === responseId && (!u || (c.utteranceId && !u.utteranceId))) u = c
    if (!u) return null
    u.committed = true
    const d = display(u)
    u.shown = { text: d.text, final: d.final }
    this.schedule()
    return { text: d.text, final: d.final, missing: d.missing, utteranceId: u.utteranceId, noted: false, truncated: !!u.truncated }
  }

  note(n) {
    this.stats.notes++
    if (n.late) this.stats.late_notes++
    if (n.update) this.stats.update_notes++
    this.onNote(n)
  }

  changed(u) {
    if (!u.shown) return true
    const d = display(u)
    return d.text !== u.shown.text || d.final !== u.shown.final
  }

  /**
   * The next thing that will happen to `u` and when. Every action records what was shown or
   * removes `u`, so a timer can never spin on the same deadline.
   */
  due(u) {
    const { pairGraceMs, holdMs, retainMs } = this.timing
    const d = display(u)
    const settle = d.final ? (this.canPair && !u.shown ? u.lastAt + pairGraceMs : u.lastAt) : u.lastAt + holdMs
    if (!u.paired && !u.shown) return { at: settle, action: 'note' }
    if (this.changed(u) && u.updates < MAX_UPDATES) {
      if (!u.paired) return { at: settle, action: 'update' }
      if (u.committed) return { at: settle, action: 'late' }
    }
    if (u.paired && !u.committed) return { at: u.lastAt + retainMs, action: 'orphan' }
    return { at: u.lastAt + retainMs, action: 'forget' }
  }

  show(u, action) {
    const d = display(u)
    const base = { text: d.text, final: d.final, missing: d.missing, utteranceId: u.utteranceId, truncated: !!u.truncated }
    const n = u.updates
    if (action === 'note') {
      this.note({ key: `u-${u.key}`, ...base, association: 'unknown', update: u.resumed })
    } else if (action === 'update') {
      // Shown before, changed since (a later part, a gap filled, the end flag): the whole text
      // again as an update note. Still association unknown; no reply link is invented.
      this.note({ key: `u-${u.key}#${n}`, ...base, association: 'unknown', update: true })
    } else {
      // Paired by id, but completed after its reply's bubble was written ('late'), or its reply
      // was never shown ('orphan'): on its own, linked by id; a reply bubble is never rewritten.
      if (action === 'orphan') u.committed = true
      this.note({ key: `l-${u.key}#${n}`, ...base, association: 'provider', responseId: u.paired, late: true })
    }
    if (u.shown) u.updates++
    u.shown = { text: d.text, final: d.final }
  }

  /** Show what has waited long enough; forget what is settled. One timer, never a poll loop. */
  sweep() {
    clearTimeout(this.timer)
    this.timer = null
    const now = this.now()
    for (const u of [...this.utterances.values()]) {
      const d = this.due(u)
      if (d.at > now) continue
      if (d.action === 'forget') {
        if (this.changed(u)) this.stats.updates_dropped++
        this.forgotten.add(u.key)
        this.utterances.delete(u.key)
      } else {
        this.show(u, d.action)
      }
    }
    this.schedule()
  }

  evict() {
    while (this.utterances.size > this.limits.utterances) {
      const [key, u] = this.utterances.entries().next().value
      // Never silently lose what was heard: an unshown or changed oldest utterance is shown
      // before it goes.
      if (!u.paired && this.changed(u)) this.show(u, u.shown ? 'update' : 'note')
      this.forgotten.add(key)
      this.utterances.delete(key)
      this.stats.evicted++
    }
  }

  schedule() {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    let next = Infinity
    for (const u of this.utterances.values()) next = Math.min(next, this.due(u).at)
    if (next === Infinity) return
    this.timer = setTimeout(() => this.sweep(), Math.max(0, next - this.now()))
    this.timer.unref?.()
  }
}
