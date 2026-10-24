// The session adapter's link to RTH. Input goes ONLY through the host control owner (the one
// controller); output is read from rth-agentd as a read-only observer. A command is tracked
// from before it is submitted, so no event can arrive "too early", and it ends exactly once:
// with the agent's terminal for its response, or with the recorded state the agent reports
// for a retried id (including `interrupted`). Nothing here re-runs anything: a retry resends
// the same command id, and the agent answers it from its durable command log.
//
// Spoken turns are PROJECTION ONLY: a reply the provider
// opened from the microphone (a Decision with a `provider:` policy, then `response_started`)
// is followed by its own ids; it is never submitted, re-asked or re-run, and its transcript is
// never sent anywhere. See runVoice() and ./voice.mjs.
import { AgentdObserver } from '@rth/dsh-events/observer'
import { Ring, VoiceLedger } from './voice.mjs'

const TERMINAL = new Set(['completed', 'cancelled', 'failed', 'interrupted', 'rejected'])
export { Track }
const BACKOFF_MS = [250, 500, 1000, 2000]

const sleep = (ms, signal) =>
  new Promise((resolve) => {
    if (signal?.aborted) return resolve()
    const t = setTimeout(resolve, ms)
    signal?.addEventListener('abort', () => { clearTimeout(t); resolve() }, { once: true })
  })

class Track {
  constructor(commandId) {
    this.commandId = commandId
    this.responseId = null
    this.text = ''
    this.queue = []
    this.final = null
    this.acked = false
    this.gap = false
    this.terminalSeen = null
    this.reaskAt = null
    this.wake = null
  }

  push(ev) {
    this.queue.push(ev)
    this.wake?.()
  }

  finish(final) {
    if (this.final) return
    this.final = final
    this.wake?.()
  }

  next() {
    if (this.queue.length || this.final || this.terminalSeen) return Promise.resolve()
    return new Promise((resolve) => { this.wake = () => { this.wake = null; resolve() } })
  }
}

export class RthBridge {
  constructor({ host, agentd = { host: '127.0.0.1', port: 7700 }, log = () => {}, faults = {}, observer, timing = {} } = {}) {
    this.host = host
    this.agentd = agentd
    this.log = log
    this.faults = { dropAcks: 0, ...faults }
    // stopMs: one absolute deadline from the first stop until a backend terminal.
    // reaskMs: the bounded re-ask schedule after an observer gap.
    // voiceIdleMs: a spoken reply with no event for this long, and no end, is shown incomplete.
    // voiceGapWaitMs: after an observer gap, how long a spoken reply waits for its end.
    // voiceOpen: spoken replies kept (open, or ended and not yet shown); the oldest is closed as unknown.
    this.timing = { stopMs: 3000, reaskMs: [250, 500, 1000, 2000, 4000, 5000], reconcileAttempts: 8, voiceIdleMs: 30_000, voiceOpen: 16, ...timing }
    this.timing.voiceGapWaitMs ??= this.timing.reaskMs.reduce((a, b) => a + b, 0)
    this.tracks = new Map()
    this.byResponse = new Map()
    this.link = { state: 'idle', boot_id: null, session_id: null, command_generation: null, attaches: 0 }
    this.stats = { submits: 0, retries: 0, lost_acks_injected: 0, duplicates: 0, admissions: 0, rejected: 0, reconciled: 0, gaps: 0 }
    // Spoken replies, keyed by response id; provider Decisions keyed by their ENVELOPE id
    // (`response_started.cause` names the envelope, not the decision_id).
    this.voiceTracks = new Map()
    this.voiceDecisions = new Map()
    this.voiceSeen = new Ring(256)
    this.voiceStats = { decisions: 0, opened: 0, duplicates: 0, unattributed: 0, superseded: 0, evicted: 0, reconciled: 0, incomplete: 0 }
    // Live speech facts already published: a replayed signal never renews or revives the view.
    this.liveSeen = new Ring(512)
    this.ledger = new VoiceLedger({ timing: timing.voice, onNote: (n) => this.emit('voice-heard', n) })
    this.observer = observer ?? null
    this.listeners = new Set()
    // Resolved on unload: every wait in run() is raced against it.
    this.unloaded = new Promise((resolve) => { this.unload = resolve })
  }

  start() {
    this.observer ??= new AgentdObserver({
      host: this.agentd.host,
      port: this.agentd.port,
      producer: `dsh-session-${Date.now().toString(16)}-${process.pid}`,
      onEnvelope: (env) => this.ingest(env),
      onState: (state, detail) => this.onLink(state, detail),
    })
    this.observer.start()
    return this
  }

  stop() {
    this.observer?.stop()
    this.ledger.stop()
    for (const t of [...this.tracks.values(), ...this.voiceTracks.values()]) t.finish({ status: 'unknown', text: t.text, detail: 'the DSH session adapter was unloaded before a result arrived', voice: t.voice })
    this.unload()
  }

  onChange(fn) {
    this.listeners.add(fn)
    return () => this.listeners.delete(fn)
  }

  emit(kind, data) {
    for (const fn of this.listeners) fn(kind, data)
  }

  onLink(state, detail) {
    const was = this.link.state
    this.link.state = state
    this.link.detail = detail ?? null
    if (state === 'attached') {
      this.link.attaches++
      // The observer has no replay cursor: anything an open command did while we were not
      // attached is unseen. Mark the gap; its terminal is then taken from the record.
      for (const t of this.tracks.values()) {
        if (t.final) continue
        if (!t.gap) this.stats.gaps++
        t.gap = true
        t.reaskAt = Date.now() + this.timing.reaskMs[0]
        t.wake?.()
      }
      // A spoken reply has no command to re-ask: it waits (bounded) for its end, then is
      // checked against a recorded reply if the owner has one, or shown incomplete.
      for (const t of this.voiceTracks.values()) {
        if (t.final) continue
        if (!t.gap) this.stats.gaps++
        t.gap = true
        t.gapDeadline = Date.now() + this.timing.voiceGapWaitMs
        t.wake?.()
      }
    } else if (was === 'attached') {
      for (const t of this.tracks.values()) if (!t.final) t.gap = true
      // A spoken reply's bounded wait starts at the link loss too (a reattach restarts it): a
      // DSH turn that starts while the observer is still away must wait, not reconcile at once
      // against a reply that is still running.
      for (const t of this.voiceTracks.values()) {
        if (t.final) continue
        t.gap = true
        t.gapDeadline = Date.now() + this.timing.voiceGapWaitMs
      }
    }
    this.emit('link', { ...this.link })
  }

  ingest(env) {
    // Every observed envelope, for delegation's journal-id transcript.
    this.emit('journal', env)
    switch (env.kind) {
      // Delegation: agentd's committed task records, for the executor. Observed
      // only; the executor reports back through the host owner, never through this link.
      case 'task_state':
        // rth-protocol carries the ledger record nested (`{task_id, record}`).
        this.emit('task-state', env.record ?? env)
        return
      case 'hello':
        this.link.boot_id = env.boot_id
        this.link.session_id = env.instance?.session_id ?? null
        this.link.command_generation = env.command_generation ?? null
        this.link.backend = env.backend ?? null
        break
      case 'command_admitted': {
        this.link.command_generation = Math.max(this.link.command_generation ?? 0, env.generation)
        const t = this.tracks.get(env.command_id)
        if (!t) break
        if (env.duplicate) this.stats.duplicates++
        this.adopt(t, env)
        break
      }
      case 'decision':
        if (typeof env.policy === 'string' && env.policy.startsWith('provider:') && env.id) {
          this.voiceStats.decisions++
          this.voiceDecisions.set(env.id, { decisionId: env.decision_id ?? null, utteranceId: env.utterance_id ?? null, policy: env.policy })
          if (this.voiceDecisions.size > 64) this.voiceDecisions.delete(this.voiceDecisions.keys().next().value)
        }
        break
      case 'response_started': {
        const t = this.tracks.get(env.request_id)
        if (!t) {
          this.openVoice(env)
          break
        }
        t.responseId = env.response_id
        this.byResponse.set(env.response_id, t)
        break
      }
      case 'agent_delta': {
        const t = this.byResponse.get(env.response_id)
        if (!t || t.final) break
        if (t.voice && this.replayed(t, env)) break
        t.text += env.text
        t.lastEventAt = Date.now()
        t.push({ type: 'delta', text: env.text })
        break
      }
      case 'agent_done': {
        const t = this.byResponse.get(env.response_id)
        if (!t || t.final) break
        // Seen live. Without a gap the live text is the whole reply; with one, run() takes
        // the recorded result before anything is committed.
        t.terminalSeen = { status: env.status, responseId: env.response_id }
        // A spoken reply seen whole is final at once (its DSH turn may still be queued behind
        // a typed one, and a later gap must not cast doubt on what was already seen).
        if (t.voice && !t.gap) t.finish({ status: env.status, text: t.text, responseId: env.response_id, voice: t.voice })
        t.wake?.()
        break
      }
      case 'provider_signal': {
        const t = env.response_id ? this.voiceTracks.get(env.response_id) : null
        if (t && !t.final) t.voice.signals.push(env.signal)
        // The provider heard speech begin (also during a reply: a barge-in): a new utterance.
        // Once per envelope: a replay of the same signal is not new speech.
        const speech = env.signal === 'voice_activity_start' || env.signal === 'voice_activity_end'
        if (speech && (!env.id || this.liveSeen.add(env.id))) {
          if (env.signal === 'voice_activity_start') this.ledger.voiceStart()
          this.emit('voice-live', { type: env.signal === 'voice_activity_start' ? 'speech-start' : 'speech-end', responseId: env.response_id ?? null })
        }
        break
      }
      case 'input_transcript':
        // Only a part the ledger accepted as new updates the live view: a duplicate never
        // brings shown words back or renews their deadline.
        if (this.ledger.transcript(env) === true) this.emit('voice-live', { type: 'heard', text: this.ledger.heardText() })
        break
      default:
    }
  }

  /** Take what the agent RECORDED for this command (an admission or a retry's answer). */
  adopt(t, c) {
    if (c.response_id && !t.responseId) {
      t.responseId = c.response_id
      this.byResponse.set(c.response_id, t)
    }
    if (TERMINAL.has(c.status)) {
      // The recorded text replaces what we saw ONLY when the record says it is whole
      // (`text_complete: true`: an unbroken observation or the command log's own result). A
      // missing field counts as not whole: a record's `result_text` can be a suffix. Otherwise the
      // known (live) text is kept.
      const responseId = c.response_id ?? t.responseId
      if (c.text_complete === true && typeof c.result_text === 'string') {
        t.finish({ status: c.status, text: c.result_text, detail: c.detail ?? null, responseId, recorded: true })
        return
      }
      if (c.status === 'completed') {
        // Without a gap the live stream carries the whole reply and its own end: wait for it.
        if (!t.gap) return
        // After a gap: it ended, but its text is not known whole yet. The bounded
        // reconciliation keeps asking (same id) until a whole record (a command-log result)
        // arrives, or ends it incomplete with the known text.
        t.incompleteRecord = c
        t.terminalSeen ??= { status: c.status, responseId }
        t.wake?.()
        return
      }
      t.finish({ status: c.status, text: t.text, detail: c.detail ?? null, responseId, recorded: true })
    }
  }

  /** Ask the owner (same id) for the recorded state; true when it was terminal. */
  async reask(t, why) {
    this.stats.retries++
    this.log(`rth-session: asking the owner for the recorded state of ${t.commandId} (${why})`)
    const r = await this.host.submit(t.commandId, t.submittedText)
    if (r.status === 200 && r.body?.command) {
      this.adopt(t, r.body.command)
      return !!t.final
    }
    return false
  }

  /** After a gap: the live terminal is not enough; wait (bounded) for the recorded result. */
  async reconcile(t) {
    for (let i = 0; i < this.timing.reconcileAttempts && !t.final && !t.closed; i++) {
      if (await this.reask(t, 'reconciling a terminal seen after an observer gap')) {
        this.stats.reconciled++
        return
      }
      await sleep(this.timing.reaskMs[Math.min(i, this.timing.reaskMs.length - 1)])
    }
    if (t.closed || t.final) return
    const c = t.incompleteRecord
    const how = c ? (c.result_text == null ? 'without its text' : `without a complete text${c.observed ? ` (observed ${c.observed})` : ''}`) : null
    t.finish({
      status: 'incomplete',
      text: t.text,
      detail: how
        ? `recorded as completed, but ${how}, and the live stream had a gap; this reply may be incomplete`
        : 'the live stream had a gap and the recorded result was not available in time; this reply may be incomplete',
      responseId: t.responseId,
      recorded: !!c,
    })
  }

  // ── spoken turns: projection only ──

  /** A `response_started` that is not a typed command's: open a spoken reply when its ids say so. */
  openVoice(env) {
    const rid = env.response_id
    if (typeof rid !== 'string') return
    const decision = env.cause ? this.voiceDecisions.get(env.cause) : undefined
    const utteranceId = env.utterance_id ?? decision?.utteranceId ?? null
    // Only an attributed reply is a spoken turn: its Decision has a provider policy, or the
    // response itself names an utterance. Anything else (a reply we saw no cause for) is left out.
    if (!decision && !env.utterance_id) {
      this.voiceStats.unattributed++
      return
    }
    if (this.voiceTracks.has(rid) || !this.voiceSeen.add(rid)) {
      this.voiceStats.duplicates++
      return
    }
    // A new spoken reply while an older one shows no end: Gemini opens one provider turn at a
    // time, so the older end was missed. It is closed as incomplete, never as completed.
    for (const o of this.voiceTracks.values()) {
      if (o.final || o.terminalSeen) continue
      this.voiceStats.superseded++
      o.superseded = true
      o.wake?.()
    }
    const t = new Track(`voice:${rid}`)
    t.responseId = rid
    t.lastEventAt = Date.now()
    t.voice = { responseId: rid, decisionId: decision?.decisionId ?? null, decisionEvent: env.cause ?? null, utteranceId, policy: decision?.policy ?? null, requestId: env.request_id ?? null, signals: [] }
    this.voiceTracks.set(rid, t)
    this.byResponse.set(rid, t)
    this.voiceStats.opened++
    this.ledger.pin({ responseId: rid, utteranceId })
    while (this.voiceTracks.size > this.timing.voiceOpen) {
      const [oldId, old] = this.voiceTracks.entries().next().value
      old.finish({ status: 'unknown', text: old.text, detail: 'too many spoken replies were open at once; this one was no longer followed', voice: old.voice })
      this.dropVoice(oldId)
      this.voiceStats.evicted++
    }
    this.emit('voice-opened', { ...t.voice, heard: this.ledger.peek(rid) })
    this.emit('voice-live', { type: 'reply', responseId: rid })
  }

  /** A spoken reply's part seen again (a replayed journal): envelope seqs only grow per producer. */
  replayed(t, env) {
    if (typeof env.producer !== 'string' || !Number.isInteger(env.seq)) return false
    t.lastSeq ??= new Map()
    if (env.seq <= (t.lastSeq.get(env.producer) ?? -1)) {
      this.voiceStats.duplicates++
      return true
    }
    t.lastSeq.set(env.producer, env.seq)
    return false
  }

  dropVoice(rid) {
    const t = this.voiceTracks.get(rid)
    this.voiceTracks.delete(rid)
    if (t && this.byResponse.get(rid) === t) this.byResponse.delete(rid)
  }

  /**
   * After an observer gap: a spoken reply has no command, so nothing is re-asked or re-sent.
   * The owner's recorded reply (`host.reply`: GET /v1/replies/<id>, read only, never `submit`)
   * replaces what we saw ONLY when it says `text_complete: true` (an unbroken start-to-terminal
   * observation, or an authoritative command-log result). A suffix, a
   * terminal-only record, a missing field or a 404 all leave our own text with the incomplete
   * marker; a recorded status is named in the detail, never used to claim the text is whole.
   */
  async reconcileVoice(t) {
    const rid = t.responseId
    let recorded = null
    // The owner records a reply when IT sees the end; seen here first (over the observer), its
    // cache can still be a moment behind. So, only when the live end was seen, a 404 is asked
    // again briefly (bounded; every wait also ends at unload, and the caller races the stop
    // deadline). Without a live end, a 404 is final.
    const notFoundBackoff = this.timing.notFoundRetryMs ?? [250, 500, 750]
    let notFound = 0
    if (typeof this.host?.reply === 'function') {
      for (let i = 0; i < this.timing.reconcileAttempts && !t.final && !t.closed; i++) {
        const r = await this.host.reply(rid)
        const rec = r.status === 200 ? r.body?.reply : null
        if (rec && TERMINAL.has(rec.status) && rec.text_complete === true && typeof rec.text === 'string') {
          this.voiceStats.reconciled++
          this.stats.reconciled++
          t.finish({ status: rec.status, text: rec.text, detail: rec.detail ?? null, responseId: rid, recorded: true, voice: t.voice })
          return
        }
        if (rec) recorded = rec
        if (r.status === 404 && t.terminalSeen && notFound < notFoundBackoff.length) {
          this.voiceStats.not_found_retries = (this.voiceStats.not_found_retries ?? 0) + 1
          await Promise.race([sleep(notFoundBackoff[notFound++]), this.unloaded])
          i-- // not a transport attempt
          continue
        }
        // An answer (incomplete text, 404, no record): nothing better will come by asking again.
        // Only a transport failure or an unavailable owner is retried.
        if (r.status !== 0 && r.status !== 503) break
        await Promise.race([sleep(this.timing.reaskMs[Math.min(i, this.timing.reaskMs.length - 1)]), this.unloaded])
      }
    }
    if (t.closed || t.final) return
    this.voiceStats.incomplete++
    const end = t.terminalSeen?.status ?? (recorded && TERMINAL.has(recorded.status) ? recorded.status : null)
    const record = recorded
      ? `; the owner's record (${recorded.status ?? 'no status'}, observed ${recorded.observed ?? 'unknown'}${recorded.source ? ` from ${recorded.source}` : ''}${recorded.truncated ? ', truncated' : ''}) does not hold the whole text`
      : ''
    t.finish({
      status: 'incomplete',
      text: t.text,
      detail: end
        ? `the live stream had a gap; the reply ended (${end}), but no complete recorded text for this spoken reply was available${record}, so this text may be incomplete`
        : 'the live stream had a gap and the end of this spoken reply was not seen; this text may be incomplete',
      responseId: rid,
      voice: t.voice,
    })
  }

  /**
   * Follow spoken reply `responseId` (opened by the provider, never submitted here): yields
   * `{type:'delta', text}` events and ends with `{type:'final', status, text, detail, voice}`.
   * A stop maps to the owner's cancel with the same one stop deadline as typed turns.
   */
  async *runVoice(responseId, signal) {
    const t = this.voiceTracks.get(responseId)
    if (!t || t.following) {
      yield { type: 'final', status: 'unknown', text: '', detail: t ? 'this spoken reply is already shown' : 'this spoken reply is no longer followed by the adapter (it was unloaded or restarted, or the reply was evicted)', voice: t?.voice ?? { responseId } }
      return
    }
    t.following = true
    let stopDeadline = null
    let hitDeadline
    const deadline = new Promise((resolve) => { hitDeadline = resolve })
    const stopNow = () => {
      if (stopDeadline !== null) return
      stopDeadline = Date.now() + this.timing.stopMs
      setTimeout(hitDeadline, this.timing.stopMs)
      void this.host?.cancel?.()
      t.wake?.()
    }
    const STOPPED = Symbol('stopped')
    const raced = (p) => Promise.race([p, deadline.then(() => STOPPED), this.unloaded.then(() => STOPPED)])
    const unconfirmed = () =>
      t.finish({ status: 'stop_unconfirmed', text: t.text, detail: 'local stop requested; no backend terminal within the stop deadline, so the backend outcome is unknown', responseId, voice: t.voice })
    const onAbort = () => stopNow()
    signal?.addEventListener('abort', onAbort, { once: true })
    if (signal?.aborted) stopNow()
    try {
      for (;;) {
        while (t.queue.length) yield t.queue.shift()
        if (t.final) break
        const now = Date.now()
        if (stopDeadline !== null && now >= stopDeadline) {
          unconfirmed()
          continue
        }
        if (t.superseded && !t.terminalSeen) {
          this.voiceStats.incomplete++
          t.finish({ status: 'incomplete', text: t.text, detail: 'a later spoken reply started before the end of this one was seen; this text may be incomplete', responseId, voice: t.voice })
          continue
        }
        if (t.gap && (t.terminalSeen || now >= (t.gapDeadline ?? 0))) {
          if ((await raced(this.reconcileVoice(t))) === STOPPED && !t.final) unconfirmed()
          continue
        }
        if (now - t.lastEventAt >= this.timing.voiceIdleMs) {
          this.voiceStats.incomplete++
          t.finish({ status: 'incomplete', text: t.text, detail: `no end of this spoken reply was seen within ${Math.round(this.timing.voiceIdleMs / 1000)} s of its last event; this text may be incomplete`, responseId, voice: t.voice })
          continue
        }
        // Each wait's timer is cleared as soon as one of them fires: no timer outlives its loop.
        const timers = new AbortController()
        const waits = [t.next(), sleep(Math.max(0, t.lastEventAt + this.timing.voiceIdleMs - now), timers.signal)]
        if (stopDeadline !== null) waits.push(sleep(Math.max(0, stopDeadline - now), timers.signal))
        if (t.gap && t.gapDeadline) waits.push(sleep(Math.max(0, t.gapDeadline - now), timers.signal))
        const woke = await raced(Promise.race(waits))
        timers.abort()
        if (woke === STOPPED && !t.final) unconfirmed()
        t.wake = null
      }
      yield { type: 'final', ...t.final, voice: t.voice }
    } finally {
      t.closed = true
      signal?.removeEventListener('abort', onAbort)
      this.dropVoice(responseId)
    }
  }

  /** The reply's user bubble is being written: what was heard for it, by explicit ids only. */
  voiceCommit(voice) {
    return this.ledger.commit({ responseId: voice.responseId })
  }

  /**
   * Submit `text` as command `commandId` through the owner and yield `{type:'admitted'}`,
   * `{type:'delta', text}` events, ending with `{type:'final', status, text, detail}`.
   * A lost ack, a dropped link or an agent restart is retried with the same id. `cancelled`
   * is reported only when the backend confirmed it; a stop with no confirmation within the
   * deadline is `stop_unconfirmed` (local stop requested, backend outcome unknown).
   */
  async *run(commandId, text, signal) {
    const t = this.tracks.get(commandId) ?? new Track(commandId)
    t.submittedText = text
    // Submitted while the observer is not attached: whatever happens before it attaches is unseen.
    if (this.link.state !== 'attached') {
      t.gap = true
      this.stats.gaps++
    }
    this.tracks.set(commandId, t)
    // ONE absolute stop deadline from the first stop; every wait below (an admission, a
    // re-ask, a reconciliation, an event) is raced against it and against unload.
    let stopDeadline = null
    let hitDeadline
    const deadline = new Promise((resolve) => { hitDeadline = resolve })
    const stopNow = () => {
      if (stopDeadline !== null) return
      stopDeadline = Date.now() + this.timing.stopMs
      setTimeout(hitDeadline, this.timing.stopMs)
      void this.host.cancel()
      t.wake?.()
    }
    const STOPPED = Symbol('stopped')
    const raced = (p) => Promise.race([p, deadline.then(() => STOPPED), this.unloaded.then(() => STOPPED)])
    const unconfirmed = () =>
      t.finish({ status: 'stop_unconfirmed', text: t.text, detail: 'local stop requested; no backend terminal within the stop deadline, so the backend outcome is unknown', responseId: t.responseId })
    const onAbort = () => stopNow()
    signal?.addEventListener('abort', onAbort, { once: true })
    try {
      let attempt = 0
      for (;;) {
        if (signal?.aborted) break
        this.stats.submits++
        const r = await raced(this.host.submit(commandId, text))
        if (r === STOPPED) break
        if (r.status === 200 && this.faults.dropAcks > 0) {
          // Fault injection (tests only): the answer is "lost"; retry with the same id.
          this.faults.dropAcks--
          this.stats.lost_acks_injected++
          this.log(`rth-session: injected lost ack for ${commandId}`)
          continue
        }
        if (r.status === 200) {
          t.acked = true
          this.stats.admissions++
          yield { type: 'admitted', command: r.body.command, duplicate: !!r.body.duplicate }
          this.adopt(t, r.body.command)
          break
        }
        if (r.status === 409) {
          this.stats.rejected++
          t.finish({ status: 'rejected', text: '', detail: r.body?.rejected ?? 'rejected' })
          break
        }
        if (r.status === 0 || r.body?.retry) {
          this.stats.retries++
          this.log(`rth-session: ${commandId} not answered (${r.status}: ${r.body?.error}); retrying with the same id`)
          if ((await raced(sleep(BACKOFF_MS[Math.min(attempt++, BACKOFF_MS.length - 1)], signal))) === STOPPED) break
          continue
        }
        t.finish({ status: 'failed', text: '', detail: r.body?.error ?? `host answered ${r.status}` })
        break
      }
      if (signal?.aborted) stopNow()
      let reask = 0
      for (;;) {
        while (t.queue.length) yield t.queue.shift()
        if (t.final) break
        const now = Date.now()
        if (stopDeadline !== null && now >= stopDeadline) {
          unconfirmed()
          continue
        }
        if (t.terminalSeen) {
          if (!t.gap) t.finish({ status: t.terminalSeen.status, text: t.text, responseId: t.terminalSeen.responseId })
          else if ((await raced(this.reconcile(t))) === STOPPED) unconfirmed()
          continue
        }
        if (t.gap && t.acked && t.reaskAt && now >= t.reaskAt) {
          t.reaskAt = Date.now() + this.timing.reaskMs[Math.min(++reask, this.timing.reaskMs.length - 1)]
          if ((await raced(this.reask(t, 'observer gap'))) === STOPPED && !t.final) unconfirmed()
          continue
        }
        const waits = [t.next()]
        if (stopDeadline !== null) waits.push(sleep(Math.max(0, stopDeadline - now)))
        if (t.gap && t.acked && t.reaskAt) waits.push(sleep(Math.max(0, t.reaskAt - now)))
        if ((await raced(Promise.race(waits))) === STOPPED && !t.final) unconfirmed()
        t.wake = null
      }
      yield { type: 'final', ...t.final }
    } finally {
      t.closed = true
      signal?.removeEventListener('abort', onAbort)
      this.tracks.delete(commandId)
      if (t.responseId) this.byResponse.delete(t.responseId)
    }
  }
}
