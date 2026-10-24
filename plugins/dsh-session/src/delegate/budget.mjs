// Soft spend caps: each child step's reported usage is priced from a pinned table and added to
// the task and day totals in an fsynced ledger. A step with missing usage is charged at its upper
// bound. Reaching a threshold cancels at the step boundary; a task or day already at its threshold
// refuses the next step. Soft: the step in flight is not capped.
import { appendFileSync, closeSync, existsSync, fsyncSync, mkdirSync, openSync, readFileSync } from 'node:fs'
import { dirname } from 'node:path'

// The everyday defaults. A run may set its own limits (`delegate.maxSteps`, `attemptsPerStep`,
// `maxOutputTokens`); the worker request carries maxOutputTokens.
export const DEFAULTS = { perTaskUsd: 0.05, perDayUsd: 1.0, maxSteps: 3, attemptsPerStep: 2, maxOutputTokens: 4000 }

/** A price table is pinned (dated, sourced) or the executor stays disabled. */
export function pricesPinned(p) {
  return !!(p && Number.isFinite(p.inputPerMTok) && Number.isFinite(p.outputPerMTok) && p.source && p.pinnedAt && p.model)
}

export function priceUsage(usage, prices) {
  const input = (usage?.inputTokens ?? 0) + (usage?.cacheReadTokens ?? 0) + (usage?.cacheWriteTokens ?? 0)
  const output = (usage?.outputTokens ?? 0) + (usage?.reasoningTokens ?? 0)
  return (input * prices.inputPerMTok + output * Math.max(prices.outputPerMTok, prices.reasoningPerMTok ?? 0)) / 1e6
}

/** Upper bound of one attempt: ≥1 byte per token over the request bytes, plus maxTokens. */
export function attemptBoundUsd(requestBytes, prices, maxOutputTokens = DEFAULTS.maxOutputTokens) {
  return (requestBytes * prices.inputPerMTok + maxOutputTokens * Math.max(prices.outputPerMTok, prices.reasoningPerMTok ?? 0)) / 1e6
}

/** The stated worst case: steps × attempts × the per-attempt bound. */
export function worstCaseUsd(maxRequestBytes, prices, d = DEFAULTS) {
  return d.maxSteps * d.attemptsPerStep * attemptBoundUsd(maxRequestBytes, prices, d.maxOutputTokens)
}

const dayOf = (ms) => new Date(ms).toISOString().slice(0, 10)

export class SpendLedger {
  constructor(path, { prices, limits = DEFAULTS, now = () => Date.now() } = {}) {
    this.path = path
    this.prices = prices
    this.limits = { ...DEFAULTS, ...limits }
    this.now = now
    mkdirSync(dirname(path), { recursive: true })
    this.rows = existsSync(path) ? readFileSync(path, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []
  }

  taskTotal(taskId) { return this.rows.filter((r) => r.task_id === taskId).reduce((n, r) => n + r.usd, 0) }
  dayTotal(day = dayOf(this.now())) { return this.rows.filter((r) => r.day === day).reduce((n, r) => n + r.usd, 0) }

  /** Before a step: refuse when the task or the day is already at its threshold. */
  mayStart(taskId) {
    if (this.taskTotal(taskId) >= this.limits.perTaskUsd) return { ok: false, reason: 'budget:task' }
    if (this.dayTotal() >= this.limits.perDayUsd) return { ok: false, reason: 'budget:day' }
    return { ok: true }
  }

  /** After a step: charge it (missing usage at `boundUsd`), fsync, and say whether to stop. */
  charge(taskId, step, usage, boundUsd) {
    const estimated = !usage
    const usd = estimated ? boundUsd : priceUsage(usage, this.prices)
    const row = { task_id: taskId, step, day: dayOf(this.now()), usd, estimated, at: this.now() }
    const fd = openSync(this.path, 'a', 0o600)
    try {
      appendFileSync(fd, `${JSON.stringify(row)}\n`)
      fsyncSync(fd)
    } finally {
      closeSync(fd)
    }
    this.rows.push(row)
    const over = this.taskTotal(taskId) >= this.limits.perTaskUsd ? 'budget:task' : this.dayTotal() >= this.limits.perDayUsd ? 'budget:day' : null
    return { usd, estimated, over }
  }
}
