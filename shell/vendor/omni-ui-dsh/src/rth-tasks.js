// Delegate tasks in the Bot's Tasks list: a one-way, READ-ONLY projection of rth-tasks ledger
// records. Nothing here writes Bot state (state.agents / state.tasks), so the Bot's resume never
// sees a projected task.
//
// The producer is a dsh-session service named `rthDelegation` exposing `snapshot()` →
// { lifetime, tasks: [{ task_id, rev, lifetime, kind, execution, publication, title,
// child_session_id, report_text, accepted_at, delivered_at, updated_at }] }. `lifetime` is the
// producer's authoritative attachment identity: the current producer is set only there, never by
// whichever row arrives last. Field names follow rth-tasks' TaskState.

/** Every execution state rth-tasks has (ledger.rs `Exec`). */
export const EXECUTION = ['refused', 'accepted', 'output_sent', 'claimed', 'running', 'returned', 'execution_done', 'start_revoked', 'start_uncertain', 'cancel_requested', 'cancelled', 'teardown_unknown', 'unknown', 'settled_exited', 'settled_failed']
/** Every publication state (ledger.rs `Publication`). */
export const PUBLICATION = ['none', 'committed', 'delivered', 'delivery_failed', 'unknown', 'visible_unconfirmed']
/** Coding-agent handoff states (room only; nothing here dispatches a handoff). */
export const HANDOFF = ['proposed', 'approved', 'dispatched', 'running', 'done', 'failed']
/** Card states. `unknown` is its own state: never shown as stopped, never given an end time. */
export const CARD = ['working', 'done', 'failed', 'stopped', 'unknown']
/** An active (non-terminal) projection older than this becomes unknown (freshness lease). */
export const LEASE_MS = 30_000

const R = (label, card, terminal) => ({ label, card, terminal })

/**
 * Exhaustive mapping, explicit precedence: publication first, then execution.
 * `done` only with the ledger's delivered receipt. Anything unrecognized is unknown.
 */
export function mapReport(execution, publication) {
  if (!EXECUTION.includes(execution) || !PUBLICATION.includes(publication)) return R('Status unknown', 'unknown', false)
  switch (publication) {
    case 'delivered': return R('Report delivered', 'done', true)
    case 'delivery_failed': return R('Report could not be posted', 'failed', true)
    case 'visible_unconfirmed': return R('Report visible, not yet confirmed saved', 'unknown', false)
    case 'unknown': return R('Report status unknown', 'unknown', false)
    case 'committed': return R('Posting the report', 'working', false)
  }
  switch (execution) {
    case 'cancel_requested': return R('Cancelling', 'working', false)
    case 'cancelled': return R('Cancelled', 'stopped', true)
    case 'refused': case 'start_revoked': case 'settled_failed': return R('Did not run or failed', 'failed', true)
    case 'teardown_unknown': case 'unknown': case 'start_uncertain': return R('Status unknown', 'unknown', false)
    case 'execution_done': case 'returned': return R('Writing finished, posting pending', 'working', false)
    case 'accepted': case 'output_sent': case 'claimed': case 'running': return R('Writing the report', 'working', false)
    case 'settled_exited': return R('Ended without a report', 'failed', true)
  }
  return R('Status unknown', 'unknown', false)
}

/** Handoff cards: their own mapping (room only). */
export function mapHandoff(state) {
  switch (state) {
    case 'proposed': return R('Waiting for your approval', 'working', false)
    case 'approved': return R('Approved, not yet sent', 'working', false)
    case 'dispatched': return R('Sent to the coding agent', 'working', false)
    case 'running': return R('Coding agent working', 'working', false)
    case 'done': return R('Handoff done', 'done', true)
    case 'failed': return R('Handoff failed', 'failed', true)
  }
  return R('Status unknown', 'unknown', false)
}

export function mapRecord(rec) {
  return rec?.kind === 'handoff' ? mapHandoff(rec.state) : mapReport(rec?.execution, rec?.publication ?? 'none')
}

/** Projected row ids are namespaced, so they can never collide with a Bot session id. */
export const projectedId = taskId => `rth-task:${taskId}`
export const isProjectedId = id => typeof id === 'string' && id.startsWith('rth-task:')

/** A local monotonic clock (never the wall clock, never a producer's timestamp). */
export const monoNow = () => (typeof performance !== 'undefined' ? performance.now() : Number(process.hrtime.bigint() / 1000000n))

const validLifetime = l => typeof l === 'string' && l.length > 0

/**
 * The projection. Authority comes only from a producer snapshot naming its
 * lifetime: a missing, invalid or retired lifetime rejects the WHOLE snapshot before any row is
 * touched. Freshness is local: a row is fresh for LEASE_MS of this process's monotonic clock
 * after the producer last renewed it (a newer revision); repeating the same record renews
 * nothing, and producer timestamps never extend it.
 */
export class Projection {
  constructor() { this.rows = new Map(); this.lifetime = null; this.retired = new Set() }
  /** Accept one snapshot `{ lifetime, tasks }`; false (and nothing changed) when refused. */
  applySnapshot(snap, nowMono = monoNow()) {
    const lifetime = snap?.lifetime
    if (!validLifetime(lifetime) || this.retired.has(lifetime) || !Array.isArray(snap.tasks)) return false
    if (lifetime !== this.lifetime) {
      if (this.lifetime) this.retired.add(this.lifetime)
      this.lifetime = lifetime
    }
    for (const rec of snap.tasks) if (rec?.lifetime === lifetime) this.apply(rec, nowMono)
    return true
  }
  apply(rec, nowMono) {
    if (!rec || typeof rec.task_id !== 'string' || !Number.isFinite(rec.rev) || rec.lifetime !== this.lifetime) return false
    const prev = this.rows.get(rec.task_id)
    // A repeat or reorder renews nothing. At the same rev, only the producer's explicit
    // liveness (`live_seq`: its executor still holds the live activation) renews a row.
    const live = r => (Number.isFinite(r?.live_seq) ? r.live_seq : 0)
    if (prev && prev.lifetime === rec.lifetime && (prev.rev > rec.rev || (prev.rev === rec.rev && live(prev) >= live(rec)))) return false
    this.rows.set(rec.task_id, { ...rec, observed_mono: nowMono })
    return true
  }
  /** View rows for the sidebar and inspector. Pure: never mutates Bot state. */
  view(nowMono = monoNow()) {
    return [...this.rows.values()].sort((a, b) => b.observed_mono - a.observed_mono).map(rec => {
      let m = mapRecord(rec)
      const current = this.lifetime !== null && rec.lifetime === this.lifetime
      const remaining = rec.observed_mono + LEASE_MS - nowMono
      if (!m.terminal && (!current || remaining < 0)) m = R('Status unknown (no recent update)', 'unknown', false)
      return {
        id: projectedId(rec.task_id),
        origin: 'rth-delegate',
        kind: rec.kind ?? 'report',
        task_id: rec.task_id,
        title: rec.title ?? 'Report',
        label: m.label,
        card: m.card,
        terminal: m.terminal,
        // The observation this answer describes, and how long it stays fresh from now: the
        // browser turns this into its own monotonic deadline.
        obs: `${rec.lifetime}:${rec.rev}${Number.isFinite(rec.live_seq) && rec.live_seq > 0 ? `.${rec.live_seq}` : ''}`,
        lease_remaining_ms: m.terminal || m.card === 'unknown' ? 0 : Math.max(0, Math.floor(remaining)),
        // Only a delivered report has an end time; unknown never fabricates one.
        ended_at: m.card === 'done' ? rec.delivered_at ?? null : null,
        started_at: m.card === 'unknown' ? null : rec.accepted_at ?? null,
        child_session_id: rec.child_session_id ?? null,
        report_text: m.card === 'done' ? rec.report_text ?? null : null,
        detail: { execution: rec.execution ?? null, publication: rec.publication ?? null, state: rec.state ?? null, rev: rec.rev, lifetime: rec.lifetime ?? null },
      }
    })
  }
}
