// The live seams of the delegate executor over DSH 0.2.1-rc.2 services. The unit tests replace
// them with fakes. `delegate.enabled` defaults off.
import { join } from 'node:path'
import { randomBytes } from 'node:crypto'
import { DelegateExecutor, TASK_KEY } from './executor.mjs'
import { PendingStore } from './pending.mjs'
import { SpendLedger, pricesPinned } from './budget.mjs'
import { AcceptLedger } from './accept-ledger.mjs'
import { TASK_PROVIDER } from './snapshot.mjs'
import { sha256 } from './canon.mjs'

const textOf = (message) => (message?.content ?? []).filter((b) => b?.type === 'text').map((b) => b.text).join('')

/** `null` (stays off) unless the prices are pinned and the services exist. */
export function makeLiveExecutor(ctx, config, host, { ROOT, log, offerTask, journal }) {
  const d = config.delegate
  const home = process.env.DSH_HOME || process.cwd()
  const prices = d.prices
  if (!pricesPinned(prices) || !d.model) {
    log('rth-session: delegate stays off: model or price table not pinned')
    return null
  }
  // The launcher passes RTH_DSH_LAUNCH_ID only after the host owner committed this launch's
  // registration. Without it there is no exit proof for this process, so nothing is executed.
  const launchId = process.env.RTH_DSH_LAUNCH_ID
  if (!launchId || !/^[A-Za-z0-9_.:-]{1,128}$/.test(launchId)) {
    log('rth-session: delegate stays off: no registered launch id (RTH_DSH_LAUNCH_ID)')
    return null
  }
  const incarnation = { launch_id: launchId, pid: process.pid, plugin_instance: randomBytes(8).toString('hex') }
  // A run's own limits; unset keys keep the defaults.
  const limits = Object.fromEntries(['maxSteps', 'attemptsPerStep', 'maxOutputTokens'].filter((k) => Number.isInteger(d[k]) && d[k] > 0).map((k) => [k, d[k]]))
  // The acceptance ledger: named by the config, it must open against this process's exact model and
  // options, or the executor stays off (never a fallback).
  let accept = null
  // (An absent section may arrive as {} from the schema: any named field means an acceptance run.)
  if (d.accept && (d.accept.control || d.accept.controlSha256)) {
    const o = AcceptLedger.open({ control: d.accept.control, controlSha256: d.accept.controlSha256, layer: 'worker', expect: { model: d.model, options: { maxSteps: d.maxSteps, attemptsPerStep: d.attemptsPerStep, maxOutputTokens: d.maxOutputTokens, reasoningEffort: d.reasoningEffort } } })
    if (!o.ok) {
      log(`rth-session: delegate stays off: ${o.reason}`)
      return null
    }
    accept = o.ledger
  }
  const children = new Map() // child session id → {events}
  const ex = new DelegateExecutor({
    incarnation,
    recipient: { provider: d.provider, model: d.model },
    grant: { provider: d.provider, model: d.model },
    pending: new PendingStore(d.pendingDir || join(home, 'rth', 'pending')),
    limits,
    accept,
    spend: new SpendLedger(d.spendLedger || join(home, 'rth', 'spend.jsonl'), { prices, limits: { perTaskUsd: d.perTaskUsd, perDayUsd: d.perDayUsd } }),
    log,
    // POST /v1/tasks/<id> and /v1/executor on the host owner (crates/rth/src/host/tasks.rs).
    postTask: (taskId, update) => host.request('POST', `/v1/tasks/${encodeURIComponent(taskId)}`, update),
    postExecutor: (status) => host.request('POST', '/v1/executor', status),
    offerTaskItem: offerTask,
    snapshotSource: () => journal.freeze(),
    // dsh-subagent: managed `spawn`, caller delivery, absolute depth 1.
    spawnChild: ({ childId, prompt, signal, hooks }) => {
      const subagents = ctx.get?.('subagents')
      if (!subagents?.startActivation) throw new Error('subagents service unavailable')
      const rec = { events: [] }
      children.set(childId, rec)
      // The parent must be live. After a DSH restart rth-root is cold until something resumes it, so
      // the Session API resumes it; otherwise the first delegation of every launch fails here.
      const act = (async () => {
        let parent = ctx.agents?.get?.(ROOT)
        if (!parent) {
          const sc = ctx.get?.('sessionController')
          const r = await sc?.resolveAgent?.(ROOT)
          parent = r?.agent
          if (!parent) throw new Error(`rth-root could not be resumed: ${sc ? JSON.stringify(r?.error ?? r ?? null).slice(0, 300) : 'no sessionController service'}`)
        }
        signal.throwIfAborted()
        return subagents.startActivation({
          provider: 'spawn',
          label: 'rth-task',
          childId,
          delivery: 'caller',
          signal,
          request: { prompt: [{ type: 'text', text: prompt }], parent, agentPreset: 'rth-delegate', agentOptions: { provider: d.provider, model: d.model, maxTokens: ex.limits.maxOutputTokens, ...(d.reasoningEffort ? { reasoningEffort: d.reasoningEffort } : {}) }, maxDepth: 1 },
        })
      })()
      const result = Promise.resolve(act).then(async (a) => {
        rec.activation = a
        await a.result
        const last = rec.events.filter((e) => e.type === 'assistant/message').at(-1)?.data
        const finish = (last?.stream ?? []).map((c) => c?.chunk ?? c).filter((c) => c?.type === 'finish').at(-1)?.reason?.kind
        const end = rec.events.filter((e) => e.type === 'turn/end').at(-1)?.data?.reason
        return { turnEnd: typeof end === 'string' ? end : end?.kind, stopReason: finish, interrupted: !!last?.interrupted, text: textOf(last?.message), toolAttempts: rec.events.filter((e) => e.type === 'tool/call').length }
      })
      return { result, dispose: () => Promise.resolve(act).then((a) => a.dispose?.()).catch(() => {}) }
    },
    // dsh-session-persistence: a fresh READ handle, from the floor; closed after.
    readBack: async ({ floor, key }) => {
      const sp = ctx.get?.('sessionPersistence')
      if (!sp?.open) return { state: 'error' }
      let h
      const t0 = performance.now()
      const heap0 = process.memoryUsage().heapUsed
      try {
        h = await sp.open(ROOT, 'read')
        const r = await h.read(floor)
        const rows = (r.events ?? []).filter((e) => e.type === 'assistant/message' && e.data?.message?.source?.provider === TASK_PROVIDER && e.data.message.source.model === key).map((e) => ({ seq: e.seq, digest: sha256(textOf(e.data.message)) }))
        // The read-back's cost on this session, kept with the stats.
        const ms = performance.now() - t0
        const m = ex.stats.readback ?? { reads: 0, max_ms: 0 }
        ex.stats.readback = { reads: m.reads + 1, last_ms: Math.round(ms * 10) / 10, max_ms: Math.max(m.max_ms, Math.round(ms * 10) / 10), last_floor: floor, last_events_returned: r.events?.length ?? 0, last_heap_delta_bytes: process.memoryUsage().heapUsed - heap0 }
        // The lineage the API exposes: the stored session's id and creation time (no generation
        // field exists on the handle; the receipt discloses this).
        return { state: 'ok', rows, lineage: `${h.header?.id}@${h.header?.createdAt}` }
      } catch {
        return { state: 'error' }
      } finally {
        await h?.close?.().catch?.(() => {})
      }
    },
  })
  // Child step usage, priced after each step (soft caps).
  ctx.on?.('session/event', (session, event) => {
    const rec = children.get(session?.id)
    if (!rec) return
    rec.events.push(event)
    if (event.type === 'assistant/message') {
      const task = ex.admittedChild(session.id)
      if (task) ex.afterChildStep(task, event.data?.usage ?? null)
    }
  })
  // Zero tools on the child: restrict and guard on the child agent itself.
  ctx.on?.('agent/created', ({ agent }) => {
    if (!children.has(agent?.session?.id)) return
    try { agent.ctx?.tools?.restrict?.({ allow: [] }) } catch (e) { log(`rth-session: restrict failed: ${e?.message ?? e}`) }
    // A ToolGuard returns its denial reason as a string (dsh-tools ToolGuard), never an object.
    try { agent.ctx?.tools?.guard?.(() => 'rth-delegate: zero tools') } catch (e) { log(`rth-session: guard failed: ${e?.message ?? e}`) }
  })
  // The caller owns cleanup: a plugin reload does not dispose dsh-subagent activations, so this
  // scope disposes them itself.
  ctx.effect?.(() => {
    void ex.heartbeat(true).catch(() => {})
    const beat = setInterval(() => void ex.heartbeat(true).catch(() => {}), 5_000)
    beat.unref?.()
    return () => {
      clearInterval(beat)
      // Fences every task first (no late start), then disposes the activations it holds.
      ex.dispose()
      for (const r of children.values()) void r.activation?.dispose?.()
    }
  }, 'rth-session: delegate executor')
  return ex
}

export { TASK_KEY }
