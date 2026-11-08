// The session adapter's link to agentd. Input goes ONLY through Unfold's session-control
// endpoint (./host-client.js): one `/submit` per composer message, which the app's chat sends to
// agentd as a typed command; the 200 names that command's id. Output is read from agentd as a
// read-only observer. The endpoint has no other verb: no re-ask, no cancel, no recorded result.
// So a message is submitted at most once (a known message id is never sent again, whatever
// happened to the first try), a broadcast that arrives before the 200 is kept briefly and
// adopted when the id is known, and a reply whose live stream had a gap ends marked as possibly
// incomplete: there is no record to take its whole text from.
//
// Spoken turns are PROJECTION ONLY: a reply the provider opened from the microphone (a Decision
// with a `provider:` policy, then `response_started`) is followed by its own ids; it is never
// submitted or re-run, and its transcript is never sent anywhere. See runVoice() and ./voice.js.
import { AgentdObserver, observerAddress } from './observer.js'
import { readChannel, refusedText, validTurnId } from './host-client.js'
import { Ring, VoiceLedger } from './voice.js'

const TERMINAL = new Set(['completed', 'cancelled', 'failed', 'interrupted', 'rejected'])
export { Track }
// Broadcasts kept for a command id not known yet (its /submit answer still on its way).
const EARLY_MAX = 512
// Why nothing was written to the control endpoint.
const NOT_REACHED = {
  no_channel: 'this DSH was not started by Unfold, so it has no control endpoint',
  no_session: 'agentd has not named the Unfold Session yet',
  unreachable: "Unfold's control endpoint is not running",
  peer: 'the control endpoint refused this process (it serves only the instance child)',
  closed: 'the control endpoint closed the connection before the request was read',
}
// Why a message may have been sent: never sent again, shown as unknown.
const UNKNOWN = {
  transport: 'the connection to the control endpoint failed after the message was written, so it may have been sent',
  unreadable: 'the control endpoint answered 200 in a form this plugin cannot read, so it may have been sent',
  too_large: "the control endpoint's answer was over 64 KiB and was cut off, so the message may have been sent",
}
// Composer messages remembered as handled, so a redelivery is never sent again.
const SENT_MAX = 1024

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

export class SessionBridge {
  constructor({ host, log = () => {}, observer, address, timing = {} } = {}) {
    this.host = host
    this.log = log
    // stopMs: one absolute deadline from the first stop until a backend terminal.
    // typedIdleMs: after Unfold took a typed message (the 200), how long its reply may go without
    //   an event (each event renews it); then it ends as not seen, or possibly incomplete.
    // gapWaitMs: after an observer gap, how long a typed reply may go without an event before it
    //   is shown as possibly incomplete (each event renews it).
    // voiceIdleMs: a spoken reply with no event for this long, and no end, is shown incomplete.
    // voiceGapWaitMs: after an observer gap, how long a spoken reply waits for its end.
    // voiceOpen: spoken replies kept (open, or ended and not yet shown); the oldest is closed as unknown.
    this.timing = { stopMs: 3000, typedIdleMs: 120_000, gapWaitMs: 12_750, voiceIdleMs: 30_000, voiceGapWaitMs: 12_750, voiceOpen: 16, ...timing }
    this.tracks = new Map()
    this.submitting = new Set()
    this.byResponse = new Map()
    this.early = []
    this.sent = new Map()
    this.link = { state: 'idle', boot_id: null, session_id: null, command_generation: null, attaches: 0 }
    this.stats = { submits: 0, admissions: 0, not_sent: 0, rejected: 0, unknown: 0, already_handled: 0, early_adopted: 0, gaps: 0 }
    // Spoken replies, keyed by response id; provider Decisions keyed by their ENVELOPE id
    // (`response_started.cause` names the envelope, not the decision_id).
    this.voiceTracks = new Map()
    this.voiceDecisions = new Map()
    this.voiceSeen = new Ring(256)
    this.voiceStats = { decisions: 0, opened: 0, duplicates: 0, unattributed: 0, superseded: 0, evicted: 0, incomplete: 0 }
    // Live speech facts already published: a replayed signal never renews or revives the view.
    this.liveSeen = new Ring(512)
    this.ledger = new VoiceLedger({ timing: timing.voice, onNote: (n) => this.emit('voice-heard', n) })
    this.observer = observer ?? null
    // agentd's address, asked at every connect: by default the observer file the preload named.
    this.address = address ?? (() => observerAddress(readChannel()?.observerFile))
    this.listeners = new Set()
    // Resolved on unload: every wait in run() is raced against it.
    this.unloaded = new Promise((resolve) => { this.unload = resolve })
  }

  start() {
    this.observer ??= new AgentdObserver({
      address: this.address,
      producer: `unfold-session-${Date.now().toString(16)}-${process.pid}`,
      onEnvelope: (env) => this.ingest(env),
      onState: (state, detail) => this.onLink(state, detail),
    })
    this.observer.start()
    return this
  }

  stop() {
    this.observer?.stop()
    this.ledger.stop()
    for (const t of [...this.submitting, ...this.tracks.values(), ...this.voiceTracks.values()]) t.finish({ status: 'unknown', text: t.text, detail: 'the DSH session adapter was unloaded before a result arrived', voice: t.voice })
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
    const typed = [...this.submitting, ...this.tracks.values()]
    if (state === 'attached') {
      this.link.attaches++
      // The observer has no replay cursor: anything an open command did while we were not
      // attached is unseen. Mark the gap; the bounded wait for its end starts now.
      for (const t of typed) {
        if (t.final) continue
        if (!t.gap) this.stats.gaps++
        t.gap = true
        t.lastEventAt = Date.now()
        t.wake?.()
      }
      // A spoken reply waits (bounded) for its end, then is shown incomplete.
      for (const t of this.voiceTracks.values()) {
        if (t.final) continue
        if (!t.gap) this.stats.gaps++
        t.gap = true
        t.gapDeadline = Date.now() + this.timing.voiceGapWaitMs
        t.wake?.()
      }
    } else if (was === 'attached') {
      for (const t of typed) if (!t.final) t.gap = true
      // A spoken reply's bounded wait starts at the link loss too (a reattach restarts it): a
      // DSH turn that starts while the observer is still away must wait for a reply that is
      // still running.
      for (const t of this.voiceTracks.values()) {
        if (t.final) continue
        t.gap = true
        t.gapDeadline = Date.now() + this.timing.voiceGapWaitMs
      }
    }
    this.emit('link', { ...this.link })
  }

  ingest(env) {
    switch (env.kind) {
      case 'hello':
        this.link.boot_id = env.boot_id
        this.link.session_id = env.instance?.session_id ?? null
        this.link.command_generation = env.command_generation ?? null
        this.link.backend = env.backend ?? null
        break
      case 'command_admitted': {
        this.link.command_generation = Math.max(this.link.command_generation ?? 0, env.generation)
        const t = this.tracks.get(env.command_id)
        if (!t) {
          this.keepEarly(env)
          break
        }
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
          this.keepEarly(env)
          this.openVoice(env)
          break
        }
        t.responseId = env.response_id
        t.lastEventAt = Date.now()
        this.byResponse.set(env.response_id, t)
        break
      }
      case 'agent_delta': {
        const t = this.byResponse.get(env.response_id)
        if (!t) this.keepEarly(env)
        if (!t || t.final) break
        if (t.voice && this.replayed(t, env)) break
        t.text += env.text
        t.lastEventAt = Date.now()
        t.push({ type: 'delta', text: env.text })
        break
      }
      case 'agent_done': {
        const t = this.byResponse.get(env.response_id)
        if (!t) this.keepEarly(env)
        if (!t || t.final) break
        // Seen live. Without a gap the live text is the whole reply; with one, run() marks it
        // possibly incomplete.
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

  /** What a broadcast `command_admitted` says of this command: its response, or its end. */
  adopt(t, c) {
    if (c.response_id && !t.responseId) {
      t.responseId = c.response_id
      this.byResponse.set(c.response_id, t)
    }
    // A fresh admission is `admitted`; a refusal after it (an interruption) is a terminal.
    if (TERMINAL.has(c.status) && c.status !== 'completed') t.finish({ status: c.status, text: t.text, detail: c.detail ?? null, responseId: c.response_id ?? t.responseId })
  }

  /** A typed command's broadcast for an id no track holds yet: kept, bounded, in order. */
  keepEarly(env) {
    this.early.push(env)
    if (this.early.length > EARLY_MAX) this.early.shift()
  }

  /** The command id is known: what was broadcast for it before the 200 is replayed, in order. */
  adoptEarly(t) {
    const responses = new Set()
    const mine = []
    this.early = this.early.filter((env) => {
      const own =
        env.kind === 'command_admitted' ? env.command_id === t.commandId
          : env.kind === 'response_started' ? env.request_id === t.commandId
            : responses.has(env.response_id)
      if (own && env.kind === 'response_started') responses.add(env.response_id)
      if (own) mine.push(env)
      return !own
    })
    this.stats.early_adopted += mine.length
    for (const env of mine) this.ingest(env)
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
   * After an observer gap: a spoken reply has no command, so nothing is re-asked or re-sent, and
   * there is no recorded reply to take its whole text from. Its own text is kept with the
   * incomplete marker; the end it showed, if any, is named in the detail, never used to claim
   * the text is whole.
   */
  reconcileVoice(t) {
    if (t.closed || t.final) return
    this.voiceStats.incomplete++
    const end = t.terminalSeen?.status ?? null
    t.finish({
      status: 'incomplete',
      text: t.text,
      detail: end
        ? `the live stream had a gap; the reply ended (${end}), but its whole text was not seen, so this text may be incomplete`
        : 'the live stream had a gap and the end of this spoken reply was not seen; this text may be incomplete',
      responseId: t.responseId,
      voice: t.voice,
    })
  }

  /**
   * Follow spoken reply `responseId` (opened by the provider, never submitted here): yields
   * `{type:'delta', text}` events and ends with `{type:'final', status, text, detail, voice}`.
   * A stop has the same one stop deadline as typed turns: no verb can cancel a reply in Unfold.
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
          this.reconcileVoice(t)
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
   * Submit composer message `key` (`text`) once through Unfold's control endpoint and yield
   * `{type:'admitted'}`, `{type:'delta', text}` events, ending with `{type:'final', status,
   * text, detail}`. Nothing is ever sent twice: a key seen before ends at once, and no answer
   * (a 503, a refusal, a lost connection) is retried. `cancelled` is reported only when the
   * backend confirmed it; a stop with no backend end within the deadline is `stop_unconfirmed`
   * (stopped here; there is no verb to stop the reply in Unfold).
   */
  async *run(key, text, signal) {
    const handled = this.sent.get(key)
    if (handled) {
      this.stats.already_handled++
      yield { type: 'final', status: 'unknown', text: '', detail: `this message was already handled (${handled}); it is never sent again` }
      return
    }
    const session = this.link.session_id
    if (!session) {
      this.stats.not_sent++
      yield { type: 'final', status: 'not_sent', text: '', detail: 'agentd has not named the Unfold Session yet, so nothing was sent' }
      return
    }
    this.sent.set(key, 'being sent')
    while (this.sent.size > SENT_MAX) this.sent.delete(this.sent.keys().next().value)
    const t = new Track(null)
    // Submitted while the observer is not attached: whatever happens before it attaches is unseen.
    if (this.link.state !== 'attached') {
      t.gap = true
      this.stats.gaps++
    }
    t.lastEventAt = Date.now()
    this.submitting.add(t)
    // ONE absolute stop deadline from the first stop; every wait below (the submit, an event) is
    // raced against it and against unload.
    let stopDeadline = null
    let hitDeadline
    const deadline = new Promise((resolve) => { hitDeadline = resolve })
    const stopNow = () => {
      if (stopDeadline !== null) return
      stopDeadline = Date.now() + this.timing.stopMs
      setTimeout(hitDeadline, this.timing.stopMs)
      t.wake?.()
    }
    const STOPPED = Symbol('stopped')
    const raced = (p) => Promise.race([p, deadline.then(() => STOPPED), this.unloaded.then(() => STOPPED)])
    const unconfirmed = () =>
      t.finish({ status: 'stop_unconfirmed', text: t.text, detail: 'stopped here; no backend end arrived within the stop deadline, so the outcome in Unfold is unknown', responseId: t.responseId })
    const onAbort = () => stopNow()
    signal?.addEventListener('abort', onAbort, { once: true })
    try {
      if (signal?.aborted) {
        this.stats.not_sent++
        this.sent.set(key, 'not sent: stopped first')
        t.finish({ status: 'not_sent', text: '', detail: 'the turn was stopped before this message was sent' })
      } else {
        this.stats.submits++
        const r = await raced(this.host.submit(session, text))
        if (r === STOPPED) {
          this.sent.set(key, 'stopped while being sent')
          if (!t.final) unconfirmed()
        } else if (r.status === 200 && r.body?.accepted === true && validTurnId(r.body.turn_id)) {
          t.commandId = r.body.turn_id
          t.lastEventAt = Date.now()
          this.sent.set(key, `sent as ${t.commandId}`)
          this.submitting.delete(t)
          this.tracks.set(t.commandId, t)
          this.stats.admissions++
          yield { type: 'admitted', command: { command_id: t.commandId, status: 'admitted' }, duplicate: false }
          this.adoptEarly(t)
        } else if (r.status === 200) {
          this.stats.unknown++
          this.sent.set(key, 'accepted response had no usable command id')
          t.finish({ status: 'unknown', text: '', detail: UNKNOWN.unreadable })
        } else if (r.body?.refused === 'command') {
          this.stats.rejected++
          this.sent.set(key, 'refused: an owner command')
          t.finish({ status: 'rejected', text: '', detail: refusedText('command') })
        } else if (r.body?.sent === 'unknown') {
          this.stats.unknown++
          this.sent.set(key, 'lost after it was written')
          t.finish({ status: 'unknown', text: '', detail: UNKNOWN[r.body.error] ?? UNKNOWN.transport })
        } else {
          this.stats.not_sent++
          const why = r.body?.refused ? refusedText(r.body.refused) : NOT_REACHED[r.body?.error] ?? `the control endpoint answered ${r.status}`
          this.sent.set(key, `not sent: ${r.body?.refused ?? r.body?.error ?? r.status}`)
          this.log(`unfold-session: a composer message was not sent (${r.body?.refused ?? r.body?.error ?? r.status})`)
          t.finish({ status: 'not_sent', text: '', detail: why })
        }
      }
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
          else t.finish({ status: 'incomplete', text: t.text, detail: `the live stream had a gap; the reply ended (${t.terminalSeen.status}), but its whole text was not seen, so this reply may be incomplete`, responseId: t.responseId })
          continue
        }
        const attached = this.link.state === 'attached'
        if (t.gap && attached && now - t.lastEventAt >= this.timing.gapWaitMs) {
          t.finish({ status: 'incomplete', text: t.text, detail: `the live stream had a gap and no event of this reply arrived for ${Math.round(this.timing.gapWaitMs / 1000)} s; this reply may be incomplete`, responseId: t.responseId })
          continue
        }
        // Taken by Unfold, then silent: a broadcast was missed or never came. It was sent; it is
        // never sent again, and the turn ends rather than holding the composer.
        const idle = t.commandId && (!t.gap || !attached) && stopDeadline === null
        if (idle && now - t.lastEventAt >= this.timing.typedIdleMs) {
          const quiet = `no event of its reply arrived for ${Math.round(this.timing.typedIdleMs / 1000)} s`
          if (t.text) t.finish({ status: 'incomplete', text: t.text, detail: `the reply stopped: ${quiet}; this reply may be incomplete`, responseId: t.responseId })
          else t.finish({ status: 'not_seen', text: '', detail: `Unfold took this message (${t.commandId}), but ${quiet}; it is not sent again`, responseId: t.responseId })
          continue
        }
        // Each wait's timer is cleared as soon as one of them fires: no timer outlives its loop.
        const timers = new AbortController()
        const waits = [t.next()]
        if (stopDeadline !== null) waits.push(sleep(Math.max(0, stopDeadline - now), timers.signal))
        if (t.gap && attached) waits.push(sleep(Math.max(0, t.lastEventAt + this.timing.gapWaitMs - now), timers.signal))
        if (idle) waits.push(sleep(Math.max(0, t.lastEventAt + this.timing.typedIdleMs - now), timers.signal))
        const woke = await raced(Promise.race(waits))
        timers.abort()
        if (woke === STOPPED && !t.final) unconfirmed()
        t.wake = null
      }
      yield { type: 'final', ...t.final }
    } finally {
      t.closed = true
      signal?.removeEventListener('abort', onAbort)
      this.submitting.delete(t)
      if (t.commandId) this.tracks.delete(t.commandId)
      if (t.responseId) this.byResponse.delete(t.responseId)
    }
  }
}
