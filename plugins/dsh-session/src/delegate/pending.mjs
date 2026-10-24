// The per-task pending file: {task_id, text, result_sha256, seq_floor, durable_receipt}. Every
// write is atomic: temp file, fsync, rename over, parent-directory fsync. It is created BEFORE the
// publication request, as a recovery intent (`pub_epoch: null`, seq_floor null), so no committed
// publication exists without one; the commit then fills pub_epoch. The floor is made durable before
// any append, so a valid null floor proves no append began. Unlinking removes it from the live
// namespace; it is not secure erasure.
import { closeSync, fsyncSync, mkdirSync, openSync, readFileSync, readdirSync, renameSync, unlinkSync, writeSync, existsSync } from 'node:fs'
import { join } from 'node:path'
import { randomBytes } from 'node:crypto'

const syncDir = (dir) => {
  const fd = openSync(dir, 'r')
  try { fsyncSync(fd) } finally { closeSync(fd) }
}

export class PendingStore {
  constructor(dir, { faults } = {}) {
    this.dir = dir
    this.faults = faults ?? {}
    mkdirSync(dir, { recursive: true })
  }

  path(taskId) {
    if (!/^[A-Za-z0-9_.-]{1,160}$/.test(taskId)) throw new Error(`pending: bad task id ${JSON.stringify(taskId)}`)
    return join(this.dir, `${taskId}.json`)
  }

  write(rec) {
    const final = this.path(rec.task_id)
    const tmp = `${final}.${randomBytes(6).toString('hex')}.tmp`
    const fd = openSync(tmp, 'wx', 0o600)
    try {
      writeSync(fd, `${JSON.stringify(rec)}\n`)
      fsyncSync(fd)
    } finally {
      closeSync(fd)
    }
    if (this.faults.crashBeforeRename) { this.faults.crashBeforeRename = false; throw new Error('injected: crash before rename') }
    renameSync(tmp, final)
    syncDir(this.dir)
  }

  /** `{state: 'missing'}`, `{state: 'unreadable', error}` or `{state: 'ok', rec}` (validated). */
  read(taskId) {
    const p = this.path(taskId)
    if (!existsSync(p)) return { state: 'missing' }
    let rec
    try {
      rec = JSON.parse(readFileSync(p, 'utf8'))
    } catch (e) {
      return { state: 'unreadable', error: String(e?.message ?? e) }
    }
    const err = validate(rec, taskId)
    return err ? { state: 'unreadable', error: err } : { state: 'ok', rec }
  }

  /** The task ids with a pending file (the successor's inventory at bootstrap); temp files are not. */
  list() {
    return readdirSync(this.dir).filter((n) => /^[A-Za-z0-9_.-]{1,160}\.json$/.test(n) && !n.endsWith('.tmp')).map((n) => n.slice(0, -5)).sort()
  }

  unlink(taskId) {
    try { unlinkSync(this.path(taskId)) } catch (e) { if (e.code !== 'ENOENT') throw e }
    syncDir(this.dir)
  }
}

/** The structural task binding is validated: a wrong task, digest or floor never counts. */
function validate(rec, taskId) {
  if (!rec || typeof rec !== 'object') return 'not an object'
  if (rec.task_id !== taskId) return 'task binding mismatch'
  if (typeof rec.text !== 'string' || !/^[0-9a-f]{64}$/.test(rec.result_sha256 ?? '')) return 'text or digest missing'
  if (!(rec.seq_floor === null || (Number.isInteger(rec.seq_floor) && rec.seq_floor >= 0))) return 'bad seq_floor'
  if (rec.durable_receipt != null) {
    const r = rec.durable_receipt
    if (r.task_id !== taskId || r.key !== `rth-task-${taskId}` || !Number.isInteger(r.seq) || r.result_sha256 !== rec.result_sha256 || typeof r.session_lineage !== 'string' || !r.session_lineage) return 'bad durable_receipt'
    if (rec.seq_floor === null || r.seq < rec.seq_floor) return 'receipt below floor'
  }
  return null
}
