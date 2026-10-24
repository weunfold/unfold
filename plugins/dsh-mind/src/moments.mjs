// Mind, the simple view: the observed rows and episodes (src/mind.mjs, unchanged) read as a few
// plain moments, like a conversation. What was heard sits on the right, what RTH replied on the
// left, and small things (a note, mute, voice waking) are quiet lines. Everything else stays one
// expand away (the steps, then each step's audit record). Pure: tested in node, bundled as is.
//
// Honesty, carried over from the sentences (src/sentences.mjs) and the freshness rule
// (src/display.mjs):
// - a transcript is what the provider transcribed, never "you said" (speaker not identified);
// - a speech candidate (local VAD onset) is not shown as anyone speaking;
// - "Replying…" only while a reply was seen to start and not to end; a decision with no reply yet
//   says what was decided, never "thinking", and never that the chosen work ran;
// - moments are grouped only by Mind's own episodes (explicit shared ids), never by similar text;
// - time passing never turns an unresolved reply or decision into "Quiet": it becomes "not sure";
// - when the panel is not live, nothing open is shown as still happening: it is "not sure";
// - text comes only from the redacted Mind sentence (the quoted part), never from raw fields.

/** The quoted part of a Mind sentence (“…”), or null. */
export function quoted(sentence) {
  if (typeof sentence !== 'string') return null
  const a = sentence.indexOf('“')
  const b = sentence.lastIndexOf('”')
  return a >= 0 && b > a ? sentence.slice(a + 1, b) : null
}

/**
 * The reply id a start or an end row names, read from the plugin's own sentence templates
 * (src/sentences.mjs: "Started reply <id> …", "Reply <id> ended: …", and the host's "Host saw reply <id>
 * end: …" / "Host marked reply <id> cancelled"); null when the
 * event carried none ("unknown"), which never matches anything.
 */
export function replyIdOf(row) {
  const re = { response_started: /^Started reply (\S+)/, agent_done: /^Reply (\S+) ended/, response_done: /^Host saw reply (\S+) end:/, response_cancelled: /^Host marked reply (\S+) cancelled/ }[row.kind]
  const m = re ? re.exec(row.sentence ?? '') : null
  return m && m[1] !== 'unknown' ? m[1] : null
}

/** A provider usage report (`gemini usage gen=<g> total_tokens=<n>`), from the Mind sentence; or null. */
export function usageOf(row) {
  if (row?.kind !== 'log') return null
  const m = /^gemini usage gen=(\d+) total_tokens=(\d+)\b/.exec(quoted(row.sentence) ?? '')
  return m ? { gen: Number(m[1]), total: Number(m[2]) } : null
}

/** How many gap marks (events not seen on this feed) the rows hold. */
export const gapsIn = (rows) => rows.filter((r) => r.status === 'gap' && r.kind === 'seq_gap').length

const DECIDED = { look_up: 'look something up first', wait: 'wait', respond: 'reply' }

/** What a decision row chose, from its sentence ("Decided to <selected> (of …) by …"). */
export function decidedTo(sentence) {
  const m = /^Decided to (\S+)/.exec(sentence ?? '')
  return m ? m[1] : null
}

/** A loose row worth one quiet line, or null (most single rows are steps, not moments). */
export function quietLine(row) {
  const s = row.sentence ?? ''
  if (row.kind === 'link_lost') return { text: 'Lost the connection to the agent. Anything open is now unsure.', tone: 'warn' }
  if (row.kind === 'link_resumed') return { text: "Connected again. What happened meanwhile isn't shown.", tone: 'gap' }
  if (row.kind === 'heartbeat_stale') return { text: 'The agent went quiet (no heartbeat)', tone: 'warn' }
  if (row.kind === 'heartbeat_resumed') return { text: 'The agent is answering again', tone: 'faint' }
  // A host journal discontinuity: host timings across it are never compared; say so once.
  if (/^journal_(replaced|shrank|rewritten|missing)$/.test(row.kind) && /^Host journal/.test(s)) return { text: "Unfold's host log changed files: its timings before and after aren't compared", tone: 'gap' }
  if (row.status === 'gap') return null // counted under More (gapsIn), not a line in the conversation
  switch (row.kind) {
    case 'log': {
      // Raw agent log text never reaches the participant view (usage lines would read as quoted
      // thoughts): usage becomes metrics (usageOf), readiness a plain line, a warning or
      // error one plain line (its text one expand away), everything else stays in Every event.
      const note = quoted(s)
      if (!note || usageOf(row)) return null
      if (/^gemini ready\b/.test(note)) return { text: 'Voice model is ready', tone: 'faint' }
      const level = /^Agent note \((\w+)\)/.exec(s)?.[1]
      if (level === 'error') return { text: 'Unfold reported an error', tone: 'bad' }
      if (level === 'warn') return { text: 'Unfold reported a warning', tone: 'warn' }
      return null
    }
    case 'mute_applied': return { text: /muted at the host/.test(s) && !/unmuted/.test(s) ? 'Microphone muted' : 'Microphone unmuted', tone: 'faint' }
    case 'call_devices_opened': return { text: 'Voice woke up', tone: 'faint' }
    case 'call_devices_released': return { text: 'Voice went to sleep', tone: 'faint' }
    case 'link_up': return row.lane === 'host' ? { text: 'Connected to the agent', tone: 'faint' } : null
    case 'link_down': return { text: 'Lost the connection to the agent', tone: 'warn' }
    case 'journal_fault': return { text: "Couldn't save history", tone: 'bad' }
    case 'command_rejected': return { text: 'A message was refused', tone: 'bad' }
    case 'outbox_fault': return { text: "Couldn't pass something on to the agent", tone: 'bad' }
    case 'rejected': return { text: 'The agent refused something', tone: 'bad' }
    case 'speaker_cleared': return { text: 'Stopped speaking', tone: 'faint' }
    default: return null
  }
}

const OPEN = new Set(['pending', 'attempted'])
/** The only outcomes that settle a reply or a message. Anything else (pending, unknown, observed) stays uncertain. */
const TERMINAL = new Set(['completed', 'stopped', 'failed'])

/**
 * @param {object[]} rows public rows (and local gap marks), any order
 * @param {Record<string, object>} episodes episode views by id
 * @param {{live: boolean}} opts live: the panel's stream and the agent feed are both live
 * @returns {object[]} moments, oldest first: {key, type: 'heard'|'reply'|'line', at, ...}
 */
export function moments(rows, episodes, { live = true } = {}) {
  const sorted = [...rows].sort((a, b) => a.observed_at - b.observed_at)
  const byEp = new Map()
  for (const r of sorted) {
    if (!r.episode || r.status === 'gap') continue
    if (!byEp.has(r.episode)) byEp.set(r.episode, [])
    byEp.get(r.episode).push(r)
  }
  const out = []
  const seenEp = new Set()
  for (const r of sorted) {
    if (r.episode && r.status !== 'gap') {
      if (seenEp.has(r.episode)) continue
      seenEp.add(r.episode)
      out.push(...episodeMoments(episodes[r.episode] ?? null, byEp.get(r.episode), live))
      continue
    }
    // A transcript with no episode is its own moment: never joined to another by its text.
    if (r.kind === 'input_transcript' && r.lane === 'agentd') {
      const text = quoted(r.sentence)
      if (text) out.push({ key: `h:${r.id}`, type: 'heard', at: r.observed_at, last_at: r.observed_at, text, final: /\(final;/.test(r.sentence), episode: null, rows: [r.id], status: r.status })
      continue
    }
    const q = quietLine(r)
    if (!q) continue
    const prev = out.at(-1)
    if (prev?.type === 'line' && prev.text === q.text) {
      prev.count += 1
      prev.at = r.observed_at
      prev.rows.push(r.id)
      continue
    }
    out.push({ key: `l:${r.id}`, type: 'line', at: r.observed_at, text: q.text, tone: q.tone, note: Boolean(q.note), count: 1, rows: [r.id], row: r })
  }
  return out.sort((a, b) => a.at - b.at)
}

function episodeMoments(ep, rows, live) {
  if (!rows?.length) return []
  const out = []
  const transcripts = rows.filter((r) => r.kind === 'input_transcript' && r.lane === 'agentd')
  const typed = rows.find((r) => ['command_written', 'command_admitted', 'command_submitted', 'user_text'].includes(r.kind))
  const delta = rows.filter((r) => r.kind === 'agent_delta').at(-1)
  const started = rows.find((r) => r.kind === 'response_started')
  const decision = rows.find((r) => r.kind === 'decision')
  // Each reply settles only by its OWN end (matched response id); an end with no observed start
  // settles nothing else.
  const startedIds = rows.filter((r) => r.kind === 'response_started' && r.lane === 'agentd').map(replyIdOf)
  const endedIds = new Set(rows.filter((r) => ['agent_done', 'response_done', 'response_cancelled'].includes(r.kind) && TERMINAL.has(r.status)).map(replyIdOf).filter(Boolean))
  const unended = startedIds.some((rid) => !rid || !endedIds.has(rid))
  const spoken = rows.some((r) => r.kind === 'playback' && r.lane === 'host' && r.status === 'completed')
  const status = !live && ep && OPEN.has(ep.status) ? 'unknown' : (ep?.status ?? 'unknown')
  const ids = rows.map((r) => r.id)
  const id = ep?.id ?? rows[0].episode
  if (transcripts.length) {
    const last = transcripts.at(-1)
    const text = quoted(last.sentence)
    if (text) out.push({ key: `h:${id}`, type: 'heard', at: transcripts[0].observed_at, last_at: last.observed_at, text, final: /\(final;/.test(last.sentence), episode: id, rows: ids, status })
  }
  const typedText = typed ? quoted(typed.sentence) : null
  if (typed && typedText && typed.kind === 'user_text') out.push({ key: `t:${id}`, type: 'heard', typed: true, at: typed.observed_at, last_at: typed.observed_at, text: typedText, final: true, episode: id, rows: ids, status })
  if (delta || started) {
    const to = decision ? decidedTo(decision.sentence) : null
    if (to && to !== 'respond') out.push({ key: `d:${id}`, type: 'line', at: decision.observed_at, text: `Chose to ${DECIDED[to] ?? to.replace(/_/g, ' ')}`, tone: 'faint', count: 1, episode: id, rows: ids, decided: to, open: false })
    const anchor = started ?? delta
    out.push({
      key: `r:${id}`,
      type: 'reply',
      at: anchor.observed_at,
      last_at: rows.at(-1).observed_at,
      text: delta ? quoted(delta.sentence) : null,
      status,
      open: OPEN.has(status), // seen in progress, and the panel is live: may animate
      // No outcome seen: never quiet, whatever the time or the link. Also uncertain while any reply
      // started in this episode has not seen its own end (an episode joined by a shared id can hold
      // several; the episode's outcome may be another one's). This only adds doubt, never removes it.
      uncertain: !TERMINAL.has(ep?.status) || unended,
      spoken,
      toTyped: Boolean(typed),
      decided: decision ? decidedTo(decision.sentence) : null,
      duration_ms: ep?.duration_ms ?? null,
      episode: id,
      rows: ids,
    })
  } else if (decision) {
    // The episode's own status here describes the decision, not an outcome: only an outcome row
    // (an end, a stop, a refusal, a finished command) settles what was decided.
    const ended = rows.some((r) => ['agent_done', 'response_done', 'response_cancelled', 'command_finished', 'command_rejected'].includes(r.kind) && TERMINAL.has(r.status))
    const to = decidedTo(decision.sentence)
    // A decision alone, respond included, stays unresolved until a reply or an outcome shows up
    // A decision to wait is complete in itself.
    if (to) out.push({ key: `d:${id}`, type: 'line', at: decision.observed_at, text: `Chose to ${DECIDED[to] ?? to.replace(/_/g, ' ')}`, tone: 'faint', count: 1, episode: id, rows: ids, decided: to, open: live && to !== 'wait' && !ended, uncertain: to !== 'wait' && !ended })
  } else if (typed) {
    out.push({ key: `c:${id}`, type: 'line', at: typed.observed_at, text: TERMINAL.has(ep?.status) ? 'A typed message' : 'Got a typed message, no reply yet', tone: 'faint', count: 1, episode: id, rows: ids, open: OPEN.has(status), uncertain: !TERMINAL.has(ep?.status) })
  } else if (!transcripts.length) {
    // An episode of system steps: one quiet line if any step deserves one.
    for (const r of rows) {
      const q = quietLine(r)
      if (q) {
        out.push({ key: `e:${id}`, type: 'line', at: r.observed_at, text: q.text, tone: q.tone, count: 1, episode: id, rows: ids, row: r })
        break
      }
    }
  }
  return out
}

/** How long an open reply may say "Replying…" before the line admits it has not seen the end. */
export const REPLY_FRESH_MS = 90_000
/** How long a decision with no reply yet is shown as just made. */
export const DECISION_FRESH_MS = 60_000

/**
 * Uncertain: a reply, a decision (other than to wait) or a typed message with no outcome seen,
 * whether it is still in progress, its outcome was lost when the link dropped (status unknown), or
 * the panel is not live. Only an outcome row settles one; time and a fresh link never do.
 * Animation is separate: only `open` moments that are still fresh animate.
 */
const uncertain = (m) => Boolean(m.uncertain) && (m.type === 'reply' || m.type === 'line')

/**
 * The one line at the top: what RTH is doing now, only from what the feed shows, or "not sure".
 * "Quiet" only when nothing is open or uncertain.
 * @param {object[]} ms moments (oldest first)
 * @param {{stream: {state, label}, agent: {state}}} fresh src/display.mjs freshness()
 * @param {number} now server-clock ms
 */
export function nowLine(ms, fresh, now) {
  if (fresh.stream.state !== 'live') return { text: /: connecting$/.test(fresh.stream.label) ? 'Connecting…' : 'Out of touch, reconnecting', tone: 'faint', state: 'unknown' }
  if (fresh.agent.state === 'down') return { text: "The agent isn't connected", tone: 'warn', state: 'unknown' }
  if (fresh.agent.state !== 'live') return { text: "Not sure what Unfold is doing (the agent's heartbeat is late)", tone: 'faint', state: 'unknown' }
  const pending = ms.filter(uncertain)
  const reply = pending.filter((m) => m.type === 'reply').at(-1)
  if (reply?.open && now - (reply.last_at ?? reply.at) < REPLY_FRESH_MS) return { text: 'Replying…', tone: 'accent', state: 'busy', pulse: true }
  const hearing = ms.filter((m) => m.type === 'heard' && !m.final && !m.typed && now - m.last_at < 4_000).at(-1)
  if (hearing) return { text: 'Transcribing…', tone: 'accent', state: 'busy', pulse: true }
  const line = pending.filter((m) => m.type === 'line').at(-1)
  if (line?.open && now - line.at < DECISION_FRESH_MS && (!reply || line.at > reply.at)) return { text: `${what(line)}, no reply yet`, tone: 'accent', state: 'busy', pulse: true }
  const last = pending.at(-1)
  if (last?.type === 'reply') return { text: 'Not sure how the last reply ended (no end seen)', tone: 'warn', state: 'unknown', stale: true, since: last.last_at ?? last.at }
  if (last) return { text: `${what(last)}, no reply seen since`, tone: 'warn', state: 'unknown', stale: true, since: last.at }
  const lastReply = ms.filter((m) => m.type === 'reply').at(-1)
  return { text: 'Quiet', tone: 'muted', state: 'idle', since: lastReply ? lastReply.last_at : null }
}
const what = (m) => (m.key.startsWith('c:') ? 'Got your message' : `Chose to ${DECIDED[m.decided] ?? String(m.decided).replace(/_/g, ' ')}`)

/**
 * The small caption under a moment. Only what its own rows show: an outcome comes from an
 * outcome row, never from a decision (a chosen lookup is not a lookup that ran).
 * It makes NO timing or playback claim: those come only
 * from the qualified activity result (metrics.mjs replyCaption). The moment's episode duration
 * (first to last retained row) and its raw playback flag stay diagnostics, never a caption.
 */
export function captionOf(m, now = null) {
  if (m.type === 'heard') return m.typed ? 'typed' : m.final ? 'heard' : 'heard (not final)'
  if (m.type !== 'reply') return ''
  const bits = []
  if (m.open && now != null && now - (m.last_at ?? m.at) >= REPLY_FRESH_MS) bits.push('no end seen yet')
  else if (m.open) bits.push(m.text ? 'replying' : 'starting a reply')
  else if (m.status === 'stopped') bits.push('stopped')
  else if (m.status === 'failed') bits.push("didn't finish")
  else if (m.uncertain || m.status === 'unknown') bits.push('not sure how it ended')
  if (m.toTyped) bits.push('to a typed message')
  return bits.join(' · ')
}
