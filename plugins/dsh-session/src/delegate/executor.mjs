// The delegate executor (text delivery only; there is no spoken summary). It runs inside
// rth-dsh-session. agentd's ledger is the authority: this module only reports, through the host
// owner, what it observed or did, and acts only on records agentd committed (claim before start;
// publication commit before any append).
//
// Every dependency is injected, so the whole chain runs on scripted fakes with zero spend:
//   postTask(taskId, update) → {status, body: task_state record}    (POST /v1/tasks/<id>)
//   postExecutor(status)     → {status, body}                        (POST /v1/executor)
//   spawnChild({childId, prompt, signal, hooks}) → activation {result: Promise, dispose()}
//   snapshotSource()         → TranscriptJournal.freeze(): journal ids, never chat adjacency
//   offerTaskItem({key, taskId})                                    (queue the rth-root task turn)
//   readBack({floor, key})   → {state: 'ok', rows: [{seq, digest}], lineage} | {state: 'error'}
//   pending                  → PendingStore (the per-task recovery file)
//
// Invariants:
// - Every continuation that can start work is fenced against the current local lifecycle
//   (teardown, disposal) and the newest committed record, never against the answer it awaited;
//   a stale answer never replaces a newer record.
// - Delivery needs this turn's own append, a flush that a durability listener answered,
//   and a fresh read-back of that exact seq. Bytes this turn did not append are delivered only on
//   a valid durable receipt already covering them; otherwise they stay visible_unconfirmed.
// - An update keeps its exact identity, sequence and payload across unknown outcomes: it is
//   resent identically (the owner answers a replay with the original record) and is also answered
//   by the committed stream record it caused. Pending publications are reconciled from the
//   plugin's bootstrap and every observer attach (`reconcile`), and a publication left `unknown`
//   by an exit receipt is reconciled from its stream record.
import { buildSnapshot, workerPrompt } from './snapshot.mjs'
import { validateResult } from './validate.mjs'
import { attemptBoundUsd, DEFAULTS } from './budget.mjs'
import { sha256 } from './canon.mjs'

export const TASK_KEY = 'rth-task-'
export const LEASE_MS = 15_000
export const BEAT_MS = 5_000
export const RUN_MS = 90_000
export const TEARDOWN_MS = 10_000
/** Backoff between identical resends of an update whose answer was lost (504, 503, transport). */
export const RETRY_MS = [250, 1_000, 2_000, 4_000, 8_000]

const same = (a, b) => !!a && !!b && a.launch_id === b.launch_id && a.pid === b.pid && a.plugin_instance === b.plugin_instance
/** An answer that carries the committed record: applied, or the original record of a replay. */
const answered = (rec) => rec?.outcome === 'applied' || rec?.outcome === 'duplicate'
/** Execution states in which this instance must never start or continue work. */
const FENCED = new Set(['start_revoked', 'cancel_requested', 'cancelled', 'teardown_unknown', 'settled_failed', 'settled_exited'])
/** The committed record `ref` (its `update` field) was caused by our update `u`. */
const causedBy = (ref, u) =>
  !!ref && ref.kind === u.kind?.kind && same(ref.incarnation, u.incarnation) &&
  (ref.seq ?? null) === (u.seq ?? null) && (ref.pub_epoch ?? null) === (u.pub_epoch ?? null) && (ref.request_id ?? null) === (u.request_id ?? null)

export class DelegateExecutor {
  constructor(deps) {
    this.d = deps
    this.executorId = deps.executorId ?? 'rth-dsh-session'
    this.incarnation = deps.incarnation
    this.clock = deps.clock ?? (() => performance.now())
    this.setTimer = deps.setTimer ?? ((f, ms) => { const t = setTimeout(f, ms); t.unref?.(); return t })
    this.clearTimer = deps.clearTimer ?? clearTimeout
    this.limits = { ...DEFAULTS, ...(deps.limits ?? {}) }
    this.offerTaskItem = deps.offerTaskItem
    this.tasks = new Map()
    this.beatSeq = 0
    this.reqSeq = 0
    this.disposed = false
    this.reconciling = null
    // The last executor failures, also on the plugin's local status route (DSH's logger may only buffer).
    this.recent = []
    this.log = (m) => {
      this.recent.push({ at: new Date().toISOString(), m: String(m).slice(0, 500) })
      if (this.recent.length > 20) this.recent.shift()
      deps.log?.(m)
    }
    this.stats = { claims: 0, runs: 0, returned: 0, failed: 0, delivered: 0, delivery_failed: 0, visible_unconfirmed: 0, teardown_unknown: 0, cancelled: 0, refused_receipts: 0, unknown_answers: 0, resends: 0, stream_answers: 0, reconciled: 0 }
  }

  /** The admitted child sessions (for the pre-step gate and the model-traffic counters). */
  admittedChild(sessionId) {
    for (const t of this.tasks.values()) if (t.childId === sessionId && t.running && !this.fenced(t)) return t.taskId
    return null
  }

  /**
   * The lease beat. `inventory`: ask agentd for its authoritative records, answered as
   * ordinary stream records. Each beat also renews `liveSeq` for the tasks whose live activation
   * this instance holds: the Tasks view's explicit liveness, never a new ledger revision.
   */
  async heartbeat(available = true, { inventory = false } = {}) {
    const seq = ++this.beatSeq
    for (const t of this.tasks.values()) if (t.running && t.activation && !this.fenced(t)) t.liveSeq = seq
    return this.d.postExecutor({ executor_id: this.executorId, incarnation: this.incarnation, seq, available: available && !this.disposed, lease_ms: LEASE_MS, ...(inventory ? { inventory: true } : {}) })
  }

  /** @param observedUnder the producer lifetime the record was observed under (default: now). */
  task(rec, observedUnder = this.lifetimeOf?.() ?? null) {
    let t = this.tasks.get(rec.task_id)
    if (!t) {
      t = { taskId: rec.task_id, childId: rec.child_id, args: rec.args, seq: 0, pubSeq: new Map(), state: null, view: null, inflight: new Set() }
      this.tasks.set(rec.task_id, t)
    }
    this.accept(t, rec, observedUnder)
    return t
  }

  /**
   * Keep the newest committed record: a replay's original or a late answer never regresses it.
   * Two authorities: `t.state` drives the executor's own operations, and any answer to
   * them may advance it; `t.view` is what the Tasks view shows, and only a record observed under
   * the CURRENT producer lifetime may change it (the lifetime captured when the request was
   * issued, or when the stream record arrived; never the one current when an answer lands).
   */
  accept(t, rec, observedUnder = this.lifetimeOf?.() ?? null) {
    if (!rec?.task_id || rec.outcome === 'refused') return
    if (!t.state || (rec.rev ?? 0) >= (t.state.rev ?? 0)) {
      t.state = rec
      t.childId ??= rec.child_id
      t.args ??= rec.args
    }
    const current = this.lifetimeOf?.() ?? null
    if (observedUnder !== current) return
    if (!t.view || t.view.lifetime !== current || (rec.rev ?? 0) >= (t.view.rec.rev ?? 0)) t.view = { rec, lifetime: current }
  }

  /** No start and no continuation once teardown, disposal or a settling record is seen. */
  fenced(t) {
    return this.disposed || !!t.tearing || !!t.disposed || FENCED.has(t.state?.execution) || !!t.state?.abandoned
  }

  async post(t, kind, { pubEpoch, requestId } = {}) {
    const update = { task_id: t.taskId, executor_id: this.executorId, incarnation: this.incarnation, kind }
    if (pubEpoch != null) {
      const n = (t.pubSeq.get(pubEpoch) ?? 0) + 1
      t.pubSeq.set(pubEpoch, n)
      Object.assign(update, { pub_epoch: pubEpoch, seq: n })
    } else if (requestId) {
      update.request_id = requestId
    } else {
      update.seq = ++t.seq
    }
    return this.send(t, update)
  }

  /**
   * One update to its committed answer. The same object is resent on an unknown outcome,
   * and the stream record it caused answers it as well. `null` after the bounded resends: the
   * outcome stays unknown, and the stream or the next reconciliation decides.
   */
  async send(t, update) {
    // The lifetime this request is issued under: its answer is evidence for that lifetime only.
    const issuedUnder = this.lifetimeOf?.() ?? null
    const w = { update, done: false, rec: null }
    const streamed = new Promise((res) => { w.resolve = res })
    t.inflight.add(w)
    try {
      for (let attempt = 0; ; attempt++) {
        if (w.done) return w.rec
        let r
        try {
          r = await this.d.postTask(t.taskId, update)
        } catch (e) {
          r = { status: 0, body: { error: String(e?.message ?? e), retry: true } }
        }
        if (w.done) return w.rec
        const rec = r?.body?.record ?? null
        if (rec) {
          if (rec.outcome === 'refused') this.stats.refused_receipts++
          this.accept(t, rec, issuedUnder)
          return rec
        }
        const unknown = !r || r.status === 0 || r.status === 503 || r.status === 504 || r.body?.retry === true
        if (!unknown || attempt >= RETRY_MS.length || this.disposed) {
          if (unknown) { this.stats.unknown_answers++; t.unresolved = true }
          else this.stats.refused_receipts++
          return null
        }
        this.stats.resends++
        await new Promise((res) => {
          const timer = this.setTimer(res, RETRY_MS[attempt])
          void streamed.then(() => { this.clearTimer(timer); res() })
        })
      }
    } finally {
      t.inflight.delete(w)
    }
  }

  /** A committed record from agentd's stream (or a receipt): act only on what is committed. */
  /** @param observedUnder the lifetime the record arrived under (the caller's, captured at ingest). */
  async onTaskState(rec, observedUnder = this.lifetimeOf?.() ?? null) {
    if (!rec?.task_id) return
    const t = this.task(rec, observedUnder)
    // The record caused by one of our updates answers it, even if its HTTP answer was lost.
    for (const w of t.inflight) if (!w.done && causedBy(rec.update, w.update)) { w.done = true; w.rec = rec; this.stats.stream_answers++; w.resolve(rec) }
    if (rec.outcome === 'refused' || this.disposed) return
    const mine = same(rec.assignment?.incarnation, this.incarnation)
    if (mine && rec.execution === 'output_sent' && !t.claimSent) return this.claim(t)
    // Our claim committed but its answer was lost after the resends: the stream starts it once.
    if (mine && rec.execution === 'claimed' && t.claimUpdate && causedBy(rec.update, t.claimUpdate)) this.startRun(t)
    // A committed cancel fences a pending start as well as a live activation.
    if (mine && rec.execution === 'cancel_requested' && t.claimSent && !t.tearing && !t.committed) {
      void this.teardown(t, rec.cancel ?? 'agentd').catch((e) => this.log(`delegate: teardown ${t.taskId}: ${e?.message ?? e}`))
    }
    // A publication an exit receipt left unknown, owned by another launch.
    if (rec.publication === 'unknown' && rec.pub_owner && rec.pub_owner.launch_id !== this.incarnation.launch_id) {
      void this.reconcileUnknown(rec).catch((e) => this.log(`delegate: reconcile ${t.taskId}: ${e?.message ?? e}`))
    }
  }

  async claim(t) {
    t.claimSent = true
    // The deadline anchor is the FIRST claim's send time: never reset.
    t.anchor = this.clock()
    t.claimUpdate = { task_id: t.taskId, executor_id: this.executorId, incarnation: this.incarnation, kind: { kind: 'claim' }, seq: ++t.seq }
    const rec = await this.send(t, t.claimUpdate)
    if (answered(rec) && rec.execution === 'claimed') this.startRun(t)
  }

  /** Arm the deadline and start the run, at most once per task, never when fenced. */
  startRun(t) {
    if (t.runStarted || this.fenced(t)) return
    t.runStarted = true
    this.stats.claims++
    t.deadline = this.setTimer(() => { if (!t.committed) void this.teardown(t, 'deadline') }, Math.max(0, t.anchor + RUN_MS - this.clock()))
    // The run is not awaited here: a record handler never blocks on a child's lifetime.
    t.done = this.run(t).catch((e) => this.log(`delegate: run ${t.taskId}: ${e?.message ?? e}`))
  }

  async run(t) {
    this.stats.runs++
    const snap = buildSnapshot(this.d.snapshotSource(), { taskId: t.taskId, recipient: this.d.recipient })
    if (this.d.grant && (this.d.grant.provider !== this.d.recipient?.provider || this.d.grant.model !== this.d.recipient?.model)) {
      await this.post(t, { kind: 'failed', reason: 'grant_mismatch' })
      return
    }
    t.snapshot = { sha256: snap.sha256, items: snap.items.length, omissions: snap.omissions }
    if (this.fenced(t)) return
    const run = await this.post(t, { kind: 'running', child_id: t.childId, input_sha256: snap.sha256 })
    if (!answered(run) || run.execution !== 'running') return
    // The answer may arrive after a deadline, a cancel or disposal: the current local
    // lifecycle decides, never the receipt. This check and the spawn below are synchronous.
    if (this.fenced(t)) return
    t.running = true
    const prompt = workerPrompt(snap, { task: t.args?.task ?? '', audience: t.args?.audience ?? 'user' })
    const promptBytes = Buffer.byteLength(prompt)
    const ac = new AbortController()
    t.abort = ac
    let steps = 0
    const hooks = {
      beforeStep: () => {
        if (this.fenced(t)) return { ok: false, reason: 'fenced' }
        steps++
        if (steps > this.limits.maxSteps) return { ok: false, reason: 'steps' }
        const soft = this.d.spend.mayStart(t.taskId)
        if (!soft.ok || !this.d.accept) return soft
        // In an acceptance run the step's provider request is admitted only once its attempt is
        // claimed in the acceptance's worker ledger.
        const a = this.d.accept.admit({ attemptId: `${this.incarnation.launch_id}:${t.taskId}:${steps}`, fields: { task_id: t.taskId, step: steps } })
        if (!a.ok) {
          this.log(`delegate: ${t.taskId} step ${steps} refused: ${a.reason}`)
          return { ok: false, reason: 'spend' }
        }
        t.attempt = a.attempt
        return soft
      },
      afterStep: (usage, requestBytes) => {
        if (this.d.accept) {
          // The hard ledger first. A replayed usage event (no attempt open, one already
          // settled) re-settles its own attempt: identical is idempotent across this whole hook
          // (no second soft charge, no budget cancel); changed fails. Usage with no
          // attempt at all is unbound. A failure has already written the stop marker
          // (accept-ledger.mjs) before this teardown.
          const replay = !t.attempt && !!t.settledAttempt
          const attempt = t.attempt ?? t.settledAttempt
          t.attempt = null
          if (attempt) t.settledAttempt = attempt
          const s = attempt ? this.d.accept.settle(attempt, { usage }) : this.d.accept.fail(`spend: worker usage for ${t.taskId} with no claimed attempt`)
          if (!s.ok) {
            this.log(`delegate: ${t.taskId}: ${s.reason}`)
            t.overBudget = 'spend'
            void this.teardown(t, 'budget')
            return { usd: 0, estimated: false, over: 'spend' }
          }
          if (replay) return t.lastCharge ?? { usd: 0, estimated: false, over: null }
        }
        const bound = attemptBoundUsd(requestBytes ?? promptBytes, this.d.spend.prices, this.limits.maxOutputTokens) * this.limits.attemptsPerStep
        const c = this.d.spend.charge(t.taskId, steps, usage, bound)
        t.lastCharge = c
        if (c.over) { t.overBudget = c.over; void this.teardown(t, 'budget') }
        return c
      },
    }
    t.hooks = hooks
    let out
    try {
      t.activation = this.d.spawnChild({ childId: t.childId, prompt, signal: ac.signal, hooks })
      out = await t.activation.result
    } catch (e) {
      out = { error: String(e?.message ?? e) }
    }
    t.settled = true
    t.running = false
    if (t.tearing || this.disposed || t.disposed) return
    if (out?.error) {
      this.stats.failed++
      this.log(`delegate: worker ${t.taskId} failed: ${out.error}`)
      await this.post(t, { kind: 'failed', reason: `worker_error` })
      return
    }
    const v = validateResult(out)
    if (!v.ok) {
      this.stats.failed++
      await this.post(t, { kind: 'failed', reason: `invalid_output:${v.reason}` })
      return
    }
    this.stats.returned++
    t.result = v
    const r = await this.post(t, { kind: 'returned', result_sha256: v.result_sha256, chars: v.report.length })
    if (!answered(r) || r.execution !== 'returned' || this.fenced(t)) return
    await this.post(t, { kind: 'execution_done' })
    if (this.fenced(t)) return
    // The recovery intent is durable BEFORE the publish can commit, so no committed
    // publication exists without a pending file, even if this process dies on the commit's answer
    // (the stream has no replay, and the exit receipt precedes the next observer). Its null floor
    // still proves no append began; `pub_epoch: null` marks it as not yet confirmed committed.
    const intent = { task_id: t.taskId, text: v.report, result_sha256: v.result_sha256, summary: v.summary, owner: this.incarnation, pub_epoch: null, seq_floor: null, durable_receipt: null }
    this.d.pending.write(intent)
    const c = await this.post(t, { kind: 'publish_request', result_sha256: v.result_sha256 })
    if (!answered(c) || c.publication !== 'committed' || !same(c.pub_owner, this.incarnation)) {
      // Refused with no publication: this request can never commit. An unknown answer keeps the
      // intent for reconciliation.
      if (c?.outcome === 'refused' && c.publication === 'none') this.d.pending.unlink(t.taskId)
      return
    }
    t.committed = true
    this.clearTimer(t.deadline)
    this.d.pending.write({ ...intent, pub_epoch: c.pub_epoch })
    this.offerTaskItem?.({ key: `${TASK_KEY}${t.taskId}`, taskId: t.taskId })
  }

  /** Cancel intent → exact teardown evidence, or teardown_unknown after 10 s. */
  teardown(t, by) {
    if (t.tearing || t.committed) return t.teardownDone ?? Promise.resolve()
    t.tearing = true
    t.teardownDone = this.#teardown(t, by)
    return t.teardownDone
  }

  async #teardown(t, by) {
    this.clearTimer(t.deadline)
    t.abort?.abort(new Error(`cancelled: ${by}`))
    try { t.activation?.dispose?.() } catch {}
    // With no activation, nothing started and nothing can start any more (the fence is set
    // above, synchronously, and every start checks it synchronously): that is exact evidence.
    const settled = t.activation ? t.activation.result.then(() => true, () => true) : Promise.resolve(true)
    const timeout = new Promise((res) => this.setTimer(() => res(false), TEARDOWN_MS))
    const ok = await Promise.race([settled, timeout])
    if (by === 'budget' || by === 'deadline') {
      // agentd may already have committed the cancel itself (deadline); a refused failed is fine.
      if (ok) {
        const r = await this.post(t, { kind: 'failed', reason: by === 'budget' ? t.overBudget ?? 'budget' : 'deadline' })
        if (r?.outcome === 'refused') await this.post(t, { kind: 'cancelled' })
      } else {
        this.stats.teardown_unknown++
        await this.post(t, { kind: 'teardown_unknown' })
      }
      return
    }
    if (ok) {
      this.stats.cancelled++
      await this.post(t, { kind: 'cancelled' })
    } else {
      this.stats.teardown_unknown++
      await this.post(t, { kind: 'teardown_unknown' })
    }
  }

  /**
   * The plugin scope is going away (unload or reload): fence every task, abort and dispose
   * the activations this instance holds. It settles nothing and posts nothing.
   */
  dispose() {
    this.disposed = true
    for (const t of this.tasks.values()) {
      t.disposed = true
      this.clearTimer(t.deadline)
      t.abort?.abort(new Error('disposed'))
      try { t.activation?.dispose?.() } catch {}
    }
  }

  /** The live seams: rth-dsh-session's child pre-step and the child's step usage call these. */
  beforeChildStep(taskId) {
    return this.tasks.get(taskId)?.hooks?.beforeStep() ?? { ok: false, reason: 'not_running' }
  }

  afterChildStep(taskId, usage, requestBytes) {
    return this.tasks.get(taskId)?.hooks?.afterStep(usage, requestBytes) ?? null
  }

  /** The UI's "Cancelar tarefa" (`cancel_intent`). */
  async cancelIntent(taskId) {
    const t = this.tasks.get(taskId)
    if (!t) return null
    return this.post(t, { kind: 'cancel_intent', by: 'user' }, { requestId: `ui-${++this.reqSeq}` })
  }

  // ── publication: the owner's chain, inside the held rth-root task turn ─────────

  /** Before the append: persist the floor. Returns the record to append, or null to skip. */
  prepareAppend(taskId, nextSeq) {
    const p = this.d.pending.read(taskId)
    if (p.state !== 'ok') return null
    const rec = p.rec
    if (!same(rec.owner, this.incarnation)) return null
    if (rec.seq_floor === null) this.d.pending.write({ ...rec, seq_floor: nextSeq })
    return { text: rec.text, result_sha256: rec.result_sha256 }
  }

  /**
   * After the turn's flush. `appendedNow`: this turn appended the row at
   * `appendedSeq`; `flushed`: what `ctx.sessions.flush()` returned (true only when a
   * durability listener took part); `flushError`: what it threw.
   */
  async afterFlush(taskId, { appendedSeq, appendedNow = false, flushed = null, flushError } = {}) {
    const t = this.tasks.get(taskId) ?? this.task({ task_id: taskId, child_id: `${TASK_KEY}${taskId}` })
    const p = this.d.pending.read(taskId)
    if (p.state !== 'ok') return { outcome: 'pending_unreadable' }
    const rec = p.rec
    const epoch = rec.pub_epoch
    if (flushError) {
      this.stats.delivery_failed++
      await this.post(t, { kind: 'delivery_failed', reason: 'flush_failed' }, { pubEpoch: epoch })
      return { outcome: 'flush_failed' }
    }
    const rb = await this.d.readBack({ floor: rec.seq_floor ?? 0, key: `${TASK_KEY}${taskId}` })
    if (rb.state !== 'ok') { t.unresolved = true; return { outcome: 'readback_error' } } // unknown; never absence
    const match = rb.rows.find((r) => r.digest === rec.result_sha256 && r.seq >= (rec.seq_floor ?? 0) && (appendedSeq == null || r.seq === appendedSeq))
    if (!match) {
      const conflict = rb.rows.length > 0
      this.stats.delivery_failed++
      await this.post(t, { kind: 'delivery_failed', reason: conflict ? 'conflict' : 'not_persisted' }, { pubEpoch: epoch })
      return { outcome: conflict ? 'conflict' : 'not_persisted' }
    }
    // The owner's own chain (its append, a listener-answered flush, this read-back), or a
    // receipt that already covers these exact bytes. Anything else is visible, not durable.
    let receipt
    if (appendedNow && flushed === true && match.seq === appendedSeq) {
      receipt = { task_id: taskId, key: `${TASK_KEY}${taskId}`, seq: match.seq, result_sha256: rec.result_sha256, session_lineage: rb.lineage }
      this.d.pending.write({ ...rec, durable_receipt: receipt })
    } else if (covers(rec.durable_receipt, rec, match, rb.lineage)) {
      receipt = rec.durable_receipt
    } else {
      this.stats.visible_unconfirmed++
      await this.post(t, { kind: 'visible_unconfirmed', seq: match.seq }, { pubEpoch: epoch })
      return { outcome: 'visible_unconfirmed', seq: match.seq }
    }
    return this.deliver(t, taskId, epoch, match.seq, receipt)
  }

  /** `delivered` with its receipt; the pending file is unlinked only once agentd committed it. */
  async deliver(t, taskId, epoch, seq, receipt) {
    const d = await this.post(t, { kind: 'delivered', chat_ref: { session: 'rth-root', seq }, receipt }, { pubEpoch: epoch })
    // agentd's record decides: a resend refused because an earlier `delivered` (its answer lost)
    // already committed is that delivery.
    if (d && d.publication === 'delivered' && (answered(d) || d.outcome === 'refused')) {
      this.stats.delivered++
      this.d.pending.unlink(taskId)
      return { outcome: 'delivered', seq }
    }
    return { outcome: d ? 'delivered_refused' : 'delivered_unknown', reason: d?.reason }
  }

  // ── recovery by a successor (takeover only with the owner's exit receipt) ──

  /**
   * The plugin's bootstrap and every observer attach: announce this incarnation, then
   * recover every pending publication another incarnation owns. Own files are this instance's
   * live chain. Concurrent calls join one pass.
   */
  reconcile() {
    if (this.disposed) return Promise.resolve({})
    this.reconciling ??= (async () => {
      try {
        // agentd answers with its current records (every unresolved task and the recent others),
        // so a publication an earlier launch left unknown, or a task this process never saw,
        // reaches onTaskState through the observer like any record.
        await this.heartbeat(true, { inventory: true })
      } catch (e) {
        this.log(`delegate: reconcile heartbeat: ${e?.message ?? e}`)
      }
      const theirs = []
      const own = []
      for (const id of this.d.pending.list()) {
        const p = this.d.pending.read(id)
        if (p.state !== 'ok' || !same(p.rec.owner, this.incarnation)) theirs.push(id)
        // Our own chain is left alone unless it ended unknown (answers lost past the resends, or
        // a failed read-back) and nothing of it is in flight now.
        else if (this.tasks.get(id)?.unresolved && !this.tasks.get(id).inflight.size) own.push(id)
      }
      this.stats.reconciled++
      const out = await this.recover(theirs)
      for (const id of own) out[id] = await this.#reconcileOwn(this.tasks.get(id), id)
      return out
    })().finally(() => { this.reconciling = null })
    return this.reconciling
  }

  async recover(taskIds) {
    const out = {}
    for (const taskId of taskIds) {
      const t = this.tasks.get(taskId) ?? this.task({ task_id: taskId, child_id: `${TASK_KEY}${taskId}` })
      // One recovery per task at a time (bootstrap, attach and the stream may all ask).
      const prev = t.recovering ?? Promise.resolve()
      const mine = prev.then(() => this.#recoverOne(t, taskId))
      t.recovering = mine.catch(() => {})
      out[taskId] = await mine
    }
    return out
  }

  /**
   * Finish our own publication whose last step ended unknown, from
   * agentd's authoritative record and our own receipt only: never from visibility, never by
   * re-running the worker. The publish and delivered resends are the assigned owner's own updates.
   */
  async #reconcileOwn(t, taskId) {
    const p = this.d.pending.read(taskId)
    if (p.state !== 'ok') return p.state
    const rec = p.rec
    t.unresolved = false
    if (rec.pub_epoch == null) {
      // The publish's answer was lost: ask again (a fresh seq). An earlier commit refuses it,
      // and that refusal's record carries the commit.
      const c = await this.post(t, { kind: 'publish_request', result_sha256: rec.result_sha256 })
      if (c && c.publication === 'committed' && same(c.pub_owner, this.incarnation) && (answered(c) || c.outcome === 'refused')) {
        t.committed = true
        this.clearTimer(t.deadline)
        this.d.pending.write({ ...rec, pub_epoch: c.pub_epoch })
        this.offerTaskItem?.({ key: `${TASK_KEY}${taskId}`, taskId })
        return 'own:published'
      }
      if (c?.outcome === 'refused' && c.publication === 'none' && (FENCED.has(c.execution) || c.cancel || c.abandoned)) {
        this.d.pending.unlink(taskId)
        return `own:stale_intent:${c.execution}`
      }
      if (!c) t.unresolved = true
      return `own:publish_${c ? `refused:${c.reason ?? 'none'}` : 'unknown'}`
    }
    if (rec.durable_receipt) {
      const r = await this.deliver(t, taskId, rec.pub_epoch, rec.durable_receipt.seq, rec.durable_receipt)
      if (r.outcome === 'delivered_unknown') t.unresolved = true
      return `own:${r.outcome}`
    }
    // No receipt: the held turn runs the owner chain again (a row it finds but did not append
    // stays visible_unconfirmed).
    this.offerTaskItem?.({ key: `${TASK_KEY}${taskId}`, taskId, again: true })
    return 'own:reoffer'
  }

  async #recoverOne(t, taskId) {
    if (this.disposed) return 'disposed'
    const p = this.d.pending.read(taskId)
    if (p.state !== 'ok') return p.state
    const rec = p.rec
    if (same(rec.owner, this.incarnation)) return 'own'
    // A recovery intent (pub_epoch null) names the requester; a commit always makes it owner at epoch 1.
    let owner = rec.owner
    let expected = rec.pub_epoch ?? 1
    let tk = null
    // The file's owner may be stale (an intermediate successor's takeover committed, and it
    // died before its own file write). agentd's record, which a refusal carries, names the current
    // owner and epoch of the SAME report: take over from that owner instead. Each hop is its own
    // takeover, so agentd still requires that owner's accepted exit receipt. Bounded.
    for (let hop = 0; hop < 4; hop++) {
      tk = await this.post(t, { kind: 'publication_takeover', expected_owner: owner, expected_epoch: expected, result_sha256: rec.result_sha256 }, { requestId: `takeover-${taskId}-${expected}` })
      const newer = tk?.outcome === 'refused' && tk.result_sha256 === rec.result_sha256 && tk.pub_owner && Number.isInteger(tk.pub_epoch) && tk.pub_epoch > expected &&
        ['committed', 'unknown', 'visible_unconfirmed'].includes(tk.publication)
      if (!newer) break
      if (same(tk.pub_owner, this.incarnation)) break // ours already (its answer was lost): adopted below
      owner = tk.pub_owner
      expected = tk.pub_epoch
    }
    // Our own takeover, committed earlier, whose answer or file write was lost.
    if (tk?.outcome === 'refused' && tk.publication === 'committed' && same(tk.pub_owner, this.incarnation) && tk.result_sha256 === rec.result_sha256) tk = { ...tk, outcome: 'applied' }
    if (!answered(tk) || tk.publication !== 'committed' || !same(tk.pub_owner, this.incarnation)) {
      // An intent whose publish never committed and now never can (execution fenced, a cancel
      // committed, or the task abandoned: agentd admits no publish_request): nothing to recover.
      if (rec.pub_epoch == null && tk?.outcome === 'refused' && tk.publication === 'none' && (FENCED.has(tk.execution) || tk.cancel || tk.abandoned)) {
        this.d.pending.unlink(taskId)
        return `stale_intent:${tk.execution}`
      }
      return `takeover_refused:${tk?.reason ?? 'none'}`
    }
    const epoch = tk.pub_epoch
    const mine = { ...rec, owner: this.incarnation, pub_epoch: epoch }
    // The file names its owner as soon as the takeover is known; a failed read-back below then
    // leaves a file a later successor's takeover can start from.
    this.d.pending.write(mine)
    const rb = await this.d.readBack({ floor: rec.seq_floor ?? 0, key: `${TASK_KEY}${taskId}` })
    if (rb.state !== 'ok') { t.unresolved = true; return 'unknown:read_failed' }
    const row = rb.rows.find((r) => r.digest === rec.result_sha256 && r.seq >= (rec.seq_floor ?? 0))
    if (row && covers(rec.durable_receipt, rec, row, rb.lineage)) {
      this.d.pending.write(mine)
      await this.deliver(t, taskId, epoch, row.seq, rec.durable_receipt)
      return 'delivered:old_receipt'
    }
    if (rec.seq_floor === null || (!row && rb.rows.length === 0)) {
      // No append began, or nothing can still add it (the old process is gone): append once.
      this.d.pending.write({ ...mine, seq_floor: null, durable_receipt: null })
      this.offerTaskItem?.({ key: `${TASK_KEY}${taskId}`, taskId })
      return 'reappend'
    }
    if (row) {
      this.d.pending.write(mine)
      this.stats.visible_unconfirmed++
      await this.post(t, { kind: 'visible_unconfirmed', seq: row.seq }, { pubEpoch: epoch })
      return 'visible_unconfirmed'
    }
    this.stats.delivery_failed++
    await this.post(t, { kind: 'delivery_failed', reason: 'conflict' }, { pubEpoch: epoch })
    return 'conflict'
  }

  /**
   * A publication an exit receipt left `unknown`. With a pending file: as above.
   * Without one, the file was never written, so no append can have happened: take over,
   * then `delivery_failed{result_lost_before_commit}` under the new epoch.
   */
  async reconcileUnknown(rec) {
    const t = this.task(rec)
    if (this.d.pending.read(rec.task_id).state !== 'missing') return this.recover([rec.task_id])
    if (t.lostBeforeCommit) return t.lostBeforeCommit
    t.lostBeforeCommit = (async () => {
      const tk = await this.post(t, { kind: 'publication_takeover', expected_owner: rec.pub_owner, expected_epoch: rec.pub_epoch, result_sha256: rec.result_sha256 }, { requestId: `takeover-${rec.task_id}-${rec.pub_epoch}` })
      if (!answered(tk) || tk.publication !== 'committed' || !same(tk.pub_owner, this.incarnation)) return `takeover_refused:${tk?.reason ?? 'none'}`
      this.stats.delivery_failed++
      await this.post(t, { kind: 'delivery_failed', reason: 'result_lost_before_commit' }, { pubEpoch: tk.pub_epoch })
      return 'result_lost_before_commit'
    })()
    return t.lostBeforeCommit
  }

  /** Read-only observation by a non-owner: recorded, never delivery. */
  async observe(taskId, row, digest) {
    const t = this.tasks.get(taskId) ?? this.task({ task_id: taskId, child_id: `${TASK_KEY}${taskId}` })
    return this.post(t, { kind: 'visible_observed', seq: row.seq, result_sha256: digest }, { requestId: `obs-${taskId}-${row.seq}` })
  }
}

/** A durable receipt counts only if every field matches the row and the store together. */
function covers(r, rec, row, lineage) {
  return !!r && r.task_id === rec.task_id && r.key === `${TASK_KEY}${rec.task_id}` && r.seq === row.seq && r.result_sha256 === rec.result_sha256 && row.digest === rec.result_sha256 && typeof lineage === 'string' && r.session_lineage === lineage
}

export const digestOf = (text) => sha256(text)
