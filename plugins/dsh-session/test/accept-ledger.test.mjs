// The acceptance run's worker spend ledger. Temporary directories only; no provider.
import test from 'node:test'
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { chmodSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { AcceptLedger, initSpend, SPEND_SCHEMA } from '../src/delegate/accept-ledger.mjs'

const WORKER = { dir: 'worker', cap_pusd: 700_000_000_000, max_attempts: 1, attempt_bound_pusd: 618_000_000_000, model: 'gpt-5.4-mini', context_window: 400_000, options: { maxSteps: 1, attemptsPerStep: 1, maxOutputTokens: 2000, reasoningEffort: 'off', routeMaxRetries: 0 }, prices: { input: 1_500_000, cached: 150_000, output: 9_000_000, source: 'test fixture', read: '2026-10-23' } }
const REALTIME = { dir: 'realtime', cap_pusd: 4_300_000_000_000, max_attempts: 12, attempt_bound_pusd: 536_600_000_000, model: 'gpt-realtime-2.1', context_window: 128_000, options: { text_only: true, vad: 'none', max_output_tokens: 1024 }, prices: { text_in: 4_000_000, text_cached: 400_000, text_out: 24_000_000, audio_in: 32_000_000, audio_cached: 400_000, audio_out: 64_000_000, source: 'test fixture', read: '2026-10-23' } }
const EXPECT = { model: 'gpt-5.4-mini', options: { maxSteps: 1, attemptsPerStep: 1, maxOutputTokens: 2000, reasoningEffort: 'off' } }

function spendDir(worker = WORKER) {
  const dir = join(mkdtempSync(join(tmpdir(), 'rth-spend-')), 'spend')
  const sha = initSpend(dir, { schema: SPEND_SCHEMA, accept_id: 'acc-test-1', stop: 'stop.json', layers: { realtime: REALTIME, worker } })
  const open = (o = {}) => AcceptLedger.open({ control: join(dir, 'control.json'), controlSha256: sha, layer: 'worker', expect: EXPECT, ...o })
  return { dir, sha, open }
}
const usage = (i, o) => ({ inputTokens: i, outputTokens: o, totalTokens: i + o })
const meta = (k) => ({ attemptId: `L1:task-1:${k}`, fields: { task_id: 'task-1', step: k } })

test('init creates the record and both ledgers once; a second init refuses', () => {
  const { dir } = spendDir()
  assert.deepEqual(readdirSync(dir).sort(), ['control.json', 'realtime', 'worker'])
  assert.throws(() => initSpend(dir, { schema: SPEND_SCHEMA, accept_id: 'acc-test-2', stop: 'stop.json', layers: { worker: WORKER } }), /EEXIST/)
  assert.throws(() => initSpend(join(dir, 'x'), { schema: SPEND_SCHEMA, accept_id: 'a', stop: 's', layers: { a: { ...WORKER, cap_pusd: 4_400_000_000_000 }, b: { ...WORKER, cap_pusd: 700_000_000_000 } } }), /exceed/)
})

test('a changed record (cap or options) under the same accept_id, or a changed effective config, refuses', () => {
  const { dir, sha, open } = spendDir()
  assert.equal(open().ok, true)
  const c = JSON.parse(readFileSync(join(dir, 'control.json'), 'utf8'))
  c.layers.worker.cap_pusd += 1
  chmodSync(join(dir, 'control.json'), 0o600)
  writeFileSync(join(dir, 'control.json'), JSON.stringify(c))
  assert.match(open().reason, /not the pinned one/)
  const fresh = spendDir()
  assert.match(fresh.open({ expect: { ...EXPECT, options: { ...EXPECT.options, maxOutputTokens: 4000 } } }).reason, /maxOutputTokens/)
  assert.match(fresh.open({ expect: { ...EXPECT, model: 'gpt-5.4' } }).reason, /model/)
  assert.equal(sha.length, 64)
})

test('a missing established ledger after restart is never recreated', () => {
  const { dir, open } = spendDir()
  rmSync(join(dir, 'worker'), { recursive: true })
  const r = open()
  assert.equal(r.ok, false)
  assert.match(r.reason, /missing or torn.*never recreated/)
  assert.equal(existsSync(join(dir, 'worker')), false)
})

test('one worker attempt, durably: a zero-cost first result does not permit a second, even after restart', () => {
  const { open } = spendDir()
  const a = open().ledger
  const first = a.admit(meta(1))
  assert.equal(first.ok, true)
  assert.deepEqual(a.settle(first.attempt, { usage: usage(0, 0) }), { ok: true, cost_pusd: 0 })
  assert.match(a.admit(meta(2)).reason, /allowance of 1 is used/)
  const restarted = open().ledger
  assert.match(restarted.admit(meta(2)).reason, /allowance of 1 is used/)
})

test('concurrent owners: a claim lost to another owner re-reads; across processes exactly one attempt is admitted', async () => {
  const { dir, sha, open } = spendDir()
  // In-process: B claims while A is between its read and its claim.
  const b = open().ledger
  const a = open({ hooks: { beforeClaim: () => { if (!b.raced) { b.raced = true; assert.equal(b.admit(meta('b')).ok, true) } } } }).ledger
  assert.match(a.admit(meta('a')).reason, /allowance of 1 is used/)
  // Across processes: eight racers on a fresh ledger.
  const x = spendDir()
  const url = new URL('../src/delegate/accept-ledger.mjs', import.meta.url).href
  const racer = `const { AcceptLedger } = await import(${JSON.stringify(url)}); const o = AcceptLedger.open({ control: process.argv[1], controlSha256: process.argv[2], layer: 'worker', expect: ${JSON.stringify(EXPECT)} }); process.stdout.write(JSON.stringify(o.ledger.admit({ attemptId: 'p' + process.pid, fields: {} }).ok))`
  const runs = await Promise.all(Array.from({ length: 8 }, () => new Promise((res) => { const p = spawn(process.execPath, ['--input-type=module', '-e', racer, join(x.dir, 'control.json'), x.sha]); let o = ''; p.stdout.on('data', (d) => { o += d }); p.on('exit', () => res(o)) })))
  assert.equal(runs.filter((r) => r === 'true').length, 1, runs.join(' '))
  assert.equal(readdirSync(join(x.dir, 'worker')).filter((f) => /^attempt-\d{3}\.json$/.test(f)).length, 1)
  assert.ok(dir && sha)
})

test('a torn reserve fails the layer and writes the stop marker first', () => {
  const { dir, open } = spendDir()
  writeFileSync(join(dir, 'worker', 'attempt-000.json'), '{"n":0,"attempt_id":"x"')
  const r = open().ledger.admit(meta(1))
  assert.equal(r.failed, true)
  assert.match(r.reason, /does not parse/)
  assert.equal(JSON.parse(readFileSync(join(dir, 'stop.json'), 'utf8')).layer, 'worker')
})

test('missing usage fails, keeps the reservation, and a restart stays stopped', () => {
  const { dir, open } = spendDir({ ...WORKER, max_attempts: 3, cap_pusd: 700_000_000_000 })
  const a = open().ledger
  const first = a.admit(meta(1))
  assert.equal(a.settle(first.attempt, { usage: null }).failed, true)
  assert.equal(existsSync(join(dir, 'worker', 'attempt-000.settle.json')), false, 'the reservation is kept')
  const restarted = open().ledger
  assert.equal(restarted.admit(meta(2)).reason, 'spend: stopped')
})

test('an identical replayed settlement is harmless; a conflicting one fails', () => {
  const { open } = spendDir()
  const a = open().ledger
  const first = a.admit(meta(1))
  assert.equal(a.settle(first.attempt, { usage: usage(3000, 400) }).ok, true)
  assert.equal(a.settle(first.attempt, { usage: usage(3000, 400) }).ok, true, 'replay')
  assert.equal(a.stopped(), null)
  const c = a.settle(first.attempt, { usage: usage(3000, 401) })
  assert.match(c.reason, /conflicting settlement/)
  assert.equal(a.stopped(), 'spend: stopped')
})

test('the stop marker, present or unreadable, refuses; a marker that cannot be written stops this process', () => {
  const { dir, open } = spendDir()
  writeFileSync(join(dir, 'stop.json'), '{"layer":"realtime"}')
  assert.equal(open().ledger.admit(meta(1)).reason, 'spend: stopped', 'the other layer\'s stop')
  const x = spendDir()
  const l = x.open().ledger
  l.stopPath = join(x.dir, 'missing-dir', 'stop.json')
  const f = l.fail('test failure')
  assert.equal(f.failed, true)
  assert.match(l.stopped(), /stop marker not written/)
  assert.match(l.admit(meta(1)).reason, /stop marker not written/)
  // A marker path whose stat fails other than ENOENT is stopped too.
  const y = spendDir()
  const m = y.open().ledger
  m.stopPath = join(y.dir, 'control.json', 'stop.json') // ENOTDIR
  assert.match(m.stopped(), /unreadable \(ENOTDIR\)/)
})

test('a cost above the bound fails; within the cap, settled cost frees room only up to the allowance', () => {
  const { open } = spendDir({ ...WORKER, max_attempts: 2 })
  const a = open().ledger
  const first = a.admit(meta(1))
  assert.equal(a.admit(meta(2)).ok, false, 'two open bounds exceed the cap')
  assert.equal(a.settle(first.attempt, { usage: usage(1000, 100) }).ok, true)
  assert.equal(a.admit(meta(2)).ok, true, 'a settled attempt frees its bound; the allowance still counts it')
  assert.match(a.admit(meta(3)).reason, /allowance of 2 is used/)
  const b = spendDir().open().ledger
  const big = b.admit(meta(1))
  assert.match(b.settle(big.attempt, { usage: usage(400_001, 2000) }).reason, /above its bound/)
})
