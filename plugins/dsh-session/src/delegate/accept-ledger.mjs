// The worker spend ledger of an acceptance run. One control record binds the acceptance; each
// attempt is one file claimed exclusively by link(2), so there is no lock to leave behind and no
// cached total: every admission reads the whole layer directory. A shared, monotonic stop marker
// is written before anything else on a failure, and every admission checks it. Integer
// picodollars throughout (1 µUSD per million tokens = 1 pUSD per token). Used only when the
// delegate config names a control record; the everyday soft caps (budget.mjs) are separate.
import { createHash, randomBytes } from 'node:crypto'
import { closeSync, fsyncSync, linkSync, mkdirSync, openSync, readFileSync, readdirSync, statSync, unlinkSync, writeSync } from 'node:fs'
import { dirname, join } from 'node:path'

export const SPEND_SCHEMA = 'rth-a2-spend/v1'
export const TOTAL_CAP_PUSD = 5_000_000_000_000

const ATTEMPT = /^attempt-(\d{3})\.json$/
const SETTLE = /^attempt-(\d{3})\.settle\.json$/
const ANOMALY = /^anomaly-[A-Za-z0-9_.:-]{1,200}\.json$/
const canon = (v) => JSON.stringify(v)

function fsyncDir(dir) {
  const fd = openSync(dir, 'r')
  try { fsyncSync(fd) } finally { closeSync(fd) }
}

/** Write `obj` to `path` once: complete and fsynced under a temporary name, then link(2)ed into place. */
export function writeOnce(path, obj) {
  const tmp = `${path}.tmp-${process.pid}-${randomBytes(4).toString('hex')}`
  const fd = openSync(tmp, 'wx', 0o600)
  try { writeSync(fd, `${canon(obj)}\n`); fsyncSync(fd) } finally { closeSync(fd) }
  try {
    linkSync(tmp, path)
  } finally {
    unlinkSync(tmp)
  }
  fsyncDir(dirname(path))
}

/** The init tool's one creation: control record, both layer directories and their open markers. */
export function initSpend(dir, control) {
  if (control.schema !== SPEND_SCHEMA) throw new Error('spend: unknown control schema')
  const caps = Object.values(control.layers).reduce((n, l) => n + l.cap_pusd, 0)
  if (!Number.isSafeInteger(caps) || caps > TOTAL_CAP_PUSD) throw new Error(`spend: caps ${caps} exceed ${TOTAL_CAP_PUSD}`)
  mkdirSync(dir, { recursive: false, mode: 0o700 })
  writeOnce(join(dir, 'control.json'), control)
  for (const [layer, l] of Object.entries(control.layers)) {
    mkdirSync(join(dir, l.dir), { mode: 0o700 })
    fsyncDir(dir)
    writeOnce(join(dir, l.dir, 'open.json'), { accept_id: control.accept_id, layer })
  }
  return createHash('sha256').update(readFileSync(join(dir, 'control.json'))).digest('hex')
}

const count = (v) => Number.isSafeInteger(v) && v >= 0

/** The worker's cost of one step's DSH usage, or null when the usage is not complete and valid. */
export function workerCost(usage, prices) {
  if (!usage || typeof usage !== 'object' || !count(usage.inputTokens) || !count(usage.outputTokens)) return null
  for (const k of ['cacheReadTokens', 'cacheWriteTokens']) if (usage[k] !== undefined && !count(usage[k])) return null
  // DSH's outputTokens is OpenAI's output_tokens, reasoning included (pi-ai): counted once.
  // Cached input at the full input rate: conservative.
  const input = usage.inputTokens + (usage.cacheReadTokens ?? 0) + (usage.cacheWriteTokens ?? 0)
  const cost = input * prices.input + usage.outputTokens * prices.output
  return Number.isSafeInteger(input) && Number.isSafeInteger(cost) && cost >= 0 ? cost : null
}

export class AcceptLedger {
  /**
   * Open the `layer` of a control record whose sha256 is pinned. Refuses (ok: false) on a changed
   * record, a missing established layer, a foreign open marker, or an effective configuration that
   * differs from the record's. `expect` is this process's own model and options.
   */
  static open({ control, controlSha256, layer, expect, hooks = {} }) {
    let bytes
    try { bytes = readFileSync(control) } catch (e) { return { ok: false, reason: `spend: control record unreadable (${e.code ?? e.message})` } }
    if (createHash('sha256').update(bytes).digest('hex') !== controlSha256) return { ok: false, reason: 'spend: control record is not the pinned one' }
    let rec
    try { rec = JSON.parse(bytes) } catch { return { ok: false, reason: 'spend: control record does not parse' } }
    const l = rec?.layers?.[layer]
    if (rec?.schema !== SPEND_SCHEMA || !l) return { ok: false, reason: `spend: no ${layer} layer in the control record` }
    const caps = Object.values(rec.layers).reduce((n, x) => n + x.cap_pusd, 0)
    if (!Number.isSafeInteger(caps) || caps > TOTAL_CAP_PUSD) return { ok: false, reason: 'spend: caps exceed the total' }
    if (expect.model !== l.model) return { ok: false, reason: `spend: model ${expect.model} is not the record's ${l.model}` }
    for (const [k, v] of Object.entries(expect.options ?? {})) if (canon(l.options?.[k]) !== canon(v)) return { ok: false, reason: `spend: option ${k}=${canon(v)} is not the record's ${canon(l.options?.[k])}` }
    const bound = l.context_window * l.prices.input + l.options.maxOutputTokens * l.prices.output
    if (!Number.isSafeInteger(bound) || bound > l.attempt_bound_pusd) return { ok: false, reason: `spend: the bound ${bound} exceeds the record's ${l.attempt_bound_pusd}` }
    const root = dirname(control)
    const dir = join(root, l.dir)
    let open
    try { open = JSON.parse(readFileSync(join(dir, 'open.json'), 'utf8')) } catch (e) { return { ok: false, reason: `spend: the ${layer} ledger is missing or torn (${e.code ?? 'parse'}): never recreated` } }
    if (open.accept_id !== rec.accept_id || open.layer !== layer) return { ok: false, reason: 'spend: the ledger belongs to another acceptance' }
    return { ok: true, ledger: new AcceptLedger({ rec, layer, l, dir, stop: join(root, rec.stop), hooks, controlSha256 }) }
  }

  constructor({ rec, layer, l, dir, stop, hooks, controlSha256 }) {
    Object.assign(this, { rec, layer, l, dir, stopPath: stop, hooks })
    // Every durable row carries this identity, and replay refuses any other.
    this.ident = { accept_id: rec.accept_id, layer, control_sha256: controlSha256 }
    this.failedHere = null // a stop this process could not write: it refuses on its own
  }

  /** A row of this acceptance, layer and pinned record. */
  mine(v) {
    return v && typeof v === 'object' && v.accept_id === this.ident.accept_id && v.layer === this.ident.layer && v.control_sha256 === this.ident.control_sha256
  }

  /** Stopped: the shared marker exists, is unreadable, or cannot be checked; or this process failed. */
  stopped() {
    if (this.failedHere) return this.failedHere
    try { statSync(this.stopPath) } catch (e) { return e.code === 'ENOENT' ? null : `spend: stop marker unreadable (${e.code})` }
    return 'spend: stopped'
  }

  /** Write the shared stop marker first; a write failure (not EEXIST) stops this process for good. */
  fail(reason) {
    try {
      writeOnce(this.stopPath, { layer: this.layer, reason, at: new Date().toISOString() })
    } catch (e) {
      if (e.code !== 'EEXIST') this.failedHere = `spend: stop marker not written (${e.code ?? e.message}): ${reason}`
    }
    return { ok: false, reason, failed: true }
  }

  /**
   * The whole layer, read now and validated: every attempt and settlement must be this
   * acceptance's, well formed, uniquely identified and consistent with its reservation, its cost
   * recomputed from its usage at the pinned prices. Any other file, and any durable fatal record (an
   * anomaly), is torn or foreign history; I/O failure is the same.
   */
  read() {
    const attempts = []
    const settles = new Map()
    let names
    try { names = readdirSync(this.dir) } catch (e) { return { torn: `spend: ledger unreadable (${e.code ?? e.message})` } }
    for (const name of names) {
      if (name === 'open.json' || name.includes('.tmp-')) continue
      let m
      let v
      try { v = JSON.parse(readFileSync(join(this.dir, name), 'utf8')) } catch { return { torn: `spend: ${name} does not parse` } }
      if (ANOMALY.test(name)) return { torn: `spend: a fatal record is present (${name})` }
      if (!this.mine(v)) return { torn: `spend: ${name} belongs to another acceptance or layer` }
      if ((m = name.match(SETTLE))) settles.set(Number(m[1]), v)
      else if ((m = name.match(ATTEMPT))) attempts[Number(m[1])] = v
      else return { torn: `spend: foreign file ${name}` }
    }
    const ids = new Set()
    for (let i = 0; i < attempts.length; i++) {
      const a = attempts[i]
      if (!a || a.n !== i) return { torn: `spend: attempt numbering broken at ${i}` }
      if (typeof a.attempt_id !== 'string' || !a.attempt_id || ids.has(a.attempt_id)) return { torn: `spend: attempt ${i} has no unique identity` }
      if (a.bound_pusd !== this.l.attempt_bound_pusd) return { torn: `spend: attempt ${i} does not carry the record's bound` }
      ids.add(a.attempt_id)
    }
    let committed = 0
    for (const [k, st] of settles) {
      const a = attempts[k]
      if (!a) return { torn: `spend: settlement without attempt ${k}` }
      if (st.n !== k || st.attempt_id !== a.attempt_id) return { torn: `spend: settlement ${k} does not match its reservation` }
      const cost = workerCost(st.usage, this.l.prices)
      if (cost === null || st.cost_pusd !== cost || cost > a.bound_pusd) return { torn: `spend: settlement ${k} is not its usage at the pinned prices` }
    }
    for (const a of attempts) committed += settles.has(a.n) ? settles.get(a.n).cost_pusd : a.bound_pusd
    return { attempts, settles, committed }
  }

  /**
   * Admission: not stopped, within the allowance and the cap; then claim the next attempt; then,
   * with the reservation durable, check the stop marker again before granting dispatch. A
   * stop seen then refuses dispatch and keeps the reservation; a stop written after this final
   * check finds an admitted, fully reserved request.
   */
  admit(meta) {
    for (let tries = 0; tries < 64; tries++) {
      const why = this.stopped()
      if (why) return { ok: false, reason: why }
      const r = this.read()
      if (r.torn) return this.fail(r.torn)
      const n = r.attempts.length
      if (n >= this.l.max_attempts) return { ok: false, reason: `spend: the ${this.layer} allowance of ${this.l.max_attempts} is used` }
      if (r.committed + this.l.attempt_bound_pusd > this.l.cap_pusd) return { ok: false, reason: `spend: the ${this.layer} cap would be exceeded` }
      this.hooks.beforeClaim?.(n)
      // Metadata never overwrites the authoritative fields.
      const attempt = { ...(meta.fields ?? {}), n, attempt_id: meta.attemptId, bound_pusd: this.l.attempt_bound_pusd, at: new Date().toISOString(), ...this.ident }
      try {
        writeOnce(join(this.dir, `attempt-${String(n).padStart(3, '0')}.json`), attempt)
      } catch (e) {
        if (e.code === 'EEXIST') continue
        return this.fail(`spend: attempt ${n} not claimed (${e.code ?? e.message})`)
      }
      const after = this.stopped()
      if (after) return { ok: false, reason: after, reserved: true }
      return { ok: true, attempt }
    }
    return this.fail('spend: no attempt could be claimed')
  }

  /** Settle one attempt from its complete usage, once. Identical replay is harmless. */
  settle(attempt, { usage, providerResponseId = null }) {
    // The supplied attempt must be the persisted reservation.
    let persisted
    try { persisted = JSON.parse(readFileSync(join(this.dir, `attempt-${String(attempt?.n).padStart(3, '0')}.json`), 'utf8')) } catch { return this.fail(`spend: no persisted reservation ${attempt?.n}`) }
    if (!this.mine(persisted) || persisted.n !== attempt.n || persisted.attempt_id !== attempt.attempt_id || persisted.bound_pusd !== attempt.bound_pusd) return this.fail(`spend: attempt ${attempt.n} is not the persisted reservation`)
    const cost = workerCost(usage, this.l.prices)
    if (cost === null) return this.fail(`spend: incomplete usage for attempt ${attempt.n}`)
    if (cost > attempt.bound_pusd) return this.fail(`spend: attempt ${attempt.n} cost ${cost} above its bound`)
    const s = { n: attempt.n, attempt_id: attempt.attempt_id, provider_response_id: providerResponseId, usage, cost_pusd: cost, ...this.ident }
    const path = join(this.dir, `attempt-${String(attempt.n).padStart(3, '0')}.settle.json`)
    try {
      writeOnce(path, s)
    } catch (e) {
      if (e.code !== 'EEXIST') return this.fail(`spend: settlement ${attempt.n} not written (${e.code ?? e.message})`)
      let prior
      try { prior = JSON.parse(readFileSync(path, 'utf8')) } catch { return this.fail(`spend: settlement ${attempt.n} unreadable`) }
      if (canon(prior) !== canon(s)) return this.fail(`spend: conflicting settlement for attempt ${attempt.n}`)
    }
    return { ok: true, cost_pusd: cost }
  }
}
