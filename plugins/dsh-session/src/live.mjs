// The live "listening…" view: what the chat shows between the moment speech begins and the moment
// the words land in a bubble. Gemini sends the input transcript only at the end of a turn (2–8 s
// after speech began), so without this nothing would show while the user speaks.
//
// A pure reducer over speech facts, with an injected clock and one timer (never a poll):
//   {type:'onset', audible, level_dbfs}  rth-host's local_vad_onset (its journal, through Mind)
//   {type:'speech-start'|'speech-end'}   the provider's voice_activity_start / _end (agentd feed)
//   {type:'heard', text}                 the words heard so far for the open utterance
//   {type:'reply'}                       a spoken reply opened: its user bubble is in the chat now
//   {type:'note'}                        words were shown on their own in the chat
//   {type:'input-closed'}                the host applied a mute, ended the call or released its devices
//
// The ONSET is gated by the caller (index.mjs): rth-host's VAD runs before its admission gate,
// so `local_vad_onset { audible:false }` is logged even while muted. Only an onset with fresh applied
// facts (call active with its input open, mic applied unmuted) reaches this reducer.
//
// Freshness: every published view carries `ttl_ms`, its remaining lifetime at the moment it
// is sent; the browser ends it itself at that deadline (and on a stream error, a stalled stream or a
// session change), so a view never outlives its evidence when the host's later idle cannot arrive.
//
// Honesty:
//   - An onset starts "listening" only while the speaker is silent (`audible: false`, which the host
//     journals as action "nothing_audible"). During playback an onset may be the speaker's own echo
//     (journalled as "observed"), so there only the provider's voice_activity_start starts it.
//   - The level is the one number rth-host measures: the level AT THE ONSET of this speech. There is
//     no continuous mic level on any RTH feed, so this is not a live meter and is labelled as such.
//   - It always ends: on the reply, on a note, a little after the end of speech with no words, when
//     an onset was never confirmed, or at a hard cap.

export const LIVE_TIMING = {
  endGraceMs: 3000, // after voice_activity_end, how long the words may still come
  onsetOnlyMs: 2500, // an onset the provider never confirmed, and no words: gone after this
  heardMs: 6000, // words shown, no reply and no note: gone after this (the ledger shows them by then)
  maxMs: 30_000, // never on for longer than this without new speech facts
}

const IDLE = Object.freeze({ state: 'idle', since: null, level: null, text: null })
const ONSET_BASIS = 'level at speech onset (rth-host)'

export class LiveView {
  constructor({ now = () => Date.now(), timing = {}, onChange = () => {} } = {}) {
    this.now = now
    this.timing = { ...LIVE_TIMING, ...timing }
    this.onChange = onChange
    this.v = IDLE
    this.confirmed = false
    this.endedAt = null
    this.lastAt = null
    this.timer = null
    this.rev = 0
  }

  view() {
    return this.v
  }

  /** The view as published: with its remaining lifetime (null when idle). */
  published() {
    const d = this.deadline()
    return { ...this.v, ttl_ms: d === null ? null : Math.max(0, Math.round(d - this.now())) }
  }

  set(v) {
    const same = v.state === this.v.state && v.since === this.v.since && v.text === this.v.text && v.level?.dbfs === this.v.level?.dbfs
    this.v = v
    if (v.state === 'idle') {
      this.confirmed = false
      this.endedAt = null
    }
    if (!same) this.publish()
    this.schedule()
  }

  /** Publish the current view (its deadline moved, or it changed). */
  publish() {
    this.rev++
    this.onChange(this.published(), this.rev)
  }

  /** The deadline moved without the view changing: published too, so the browser's copy agrees. */
  touch() {
    this.publish()
    this.schedule()
  }

  input(ev) {
    const now = this.now()
    const v = this.v
    switch (ev.type) {
      case 'onset':
        if (ev.audible !== false) return // during playback: possibly echo, wait for the provider
        this.lastAt = now
        if (v.state === 'idle') this.set({ state: 'listening', since: now, level: levelOf(ev.level_dbfs), text: null })
        return
      case 'speech-start':
        this.lastAt = now
        this.endedAt = null
        this.confirmed = true
        if (v.state === 'listening') return this.touch()
        this.set({ state: 'listening', since: now, level: null, text: null })
        return
      case 'speech-end':
        if (v.state === 'idle') return
        this.endedAt = now
        this.touch()
        return
      case 'heard': {
        const text = typeof ev.text === 'string' ? ev.text.trim() : ''
        if (!text) return
        this.lastAt = now
        if (v.state === 'heard' && v.text === text) return this.touch()
        this.set({ state: 'heard', since: v.state === 'idle' ? now : v.since, level: v.level, text })
        return
      }
      case 'reply':
      case 'note':
      case 'input-closed':
        if (v.state !== 'idle') this.set(IDLE)
        return
      default:
    }
  }

  /** When the view next changes with no input, or null. */
  deadline() {
    const v = this.v
    if (v.state === 'idle') return null
    const t = this.timing
    const ends = [(this.lastAt ?? v.since) + t.maxMs]
    if (v.state === 'heard') ends.push(this.lastAt + t.heardMs)
    else {
      if (this.endedAt !== null) ends.push(this.endedAt + t.endGraceMs)
      if (!this.confirmed) ends.push(v.since + t.onsetOnlyMs)
    }
    return Math.min(...ends)
  }

  tick() {
    const d = this.deadline()
    if (d !== null && d <= this.now()) this.set(IDLE)
  }

  schedule() {
    clearTimeout(this.timer)
    this.timer = null
    const d = this.deadline()
    if (d === null) return
    this.timer = setTimeout(() => this.tick(), Math.max(0, d - this.now()))
    this.timer.unref?.()
  }

  stop() {
    clearTimeout(this.timer)
    this.timer = null
  }
}

function levelOf(dbfs) {
  return Number.isFinite(dbfs) ? { dbfs: Math.round(dbfs * 10) / 10, basis: ONSET_BASIS } : null
}
