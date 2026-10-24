// The presence: one creature, as a pure reducer over facts the dsh-events observer already
// receives. `presence(state, input, now)` folds one input; `view(state, now)` derives what may
// be drawn; `nextDeadline(state, now)` says when the view can next change with no input (a
// lease running out, a wince ending, sleep). The clock is always an argument (ms, one domain:
// the caller's; the plugin passes Date.now(), so fault times are wall-clock), so replay is
// deterministic.
//
// Honesty rules: every pose cites a source; a missing, stale or lagging source draws `unknown`;
// `command_admitted` is `pending`, never thinking; there is no thinking, mood or confidence;
// sleep is an authored idle pose that says nothing about the microphone, which has its own
// indicator, read from the host owner's status.
//
// Inputs: {type: 'link', state, detail} from the observer; {type: 'event', d} with d a
// catalog row description (`Catalog.describe`); {type: 'host', status} with the owner's
// /v1/status body, or null when it could not be read; {type: 'tick'}.

export const LEASE = {
  heartbeatMs: 5_000, // agentd heartbeats every 1 s; five missed is a stale link
  hostMs: 15_000, // the owner's status, read every HOST_READ_MS by the plugin
}
export const HOST_READ_MS = 5_000
// The presence stream's own liveness: a heartbeat at this period, so a reader can lease it
// (views are pushed on change only, and an idle creature may not change for minutes).
export const STREAM_HEARTBEAT_MS = 10_000
export const STREAM_LEASE_MS = 25_000
export const WINCE_MS = 4_000
// Sleepy after ~2 min idle, asleep after ~10.
export const IDLE = { sleepyMs: 2 * 60_000, asleepMs: 10 * 60_000 }
const FAULTS_KEPT = 8

export const POSES = ['unknown', 'wince', 'speaking', 'pending', 'idle', 'sleepy', 'asleep']

const LIVE = new Set(['admitted', 'dispatched'])

export function initialPresence(now) {
  return {
    link: { state: 'none', detail: null, since: now },
    attachedAt: null,
    heartbeatAt: null,
    boot: null,
    // Requests are known only from events seen since the last attach, or from the host record.
    requestsKnown: false,
    commands: {}, // command_id -> { response_id, since, source }
    replies: {}, // response_id -> { since, source } (message_started, not yet message_finished)
    hostUnfinished: null, // the owner's count of unfinished commands at its last read
    audioSeen: false, // a playback report has reached this feed at least once
    playing: {}, // `${response_id}#${epoch}` -> { since, source }
    faults: [],
    winceUntil: 0,
    lastActivity: now,
    mic: { state: 'unknown', at: null, detail: 'no host status read yet' },
  }
}

export function presence(state, input, now) {
  switch (input.type) {
    case 'link':
      return onLink(state, input, now)
    case 'event':
      return onEvent(state, input.d, now)
    case 'host':
      return onHost(state, input.status, now)
    default:
      return state
  }
}

function onLink(state, { state: link, detail = null }, now) {
  const next = { ...state, link: { state: link, detail, since: now } }
  if (link !== 'attached') return next
  // No replay: a reattach resumes live, so what was open before it is not known any more.
  return { ...next, attachedAt: now, heartbeatAt: null, requestsKnown: false, commands: {}, replies: {}, playing: {}, hostUnfinished: null, lastActivity: now }
}

function onEvent(state, d, now) {
  const f = d.fields ?? {}
  const src = d.id ?? null
  switch (d.name) {
    case 'agent_status_reported':
      return { ...state, heartbeatAt: now } // a heartbeat is liveness, not activity
    case 'agent_available': {
      // A new agent boot is a new run: earlier fault marks belong to the previous one.
      const boot = f.boot_id ?? null
      const faults = boot && state.boot && boot !== state.boot ? [] : state.faults
      return { ...active(state, now), boot, faults }
    }
    case 'command_admitted': {
      const commands = { ...state.commands }
      if (LIVE.has(f.status)) commands[f.command_id] = { response_id: f.response_id ?? commands[f.command_id]?.response_id ?? null, since: commands[f.command_id]?.since ?? now, source: src }
      else delete commands[f.command_id]
      return { ...active(state, now), commands }
    }
    case 'message_started': {
      // A command's reply names it as request_id (the admission ack carries no response_id):
      // link them, so the reply's message_finished ends the command's pending too.
      const cmd = state.commands[f.request_id]
      const commands = cmd ? { ...state.commands, [f.request_id]: { ...cmd, response_id: f.response_id } } : state.commands
      return { ...active(state, now), commands, replies: { ...state.replies, [f.response_id]: { since: now, source: src } } }
    }
    case 'message_finished': {
      const replies = { ...state.replies }
      delete replies[f.response_id]
      const commands = Object.fromEntries(Object.entries(state.commands).filter(([, c]) => c.response_id !== f.response_id))
      const next = { ...active(state, now), replies, commands }
      return f.status === 'failed' ? fault(next, d, now) : next
    }
    case 'message_playback_reported': {
      const key = `${f.response_id}#${f.epoch ?? ''}`
      const playing = { ...state.playing }
      if (f.phase === 'started') playing[key] = { since: now, source: src }
      else delete playing[key]
      const next = { ...active(state, now), audioSeen: true, playing }
      // device_unavailable is a fault, never a yield.
      return f.phase === 'cleared' && f.reason === 'device_unavailable' ? fault(next, d, now) : next
    }
    case 'history_save_failed':
      return fault(active(state, now), d, now)
    default:
      return active(state, now)
  }
}

function onHost(state, status, now) {
  if (!status || typeof status !== 'object' || !status.host) {
    return { ...state, mic: { state: 'unknown', at: now, detail: status?.unavailable ?? 'host status unavailable' } }
  }
  // `host.mic.state` is the APPLIED input fence (rth-host): `muted` only once nothing more can
  // be sent; `muting` / `unmuting` while a request waits for the voice loop.
  const io = status.host.voice_policy?.voice_io ?? null
  const fence = status.host.mic?.state
  let mic
  if (fence === 'muted') mic = { state: 'muted', at: now, detail: 'muted at the host: no microphone audio is sent (input only)' }
  else if (fence === 'muting' || fence === 'unmuting') mic = { state: fence, at: now, detail: `${fence === 'muting' ? 'mute' : 'unmute'} requested, not yet applied by rth-host` }
  else if (io === 'off') mic = { state: 'off', at: now, detail: 'voice is off: rth-host opened no microphone' }
  else if (fence === 'unmuted') mic = { state: 'not_muted', at: now, detail: `not muted (${io ?? 'voice'}); whether the device is capturing is not reported` }
  else mic = { state: 'unknown', at: now, detail: 'this rth-host reports no mute state' }
  mic = withCall(mic, status.host.call)
  const unfinished = status.commands?.unfinished
  const next = { ...state, mic }
  if (typeof unfinished !== 'number') return next
  // The owner's durable record reconciles what a reattach forgot.
  return { ...next, hostUnfinished: unfinished, requestsKnown: state.requestsKnown || (state.link.state === 'attached' && unfinished === 0) }
}

// The call (`host.call`, rth-host's call owner): desired and actual kept apart; only the actual
// `state` is shown. `mic.state` stays the applied mute fence (the native tray reads it); the call
// says whether there is a microphone to speak of at all, so the label leads with it. A host
// without a call owner (`call: null`) or that reports no `call` key keeps the
// fence-only wording and is never labelled as being in a call.
const CALL_STATES = new Set(['off', 'starting', 'active', 'ending', 'error'])

function withCall(mic, call) {
  if (!call || typeof call !== 'object') return { ...mic, call: null }
  const reason = typeof call.reason === 'string' && call.reason ? call.reason : null
  const shown = { state: CALL_STATES.has(call.state) ? call.state : 'unknown', reason }
  const why = reason ? ` (${reason})` : ''
  const fence = mic.detail // the applied mute fence, in its own words
  const detail = {
    off: 'no call: the call devices are released and no microphone is open',
    error: `no call: the start failed${call.error ? ` (${call.error})` : why}`,
    starting: `call starting${why}; ${fence}`,
    active: `call active; ${fence}`,
    ending: `call ending${why}; ${fence}`,
    unknown: `call state unknown (${String(call.state)}); ${fence}`,
  }[shown.state]
  return { ...mic, detail, call: shown }
}

function active(state, now) {
  return { ...state, lastActivity: now }
}

function fault(state, d, now) {
  const faults = [...state.faults, { name: d.name, summary: d.summary ?? d.name, at: now, source: d.id ?? null }].slice(-FAULTS_KEPT)
  return { ...state, faults, winceUntil: now + WINCE_MS }
}

/** What may be drawn now, every layer with its source; never a pose without one. */
export function view(state, now) {
  const link = linkLayer(state, now)
  const request = requestLayer(state)
  const audio = !state.audioSeen
    ? { state: 'not_observed', label: 'device audio: not on this feed (agentd does not forward host playback reports)' }
    : Object.keys(state.playing).length
      ? { state: 'rendering', label: 'device audio: samples are being handed to the device', sources: Object.values(state.playing).map((p) => p.source) }
      : { state: 'quiet', label: 'device audio: none' }
  const mic = state.mic.at === null || now - state.mic.at > LEASE.hostMs
    ? { state: 'unknown', label: `mic: unknown (${state.mic.at === null ? state.mic.detail : 'host status is stale'})` }
    : { state: state.mic.state, label: `mic: ${state.mic.detail}`, at: state.mic.at, call: state.mic.call ?? null }
  const faults = state.faults

  let pose
  let because
  if (link.state !== 'live') [pose, because] = ['unknown', link.label]
  else if (now < state.winceUntil) [pose, because] = ['wince', `fault: ${state.faults.at(-1)?.summary}`]
  else if (audio.state === 'rendering') [pose, because] = ['speaking', audio.label]
  else if (request.state === 'pending') [pose, because] = ['pending', request.label]
  else if (request.state === 'unknown') [pose, because] = ['unknown', request.label]
  else {
    const quiet = now - state.lastActivity
    pose = quiet >= IDLE.asleepMs ? 'asleep' : quiet >= IDLE.sleepyMs ? 'sleepy' : 'idle'
    because = pose === 'idle'
      ? 'no open request or reply'
      : `no open request or reply for ${pose === 'asleep' ? '10' : '2'}+ min (an authored rest pose: it says nothing about the microphone)`
  }
  return { pose, label: LABELS[pose], because, layers: { link, request, audio, mic }, faults }
}

export const LABELS = {
  unknown: 'Unknown',
  wince: 'Fault',
  speaking: 'Speaking (device-rendered)',
  pending: 'Pending',
  idle: 'Idle',
  sleepy: 'Sleepy',
  asleep: 'Asleep',
}

function linkLayer(state, now) {
  if (state.link.state !== 'attached') return { state: 'unknown', label: `link: ${state.link.state}${state.link.detail == null ? '' : ` (${state.link.detail})`}` }
  const last = state.heartbeatAt ?? state.attachedAt
  if (now - last > LEASE.heartbeatMs) return { state: 'unknown', label: `link: no heartbeat within ${LEASE.heartbeatMs / 1000} s (stale)` }
  return { state: 'live', label: 'link: attached, heartbeat fresh' }
}

function requestLayer(state) {
  const commands = Object.entries(state.commands)
  const replies = Object.entries(state.replies)
  if (commands.length || replies.length) {
    const parts = [...commands.map(([id]) => `command ${id} admitted`), ...replies.map(([id]) => `reply ${id} started`)]
    return { state: 'pending', label: `pending: ${parts.join(', ')}, not finished`, sources: [...commands, ...replies].map(([, v]) => v.source) }
  }
  if (!state.requestsKnown) {
    if (state.hostUnfinished > 0) return { state: 'pending', label: `pending: ${state.hostUnfinished} unfinished command(s) in the host record` }
    return { state: 'unknown', label: 'requests: unknown since the link came up (no host record yet)' }
  }
  return { state: 'none', label: 'requests: none open' }
}

/** When the view can next change with no input, or null. The plugin arms one timer for it. */
export function nextDeadline(state, now) {
  const at = []
  if (state.link.state === 'attached') at.push((state.heartbeatAt ?? state.attachedAt) + LEASE.heartbeatMs + 1)
  if (state.mic.at !== null) at.push(state.mic.at + LEASE.hostMs + 1)
  if (state.winceUntil > now) at.push(state.winceUntil)
  at.push(state.lastActivity + IDLE.sleepyMs, state.lastActivity + IDLE.asleepMs)
  const future = at.filter((t) => t > now)
  return future.length ? Math.min(...future) : null
}
