// Unfold -> trajectory layout adapter (the design notes).
//
// Input: the Mind projection, exactly as /api/unfold-mind serves it (redacted public rows with
// their structured `ref`, `clock`, `episode` and `observed_at`, plus episodes). Output: the upstream
// TrajectoryTurnModel shape (turns -> groups -> cells) that the forked ledger, overview and
// inspector render, with Unfold fields beside the upstream ones:
//   unfoldKind     the taxonomy kind (badge, icon, colour, filter group)
//   unfoldLane     overview lane 0..4 (Hear, Think, Speak, Act, Life)
//   unfoldSpan     'measured' | 'instant' | 'unknown'  (honesty: only 'measured' has a width)
//   unfoldClock    the clock a measured width came from ('host' | 'agent'), never mixed
//   unfoldTiming   named spans [{name, ms|null, why, start, end, clock}] for the inspector
//   unfoldRows     the row ids the record was built from (evidence; copyable)
// Nothing here manufactures DSH request/tool events. Every reply measurement comes from the
// Mind's replyMetrics (lifetime-qualified chains, finite-time spans, fail closed on ambiguity).
// Other durations (playback, commands) use the same rule locally: two finite endpoints in the same
// established clock lifetime, else no width.
import { replyMetrics, usageReports } from './mind/metrics.js'
import { decidedTo, quoted } from './mind/moments.js'
import { KINDS, LANE, REPLY_PARTS, kindOf } from './taxonomy.mjs'

const ref = (r) => r?.ref ?? {}
const finite = (v) => typeof v === 'number' && Number.isFinite(v)
const hostT = (r) => (finite(r?.clock?.t_ms) ? r.clock.t_ms : null)
const agentT = (r) => (finite(r?.clock?.t_ns) ? r.clock.t_ns / 1e6 : null)

/**
 * One duration on one producer clock, or the reason there is none: never a span across clocks.
 * @returns {{ms: number|null, why?: string, clock: string|null}}
 */
export function pairSpan(start, end) {
  if (!start || !end) return { ms: null, why: !start ? 'its start was not observed' : 'its end was not observed', clock: null }
  if (start.lane !== end.lane) return { ms: null, why: 'start and end come from different producers (clocks are never mixed)', clock: null }
  if (start.lane === 'host') {
    const a = ref(start).host_life
    const b = ref(end).host_life
    if (a == null || b == null) return { ms: null, why: 'host process not established (no started record observed for it)', clock: null }
    if (a !== b) return { ms: null, why: 'start and end come from different host processes', clock: null }
    const s = hostT(start)
    const e = hostT(end)
    if (s == null || e == null) return { ms: null, why: 'a time was not reported', clock: null }
    return e >= s ? { ms: e - s, clock: 'host' } : { ms: null, why: 'clock went backwards', clock: null }
  }
  if (start.lane === 'agentd') {
    if (!ref(start).producer || ref(start).producer !== ref(end).producer) return { ms: null, why: 'start and end come from different agent boots', clock: null }
    const s = agentT(start)
    const e = agentT(end)
    if (s == null || e == null) return { ms: null, why: 'a time was not reported', clock: null }
    return e >= s ? { ms: e - s, clock: 'agent' } : { ms: null, why: 'clock went backwards', clock: null }
  }
  return { ms: null, why: 'no producer clock', clock: null }
}

const fmt = (ms) => (ms == null ? null : ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`)

function textOf(row) {
  const q = quoted(row.sentence)
  return q ?? row.sentence ?? row.kind
}

/** Local paths in projected sentences (journal files, data dirs) show only their last part. Single
 * interior spaces are part of a segment (macOS "Application Support"); producers end paths with ":". */
export const shortPaths = (text) => (typeof text === 'string' ? text.replace(/(?:~|\/(?:Users|home|private|var|tmp|opt))(?:\/[^\s/:;,()]+(?: [^\s/:;,()]+)*)+/g, (p) => `…/${p.split('/').at(-1)}`) : text)

/**
 * Every string in a projected row with local paths shortened, so the ledger, the inspector
 * (Summary and Raw), the search index and every copy action only ever see the short form.
 * Memoized per row object: the projection hands the same objects back until they change.
 */
const scrubbed = new WeakMap()
function scrubDeep(v) {
  if (typeof v === 'string') return shortPaths(v)
  if (Array.isArray(v)) return v.map(scrubDeep)
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, scrubDeep(x)]))
  return v
}
export function scrubRow(row) {
  if (!row || typeof row !== 'object') return row
  let out = scrubbed.get(row)
  if (!out) { out = scrubDeep(row); scrubbed.set(row, out) }
  return out
}

/** The plain-words row text for a standalone record. */
export function rowText(kind, row) { return shortPaths(rowTextRaw(kind, row)) }
// Mind's sentence is the wording shown (it keeps the source's action and uncertainty); the
// short forms below never claim more than the row does.
function rowTextRaw(kind, row) {
  const r = ref(row)
  switch (kind) {
    case 'vad': return row.sentence ?? (r.level != null ? `possible speech start · ${Math.round(r.level)} dBFS (not proof anyone spoke)` : 'possible speech start (not proof anyone spoke)')
    case 'speaking': return r.phase === 'started' ? 'playback started (host report)' : r.phase === 'finished' ? 'playback finished (host report; not proof it was heard)' : r.phase === 'cleared' ? 'playback cleared (host report)' : 'playback reported (host)'
    case 'audio': return 'audio generated by the agent (not proof it was played)'
    case 'interrupt': return row.kind === 'speaker_cleared' ? 'speaker cleared' : row.kind === 'response_cancelled' ? `reply ${r.response_id ?? ''} cancelled`.replace('  ', ' ') : 'stop requested'
    case 'echo': return row.sentence ?? 'echo check (outcome not stated)'
    case 'decision': {
      const to = decidedTo(row.sentence)
      return to ? `decided: ${to.replace(/_/g, ' ')}` : row.sentence
    }
    case 'signal': return r.signal ?? r.tag ?? row.kind.replace(/_/g, ' ')
    default: return row.sentence ?? row.kind.replace(/_/g, ' ')
  }
}

/** A record (cell) in the upstream shape plus the Unfold fields. */
function cell(base) {
  const k = KINDS[base.unfoldKind]
  return {
    kind: k.upstream,
    unfoldLane: k.lane,
    timeSeconds: null,
    unfoldSpan: 'instant',
    ...base,
    text: shortPaths(base.text),
    inputDetail: shortPaths(base.inputDetail),
  }
}
const at = (row) => (row ? { kind: row.kind, row: row.id } : null)
const sameLife = (a, b) => a?.lane === 'host' && b?.lane === 'host' && ref(a).host_life != null && ref(a).host_life === ref(b).host_life
const sameBoot = (a, b) => a?.lane === 'agentd' && b?.lane === 'agentd' && ref(a).producer != null && ref(a).producer === ref(b).producer

/** The reply record: one per reply moment, built from Mind's reply metrics. */
function replyCell(epRows, byId, idx, cmdIndex = new Map()) {
  const m = { type: 'reply', rows: epRows.map((r) => r.id) }
  const met = replyMetrics(m, byId)
  const deltas = epRows.filter((r) => r.kind === 'agent_delta' || r.kind === 'reply_text')
  const text = deltas.length ? textOf(deltas.at(-1)) : null
  const hStart = epRows.find((r) => r.kind === 'response_started' && r.lane === 'host' && ref(r).response_id === met.reply) ?? null
  const aStart = epRows.find((r) => r.kind === 'response_started' && r.lane === 'agentd' && ref(r).response_id === met.reply) ?? null
  const started = hStart ?? aStart ?? epRows.find((r) => r.kind === 'response_started')
  const duration = met.spans.find((s) => s.name === 'reply duration')
  const measured = !met.why && duration?.ms != null
  const parts = epRows.filter((r) => REPLY_PARTS.has(r.kind))
  // Three different things, each only with evidence naming THIS reply in ITS lifetime:
  // - a stop REQUEST (agent `cancel`, host `cancel_written`: "sent, not yet confirmed") is an
  //   attempted action; it settles nothing and asserts no interruption;
  // - a confirmed LOCAL cancellation (host `response_cancelled`: "Host marked reply R cancelled")
  //   is the reply's cancelled outcome when no terminal status says otherwise;
  // - a CLEARED playback (host playback phase "cleared") says the output was cleared, not that the
  //   provider stopped. A speaker clear without a reply id is never causal.
  const ofR = (r) => met.reply != null && !met.why && ref(r).response_id === met.reply
  const mine = (r) => ofR(r) && (sameLife(r, hStart) || sameBoot(r, aStart))
  const stops = epRows.filter((r) => (r.kind === 'cancel' || r.kind === 'cancel_written') && mine(r))
  const cancels = epRows.filter((r) => r.kind === 'response_cancelled' && r.lane === 'host' && mine(r))
  const cleared = epRows.filter((r) => r.kind === 'playback' && ref(r).phase === 'cleared' && ofR(r) && sameLife(r, hStart))
  // An end reported by another identity (another host process or agent boot) is not this
  // reply's outcome: the reply is "not attributed", never "no end observed" nor "Completed".
  const foreignEnds = met.why || met.status ? [] : epRows.filter((r) => ofR(r) && (r.kind === 'response_done' || r.kind === 'agent_done') && !(sameLife(r, hStart) || sameBoot(r, aStart)))
  const outcome = met.why ? null : met.status ?? (cancels.length ? 'cancelled' : null)
  const outcomeWhy = outcome == null && foreignEnds.length ? 'its end was reported by another host process or agent boot: not attributed' : null
  return cell({
    unfoldKind: 'unfold',
    recordId: `unfold:reply:${started?.id ?? parts[0]?.id}`,
    index: idx,
    text: text ?? (met.reply ? `reply ${met.reply} (no text seen)` : 'a reply (no text seen)'),
    outputDetail: text ?? undefined,
    startedAt: (started ?? parts[0])?.observed_at ?? null,
    timeSeconds: measured ? duration.ms / 1000 : null,
    unfoldDurationMs: measured ? duration.ms : null,
    unfoldSpan: measured ? 'measured' : 'unknown',
    unfoldClock: measured ? 'host' : null,
    unfoldTiming: met.why ? [{ name: 'timing', ms: null, why: met.why }] : met.spans.map((s) => ({ name: s.name, ms: s.ms, why: s.why ?? null, clock: s.clock, start: s.start, end: s.end })),
    unfoldStatus: met.status,
    unfoldOutcome: outcome,
    unfoldOutcomeWhy: outcomeWhy,
    // Typed only when the reply's request names exactly one command written in the reply's own
    // host process (replyMetrics reads the episode; this checks the whole projection).
    unfoldTyped: !met.why && met.typed === true && hStart != null && ref(hStart).host_life != null && (cmdIndex.get(ref(hStart).request_id) ?? []).filter((r) => r.kind === 'command_written' && ref(r).host_life === ref(hStart).host_life).length === 1,
    unfoldCancelledBy: cancels.map((r) => r.id),
    unfoldStopRequested: stops.map((r) => r.id),
    unfoldPlaybackCleared: cleared.map((r) => r.id),
    unfoldHostLife: hStart ? ref(hStart).host_life ?? null : null,
    unfoldPlayback: met.playback,
    unfoldReply: met.why ? null : met.reply,
    unfoldRequest: started ? ref(started).request_id ?? null : null,
    unfoldWhy: met.why ?? null,
    isError: !met.why && met.status === 'failed',
    unfoldRows: [...new Set([...parts, ...cancels, ...stops].map((r) => r.id))],
  })
}

/**
 * Playback records (host reports only; generated agent audio is its own AUDIO kind). One record
 * per reply id and host process. Measured only with exactly one start and exactly one end
 * (finished or cleared) in that identity; repeated or conflicting reports stay unknown, with the
 * reason and the counts. Rows without a reply id or process stand alone.
 */
function playbackCells(rows) {
  const out = []
  const groups = new Map()
  for (const r of rows) {
    const key = ref(r).response_id != null && ref(r).host_life != null ? `${ref(r).response_id}|${ref(r).host_life}` : `row:${r.id}`
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key).push(r)
  }
  for (const group of groups.values()) {
    const R = ref(group[0]).response_id ?? null
    const starts = group.filter((r) => ref(r).phase === 'started')
    const ends = group.filter((r) => ref(r).phase === 'finished' || ref(r).phase === 'cleared')
    let span
    if (starts.length === 1 && ends.length === 1) span = pairSpan(starts[0], ends[0])
    else if (starts.length > 1 || ends.length > 1) span = { ms: null, clock: null, why: `${starts.length} playback start${starts.length === 1 ? '' : 's'} and ${ends.length} end${ends.length === 1 ? '' : 's'} for reply ${R} in one host process: which interval ended is not established` }
    else span = { ms: null, clock: null, why: starts.length ? 'no end reported' : 'its start was not reported' }
    const end = ends.length === 1 ? ends[0] : null
    const first = group[0]
    out.push(cell({
      unfoldKind: 'speaking',
      recordId: `unfold:play:${first.id}`,
      index: 0,
      text: span.ms != null
        ? end.kind === 'playback' && ref(end).phase === 'cleared' ? 'playback cut off (host report: cleared)' : 'playback reported (handed to the speaker; not proof it was heard)'
        : group.length === 1 ? rowTextRaw('speaking', first) : `playback reports for reply ${R} (interval not established)`,
      startedAt: first.observed_at,
      timeSeconds: span.ms != null ? span.ms / 1000 : null,
      unfoldDurationMs: span.ms,
      unfoldSpan: span.ms != null ? 'measured' : 'unknown',
      unfoldClock: span.clock,
      unfoldTiming: [{ name: 'playback (host report)', ms: span.ms, why: span.ms == null ? span.why : null, clock: span.clock, start: starts.length === 1 ? at(starts[0]) : null, end: end ? at(end) : null }],
      unfoldReplyId: R,
      unfoldConflict: starts.length > 1 || ends.length > 1,
      unfoldLife: ref(group[0]).host_life ?? null,
      unfoldRows: group.map((r) => r.id),
    }))
  }
  return out
}

/**
 * Command records: one per command id in the section. A command's identity is (command_id, host
 * process): its finish and outcome come only from the same process, and an id written more
 * than once in one process, or a write that is not established, is ambiguous (no outcome, no span).
 * The agent's admission is a different producer; it is shown only when exactly one exists for the id.
 */
function commandCells(rows, cmdIndex) {
  const out = []
  const byId = new Map()
  for (const r of rows) {
    const id = ref(r).command_id ?? r.id
    if (!byId.has(id)) byId.set(id, [])
    byId.get(id).push(r)
  }
  for (const [id, group] of byId) {
    const all = cmdIndex.get(id) ?? group
    const ownWrites = group.filter((r) => r.kind === 'command_written')
    const written = ownWrites.length === 1 ? ownWrites[0] : null
    const life = written ? ref(written).host_life ?? null : null
    const writesInLife = life == null ? [] : all.filter((r) => r.kind === 'command_written' && ref(r).host_life === life)
    const finishes = all.filter((r) => r.kind === 'command_finished' && r.lane === 'host')
    const finishesInLife = life == null ? [] : finishes.filter((r) => ref(r).host_life === life)
    const admits = all.filter((r) => r.kind === 'command_admitted')
    const admitted = admits.length === 1 ? admits[0] : null
    const why = ownWrites.length > 1 || writesInLife.length > 1 ? `command ${id} was sent more than once in one host process: not attributed`
      : written && life == null ? 'host process not established (no started record observed for it): not attributed'
      : written && finishesInLife.length > 1 ? `command ${id} finished ${finishesInLife.length} times in its host process: not attributed`
      : written && finishesInLife.length === 0 && finishes.length > 0 ? 'its finish comes from a different host process: not attributed'
      : !written && finishes.length > 0 ? 'its write was not observed: the finish is not attributed'
      : null
    const finished = !why && written && finishesInLife.length === 1 ? finishesInLife[0] : null
    const span = why ? { ms: null, why, clock: null } : written && finished ? pairSpan(written, finished) : { ms: null, why: !written ? 'not sent by this host log' : 'no finish observed', clock: null }
    const outcome = finished ? ref(finished).status ?? 'finished' : null
    const first = written ?? group.find((r) => r.kind === 'command_admitted') ?? group[0]
    const parts = [`command ${id}`, written ? 'sent (host)' : null, admitted ? `${ref(admitted).status ?? 'admitted'} (agent)` : null, outcome ? `${outcome} (host)` : why ? 'outcome not attributed' : null].filter(Boolean)
    out.push(cell({
      unfoldKind: 'command',
      recordId: `unfold:cmd:${first.id}`,
      unfoldCommand: ref(first).command_id ?? null,
      index: 0,
      text: parts.join(' · '),
      startedAt: first.observed_at,
      timeSeconds: span.ms != null ? span.ms / 1000 : null,
      unfoldDurationMs: span.ms,
      unfoldSpan: span.ms != null ? 'measured' : written || finishes.length ? 'unknown' : 'instant',
      unfoldClock: span.clock,
      unfoldTiming: [{ name: 'command (sent -> finished)', ms: span.ms, why: span.ms == null ? span.why : null, clock: span.clock, start: at(written), end: at(finished) }],
      unfoldOutcome: outcome,
      unfoldOutcomeWhy: why,
      isError: outcome === 'failed' || (!why && admitted != null && ref(admitted).status === 'rejected'),
      unfoldRows: group.map((r) => r.id),
    }))
  }
  return out
}

/**
 * Fold SIGNAL and ECHO GUARD rows of one section into one quiet summary record (they are noise on their own).
 * The single rows are built too (marked unfoldQuiet): filterTurns shows the summary while the Signals
 * filter is off (the default) and the single rows instead of it once Signals is switched on.
 */
function foldedSignals(rows) {
  if (!rows.length) return []
  const signals = rows.filter((r) => kindOf(r) === 'signal').length
  const echo = rows.filter((r) => kindOf(r) === 'echo').length
  const parts = [signals ? `${signals} signal${signals > 1 ? 's' : ''}` : null, echo ? `${echo} echo check${echo > 1 ? 's' : ''}` : null].filter(Boolean)
  return [cell({
    unfoldKind: signals ? 'signal' : 'echo',
    recordId: `unfold:fold:${rows[0].id}`,
    index: 0,
    text: `⌁ ${parts.join(' · ')}`,
    startedAt: rows[0].observed_at,
    unfoldFolded: rows.map((r) => ({ id: r.id, kind: kindOf(r), text: rowText(kindOf(r), r) })),
    unfoldRows: rows.map((r) => r.id),
  }), ...rows.map((r) => cell({
    unfoldKind: kindOf(r),
    recordId: `unfold:row:${r.id}`,
    index: 0,
    text: rowText(kindOf(r), r),
    startedAt: r.observed_at + 1e-6,
    unfoldQuiet: true,
    unfoldRows: [r.id],
  }))]
}

const NO_ASSOCIATION = 'not available: the projection links no utterance to a reply (a reply names only a stream or request id)'

/** Records for the rows of one section (one Mind episode, or loose rows between episodes). */
function sectionCells(rows, byId, idx, cmdIndex) {
  const cells = []
  const order = (c) => c.startedAt ?? Infinity
  const hasReply = rows.some((r) => r.kind === 'response_started' || r.kind === 'agent_delta' || r.kind === 'reply_text')
  // YOU · VOICE: one record per utterance (utterance_id when the row has one; otherwise each final
  // closes one). It sits with a reply only when Mind put both in one episode.
  const utterances = []
  let open = null
  for (const r of rows.filter((x) => x.kind === 'input_transcript')) {
    const u = ref(r).utterance_id ?? null
    if (!open || (u !== null && open.u !== u) || (u === null && open.closed)) { open = { u, rows: [], closed: false }; utterances.push(open) }
    open.rows.push(r)
    if (/\(final;/.test(r.sentence ?? '')) open.closed = true
  }
  for (const { rows: group } of utterances) {
    const last = group.at(-1)
    const final = /\(final;/.test(last.sentence ?? '')
    const text = textOf(last)
    cells.push(cell({ unfoldKind: 'voice', recordId: `unfold:heard:${group[0].id}`, index: 0, text: final ? text : `${text} (heard, partial)`, inputDetail: text, startedAt: group[0].observed_at, unfoldAssociation: hasReply ? 'same Mind episode as the reply' : NO_ASSOCIATION, unfoldRows: group.map((r) => r.id) }))
  }
  // YOU · TYPED: a typed request with no typed-text row of its own is still your input; its text
  // lives in the chat (Mind's projection carries the command id, not the text).
  const command = rows.find((r) => r.kind === 'command_written' || (r.kind === 'command_admitted' && r.lane === 'agentd'))
  if (command && !rows.some((r) => kindOf(r) === 'typed')) cells.push(cell({ unfoldKind: 'typed', recordId: `unfold:typed:${command.id}`, index: 0, text: 'typed message (its text is in the chat)', inputDetail: `Typed request ${ref(command).command_id ?? 'unknown'}`, startedAt: command.observed_at - 0.001, unfoldRows: [command.id] }))
  if (hasReply) cells.push(replyCell(rows, byId, 0, cmdIndex))
  cells.push(...playbackCells(rows.filter((r) => kindOf(r) === 'speaking')))
  cells.push(...commandCells(rows.filter((r) => kindOf(r) === 'command' || r.kind === 'command_finished'), cmdIndex))
  const quiet = []
  for (const r of rows) {
    const k = kindOf(r)
    if (!k || k === 'speaking' || k === 'command') continue
    if (k === 'signal' || k === 'echo') { quiet.push(r); continue }
    const typedText = k === 'typed' ? textOf(r) : null
    cells.push(cell({
      unfoldKind: k,
      recordId: `unfold:row:${r.id}`,
      index: 0,
      text: typedText ?? rowText(k, r),
      ...(k === 'typed' ? { inputDetail: typedText } : {}),
      startedAt: r.observed_at,
      isError: k === 'fault',
      unfoldRows: [r.id],
    }))
  }
  cells.push(...foldedSignals(quiet))
  const reply = cells.find((c) => c.unfoldKind === 'unfold')
  for (const c of cells) if (c.unfoldKind === 'typed') c.unfoldAssociation = reply?.unfoldTyped ? 'the reply names this command, sent in its own host process' : reply ? 'not established: the reply chain does not tie this command to the reply (missing, reused or from another process)' : 'no reply in this episode'
  cells.sort((a, b) => order(a) - order(b))
  for (const c of cells) c.index = idx()
  return cells
}

/**
 * Where a record sits on its producer's own clock, for the Duration overview. A placement
 * names one clock ('host'
 * = host clock.t_ms, 'agent' = agent clock.t_ns) and one lifetime (host process / agent boot).
 * Interval endpoints are the metric's own named start/end rows, resolved against the projection
 * and re-validated (established lifetime, same clock and lifetime, finite, not backwards); the
 * metric must have measured it. Shapes:
 *   'span'  start and end valid           'zero'  a measured 0 (distinct from missing)
 *   'start' start valid, end not          'end'   end valid, start not (end-only marker)
 *   'tick'  an observed instant
 * observed_at never positions anything. Unknown identity is unplaced (with the reason), never a
 * guessed group.
 */
const hostAt = (r) => (r?.lane === 'host' && ref(r).host_life != null && finite(r.clock?.t_ms) ? { clock: 'host', life: ref(r).host_life, t: r.clock.t_ms } : null)
const agentAt = (r) => (r?.lane === 'agentd' && ref(r).producer && finite(r.clock?.t_ns) ? { clock: 'agent', life: ref(r).producer, t: r.clock.t_ns / 1e6 } : null)
const clockAt = (r) => hostAt(r) ?? agentAt(r)
function placeInterval(entry, lane, name, byId) {
  if (!entry) return null
  const s = entry.start ? clockAt(byId.get(entry.start.row)) : null
  const e = entry.end ? clockAt(byId.get(entry.end.row)) : null
  const base = { lane, name, startRow: entry.start?.row ?? null, endRow: entry.end?.row ?? null, why: entry.why ?? null }
  if (entry.ms != null && s && e && s.clock === e.clock && s.life === e.life && e.t >= s.t) return { ...base, clock: s.clock, life: s.life, start: s.t, end: e.t, shape: e.t === s.t ? 'zero' : 'span' }
  if (s) return { ...base, clock: s.clock, life: s.life, start: s.t, end: null, shape: 'start', why: entry.why ?? 'end not observed' }
  if (e) return { ...base, clock: e.clock, life: e.life, start: null, end: e.t, shape: 'end', why: entry.why ?? 'start not observed' }
  return null
}
function placementsOf(c, byId) {
  const lane = KINDS[c.unfoldKind].lane
  const timing = (n) => c.unfoldTiming?.find((x) => x.name === n)
  const out = []
  if (c.unfoldKind === 'unfold') {
    if (c.unfoldWhy) return { placements: [], off: 'reply identity not attributed' }
    const h = placeInterval(timing('reply duration'), lane, 'host-observed reply', byId)
    if (h && h.clock === 'host') out.push(h)
    const f = placeInterval(timing('first text received'), lane, 'first text received', byId)
    if (f && f.clock === 'agent') out.push(f)
    // No host start: the reply's duration is a host measurement, so the agent's own start is shown
    // as a start marker only (never closed by an agent row: that is not Mind's metric).
    if (!h) {
      const aStart = (c.unfoldRows ?? []).map((id) => byId.get(id)).find((r) => r?.lane === 'agentd' && r.kind === 'response_started' && ref(r).response_id === c.unfoldReply)
      const at = agentAt(aStart)
      if (at) out.push({ lane, name: 'agent-observed reply start', clock: 'agent', life: at.life, start: at.t, end: null, shape: 'start', startRow: aStart.id, endRow: null, why: 'the reply duration is measured on the host clock; no host start was observed' })
    }
  } else if (c.unfoldKind === 'speaking' || c.unfoldKind === 'command') {
    if (c.unfoldConflict) return { placements: [], off: 'conflicting reports: which interval is meant is not established' }
    const p = placeInterval(c.unfoldTiming?.[0], lane, c.unfoldKind === 'speaking' ? 'playback (host report)' : 'command (sent → finished)', byId)
    if (p) out.push(p)
  } else {
    const first = (c.unfoldRows ?? []).map((id) => byId.get(id)).find((r) => clockAt(r))
    const at = first ? clockAt(first) : null
    if (at) out.push({ lane, name: KINDS[c.unfoldKind].label, clock: at.clock, life: at.life, start: at.t, end: null, shape: 'tick', startRow: first.id, endRow: null, why: null })
  }
  if (out.length) return { placements: out, off: null }
  const rows = (c.unfoldRows ?? []).map((id) => byId.get(id)).filter(Boolean)
  return { placements: [], off: rows.some((r) => r.lane === 'host' && ref(r).host_life == null) ? 'host process not established (no started record observed)' : rows.some((r) => r.lane === 'host' || r.lane === 'agentd') ? 'no valid producer time' : 'no producer clock (a Mind observation)' }
}

/** A one-line turn card: every chip measured or evidenced, or absent. */
export function turnCard(turn, cells) {
  const reply = cells.find((c) => c.unfoldKind === 'unfold')
  // Typed only when the reply chain established it: the reply names one command written in its
  // own host process (replyMetrics, lifetime- and conflict-checked). A command row Mind placed
  // nearby is shown, but it is not a typed-mode claim by itself.
  const typed = reply?.unfoldTyped === true
  const voice = !typed && cells.some((c) => c.unfoldKind === 'voice')
  const first = reply?.unfoldTiming?.find((s) => s.name === 'first text received' && s.ms != null)
  // Speaking: the measured playback of THIS reply id in THIS reply's host process only.
  // Playback records are one per (reply id, process); unknown lifetime means no chip.
  const mine = reply?.unfoldReply != null && reply.unfoldHostLife != null ? cells.filter((c) => c.unfoldKind === 'speaking' && c.unfoldReplyId === reply.unfoldReply && c.unfoldLife === reply.unfoldHostLife) : []
  const speaking = mine.length === 1 && mine[0].unfoldSpan === 'measured' ? mine[0].unfoldDurationMs : null
  const cancelled = (reply?.unfoldCancelledBy?.length ?? 0) > 0 && reply.unfoldOutcome === 'cancelled'
  const chips = [
    typed ? 'typed' : voice ? 'voice' : null,
    first ? `first text ${fmt(first.ms)}` : null,
    speaking != null ? `speaking ${fmt(speaking)}` : null,
    cancelled ? '⚡ cancelled (host)' : null,
    (reply?.unfoldPlaybackCleared?.length ?? 0) > 0 ? 'playback cleared' : null,
    (reply?.unfoldStopRequested?.length ?? 0) > 0 && !cancelled ? 'stop requested' : null,
    reply?.unfoldWhy ? 'timing not attributed' : null,
  ].filter(Boolean)
  return { icon: typed ? 'keyboard' : voice ? 'mic' : 'sparkles', label: `Turn ${turn}`, chips }
}

/**
 * Build the trajectory layout from a Mind snapshot. Turns are Mind's episodes, nothing more: the
 * adapter joins no episodes and moves no rows between them. A section with a reply is a
 * numbered turn; heard-only and loose sections are unnumbered.
 * @param {{rows: object[], episodes?: object, arrivals?: Map<string, number>}} state
 */
export function buildUnfoldTrajectory(state) {
  const rows = (state.rows ?? []).map(scrubRow).sort((a, b) => a.observed_at - b.observed_at)
  const byId = new Map(rows.map((r) => [r.id, r]))
  const arrivals = state.arrivals instanceof Map ? state.arrivals : null
  let n = 0
  const idx = () => ++n
  const cmdIndex = new Map()
  for (const r of rows) {
    if (!['command_written', 'command_admitted', 'command_finished'].includes(r.kind) || ref(r).command_id == null) continue
    if (!cmdIndex.has(ref(r).command_id)) cmdIndex.set(ref(r).command_id, [])
    cmdIndex.get(ref(r).command_id).push(r)
  }
  const sections = []
  const byEpisode = new Map()
  for (const r of rows) {
    if (r.episode && r.status !== 'gap') {
      if (!byEpisode.has(r.episode)) {
        byEpisode.set(r.episode, [])
        sections.push({ episode: r.episode, rows: byEpisode.get(r.episode) })
      }
      byEpisode.get(r.episode).push(r)
    } else {
      const last = sections.at(-1)
      if (last && last.episode === null) last.rows.push(r)
      else sections.push({ episode: null, rows: [r] })
    }
  }
  const turns = []
  const cards = new Map()
  let turnNo = 0
  for (const sec of sections) {
    const cells = sectionCells(sec.rows, byId, idx, cmdIndex)
    if (!cells.length) continue
    for (const c of cells) {
      const { placements, off } = placementsOf(c, byId)
      c.unfoldPlacements = placements
      c.unfoldOffAxis = off
    }
    if (arrivals) for (const c of cells) c.unfoldArrival = Math.max(-1, ...c.unfoldRows.map((id) => arrivals.get(id) ?? -1))
    const reply = cells.some((c) => c.unfoldKind === 'unfold')
    const turn = sec.episode !== null && reply ? ++turnNo : null
    if (turn !== null) cards.set(turn, turnCard(turn, cells))
    const card = turn !== null ? cards.get(turn) : null
    const title = card ? (card.icon === 'keyboard' ? 'Typed turn' : card.icon === 'mic' ? 'Voice turn' : 'Reply') : cells.some((c) => c.unfoldKind === 'voice') ? 'Heard (no reply linked)' : 'Between turns'
    turns.push({ turn, unfoldEpisode: sec.episode, unfoldEpisodes: sec.episode === null ? [] : [sec.episode], groups: [{ title, cells }] })
  }
  const all = turns.flatMap((t) => t.groups.flatMap((g) => g.cells))
  const counts = {}
  for (const c of all) if (!c.unfoldQuiet) counts[KINDS[c.unfoldKind].group] = (counts[KINDS[c.unfoldKind].group] ?? 0) + (c.unfoldFolded?.length ?? 1)
  const firsts = all.filter((c) => c.unfoldKind === 'unfold').map((c) => c.unfoldTiming?.find((s) => s.name === 'first text received' && s.ms != null)?.ms).filter((v) => v != null)
  const strip = {
    turns: turnNo,
    avgFirstTextMs: firsts.length ? firsts.reduce((a, b) => a + b, 0) / firsts.length : null,
    firstTextCoverage: `${firsts.length} of ${all.filter((c) => c.unfoldKind === 'unfold').length}`,
    interrupts: all.filter((c) => c.unfoldKind === 'interrupt').length,
    faults: all.filter((c) => c.unfoldKind === 'fault').length,
    tokens: 'not linked yet',
    usageReports: usageReports(rows).length,
  }
  return { turns, cards, lastIndex: n, counts, strip }
}

/** Keep only the records of the enabled filter groups; empty sections drop out. */
export function filterTurns(turns, enabled) {
  const on = (group) => !enabled || enabled.has(group)
  const signals = on('signals')
  const keep = (c) => (c.unfoldFolded ? !signals : c.unfoldQuiet ? signals : on(KINDS[c.unfoldKind].group))
  return turns.map((t) => ({ ...t, groups: t.groups.map((g) => ({ ...g, cells: g.cells.filter(keep) })) })).filter((t) => t.groups.some((g) => g.cells.length))
}

/**
 * Records that arrived after an arrival watermark: counted as distinct source rows of the
 * visible records, by their stream arrival number, never by display position. Folding or unfolding
 * signals does not change it.
 */
export function newSince(turns, seq, arrivals) {
  if (!(arrivals instanceof Map) || !Number.isFinite(seq)) return 0
  const ids = new Set()
  for (const t of turns) for (const g of t.groups) for (const c of g.cells) for (const id of c.unfoldRows ?? []) if ((arrivals.get(id) ?? -1) > seq) ids.add(id)
  return ids.size
}

/** The filter groups on by default: everything but Signals (calm by default). */
export const DEFAULT_GROUPS = ['you', 'unfold', 'voice', 'control', 'faults']

export { LANE }
