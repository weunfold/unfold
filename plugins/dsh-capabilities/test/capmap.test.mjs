import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { Context } from '@deepseek-ai/cordis'
import * as plugin from '../index.mjs'
import { computeMap, loadRegistry, loadRuns } from '../src/capmap.mjs'
import { needRth, rthFile } from '../../../test/rth-checkout.mjs'

// The synthetic checks need only a valid registry document: the real capabilities/registry.json
// from the rth checkout when it is there, else a minimal one in the same format.
const REAL = needRth('capabilities/registry.json')
const registry = REAL.skip
  ? { registry_format: 'rth.capabilities/v2', registry_version: 0, rules: [], statuses: {}, groups: { capability: 'synthetic test group' }, comparison: null, cells: [] }
  : loadRegistry(rthFile('capabilities/registry.json'))

const SUITE = { id: 'synthetic', sha256: 'a'.repeat(64), epoch: 1 }
const SCORER = { id: 'rth-bench.synthetic', version: '1.0.0' }
const COHORT = { suite_id: SUITE.id, suite_sha256: SUITE.sha256, suite_epoch: SUITE.epoch, scorer_id: SCORER.id, scorer_version: SCORER.version, selection_split: 'promotion' }

const card = (overrides) => ({
  scorecard_format: 'rth-bench.scorecard/v1', run_id: 'r1', started_utc: '2026-10-21T00:00:00Z', eligible: true,
  suite_id: SUITE.id, suite_sha256: SUITE.sha256, suite_epoch: SUITE.epoch, scorer: SCORER,
  selection_split: 'promotion', candidate: { backend: 'fake@v1', capabilities: { text_in: true, text_out: true } },
  tasks: [{ task: 'qa.answer@1', splits: { promotion: { score: 1, status: 'pass' } } }], probes: [], envelopes: [], ...overrides,
})

/** The manifest rth-bench writes beside a scorecard, consistent with it unless overridden. */
const manifestFor = (s, overrides = {}) => ({
  manifest_format: 'rth-bench.run-manifest/v1', run_id: s.run_id, status: 'complete',
  suite: { id: s.suite_id, sha256: s.suite_sha256, epoch: s.suite_epoch }, scorer: s.scorer,
  candidate: { backend: s.candidate.backend, agentd_sha256: 'b'.repeat(64), agentd_sha256_in_guest: 'b'.repeat(64) },
  tasks: [...s.tasks, ...s.probes].map((t) => ({ task: t.task, sha256: 'c'.repeat(64) })), ...overrides,
})

/** A results dir holding synthetic runs; `manifest: null` leaves one out. */
function resultsWith(...entries) {
  const dir = mkdtempSync(join(tmpdir(), 'capmap-'))
  for (const entry of entries) {
    const { scorecard, manifest } = entry.scorecard ? entry : { scorecard: entry }
    mkdirSync(join(dir, scorecard.run_id))
    writeFileSync(join(dir, scorecard.run_id, 'scorecard.json'), JSON.stringify(scorecard))
    const m = manifest === undefined ? manifestFor(scorecard) : manifest
    if (m) writeFileSync(join(dir, scorecard.run_id, 'manifest.json'), JSON.stringify(m))
  }
  return dir
}

const oneCell = (cell) => ({ ...registry, comparison: COHORT, cells: [{ id: 'x', group: 'capability', title: 'x', evidence: ['qa.answer@1'], ...cell }] })
const mapOf = (cell, ...entries) => computeMap(oneCell(cell), loadRuns(resultsWith(...entries))).cells[0]

test('a fake backend never makes a real-model capability supported', () => {
  const fake = mapOf({ requires: { real_model: true } }, card())
  assert.deepEqual([fake.status, fake.reason_code], ['unsupported', 'prerequisite_missing'])
  assert.match(fake.reason, /fake@v1 lacks a real model/)
  const real = mapOf({ requires: { real_model: true } }, card({ candidate: { backend: 'gemini-3.8-live', capabilities: {} } }))
  assert.equal(real.status, 'supported')
  const untested = mapOf({ evidence: [] })
  assert.deepEqual([untested.status, untested.reason_code], ['unsupported', 'not_evaluated'], 'untested is not the same as impossible')
})

test('a failing task, an ineligible card and a text proxy each say so; ineligible runs are listed apart', () => {
  const failing = card({ tasks: [{ task: 'qa.answer@1', splits: { promotion: { score: 0.5, status: 'fail' } } }] })
  assert.deepEqual([mapOf({}, failing).status, mapOf({}, failing).reason_code], ['failing', 'task_failed'])
  const later = card({ run_id: 'r2', started_utc: '2026-10-21T01:00:00Z', eligible: false, eligibility: [{ rule: 'no_critical_veto', pass: false }] })
  const ineligible = mapOf({}, card(), later)
  assert.deepEqual([ineligible.status, ineligible.reason_code, ineligible.run], ['failing', 'scorecard_ineligible', 'r2'])
  assert.deepEqual(ineligible.ineligible_runs, [{ run: 'r2', score: 1, why: ['no_critical_veto'] }])
  assert.deepEqual(ineligible.trend.map((t) => t.run), ['r1'], 'an ineligible run never enters the trend')
  assert.equal(mapOf({ proxy: true }, card()).status, 'proxy')
  assert.equal(mapOf({ proxy: true }, card()).proxy_kind, 'text')
  assert.equal(mapOf({ proxy: true, proxy_kind: 'transport' }, card()).proxy_kind, 'transport')
})

test('task@1 and task@999 are different tasks: no shared best or trend', () => {
  const v1 = card({ run_id: 'r1', tasks: [{ task: 'tc.example@1', splits: { promotion: { score: 1, status: 'pass' } } }] })
  const v999 = card({ run_id: 'r2', started_utc: '2026-10-21T01:00:00Z', tasks: [{ task: 'tc.example@999', splits: { promotion: { score: 0.4, status: 'fail' } } }] })
  const at1 = mapOf({ evidence: ['tc.example@1'] }, v1, v999)
  assert.deepEqual([at1.status, at1.best, at1.trend.map((t) => t.score), at1.run], ['supported', 1, [1], 'r1'])
  assert.deepEqual(at1.incomparable_runs, [{ run: 'r2', why: 'does not score tc.example@1' }])
  const at999 = mapOf({ evidence: ['tc.example@999'] }, v1, v999)
  assert.deepEqual([at999.status, at999.best, at999.trend.map((t) => t.score)], ['failing', 0.4, [0.4]])
})

test('runs outside the comparison cohort, or with changed task content, are listed and never compared', () => {
  const otherSuite = card({ run_id: 'r2', started_utc: '2026-10-21T01:00:00Z', suite_sha256: 'd'.repeat(64), tasks: [{ task: 'qa.answer@1', splits: { promotion: { score: 0.2, status: 'fail' } } }] })
  const cell = mapOf({}, card(), otherSuite)
  assert.deepEqual([cell.status, cell.run, cell.trend.length], ['supported', 'r1', 1])
  assert.match(cell.incomparable_runs[0].why, /suite synthetic@1 \(dddddddddddd\) is not the cohort's/)
  const newerScorer = card({ run_id: 'r3', started_utc: '2026-10-21T02:00:00Z', scorer: { ...SCORER, version: '2.0.0' } })
  assert.match(mapOf({}, card(), newerScorer).incomparable_runs[0].why, /scorer rth-bench\.synthetic@2\.0\.0/)
  const rehashed = card({ run_id: 'r4', started_utc: '2026-10-21T03:00:00Z' })
  const drift = mapOf({}, card(), { scorecard: rehashed, manifest: manifestFor(rehashed, { tasks: [{ task: 'qa.answer@1', sha256: 'e'.repeat(64) }] }) })
  assert.deepEqual([drift.run, drift.incomparable_runs.map((r) => r.run), drift.trend.length], ['r4', ['r1'], 1])
})

test('a run selected on another split is not in the cohort', () => {
  const dev = card({ run_id: 'r1', selection_split: 'dev', tasks: [{ task: 'qa.answer@1', splits: { dev: { score: 1, status: 'pass' } } }] })
  const promotion = card({ run_id: 'r2', started_utc: '2026-10-21T01:00:00Z', tasks: [{ task: 'qa.answer@1', splits: { promotion: { score: 0.4, status: 'fail' } } }] })
  const cell = mapOf({}, dev, promotion)
  assert.deepEqual([cell.status, cell.best, cell.trend], ['failing', 0.4, [{ run: 'r2', score: 0.4 }]])
  assert.deepEqual(cell.incomparable_runs, [{ run: 'r1', why: "selection split dev is not the cohort's (promotion)" }])
})

test('prerequisites hold for every history member, not only the newest', () => {
  const fake = card({ run_id: 'r1' })
  const real = card({ run_id: 'r2', started_utc: '2026-10-21T01:00:00Z', candidate: { backend: 'gemini-3.8-live', capabilities: {} },
    tasks: [{ task: 'qa.answer@1', splits: { promotion: { score: 0.4, status: 'pass' } } }] })
  const cell = mapOf({ requires: { real_model: true } }, fake, real)
  assert.deepEqual([cell.status, cell.best, cell.trend], ['supported', 0.4, [{ run: 'r2', score: 0.4 }]])
  assert.deepEqual(cell.prerequisite_missing_runs, [{ run: 'r1', why: 'candidate fake@v1 lacks a real model' }])
  const newestFake = mapOf({ requires: { real_model: true } }, real, { ...fake, run_id: 'r3', started_utc: '2026-10-21T02:00:00Z' })
  assert.deepEqual([newestFake.status, newestFake.reason_code, newestFake.trend.map((t) => t.run)], ['unsupported', 'prerequisite_missing', ['r2']])
})

test('runs without provenance are unverified and stay out of comparable history', () => {
  const alone = mapOf({}, { scorecard: card(), manifest: null })
  assert.deepEqual([alone.status, alone.reason_code, alone.score], ['unverified', 'unverified_provenance', null])
  assert.deepEqual(alone.unverified_runs, [{ run: 'r1', why: ['no manifest.json'] }])
  const unproven = card({ run_id: 'r2', started_utc: '2026-10-21T01:00:00Z', tasks: [{ task: 'qa.answer@1', splits: { promotion: { score: 0, status: 'fail' } } }] })
  const swapped = manifestFor(unproven, { candidate: { backend: 'fake@v1', agentd_sha256: 'b'.repeat(64), agentd_sha256_in_guest: 'f'.repeat(64) } })
  const regs = oneCell({})
  const loaded = loadRuns(resultsWith(card(), { scorecard: unproven, manifest: swapped }))
  const [mixed] = computeMap(regs, loaded).cells
  assert.deepEqual([mixed.status, mixed.run, mixed.trend.map((t) => t.run)], ['supported', 'r1', ['r1']])
  assert.deepEqual(mixed.unverified_runs, [{ run: 'r2', why: ['guest ran a different agentd'] }])
  assert.equal(computeMap(regs, loaded).latest_measured_candidate.run, 'r1', 'an unverified run is never the measured candidate')
})

test('loads as a Cordis plugin, streams the map over HTTP, recomputes on notify, and unload closes streams', async () => {
  const dir = resultsWith(card())
  const ctx = new Context()
  const registered = []
  ctx.provide('webServer', { register: (route) => (registered.push(route), () => registered.splice(registered.indexOf(route), 1)) })
  const reg = join(dir, 'registry.json')
  writeFileSync(reg, JSON.stringify(oneCell({})))
  const fiber = ctx.plugin(plugin, { registry: reg, results: dir, transport: 'webServer' })
  await fiber
  await new Promise((r) => setImmediate(r))
  const service = ctx.get('rthCapabilities')
  assert.equal(service.map().cells[0].status, 'supported')
  assert.deepEqual(registered.map((r) => r.path), ['/api/rth-capabilities', '/api/rth-capabilities/refresh', '/api/rth-capabilities/stream'])
  const server = http.createServer((req, res) => registered.find((r) => r.path === req.url)?.handler(req, res) ?? res.writeHead(404).end())
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const { port } = server.address()

  const frames = []
  let onFrame = () => {}
  http.get({ host: '127.0.0.1', port, path: '/api/rth-capabilities/stream' }, (res) => {
    let buf = ''
    res.setEncoding('utf8')
    res.on('data', (chunk) => {
      buf += chunk
      for (let i; (i = buf.indexOf('\n\n')) >= 0; buf = buf.slice(i + 2)) {
        frames.push({ event: /^event: (.*)$/m.exec(buf.slice(0, i))[1], data: JSON.parse(/^data: (.*)$/m.exec(buf.slice(0, i))[1]) })
        onFrame()
      }
    })
  }).on('error', () => {})
  const frame = (n) => new Promise((resolve) => (frames.length >= n ? resolve(frames[n - 1]) : (onFrame = () => frames.length >= n && resolve(frames[n - 1]))))
  assert.equal((await frame(1)).data.cells[0].status, 'supported')

  mkdirSync(join(dir, 'r2'))
  const r2 = card({ run_id: 'r2', started_utc: '2026-10-21T01:00:00Z', tasks: [{ task: 'qa.answer@1', splits: { promotion: { score: 0, status: 'fail' } } }] })
  writeFileSync(join(dir, 'r2', 'scorecard.json'), JSON.stringify(r2))
  writeFileSync(join(dir, 'r2', 'manifest.json'), JSON.stringify(manifestFor(r2)))
  const body = await new Promise((resolve) => {
    const req = http.request({ host: '127.0.0.1', port, path: '/api/rth-capabilities/refresh', method: 'POST' }, (res) => {
      let text = ''
      res.on('data', (c) => (text += c))
      res.on('end', () => resolve(text))
    })
    req.end()
  })
  assert.deepEqual(JSON.parse(body), { latest_run: 'r2', runs: 2 })
  const pushed = (await frame(2)).data
  assert.equal(pushed.cells[0].status, 'failing')
  assert.deepEqual(pushed.cells[0].trend.map((t) => t.score), [1, 0])

  await fiber.dispose()
  assert.equal(registered.length, 0)
  assert.deepEqual([(await frame(3)).event, frames[2].data.reason, service.streams.clients.size], ['resnapshot', 'unloaded', 0])
  server.closeAllConnections()
  server.close()
})

test('DSH transport: the map, refresh and stream are connection.fetch routes', async () => {
  const dir = resultsWith(card())
  const reg = join(dir, 'registry.json')
  writeFileSync(reg, JSON.stringify(oneCell({})))
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  const fiber = ctx.plugin(plugin, { registry: reg, results: dir })
  await fiber
  await new Promise((r) => setImmediate(r))
  assert.deepEqual(routes.map((r) => [r.path, r.methods]), [
    ['/api/rth-capabilities', ['GET']], ['/api/rth-capabilities/refresh', ['POST']], ['/api/rth-capabilities/stream', ['GET']],
  ])
  const abort = new AbortController()
  const body = (await routes[2].fetch(new Request('http://h/api/rth-capabilities/stream', { signal: abort.signal }))).body.getReader()
  const read = async () => new TextDecoder().decode((await body.read()).value)
  assert.match(await read(), /^event: map\n/)
  const refreshed = await (await routes[1].fetch(new Request('http://h/api/rth-capabilities/refresh', { method: 'POST' }))).json()
  assert.equal(refreshed.runs, 1)
  assert.match(await read(), /^event: map\n/, 'refresh pushes the map to the open stream')
  abort.abort()
  await fiber.dispose()
  assert.equal(routes.length, 0)
})
