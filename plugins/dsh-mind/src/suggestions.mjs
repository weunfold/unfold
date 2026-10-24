// Human suggestions: proposals about how a mind would handle an episode differently. They are
// appended to one local JSONL file in this plugin's data dir and go nowhere else. A suggestion
// never mutates a prompt, setting or memory, and grants itself no effect authority: the record
// says so, and nothing in this plugin reads it back as an instruction.
//
// Append-only: there is no update, delete, clear, truncate or rewrite method, and no existing
// byte is ever changed. Idempotent: a retry with the same `key` returns the stored record and
// writes nothing; the same key with different content is a conflict.
//
// Durability: a record is
// acknowledged only after it is on disk as its OWN complete line and fsynced. If the file does
// not end in a newline (a torn tail from a crash, or a complete record missing its newline),
// the new record starts with a newline, in the same write, so it can never fuse with the bytes
// before it. On load, every complete record is kept, wherever it sits; only an unparseable
// fragment is skipped, and it is reported (offset, length, sha256) as damage, never silently
// dropped and never repaired in place.
//
// An unconfirmed write (it failed after some or all bytes may have reached the file) leaves the
// log uncertain. The next append, in the same process, first
// reconciles against the bytes on disk: it re-reads the file and fsyncs it (and its directory)
// before trusting anything. A record found there for an unconfirmed key is acknowledged then, as
// the original outcome (stored, `reconciled`), and never written twice; a fragment is reported and fenced off. Until a reconcile
// succeeds, nothing is acknowledged.
import { createHash } from 'node:crypto'
import * as nodeFs from 'node:fs'
import { dirname, join } from 'node:path'
import { redact } from './redact.mjs'

export const FORMAT = 'rth.mind-suggestion/v1'
export const FILE = 'suggestions.jsonl'
export const LIMITS = { text: 4000, actor: 80, key: 128, id: 200 }
const KEY = /^[A-Za-z0-9._:-]{8,128}$/

export class SuggestionError extends Error {
  constructor(status, message) {
    super(message)
    this.status = status
  }
}

export class SuggestionLog {
  /** `io` replaces node:fs calls (openSync, writeSync, fsyncSync, closeSync, fstatSync, readSync), for crash tests. */
  constructor(dir, { io = {} } = {}) {
    this.dir = dir
    this.path = join(dir, FILE)
    this.io = { ...nodeFs, ...io }
    this.byKey = new Map()
    this.records = []
    this.damaged = [] // { offset, length, sha256, kind: 'torn_tail' | 'malformed_line' | 'not_a_suggestion', basis }
    this.acked = new Set() // keys this process acknowledged as newly written
    this.unconfirmed = new Set() // keys whose write failed before durability was confirmed
    this.uncertain = false // a write failed after it may have reached the file: reconcile before the next one
    this.load()
  }

  get corrupt() {
    return this.damaged.length
  }

  load() {
    this.byKey = new Map()
    this.records = []
    this.damaged = []
    if (!nodeFs.existsSync(this.path)) return
    const bytes = nodeFs.readFileSync(this.path)
    let start = 0
    while (start < bytes.length) {
      const nl = bytes.indexOf(0x0a, start)
      const end = nl < 0 ? bytes.length : nl
      const line = bytes.subarray(start, end)
      if (line.length) this.take(line, start, nl < 0)
      start = end + 1
    }
  }

  take(line, offset, tail) {
    let r = null
    try {
      r = JSON.parse(line.toString('utf8'))
    } catch {}
    if (r?.format === FORMAT && typeof r.key === 'string') {
      // A complete record kept even if its newline never reached the disk.
      if (!this.byKey.has(r.key)) this.byKey.set(r.key, r)
      this.records.push(r)
      return
    }
    this.damaged.push({
      offset,
      length: line.length,
      sha256: sha(line),
      kind: r === null ? (tail ? 'torn_tail' : 'malformed_line') : 'not_a_suggestion',
      basis: r === null
        ? `${tail ? 'the file ends inside this fragment (a write cut short, e.g. by a crash)' : 'an unparseable line (e.g. a fragment a later record was started after)'}; kept on disk unchanged, skipped here`
        : 'valid JSON but not an rth.mind-suggestion/v1 record; kept on disk unchanged, skipped here',
    })
  }

  /**
   * Validate, then append once. Returns { record, duplicate }. Throws SuggestionError.
   * `context` is the server's provenance (Mind lifetime, retained event ids): stored beside the
   * proposal, outside its content digest, so a retry after a restart is still the same proposal.
   */
  append(input, now = Date.now(), context = null) {
    const b = input && typeof input === 'object' ? input : {}
    const str = (v, name, max, { required = true } = {}) => {
      if (v === undefined || v === null || v === '') {
        if (required) throw new SuggestionError(400, `${name} is required`)
        return null
      }
      if (typeof v !== 'string') throw new SuggestionError(400, `${name} must be a string`)
      if (v.length > max) throw new SuggestionError(400, `${name} is longer than ${max} characters`)
      return v
    }
    const key = str(b.key, 'key', LIMITS.key)
    if (!KEY.test(key)) throw new SuggestionError(400, 'key must be 8-128 characters of A-Z a-z 0-9 . _ : -')
    const text = str(b.text, 'text', LIMITS.text).trim()
    if (!text) throw new SuggestionError(400, 'text is required')
    const actor = (str(b.actor, 'actor', LIMITS.actor, { required: false }) ?? '').trim() || 'unnamed person'
    const scope = b.scope && typeof b.scope === 'object' ? b.scope : {}
    const episode = str(scope.episode, 'scope.episode', LIMITS.id)
    const event = str(scope.event, 'scope.event', LIMITS.id, { required: false })
    const episodeTitle = str(scope.title, 'scope.title', 400, { required: false })
    const content = { actor, scope: { episode, event, title: episodeTitle }, text }
    const digest = sha(JSON.stringify(redact(content)))
    if (this.uncertain) this.reconcile()
    const prior = this.byKey.get(key)
    if (prior) {
      if (prior.content_sha256 !== digest) throw new SuggestionError(409, `key ${key} was already used for a different suggestion`)
      // The first confirmed acknowledgement of a record an unconfirmed write left on disk is the
      // original outcome (stored once); later retries are duplicates.
      if (this.unconfirmed.has(key)) {
        this.unconfirmed.delete(key)
        this.acked.add(key)
        return { record: prior, duplicate: false, reconciled: 'written by the earlier attempt, found complete on disk, then fsynced (file and directory); not written again' }
      }
      return { record: prior, duplicate: true }
    }
    const record = {
      format: FORMAT,
      key,
      at: new Date(now).toISOString(),
      ...redact(content),
      content_sha256: digest,
      kind: 'proposal',
      source: 'UI',
      actor_basis: 'typed into the Mind panel by whoever holds this DSH browser session; not otherwise verified',
      effect: 'none: a proposal for people to read; it changes no prompt, setting, memory or behaviour',
      training_eligible: null,
      training_basis: 'unknown: a human note, not reviewed for training use',
      context: context ? redact(context) : null,
    }
    try {
      this.write(`${JSON.stringify(record)}\n`)
    } catch (e) {
      this.uncertain = true
      this.unconfirmed.add(key)
      // Not acknowledged. Whatever reached the disk is either a complete line (a retry with
      // this key then finds it after a reload) or a fragment the next append fences off.
      throw new SuggestionError(500, `the suggestion was not stored durably (${e.code ?? e.message}); retry with the same key`)
    }
    this.byKey.set(key, record)
    this.records.push(record)
    this.acked.add(key)
    this.unconfirmed.delete(key)
    return { record, duplicate: false }
  }

  /** After an unconfirmed write: trust only what is on disk, and only once it is fsynced. */
  reconcile() {
    const { io } = this
    try {
      if (nodeFs.existsSync(this.path)) {
        const fd = io.openSync(this.path, 'r')
        try {
          io.fsyncSync(fd)
        } finally {
          io.closeSync(fd)
        }
        const dfd = io.openSync(dirname(this.path), 'r')
        try {
          io.fsyncSync(dfd)
        } finally {
          io.closeSync(dfd)
        }
      }
    } catch (e) {
      throw new SuggestionError(500, `an earlier write is still not confirmed durable (${e.code ?? e.message}); nothing was acknowledged; retry with the same key`)
    }
    this.load()
    this.uncertain = false
    return true
  }

  /** One record as its own line: a leading newline if the file does not end in one, then fsync. */
  write(line) {
    const { io } = this
    const fresh = !nodeFs.existsSync(this.path)
    nodeFs.mkdirSync(this.dir, { recursive: true })
    const fd = io.openSync(this.path, 'a+', 0o600) // O_APPEND for writes; reads at a position allowed
    try {
      const size = io.fstatSync(fd).size
      let lead = ''
      if (size > 0) {
        const last = Buffer.alloc(1)
        io.readSync(fd, last, 0, 1, size - 1)
        if (last[0] !== 0x0a) lead = '\n'
      }
      const buf = Buffer.from(lead + line, 'utf8')
      for (let off = 0; off < buf.length; ) {
        const n = io.writeSync(fd, buf, off, buf.length - off)
        if (!(n > 0)) throw new Error('short write')
        off += n
      }
      io.fsyncSync(fd)
    } finally {
      io.closeSync(fd)
    }
    // A new file's directory entry must be durable too, or the fsynced record can vanish with it.
    if (fresh) {
      const dfd = io.openSync(dirname(this.path), 'r')
      try {
        io.fsyncSync(dfd)
      } finally {
        io.closeSync(dfd)
      }
    }
  }

  list({ episode, episodes } = {}) {
    const ids = episodes ?? (episode ? [episode] : null)
    return ids ? this.records.filter((r) => ids.includes(r.scope?.episode)) : [...this.records]
  }
}

const sha = (s) => createHash('sha256').update(s).digest('hex')
