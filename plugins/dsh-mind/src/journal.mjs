// A read-only tail of one JSONL journal file (rth-host --log, or the agentd journal in the
// rth-state volume). Opened with 'r' only; never written, truncated or locked. On start it
// reads at most `backfillBytes` from the end (the first partial line is skipped and reported),
// then follows appends through fs.watch (pushed by the OS; no polling loop). Every record is
// handed over with its byte offset, length and the sha256 of its exact line bytes, so anyone
// can re-check it against the file.
//
// Continuity: the tail only continues a file it can prove is
// the same one, grown by appends: the same device and inode it opened, not shorter than its cursor,
// and the first and last lines it read still byte-identical at their offsets (bytes between them
// are not re-hashed: a rewrite that keeps both exact lines in place is not detected). Anything else (a rename or other
// replacement, a shrink, an in-place rewrite, a read failure) is a DISCONTINUITY: reported as a
// note of type 'replaced' | 'shrank' | 'rewritten' | 'missing' BEFORE any record of the new
// content, then the file is read again from its start (within backfillBytes) and re-watched, so a
// new process's started record is never skipped.
import { createHash } from 'node:crypto'
import { closeSync, fstatSync, openSync, readSync, statSync, watch } from 'node:fs'

export const MAX_LINE = 1 << 20

export class JsonlTail {
  static BREAKS = new Set(['replaced', 'shrank', 'rewritten', 'missing'])

  constructor(path, { backfillBytes = 256 * 1024, onRecord, onNote = () => {} }) {
    this.path = path
    this.backfillBytes = backfillBytes
    this.onRecord = onRecord
    this.onNote = onNote
    this.offset = 0
    this.state = 'idle'
    this.watcher = null
    this.records = 0
    this.skipped = 0
    this.identity = null // { dev, ino } of the file the cursor belongs to
    this.anchor = null // { at, length, sha256 } of the last complete line read
    this.head = null // the same for the line at offset 0, when it was read
    this.broken = false // a read failure: the next successful read starts over
  }

  start() {
    let size
    try {
      const st = statSync(this.path)
      size = st.size
      this.identity = { dev: st.dev, ino: st.ino }
    } catch (e) {
      this.state = 'missing'
      this.onNote('missing', `cannot read ${this.path}: ${e.code ?? e.message}`)
      return this
    }
    const from = Math.max(0, size - this.backfillBytes)
    // Follow first, then read: an append landing between the two is read by whichever runs
    // second (both read from this.offset), never lost.
    this.follow()
    this.offset = from
    this.read(true, from > 0 && !this.atLineStart(from))
    if (from > 0) this.onNote('truncated_history', `only the last ${this.backfillBytes} bytes were read at load; ${from} earlier bytes of history are not shown`)
    this.state = 'following'
    return this
  }

  follow() {
    this.watcher?.close()
    this.watcher = null
    try {
      this.watcher = watch(this.path, { persistent: false }, () => this.read(false, false))
      this.watcher.on('error', (e) => this.onNote('watch_error', e.message))
    } catch (e) {
      this.onNote('watch_error', `cannot follow appends: ${e.message}`)
    }
  }

  atLineStart(at) {
    const fd = openSync(this.path, 'r')
    try {
      const b = Buffer.alloc(1)
      readSync(fd, b, 0, 1, at - 1)
      return b[0] === 0x0a
    } finally {
      closeSync(fd)
    }
  }

  stop() {
    this.watcher?.close()
    this.watcher = null
    this.state = 'stopped'
  }

  read(history, skipFirstPartial) {
    let fd
    try {
      fd = openSync(this.path, 'r')
    } catch (e) {
      if (!this.broken) { this.broken = true; this.onNote('missing', `cannot read ${this.path}: ${e.code ?? e.message}; continuity is lost until it is read again from its start`) }
      return
    }
    try {
      const st = fstatSync(fd)
      const size = st.size
      const why = this.broken ? ['missing', 'it could not be read for a while']
        : this.identity && (st.dev !== this.identity.dev || st.ino !== this.identity.ino) ? ['replaced', `another file now has this path (inode ${this.identity.ino} -> ${st.ino})`]
        : size < this.offset ? ['shrank', `it shrank from ${this.offset} to ${size} bytes`]
        : [this.head, this.anchor].some((a) => a && !this.sameBytes(fd, a)) ? ['rewritten', 'a line it already read changed (not an append)']
        : null
      if (why) {
        // A discontinuity: say so before any record of the new content, then start over.
        this.onNote(why[0], `${this.path}: ${why[1]}; reading it again from its start`)
        this.broken = false
        this.identity = { dev: st.dev, ino: st.ino }
        this.anchor = null
        this.head = null
        const from = Math.max(0, size - this.backfillBytes)
        this.offset = from
        if (from > 0) this.onNote('truncated_history', `only the last ${this.backfillBytes} bytes of the new content were read; ${from} earlier bytes are not shown`)
        this.follow()
        skipFirstPartial = from > 0
        history = true
      }
      if (size === this.offset) return
      const buf = Buffer.alloc(size - this.offset)
      readSync(fd, buf, 0, buf.length, this.offset)
      let start = 0
      if (skipFirstPartial) {
        const nl = buf.indexOf(0x0a)
        if (nl < 0) return
        start = nl + 1
      }
      for (let nl = buf.indexOf(0x0a, start); nl >= 0; nl = buf.indexOf(0x0a, start)) {
        const line = buf.subarray(start, nl)
        const offset = this.offset + start
        start = nl + 1
        if (!line.length) continue
        if (line.length > MAX_LINE) {
          this.skipped++
          continue
        }
        let rec
        try {
          rec = JSON.parse(line.toString('utf8'))
        } catch {
          this.skipped++
          continue
        }
        this.records++
        const line_sha256 = createHash('sha256').update(line).digest('hex')
        this.anchor = { at: offset, length: line.length, sha256: line_sha256 }
        if (offset === 0) this.head = this.anchor // the file's first line (rth-host's started record)
        this.onRecord(rec, { file: this.path, offset, length: line.length, line_sha256, history })
      }
      this.offset += start // a trailing partial line waits for its newline
    } finally {
      closeSync(fd)
    }
  }

  sameBytes(fd, { at, length, sha256 }) {
    const b = Buffer.alloc(length)
    const n = readSync(fd, b, 0, length, at)
    return n === length && createHash('sha256').update(b).digest('hex') === sha256
  }
}

/**
 * Follow rth-host's --log journal into Mind. A tail discontinuity (replaced, shrank, rewritten,
 * missing) reaches Mind as hostBreak BEFORE any record of the new content, so no host lifetime
 * continues across it; only a newly observed started record establishes the next one.
 */
export function followHostJournal(path, mind, { backfillBytes, onNote = () => {} } = {}) {
  let hostPid = null
  return new JsonlTail(path, {
    backfillBytes,
    onRecord: (rec, at) => {
      if (!rec || typeof rec !== 'object') return
      if (rec.kind === 'started' && Number.isFinite(rec.pid)) hostPid = rec.pid
      mind.host(rec, { via: 'rth-host --log file', process: `rth-host${hostPid ? ` (pid ${hostPid}, from its started record)` : ' (pid unknown)'}`, ...at })
    },
    onNote: (type, msg) => {
      if (JsonlTail.BREAKS.has(type)) { hostPid = null; mind.hostBreak(`${type}: ${msg}`) }
      onNote(type, msg)
    },
  })
}
