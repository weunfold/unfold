// Activity metrics for Mind: latency, costs, time to first token. Pure; tested in node.
//
// Rules:
// - Joins by explicit ids only (response_id, request_id/command_id), read from the rows' structured
//   projection (`row.ref`, src/mind.mjs refOf), never from display sentences, never by order.
// - Each metric names its start and end events, their clock origin and their row ids. Host times
//   subtract only within one rth-host process (ref.host_life); agent times only within one agent
//   boot (ref.producer). A row re-added after eviction (ref.continuation) is never a "first".
//   A host row whose process is not established (no "started" record seen: host_life null) never
//   enters a span. Both endpoints must be finite numbers; a missing time is never read as zero.
// - Chains, not pairs: a span's request -> reply start -> endpoint rows must all belong to the one
//   identity that bound them (the reply start's host lifetime, or its agent boot). Two endpoints
//   sharing a clock is not enough; a row from another identity never becomes an endpoint or status.
// - Ids are qualified by their producer lifetime. A raw reply id seen in more than one start (two
//   host processes, two agent boots, or two different request bindings, including host and agent
//   disagreeing on the request) is ambiguous: the reply gets no status and no timing (fail closed).
//   A request id typed in more than one process is ambiguous the same way.
// - Tokens and cost: the provider usage report carries a connection generation and a total, not a
//   reply id (crates/rth-agentd/src/gemini.rs), so no reply gets tokens or cost; the reports are
//   counted apart as "not linked". Missing is shown as missing; never zero; never a guess.

import { captionOf } from './moments.mjs'

/**
 * The pinned price table, kept for when per-reply usage with billable categories exists (no cost
 * is computed). USD per 1M tokens, paid tier, standard.
 */
export const PRICES = Object.freeze({
  id: 'gemini-api-pricing@2026-10-19',
  source: 'https://ai.google.dev/gemini-api/docs/pricing',
  page_updated: '2026-10-19 UTC',
  verified: '2026-10-22',
  currency: 'USD',
  unit: 'per 1M tokens',
  tier: 'paid, standard',
  models: Object.freeze({
    'gemini-3.8-live': Object.freeze({ text_in: 0.75, audio_in: 3.0, text_out: 4.5, audio_out: 12.0 }),
    'gemini-3.8-live-extended-thinking': Object.freeze({ text_in: 0.75, audio_in: 3.0, text_out: 4.5, audio_out: 12.0 }),
    'gemini-3.1-flash-live-preview': Object.freeze({ text_in: 0.75, audio_in: 3.0, text_out: 4.5, audio_out: 12.0 }),
    'gemini-2.5-flash-native-audio-preview-12-2025': Object.freeze({ text_in: 0.5, audio_in: 3.0, text_out: 2.0, audio_out: 12.0 }),
  }),
})

const ref = (r) => r?.ref ?? {}
const isHost = (r, kind) => r.lane === 'host' && r.kind === kind
const isAgent = (r, kind) => r.lane === 'agentd' && r.kind === kind
const time = (v) => (typeof v === 'number' && Number.isFinite(v) ? v : null)
const hostT = (r) => time(r?.clock?.t_ms)
const agentT = (r) => time(r?.clock?.t_ns)

/** One measured span, or an explicit reason it is not available. */
function span(name, start, end, clock, why = null) {
  const base = { name, start: start ? { kind: start.kind, row: start.id } : null, end: end ? { kind: end.kind, row: end.id } : null, clock }
  if (why) return { ...base, ms: null, why }
  if (!start || !end) return { ...base, ms: null, why: !start ? 'its start was not observed' : 'its end was not observed' }
  if (clock === 'host') {
    if (ref(start).host_life == null || ref(end).host_life == null) return { ...base, ms: null, why: 'host process not established (no started record observed for it)' }
    if (ref(start).host_life !== ref(end).host_life) return { ...base, ms: null, why: 'start and end come from different rth-host processes' }
    const a = hostT(start)
    const b = hostT(end)
    if (a == null || b == null) return { ...base, ms: null, why: `the ${a == null && b == null ? 'start and end times were' : a == null ? 'start time was' : 'end time was'} not reported` }
    const ms = b - a
    return ms >= 0 ? { ...base, ms } : { ...base, ms: null, why: 'clock went backwards' }
  }
  if (!ref(start).producer || ref(start).producer !== ref(end).producer) return { ...base, ms: null, why: 'start and end come from different agent boots' }
  if (ref(end).continuation) return { ...base, ms: null, why: 'the first event was evicted; its first time is unknown' }
  const a = agentT(start)
  const b = agentT(end)
  if (a == null || b == null) return { ...base, ms: null, why: `the ${a == null && b == null ? 'start and end times were' : a == null ? 'start time was' : 'end time was'} not reported` }
  const ns = b - a
  return ns >= 0 ? { ...base, ms: ns / 1e6 } : { ...base, ms: null, why: 'clock went backwards' }
}

/**
 * The metrics of one reply moment (its own episode's rows).
 * @param {{rows: string[]}} m a reply moment
 * @param {Map<string, object>} byId every row by id
 * @returns {{reply: string|null, typed: boolean, status: string|null, spans: object[], playback: string, tokens: object, cost: object, why?: string}}
 */
export function replyMetrics(m, byId) {
  const own = m.rows.map((id) => byId.get(id)).filter(Boolean)
  const replies = [...new Set(own.filter((r) => r.kind === 'response_started').map((r) => ref(r).response_id).filter(Boolean))]
  const out = { reply: null, typed: false, status: null, spans: [], playback: 'not reported', tokens: { value: null, why: "the agent's usage report names no reply" }, cost: { value: null, why: 'needs per-reply usage with billable categories' } }
  if (replies.length !== 1) return { ...out, why: replies.length ? `${replies.length} replies share this episode: not attributed` : 'no reply start observed' }
  const R = replies[0]
  out.reply = R
  // Lifetime-qualified identity: R must name exactly one start per lane, and the lanes must agree.
  const hStarts = own.filter((r) => isHost(r, 'response_started') && ref(r).response_id === R)
  const aStarts = own.filter((r) => isAgent(r, 'response_started') && ref(r).response_id === R)
  const ambiguous = hStarts.length > 1 ? `reply ${R} was started ${hStarts.length} times on rth-host (${new Set(hStarts.map((r) => ref(r).host_life)).size > 1 ? 'in different host processes' : 'in one process'})`
    : aStarts.length > 1 ? `reply ${R} was started ${aStarts.length} times by the agent (${new Set(aStarts.map((r) => ref(r).producer)).size > 1 ? 'in different agent boots' : 'in one boot'})`
    : hStarts[0] && aStarts[0] && ref(hStarts[0]).request_id != null && ref(aStarts[0]).request_id != null && ref(hStarts[0]).request_id !== ref(aStarts[0]).request_id ? `rth-host and the agent bind reply ${R} to different requests`
    : null
  if (ambiguous) return { ...out, why: `${ambiguous}: the id is ambiguous, not attributed` }
  const hStart = hStarts[0] ?? null
  const aStart = aStarts[0] ?? null
  // Chain rule: each lane's request -> reply start ->
  // endpoint chain lives in ONE established identity: the host chain in the reply start's rth-host
  // lifetime, the agent chain in the agent start's boot. A row from another identity may explain why
  // a span is unavailable; it never becomes an endpoint or a status. No chain, no measurement.
  const life = hStart ? ref(hStart).host_life : null
  const boot = aStart ? ref(aStart).producer : null
  const inHost = (r) => life != null && ref(r).host_life === life
  const inBoot = (r) => boot != null && ref(r).producer === boot
  const pick = (rows, inId) => ({ row: rows.find(inId) ?? null, other: rows.find((r) => !inId(r)) ?? null })
  const hostOf = (kind, f = () => true) => own.filter((r) => isHost(r, kind) && f(r))
  const agentOf = (kind, f = () => true) => own.filter((r) => isAgent(r, kind) && f(r))
  const ofR = (r) => ref(r).response_id === R
  const hDone = pick(hostOf('response_done', ofR), inHost)
  const hPlay = pick(hostOf('playback', (r) => ofR(r) && ref(r).phase === 'started'), inHost)
  const aDelta = pick(agentOf('agent_delta', ofR), inBoot)
  const aDone = pick(agentOf('agent_done', ofR), inBoot)
  out.status = ref(hDone.row).status ?? ref(aDone.row).status ?? null
  // Each lane's request id comes from that lane's own reply start (its R -> Q binding).
  const Qh = hStart ? ref(hStart).request_id ?? null : null
  const Qa = aStart ? ref(aStart).request_id ?? null : null
  const hCmds = Qh ? hostOf('command_written', (r) => ref(r).command_id === Qh) : []
  const aCmds = Qa ? agentOf('command_admitted', (r) => ref(r).command_id === Qa && ref(r).status === 'admitted') : []
  if (hCmds.length > 1 || aCmds.length > 1) return { ...out, why: `request ${hCmds.length > 1 ? Qh : Qa} was typed more than once (${hCmds.length > 1 ? 'rth-host' : 'agent'}): the id is ambiguous, not attributed` }
  const hCmd = pick(hCmds, inHost)
  const aCmd = pick(aCmds, inBoot)
  const hostChain = !hStart ? "the reply's start on rth-host was not observed (its request binding is unknown)"
    : life == null ? 'host process not established (no started record observed for it)' : null
  const agentChain = !aStart ? "the agent's own start for this reply (its request binding) was not observed"
    : boot == null ? 'the agent boot is unknown' : null
  const chain = (name, start, end, clock, whyChain, idName) => {
    if (whyChain) return span(name, start.row ?? start.other, end.row ?? end.other, clock, whyChain)
    if (!start.row) return span(name, null, end.row, clock, start.other ? `its start comes from another ${idName}` : 'its start was not observed')
    if (!end.row) return span(name, start.row, null, clock, end.other ? `its end comes from another ${idName} than its start` : 'its end was not observed')
    return span(name, start.row, end.row, clock)
  }
  const one = (row) => ({ row, other: null })
  out.typed = Boolean(hCmds.length || aCmds.length)
  if (out.typed) out.spans.push(chain('reply finished', hCmd, hDone, 'host', hostChain, 'rth-host process'))
  out.spans.push(chain('reply duration', one(hStart), hDone, 'host', hostChain, 'rth-host process'))
  if (hPlay.row) {
    out.playback = 'reported'
    out.spans.push(out.typed ? chain('playback started', hCmd, hPlay, 'host', hostChain, 'rth-host process') : chain('playback started after the reply began', one(hStart), hPlay, 'host', hostChain, 'rth-host process'))
  }
  if (out.typed) out.spans.push(chain('first text received', aCmd, aDelta, 'agent', agentChain, 'agent boot'))
  out.spans.push({ name: 'provider time to first token', ms: null, why: 'not instrumented (the provider adapter emits no first-token time)', clock: null, start: null, end: null })
  return out
}

/** Provider usage reports seen (each a connection generation and a total), counted apart: not linked to replies. */
export function usageReports(rows) {
  const seen = new Set()
  const reports = []
  for (const r of rows) {
    const x = ref(r)
    if (x.tag !== 'provider_usage') continue
    const key = r.event_id ?? r.id
    if (seen.has(key)) continue // a replayed or duplicated report counts once
    seen.add(key)
    reports.push({ row: r.id, gen: x.gen, total_tokens: x.total_tokens, producer: x.producer })
  }
  return reports
}

/**
 * Observed configuration context, not a per-reply binding: the model named by the latest readiness
 * note before this reply's agent start, from the same agent boot, by order. The provider does not
 * say which model generated a reply, so this is never used for pricing.
 */
export function modelFor(m, rows, byId) {
  const own = m.rows.map((id) => byId.get(id)).filter(Boolean)
  const aStart = own.find((r) => isAgent(r, 'response_started'))
  if (!aStart) return null
  const i = rows.indexOf(aStart)
  for (let k = i - 1; k >= 0; k--) {
    const x = ref(rows[k])
    if (x.tag === 'provider_ready' && x.producer === ref(aStart).producer) return x.model
  }
  return null
}

const measured = (met, name) => met?.spans?.find((s) => s.name === name && s.ms != null)?.ms ?? null

/** The calm session line: replies, timing coverage, the average of one basis, usage not linked. */
export function sessionSummary(items, rows) {
  const replies = items.filter((x) => x.type === 'reply' && x.metrics)
  const done = replies.filter((x) => x.metrics.status === 'completed')
  const durations = done.map((x) => measured(x.metrics, 'reply duration')).filter((v) => v != null)
  return {
    replies: replies.length,
    completed: done.length,
    timed: durations.length,
    avgDurationMs: durations.length ? durations.reduce((a, b) => a + b, 0) / durations.length : null,
    usageReports: usageReports(rows).length,
    tokensLinked: 0,
  }
}

export const fmtSec = (v) => (v == null ? null : v < 1000 ? `${(v / 1000).toFixed(2)} s` : `${(v / 1000).toFixed(1)} s`)

/**
 * The caption under a reply: outcome words from captionOf, then timing and playback ONLY from
 * the qualified activity result. An unavailable, ambiguous or unestablished metric shows no timing
 * and no playback claim; nothing falls back to the episode's first-to-last duration or a raw
 * playback row. Playback is the host's report ("playback reported"), never "played" or "heard".
 */
export function replyCaption(m, now = null) {
  return [captionOf(m, now), metricLine(m.metrics)].filter(Boolean).join(' · ')
}

/** The compact line: the same names as the expanded breakdown, only measured values. */
export function metricLine(met) {
  if (!met || met.why) return ''
  const parts = []
  const fin = measured(met, 'reply finished')
  const dur = measured(met, 'reply duration')
  const first = measured(met, 'first text received')
  if (fin != null) parts.push(`finished in ${fmtSec(fin)}`)
  else if (dur != null) parts.push(`reply ${fmtSec(dur)}`)
  if (first != null) parts.push(`first text ${fmtSec(first)}`)
  if (met.playback === 'reported') parts.push('playback reported')
  return parts.join(' · ')
}
