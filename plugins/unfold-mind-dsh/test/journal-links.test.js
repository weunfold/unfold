// A journal is read only where it really is: a symbolic link at the file, or at a directory between
// the instance's data dir and the file (<data>/app, <data>/app/<label>), is refused with a note, and
// so is any path with a '..' segment. The tail never follows a link to another file's records.
import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { JsonlTail, followHostJournal } from '../src/journal.js'

const J = (...recs) => recs.map((r) => `${JSON.stringify(r)}\n`).join('')
const RECS = J({ kind: 'started', pid: 7, t_ms: 0 }, { kind: 'response_started', response_id: 'linked-reply', request_id: 'mic-1', t_ms: 10 })

function fakeMind() {
  const got = { records: [], breaks: [] }
  return { got, host: (rec) => got.records.push(rec), hostBreak: (why) => got.breaks.push(why) }
}

function scratch(fn) {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-mind-links-'))
  try {
    const data = join(dir, 'data')
    const elsewhere = join(dir, 'elsewhere')
    mkdirSync(join(elsewhere, 'lab'), { recursive: true })
    writeFileSync(join(elsewhere, 'host.jsonl'), RECS)
    writeFileSync(join(elsewhere, 'lab', 'host.jsonl'), RECS)
    return fn({ dir, data, elsewhere })
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

function follow(path, below) {
  const mind = fakeMind()
  const notes = []
  const tail = followHostJournal(path, mind, { below, onNote: (t, m) => notes.push([t, m]) }).start()
  const state = tail.state
  tail.stop()
  return { state, notes, records: mind.got.records }
}

test('a host journal that is a symbolic link to another file is refused, and none of its records are read', () => scratch(({ data, elsewhere }) => {
  mkdirSync(join(data, 'app', 'lab'), { recursive: true })
  symlinkSync(join(elsewhere, 'host.jsonl'), join(data, 'app', 'lab', 'host.jsonl'))
  const r = follow(join(data, 'app', 'lab', 'host.jsonl'), data)
  assert.equal(r.records.length, 0, 'the link target is never read')
  assert.equal(r.state, 'refused')
  assert.deepEqual(r.notes.map(([t]) => t), ['refused'])
  assert.match(r.notes[0][1], /symbolic link/)
}))

test('a symbolic link at <data>/app/<label> or at <data>/app is refused too', () => scratch(({ data, elsewhere }) => {
  mkdirSync(join(data, 'app'), { recursive: true })
  symlinkSync(join(elsewhere, 'lab'), join(data, 'app', 'lab'))
  const a = follow(join(data, 'app', 'lab', 'host.jsonl'), data)
  assert.equal(a.records.length, 0)
  assert.equal(a.state, 'refused')
  assert.match(a.notes[0][1], /app\/lab is a symbolic link/)

  rmSync(join(data, 'app'), { recursive: true })
  symlinkSync(elsewhere, join(data, 'app'))
  const b = follow(join(data, 'app', 'lab', 'host.jsonl'), data)
  assert.equal(b.records.length, 0)
  assert.equal(b.state, 'refused')
  assert.match(b.notes[0][1], /data\/app is a symbolic link/)
}))

test("a journal path with a '..' segment is refused, never resolved", () => scratch(({ data, elsewhere }) => {
  mkdirSync(join(data, 'app', 'lab'), { recursive: true })
  const path = `${join(data, 'app', 'lab')}/../../../elsewhere/host.jsonl`
  const r = follow(path, data)
  assert.equal(r.records.length, 0)
  assert.equal(r.state, 'refused')
  assert.match(r.notes[0][1], /'\.\.' segment/)
  const plain = new JsonlTail(`${elsewhere}/lab/../host.jsonl`, { onRecord: () => assert.fail('read'), onNote: () => {} }).start()
  assert.equal(plain.state, 'refused', 'any journal, not only the instance-bound one')
}))

test('any journal that is itself a symbolic link is refused; a real file in real dirs is followed', () => scratch(({ dir, data, elsewhere }) => {
  const link = join(dir, 'agentd.jsonl')
  symlinkSync(join(elsewhere, 'host.jsonl'), link)
  const notes = []
  const got = []
  const t = new JsonlTail(link, { onRecord: (rec) => got.push(rec), onNote: (type) => notes.push(type) }).start()
  t.stop()
  assert.equal(got.length, 0)
  assert.deepEqual(notes, ['refused'])

  // Control: above <data> nothing is checked (the system temp dir may itself sit under a link).
  mkdirSync(join(data, 'app', 'lab'), { recursive: true })
  writeFileSync(join(data, 'app', 'lab', 'host.jsonl'), RECS)
  const ok = follow(join(data, 'app', 'lab', 'host.jsonl'), data)
  assert.equal(ok.state, 'following')
  assert.deepEqual(ok.records.map((x) => x.kind), ['started', 'response_started'])
  assert.deepEqual(ok.notes, [])
}))

test('a file swapped for a symbolic link after load is refused at the next read, and a break is noted before any record', () => scratch(({ data, elsewhere }) => {
  mkdirSync(join(data, 'app', 'lab'), { recursive: true })
  const path = join(data, 'app', 'lab', 'host.jsonl')
  writeFileSync(path, J({ kind: 'started', pid: 1, t_ms: 0 }))
  const mind = fakeMind()
  const notes = []
  const tail = followHostJournal(path, mind, { below: data, onNote: (t) => notes.push(t) }).start()
  tail.stop() // read() is driven by hand below; no watcher races the swap
  assert.equal(mind.got.records.length, 1)
  renameSync(path, join(data, 'app', 'lab', 'old.jsonl'))
  symlinkSync(join(elsewhere, 'host.jsonl'), path)
  tail.read(false, false)
  assert.equal(mind.got.records.length, 1, 'nothing is read through the link')
  assert.deepEqual(notes, ['refused'])
  rmSync(path)
  writeFileSync(path, J({ kind: 'started', pid: 2, t_ms: 0 }))
  tail.read(false, false)
  assert.deepEqual(notes, ['refused', 'missing'], 'the real file is read again from its start, after a discontinuity')
  assert.ok(mind.got.breaks.length >= 1)
  assert.equal(mind.got.records.at(-1).pid, 2)
}))
