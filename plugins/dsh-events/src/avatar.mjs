// The avatar in the presence tab: you click it to wake voice, always realtime, never "called".
// Pure, so it is tested in node and bundled into the browser half as is. Two inputs, both host
// receipts, kept apart:
//   - the presence view (src/presence.mjs `view()`, pushed by this plugin): what the creature may
//     show (link, request, device audio, speech candidate, faults, rest);
//   - the voice receipt (`host.call`, `host.mic` read through rth-session's
//     `GET api/rth-session/call`, folded by its call-view.mjs): whether the voice devices
//     are awake, and the APPLIED mute fence.
// A tap is a request (`POST api/rth-session/call`); it is
// shown as "requested" beside the actual state until a receipt answers it, never as the state.
// Voice asleep means only that rth-host released the mic and speaker: RTH, its agent link and the
// conversation keep running, and the creature keeps showing what the presence establishes.
import { LEASE_MS, PENDING_MS, READ_TIMEOUT_MS, RESTART_PENDING_MS, callView, createCallReader, muteView, nextRead, pollMs } from '../../dsh-session/src/call-view.mjs'

export { PENDING_MS, READ_TIMEOUT_MS, createCallReader, nextRead }
export const CALL_ROUTE = 'api/rth-session/call'
export const MUTE_ROUTE = 'api/rth-session/mute'
/** "Restart voice host": a request to the RTH app that owns rth-host. */
export const RESTART_ROUTE = 'api/rth-session/restart-host'

// What the surface says about voice, in plain words. rth-host's own wording stays in `title` (the
// technical fold), never on the surface.
const ACTUAL = { off: 'Voice asleep', starting: 'Waking up…', active: 'Voice awake', ending: 'Going to sleep…', error: "Voice didn't wake", stopped: 'Voice stopped', unknown: 'Out of touch' }
const WORD = { off: 'asleep', starting: 'waking up', active: 'awake', ending: 'going to sleep', error: "didn't wake", stopped: 'stopped (rth-host ended)', unknown: 'unknown' }
const ASKED = { start: 'wake', end: 'sleep', restart: 'restart' }

/**
 * What the avatar control may say and do, from the voice receipt only.
 * @param {object|null} read the last `GET api/rth-session/call` body (with `receivedAt`), or null
 * @param {{want: 'start'|'end', at: number}|null} pending a tap not answered yet
 * @param {number} now local monotonic ms (the same clock as `receivedAt`)
 */
export function voiceControl(read, pending, now) {
  const v = callView(read, pending, now)
  const actual = ['off', 'starting', 'active', 'ending', 'error', 'stopped'].includes(v.state) ? v.state : 'unknown'
  const escape = actual === 'unknown' && v.action === 'end' // the last good receipt said voice may be up
  let label
  if (v.action === 'start') label = actual === 'error' ? 'Try waking voice again' : 'Wake voice'
  else if (v.action === 'end') label = escape ? 'Put voice to sleep (not sure it is awake)' : actual === 'starting' ? 'Put voice back to sleep' : 'Put voice to sleep'
  else if (v.action === 'restart') label = 'Restart voice'
  else if (v.pending === 'waiting') label = `${pending.want === 'start' ? 'Waking voice' : pending.want === 'restart' ? 'Restarting voice' : 'Putting voice to sleep'}: waiting for Unfold to confirm`
  else if (!read) label = 'Checking voice…'
  else label = actual === 'ending' ? 'Going to sleep…' : actual === 'stopped' ? 'Restart from the Unfold menu or the tray' : "Voice can't be changed right now"
  // A tap is a request until a receipt answers it: the headline keeps saying what voice IS.
  let settling = null
  if (v.pending === 'waiting') settling = `${pending.want === 'start' ? 'Waking' : pending.want === 'restart' ? 'Restarting' : 'Going to sleep'}… waiting for Unfold to confirm`
  else if (v.pending === 'no_receipt') settling = `Unfold didn't confirm the ${ASKED[pending.want] ?? 'request'} within ${(pending.want === 'restart' ? RESTART_PENDING_MS : PENDING_MS) / 1000} s. Voice is ${WORD[actual]}.`
  else if (v.desired && actual !== 'unknown' && actual !== 'error' && v.desired !== (actual === 'active' || actual === 'starting' ? 'on' : 'off')) settling = `Asked for voice ${v.desired}; it is ${WORD[actual]} so far`
  const status = !read ? 'Checking…' : ACTUAL[actual]
  const reason = actual === 'error' || actual === 'stopped' ? (v.reason ?? null) : null
  const hint = v.action === 'start' ? (actual === 'error' ? 'Tap to try again' : 'Tap to wake voice')
    : v.action === 'end' ? (actual === 'starting' ? 'Tap to cancel' : 'Tap to put voice to sleep')
      : v.action === 'restart' ? 'Tap to restart voice (it comes back asleep)'
        : null
  return {
    actual,
    desired: v.desired ?? null,
    action: v.action,
    label,
    status,
    word: WORD[actual],
    reason,
    settling,
    hint,
    title: v.title,
    stale: Boolean(v.stale),
    read: Boolean(read),
    readAt: read?.receivedAt ?? null, // local monotonic ms of the last receipt (technical fold only)
    pending: v.pending,
    call: v, // for pollMs
    mute: muteGlyph(read, now),
  }
}

/**
 * The mic glyph on the frame: the APPLIED mute fence from a fresh receipt, whatever the voice and
 * the creature show (mute survives sleep and wake; a resting creature never implies a closed mic).
 */
export function muteGlyph(read, now) {
  const age = read && typeof read.receivedAt === 'number' ? now - read.receivedAt : 0
  const fresh = read && !read.unavailable && age <= LEASE_MS && read.call && typeof read.call.state === 'string'
  const m = fresh ? muteView(read.mic) : null
  if (!m) {
    return { state: 'unknown', muted: null, action: null, label: 'Microphone: not sure if muted', title: fresh ? 'this rth-host reports no mute state' : 'no fresh receipt from rth-host: the mute state is unknown' }
  }
  const state = read.mic.state
  const label = m.action === true ? 'Mute microphone' : m.action === false ? 'Unmute microphone' : m.label
  return { state, muted: m.muted, action: m.action, label, title: m.title }
}

/** The request a tap makes, or null (nothing is sent while a request waits or the state forbids it). */
export function toggleRequest(voice) {
  if (voice.action === 'restart') return { url: RESTART_ROUTE, body: { restart: true } }
  return voice.action ? { url: CALL_ROUTE, body: { call: voice.action } } : null
}
export function muteRequest(voice) {
  return voice.mute.action === null ? null : { url: MUTE_ROUTE, body: { muted: voice.mute.action } }
}
export const readDelay = (voice) => pollMs(voice.call)

/**
 * While the page says it is hidden, the voice receipt is still read this often (a GET only): under
 * the receipt lease, so a page wrongly reporting hidden while on screen keeps a fresh, known state.
 */
export const HIDDEN_READ_MS = LEASE_MS - 2_000
/**
 * Whether the tab should read the voice receipt now. The first read always runs, and a page that
 * says hidden still reads slowly: a WKWebView launched in the background can report `hidden` while
 * it is on screen, and the tab must not stay on "Checking…" for that. Reads never change devices.
 */
export function shouldRead({ hidden, sinceLast, delay }) {
  if (sinceLast === Infinity) return true
  return sinceLast >= (hidden ? Math.max(delay, HIDDEN_READ_MS) : delay)
}

// ---- The scene: what the dust draws, from the presence view and the actual voice state ----

/** How often a visible, animating scene may step. Hidden: 0. Reduced motion: one still frame per change. */
// Idle is a still frame (redrawn only when the scene changes): the measured cost of an animated
// frame in the native app (~0.5 % of a core per fps at DPR 2) puts any idle loop over the
// idle budget (+0.3 %). Motion is kept for what is happening: a transition (30 fps) and an open
// request (8 fps). While idle, life is the capsule's compositor-only breathing glow.
export const FPS = { active: 30, calm: 8, rest: 0, still: 0 }

/**
 * @param {object} view the presence view (`view()`), or the client's stale placeholder
 * @param {{actual: string}} voice from `voiceControl`
 * @returns {{key, form, face, tint, dim, density, motion, status, because}}
 *   form: scatter | gather | disperse | drift | slow | settle | orbit | face | fault
 *   face: null | listen | speak | wince
 */
export function sceneFor(view, voice) {
  const pose = view?.pose ?? 'unknown'
  const speech = view?.layers?.speech?.state ?? 'unknown'
  const actual = voice?.actual ?? 'unknown'
  const s = (form, face, motion, dim, density, status, extra = {}) => ({ form, face, motion, dim, density, status, tint: null, pose, label: status, because: view?.because ?? '', ...extra })
  let scene
  // Unknown (either source): one still grey frame, no sign of life.
  if (pose === 'unknown') scene = s('scatter', null, 'still', 0.5, 0.6, `Unknown: ${view?.because ?? 'no presence'}`, { tint: 'grey' })
  else if (actual === 'unknown') scene = s('scatter', null, 'still', 0.5, 0.6, `Unknown: ${voice?.title || 'no fresh voice receipt from rth-host'}`, { tint: 'grey' })
  else if (actual === 'stopped') scene = s('fault', 'wince', 'still', 0.6, 0.5, `Voice stopped: ${voice?.reason ?? 'rth-host ended'}`, { tint: 'warm' })
  else if (pose === 'wince') scene = s('fault', 'wince', 'active', 1, actual === 'active' ? 1 : 0.6, `Fault: ${view.because}`, { tint: 'warm' })
  else if (actual === 'starting') scene = s('gather', null, 'active', 0.8, 0.85, 'Voice waking: the dust gathers')
  else if (actual === 'ending') scene = s('disperse', null, 'active', 0.55, 0.5, 'Voice going to sleep: the dust disperses')
  else if (actual === 'off' || actual === 'error') {
    // Voice asleep: dim, sparse dust. RTH itself is awake: a typed request still shows as pending.
    const rest = pose === 'asleep' ? 'settle' : pose === 'sleepy' ? 'slow' : pose === 'pending' ? 'orbit' : 'drift'
    const what = pose === 'pending' ? 'a typed request is open (pending, not thinking)' : 'Unfold stays on; typed chat works'
    scene = s(rest, null, pose === 'pending' ? 'calm' : 'rest', 0.42, 0.35, `Voice asleep · ${what}`)
  } else if (pose === 'speaking') scene = s('face', 'speak', 'active', 1, 1, 'Voice awake · speaking (device-rendered audio reported by rth-host)')
  else if (speech === 'candidate') scene = s('face', 'listen', 'active', 1.05, 1, 'Voice awake · speech candidate (the provider transcribed input; not proof anyone spoke to Unfold)')
  else if (pose === 'pending') scene = s('orbit', null, 'calm', 0.9, 1, 'Voice awake · pending: a request is open (not thinking)')
  else if (pose === 'sleepy') scene = s('slow', null, 'rest', 0.6, 0.9, 'Voice awake · resting (idle 2+ min); the mic glyph says what the mic is')
  else if (pose === 'asleep') scene = s('settle', null, 'rest', 0.4, 0.9, 'Voice awake · resting (idle 10+ min); the mic glyph says what the mic is')
  else scene = s('drift', null, 'rest', 0.85, 1, 'Voice awake · idle, waiting for the next event')
  scene.key = [scene.form, scene.face, scene.tint, scene.dim, scene.density, scene.motion].join('|')
  return scene
}

/**
 * Whether the frame may show life-like motion (the capsule's breathing glow): only when the presence
 * is known AND the voice receipt says awake. Never on the voice receipt alone.
 */
export function lifeFor(view, voice) {
  return (view?.pose ?? 'unknown') !== 'unknown' && voice?.actual === 'active'
}

// ---- Cosmic points: levels, colour tiers and the unearned starting preview ----
export const GRAINS_PER_CP = 100
export const levelOf = (cp) => Math.max(1, 1 + Math.floor((cp - 100) / 10))
export const coloursOf = (level) => Math.min(10, 1 + Math.floor((level - 1) / 10))
/** The starting values, labelled preview: nothing has been earned. */
export const CP_PREVIEW = Object.freeze({ cp: 100, level: levelOf(100), stage: 'Seed', colours: coloursOf(levelOf(100)), preview: true })

/** The sidebar's grain budget: the creature's CP x 100, capped by the tab's size (20k to 40k). */
export function grainBudget(cssWidth, cssHeight, cp = CP_PREVIEW.cp) {
  const cap = Math.max(20_000, Math.min(40_000, Math.round((Math.max(0, cssWidth) * Math.max(0, cssHeight)) / 4)))
  return { cap, grains: Math.min(cp * GRAINS_PER_CP, cap) }
}

/**
 * The voice half of the tab without React: bounded, ordered reads; a tap or the mute glyph is the
 * ONLY thing that sends a request. Mounting, reading, re-rendering and receiving events never do.
 */
export function createVoiceController({ fetch: f, now = () => performance.now(), onChange = () => {} } = {}) {
  let read = null
  let pending = null
  let note = null
  let alive = true
  const reader = createCallReader({ fetch: f, url: CALL_ROUTE, now, onRead: (got) => { read = nextRead(read, got); settle(); if (alive) onChange() } })
  const settle = () => {
    // A restart is answered by a readable owner (asleep); a wake or sleep by its desired state.
    if (pending?.want === 'restart') {
      if (read && !read.unavailable) pending = null
    } else if (pending && read?.call?.desired === (pending.want === 'start' ? 'on' : 'off')) pending = null
  }
  const post = async (req, label) => {
    note = null
    let why = null
    try {
      const r = await f(req.url, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(req.body), signal: AbortSignal.timeout(READ_TIMEOUT_MS) })
      if (!(r.ok || r.status === 202)) {
        const b = await r.json().catch(() => ({}))
        why = b.error ?? b.refused ?? `refused (${r.status})`
      }
    } catch (e) {
      why = String(e?.message ?? e)
    }
    if (why) note = `${label} not delivered: ${why}` // a failed request is no outcome: receipts still say what voice is
    if (alive) onChange()
    void reader.load()
    return why
  }
  return {
    view: () => ({ ...voiceControl(read, pending, now()), note }),
    load: () => reader.load(),
    async toggle() {
      const req = toggleRequest(voiceControl(read, pending, now()))
      if (!req) return null
      pending = { want: req.body.restart ? 'restart' : req.body.call, at: now() }
      onChange()
      const why = await post(req, req.body.restart ? 'Restart voice' : req.body.call === 'start' ? 'Wake voice' : 'Sleep voice')
      if (why) pending = null
      return req
    },
    async mute() {
      const req = muteRequest(voiceControl(read, pending, now()))
      if (!req) return null
      await post(req, req.body.muted ? 'Mute' : 'Unmute')
      return req
    },
    get disposed() {
      return !alive
    },
    dispose() {
      alive = false
    },
  }
}

/** What the tab re-renders on: the voice view minus its clock-derived wording. */
export const voiceKey = (v) => [v.actual, v.desired, v.action, v.pending, v.stale, v.mute.state, v.mute.action, v.settling, v.status, v.reason, v.note].join('|')

// ---- Plain words for the surface (one glance) and the first fold (what is known) ----

/**
 * The one quiet line under the headline: what RTH is doing, only what the presence establishes.
 * Never thinking, never mood, never "listening" (an open mic is not shown as attention), never
 * "you said". Null when there is nothing worth a line.
 */
export function presenceWords(view, voice) {
  const pose = view?.pose ?? 'unknown'
  const request = view?.layers?.request
  if (pose === 'unknown') {
    if (view?.layers === null || view?.layers === undefined) return { line: 'Reconnecting…', tone: 'faint' }
    if (request?.state === 'unknown' && view.layers.link?.state === 'live') return { line: 'Catching up…', tone: 'faint' }
    return { line: "Can't see what Unfold is doing right now", tone: 'faint' }
  }
  if (pose === 'wince') return { line: 'Something went wrong. See below.', tone: 'warn' }
  if (pose === 'speaking') return { line: 'Speaking', tone: 'accent' }
  if (pose === 'pending') return { line: /\breply \S+ started/.test(request?.label ?? '') ? 'Replying…' : 'Got a message, no reply yet', tone: 'accent' }
  if (voice?.mute?.state === 'muted' && voice.actual === 'active') return { line: 'Your microphone is muted', tone: 'warn' }
  if (pose === 'sleepy' || pose === 'asleep') return { line: 'Quiet for a while', tone: 'faint' }
  if (voice?.actual === 'off' || voice?.actual === 'error' || voice?.actual === 'stopped') return { line: 'You can still type to Unfold', tone: 'faint' }
  return null
}

const MIC_WORDS = { muted: 'Muted', unmuted: 'Not muted', not_muted: 'Not muted', muting: 'Muting…', unmuting: 'Unmuting…', off: 'Off (voice asleep)', unknown: 'Not sure' }

/** The first fold: a few plain facts, each from a receipt; anything unsure says so. */
export function presenceFacts(view, voice) {
  const link = view?.layers?.link?.state
  const audio = view?.layers?.audio?.state
  const request = view?.layers?.request?.state
  const faults = view?.faults ?? []
  const micState = voice?.mute?.state && voice.mute.state !== 'unknown' ? voice.mute.state : (view?.layers?.mic?.state ?? 'unknown')
  return [
    ['Connection', link === 'live' ? 'Connected' : view?.layers ? 'Not connected right now' : 'Not sure (reconnecting)'],
    ['Voice', voice?.read ? `${voice.word[0].toUpperCase()}${voice.word.slice(1)}${voice.reason ? ` (${voice.reason})` : ''}` : 'Checking…'],
    ['Microphone', voice?.actual === 'off' && micState !== 'muted' ? 'Closed (voice asleep)' : MIC_WORDS[micState] ?? 'Not sure'],
    ['Right now', request === 'pending' ? 'Handling a message' : request === 'none' ? 'Nothing open' : 'Not sure'],
    ['Sound out', audio === 'rendering' ? 'Playing a reply' : audio === 'quiet' ? 'Nothing playing' : 'Not reported to this tab'],
    ['Problems', faults.length ? `${faults.length} recent: ${faults.at(-1).summary}` : 'None seen'],
  ]
}
