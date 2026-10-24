// The presence tab's "what is it doing": one discreet state word and a small RTS-style
// production queue of the things actually running. Pure, so it is tested in node and bundled
// into the browser half as is.
//
// Every word and every slot comes from the presence view (src/presence.mjs) and the
// voice receipt; nothing is inferred:
// - a slot exists only while the reducer holds that command or reply open, by its own id; it
//   settles only when that id leaves the open set (the reducer's own per-id settlement);
// - there is no progress source, so a slot never shows a percentage: an indeterminate shimmer and
//   the time since this tab first saw it;
// - a slot ends neutrally ("ended"): the presence view does not say how it ended, so no slot claims
//   success, failure or a stop;
// - no slot offers a cancel: the only stop route (api/rth-session/stop -> /v1/cancel {}) is global,
//   not per reply (the RTH status tab keeps that one global Stop speaking);
// - only identified items get slots: the host's unfinished count is an unidentified aggregate,
//   and its leaving the label is not terminal evidence (it still makes the state word "working");
// - no "thinking", no "listening" (there is no speech-candidate source on this feed).

/** Words in the page language (DSH is English by default; pt-BR when the page says so). */
export const WORDS = {
  en: { unknown: '?', speaking: 'Speaking', replying: 'Replying', writing: 'Writing', listening: 'Listening', working: 'Working', quiet: 'Quiet', asleep: 'Asleep' },
  pt: { unknown: '?', speaking: 'Falando', replying: 'Respondendo', writing: 'Escrevendo', listening: 'Ouvindo', working: 'Trabalhando', quiet: 'Em silêncio', asleep: 'Dormindo' },
}
export const langOf = (tag) => (/^pt\b/i.test(tag ?? '') ? 'pt' : 'en')

/** The open commands and replies the reducer reports, each by its own id (from its request-layer label). */
export function openItems(view) {
  const request = view?.layers?.request
  if (request?.state !== 'pending') return []
  const label = String(request.label ?? '')
  const items = []
  for (const m of label.matchAll(/\bcommand (\S+) admitted\b/g)) items.push({ id: `c:${m[1]}`, kind: 'request', ref: m[1] })
  for (const m of label.matchAll(/\breply (\S+) started\b/g)) items.push({ id: `r:${m[1]}`, kind: 'reply', ref: m[1] })
  return items
}

/**
 * The one state word. Priority: unknown, then what is happening (speaking, replying, working),
 * then rest (asleep when voice sleeps, quiet when awake with nothing open). An open response is
 * "replying" (modality-neutral: response_started carries no text or audio evidence). "writing" needs
 * agent_delta text evidence and "listening" a speech candidate; this feed's view has neither, so
 * neither is ever returned.
 * @returns {'unknown'|'speaking'|'replying'|'working'|'asleep'|'quiet'}
 */
export function stateOf(view, voice, queue = null) {
  const pose = view?.pose ?? 'unknown'
  if (pose === 'unknown' || !voice?.read || voice.actual === 'unknown') return 'unknown'
  const lost = (queue?.slots ?? []).some((s) => s.phase === 'lost')
  if (pose === 'speaking') return 'speaking' // device-rendered audio reported (layers.audio rendering)
  const items = openItems(view)
  if (items.some((i) => i.kind === 'reply')) return 'replying' // a reply was seen to start and not to end
  if (items.length || view?.layers?.request?.state === 'pending') return 'working' // a request is open (identified, or the host's unidentified count)
  if (lost) return 'unknown' // something's outcome was never observed: not "quiet", not "asleep"
  if (voice.actual === 'off' || voice.actual === 'error') return 'asleep'
  return 'quiet'
}

export const SETTLE_MS = 1_400
export const MAX_SLOTS = 5

/**
 * The queue's lifecycle, one step per view. A slot appears when its id opens and keeps the local
 * time this tab first saw it. It ends (neutrally, then fades after SETTLE_MS) only when its id leaves
 * the open set while the presence stayed live. If the presence went unknown (stream lost, link
 * dropped, reattach) while it was running, its outcome was not observed: when it is gone after
 * that, it stays as a "?" slot, "not sure how it ended", until the same id shows up open again:
 * a missing outcome never becomes idle or settled through time or a gap.
 * @param {{slots: object[]}} prev
 * @param {object} view the presence view (or the client's stale placeholder)
 * @param {number} now local ms
 */
export function stepQueue(prev, view, now) {
  const live = view?.pose !== 'unknown' && view?.layers?.link?.state === 'live'
  const open = live ? openItems(view) : null
  const byId = new Map((prev?.slots ?? []).map((s) => [s.id, s]))
  const slots = []
  if (open) {
    for (const item of open) {
      const old = byId.get(item.id)
      const keep = old && (old.phase === 'running' || old.phase === 'lost')
      slots.push({ ...item, since: keep ? old.since : now, phase: 'running', unsure: false })
      byId.delete(item.id)
    }
  }
  for (const s of byId.values()) {
    if (!live) slots.push(s.phase === 'running' ? { ...s, unsure: true } : s) // nothing settles by silence
    else if (s.phase === 'running') slots.push(s.unsure ? { ...s, phase: 'lost', unsure: false } : { ...s, phase: 'ended', endedAt: now })
    else if (s.phase === 'lost') slots.push(s) // kept: its outcome was never observed
    else if (now - s.endedAt < SETTLE_MS) slots.push(s)
  }
  slots.sort((a, b) => a.since - b.since)
  return { slots }
}

/** Anything open or of unknown outcome (the state word is never "quiet" while one remains). */
export const unresolvedIn = (q) => (q?.slots ?? []).some((s) => s.phase === 'running' || s.phase === 'lost')

/** When the queue next changes with no new view (a settle finishing), or null. */
export function nextQueueChange(q) {
  const ends = (q?.slots ?? []).filter((s) => s.phase === 'ended').map((s) => s.endedAt + SETTLE_MS)
  return ends.length ? Math.min(...ends) : null
}

const ago = (ms) => (ms < 60_000 ? `${Math.max(0, Math.round(ms / 1000))} s` : `${Math.round(ms / 60_000)} min`)

/** One plain line for an expanded slot. */
export function slotLine(slot, now) {
  const age = ago(now - slot.since)
  if (slot.phase === 'ended') return slot.kind === 'reply' ? 'A reply ended' : 'A request ended'
  if (slot.phase === 'lost') return `${slot.kind === 'reply' ? 'A reply' : 'A request'} · not sure how it ended (the link dropped while it was open)`
  const unsure = slot.unsure ? ' · not sure it is still running (out of touch)' : ''
  if (slot.kind === 'reply') return `Replying · ${age}${unsure}`
  // Neutral: a command stays open until its reply settles, so a reply may already have started
  // (request and reply are overlapping records, not independent tasks).
  return `A request is open · ${age}${unsure}`
}

