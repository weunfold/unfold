// What the composer's call button may show. Only the native owner's receipts
// (`host.call`, `host.mic` read through `GET api/unfold-session/call`) say what the call IS; a
// click is a request, kept apart as `pending` until a receipt answers it or it times out.
// Pure, so it is tested in node and bundled into the browser half as is.

/** How long a click waits for a receipt before the button says none came. */
export const PENDING_MS = 6_000
/** How long one read stays evidence (local monotonic clock): past it the call is unknown. */
export const LEASE_MS = 10_000
/** A status read that has not answered within this is aborted (and is no receipt). */
export const READ_TIMEOUT_MS = 4_000

/**
 * Bounded, ordered reads of `GET api/unfold-session/call`. Each read is aborted after
 * `timeoutMs`; its result is stamped with the LOCAL monotonic time it arrived (`receivedAt`,
 * never the server's clock), and a response from an older request that arrives after a newer
 * one has been applied is dropped, so a late answer can never overwrite a newer view.
 */
export function createCallReader({ fetch: get, url = 'api/unfold-session/call', now = () => performance.now(), timeoutMs = READ_TIMEOUT_MS, onRead = () => {} } = {}) {
  let issued = 0
  let applied = 0
  let latest = null
  return {
    latest: () => latest,
    async load() {
      const seq = ++issued
      const ctl = new AbortController()
      const timer = setTimeout(() => ctl.abort(), timeoutMs)
      let body
      try {
        const r = await get(url, { cache: 'no-store', signal: ctl.signal })
        body = r.ok ? await r.json() : { unavailable: `DSH route ${r.status}` }
      } catch (e) {
        body = { unavailable: ctl.signal.aborted ? `no answer within ${timeoutMs / 1000} s` : String(e?.message ?? e) }
      } finally {
        clearTimeout(timer)
      }
      if (seq < applied) return { stale: true, seq }
      applied = seq
      latest = { ...body, receivedAt: now(), seq }
      onRead(latest)
      return latest
    },
  }
}

const want = (p) => (p.want === 'start' ? 'on' : 'off')

/**
 * @param {object|null} read the last `GET api/unfold-session/call` body (null before the first)
 * @param {{want: 'start'|'end', at: number}|null} pending the click not answered yet
 * @param {number} now
 */
export function callView(read, pending, now) {
  const base = { state: 'unknown', label: 'Call', action: null, busy: false, tone: 'neutral', title: '', reason: null, mute: null, pending: null }
  if (!read) return { ...base, busy: true, title: 'reading the call state from the native owner…' }
  const age = typeof read.receivedAt === 'number' ? now - read.receivedAt : 0
  // A last read that failed or timed out says nothing about the call: unknown. If the last
  // good receipt said a call may be up, End stays offered as a best-effort escape (a request,
  // never claimed as applied).
  const escape = (last) => (last === 'active' || last === 'starting' ? { action: 'end', label: 'End call (state unknown)' } : {})
  if (read.unavailable) {
    return { ...base, label: 'Call unavailable', title: `the native owner cannot be read: ${read.unavailable}`, reason: read.unavailable, ...escape(read.lastState) }
  }
  if (age > LEASE_MS) {
    return { ...base, state: 'unknown', label: 'Call: state unknown', tone: 'pending', stale: true, title: `no fresh receipt from the native owner for ${Math.round(age / 1000)} s: the call and the mute state are unknown`, reason: 'stale', ...escape(read.call?.state) }
  }
  const call = read.call
  if (!call || typeof call.state !== 'string') {
    return { ...base, label: 'Call unavailable', title: 'this host predates in-app calls: relaunch Unfold with the new build', reason: 'no call owner' }
  }
  const link = read.link && read.link !== 'up' ? ` · agent link: ${read.link}` : ''
  let v
  switch (call.state) {
    case 'off':
      v = { ...base, state: 'off', label: 'Start call', action: 'start', title: `start a voice call: the host opens the microphone and speaker${link}` }
      break
    case 'starting':
      v = { ...base, state: 'starting', label: 'Starting call…', action: 'end', busy: true, tone: 'pending', title: `${call.reason ?? 'starting'}. Click to end it (the start completes, then the devices are released)${link}` }
      break
    case 'active':
      v = { ...base, state: 'active', label: 'End call', action: 'end', tone: 'danger', title: `in a call: ${call.reason ?? 'the microphone is admitted, the speaker plays'}. End call closes the microphone and the speaker; the conversation stays${link}` }
      break
    case 'ending':
      v = { ...base, state: 'ending', label: 'Ending call…', busy: true, tone: 'pending', title: `${call.reason ?? 'releasing the devices'}${link}` }
      break
    case 'error': {
      const why = call.error ?? call.reason ?? 'the call did not start'
      v = { ...base, state: 'error', label: 'Start call', action: 'start', tone: 'error', reason: why, title: `the last call did not start: ${why}. Click to try again${link}` }
      break
    }
    default:
      return { ...base, label: 'Call unavailable', title: `unknown call state ${call.state}`, reason: `unknown state ${call.state}` }
  }
  v.desired = call.desired ?? null
  if (pending) {
    const answered = call.desired === want(pending)
    if (!answered && now - pending.at < PENDING_MS) {
      v = { ...v, busy: true, action: null, pending: 'waiting', title: `${pending.want === 'start' ? 'start' : 'end'} requested; waiting for the native owner's receipt` }
    } else if (!answered) {
      v = { ...v, pending: 'no_receipt', title: `no receipt for the ${pending.want} request within ${PENDING_MS / 1000} s. ${v.title}` }
    }
  }
  if (v.state === 'active') v.mute = muteView(read.mic)
  return v
}

/** The mute toggle shown beside an active call: the input fence the host has APPLIED, never the click. */
export function muteView(mic) {
  switch (mic?.state) {
    case 'muted':
      return { label: 'Unmute', muted: true, action: false, busy: false, title: 'muted: no microphone audio is sent (input only; the speaker keeps playing)' }
    case 'unmuted':
      return { label: 'Mute', muted: false, action: true, busy: false, title: 'mute the microphone (input only; the speaker keeps playing; not End call)' }
    case 'muting':
      return { label: 'Muting…', muted: false, action: null, busy: true, title: 'mute requested, not yet applied by the host' }
    case 'unmuting':
      return { label: 'Unmuting…', muted: true, action: null, busy: true, title: 'unmute requested, not yet applied by the host (still muted)' }
    default:
      return null
  }
}

/**
 * The reader's result as the next view input: a failed read keeps no state of its own but
 * remembers the last good receipt's state (for the End escape only).
 */
export function nextRead(prev, got) {
  if (!got || got.stale) return prev
  if (got.unavailable) return { ...got, lastState: prev?.call?.state ?? prev?.lastState ?? null }
  return got
}

/** How soon to read again: fast while something is changing, slower in a steady state. */
export function pollMs(v) {
  if (v.busy || v.pending === 'waiting' || v.mute?.busy || v.stale || v.state === 'unknown') return 600
  return v.state === 'active' ? 3_000 : 5_000
}
