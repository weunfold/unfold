// Mind follows THIS app instance's host journal, named by UNFOLD_HOST_JOURNAL, which the app will
// set on its instance child (reserved; not set yet): <data>/app/<label>/host.jsonl, the app's own dir. Pure path parsing (the
// resolver opens nothing), off with a reason for any path outside that layout, never another
// instance's log, and the variable is taken out of the environment at load.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { Context } from '@deepseek-ai/cordis'
import { loadCatalog } from './support/catalog.js'
import { EventWindow } from './support/window.js'
import * as plugin from '../index.js'
import { HOST_JOURNAL_ENV, hostJournalFor, resolveHostJournal, takeHostJournal } from '../src/host-binding.js'

test('resolver: the app layout binds data root and label; the path is parsed, never opened', () => {
  assert.equal(HOST_JOURNAL_ENV, 'UNFOLD_HOST_JOURNAL')
  assert.deepEqual(hostJournalFor('/srv/proof/app/native-proof-2/host.jsonl'), { path: '/srv/proof/app/native-proof-2/host.jsonl', data: '/srv/proof', label: 'native-proof-2', from: '/srv/proof/app/native-proof-2/host.jsonl' })
  for (const v of ['/a/b/../c/app/x.y-1/host.jsonl', '/x/app/lab/../../etc/host.jsonl', '/x/app/lab/../lab/host.jsonl']) {
    const r = hostJournalFor(v)
    assert.equal(r.path, null, v)
    assert.match(r.why, /has a '\.\.' segment, which is refused rather than resolved/, v)
  }
  const missing = '/definitely/not/here/app/lab/host.jsonl'
  assert.equal(hostJournalFor(missing).path, missing, 'binding is pure: nothing on disk is consulted')
  const src = readFileSync(new URL('../src/host-binding.js', import.meta.url), 'utf8')
  assert.doesNotMatch(src, /from 'node:fs'|readFile|statSync|existsSync|openSync/, 'the resolver module imports no filesystem access')
})

test('resolver: a path outside the instance layout is refused with a reason, never a guess', () => {
  for (const v of [undefined, null, '', '   ']) assert.match(hostJournalFor(v).why, /UNFOLD_HOST_JOURNAL is not set/)
  assert.match(hostJournalFor('data/app/lab/host.jsonl').why, /not an absolute path/)
  const outside = [
    '/x/app/lab/other.jsonl', '/x/apps/lab/host.jsonl', '/x/app/host.jsonl', '/x/app/la b/host.jsonl',
    '/x/app/lab/sub/host.jsonl', '/x/app/.hidden/host.jsonl', `/x/app/${'a'.repeat(65)}/host.jsonl`,
    '/tmp/host.jsonl', '/x/host/lab/token', '/x/instances/i/agentd/journal.jsonl',
  ]
  for (const v of outside) {
    const r = hostJournalFor(v)
    assert.equal(r.path, null, v)
    assert.match(r.why, /is not <data>\/app\/<label>\/host\.jsonl/, v)
  }
  assert.equal(hostJournalFor(`/x/app/${'a'.repeat(64)}/host.jsonl`).label.length, 64, 'the app allows labels up to 64')
})

test('config: explicit journals.host wins; the flag binds only when it is empty; off by default', () => {
  const value = '/srv/d/app/lab/host.jsonl'
  assert.equal(resolveHostJournal({ host: '/explicit.jsonl', hostFromInstance: true }, value).path, '/explicit.jsonl')
  assert.equal(resolveHostJournal({ host: '', hostFromInstance: true }, value).path, value)
  assert.match(resolveHostJournal({ host: '', hostFromInstance: true }, value).detail, /bound to this app instance: data \/srv\/d, label lab, from UNFOLD_HOST_JOURNAL/)
  assert.equal(resolveHostJournal({ host: '' }, value).path, null, 'the flag defaults off: a profile opts in')
  assert.match(resolveHostJournal({ host: '', hostFromInstance: true }, '/tmp/dummy-host-journal').detail, /^host journal binding unsupported: /)
  assert.match(resolveHostJournal({ host: '', hostFromInstance: true }, null).detail, /^host journal binding unsupported: UNFOLD_HOST_JOURNAL is not set/)
})

test('take: read once from the env and deleted; the preload registry wins; a reload keeps the launch value', () => {
  const env = { UNFOLD_HOST_JOURNAL: '/srv/d/app/lab/host.jsonl', OTHER: '1' }
  assert.equal(takeHostJournal(env, undefined), '/srv/d/app/lab/host.jsonl')
  assert.deepEqual(env, { OTHER: '1' }, 'the variable is gone after the first read')
  assert.equal(takeHostJournal(env, undefined), '/srv/d/app/lab/host.jsonl', 'a later load (config change, reload) keeps the launch value')
  const channel = Object.freeze({ hostJournal: '/srv/e/app/lab/host.jsonl' })
  assert.equal(takeHostJournal({ UNFOLD_HOST_JOURNAL: '/srv/f/app/lab/host.jsonl' }, channel), '/srv/e/app/lab/host.jsonl', 'the preload registry wins over the env')
})

test('take: a tool spawned after load has no UNFOLD_HOST_JOURNAL', () => {
  const env = { ...process.env, UNFOLD_HOST_JOURNAL: '/srv/d/app/lab/host.jsonl' }
  takeHostJournal(env, undefined)
  const seen = execFileSync(process.execPath, ['-e', 'process.stdout.write(String(process.env.UNFOLD_HOST_JOURNAL))'], { env, encoding: 'utf8' })
  assert.equal(seen, 'undefined')
})

// A fresh process per case: the launch value is module state, so each case starts with none.
function takeIn(setup) {
  const mod = new URL('../src/host-binding.js', import.meta.url).href
  const code = `${setup}\nconst { takeHostJournal } = await import(${JSON.stringify(mod)})\nconst got = takeHostJournal()\nprocess.stdout.write(JSON.stringify({ got, env: process.env.UNFOLD_HOST_JOURNAL ?? null }))`
  const env = { ...process.env, UNFOLD_HOST_JOURNAL: '/srv/env/app/lab/host.jsonl' }
  return JSON.parse(execFileSync(process.execPath, ['--input-type=module', '-e', code], { env, encoding: 'utf8' }))
}

test('take: the launcher registry key is hostJournal under Symbol.for("unfold.channel"); the env is used only with no registry', () => {
  assert.deepEqual(takeIn('globalThis[Symbol.for("unfold.channel")] = Object.freeze({ hostJournal: "/srv/reg/app/lab/host.jsonl" })'),
    { got: '/srv/reg/app/lab/host.jsonl', env: null }, 'the registry value, and the variable is deleted')
  assert.deepEqual(takeIn('globalThis[Symbol.for("unfold.channel")] = Object.freeze({ socket: "/s" })'),
    { got: null, env: null }, 'a registry without hostJournal: the preload had none, so the env is not consulted')
  assert.deepEqual(takeIn('globalThis[Symbol.for("unfold.channel")] = Object.freeze({ host_journal: "/srv/reg/app/lab/host.jsonl" })'),
    { got: null, env: null }, 'only the exact key counts')
  assert.deepEqual(takeIn(''), { got: '/srv/env/app/lab/host.jsonl', env: null }, 'no registry: the env, read once and deleted')
})

// ---- through the real plugin: one Mind instance, the bound journal followed, a decoy never read ----

const catalog = loadCatalog()
const J = (...recs) => recs.map((r) => `${JSON.stringify(r)}\n`).join('')
async function loaded(dir, config) {
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  ctx.provide('unfoldEvents', { window: new EventWindow(catalog, { limit: 200 }), catalog, snapshot: () => ({ link: { state: 'attached', detail: 'observer' } }) })
  const fiber = ctx.plugin(plugin, { dataDir: join(dir, 'mind-data'), ...config })
  await fiber
  // Bounded, on a ref'd timer: the route is there, or the test fails with one line.
  for (const end = Date.now() + 2000; !routes.some((r) => r.path === '/api/unfold-mind'); ) {
    if (Date.now() > end) throw new Error('the snapshot route did not register in 2000 ms')
    await new Promise((r) => setTimeout(r, 5))
  }
  const snap = async () => (await (await routes.find((r) => r.path === '/api/unfold-mind').fetch(new Request('http://h/api/unfold-mind'))).json())
  return { fiber, snap }
}

test('plugin: the bound instance journal is followed; a decoy under another label is never ingested; unsupported is off with its reason', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-mind-bind-'))
  const saved = process.env.UNFOLD_HOST_JOURNAL
  try {
    const data = join(dir, 'data')
    mkdirSync(join(data, 'app', 'lab-a'), { recursive: true })
    mkdirSync(join(data, 'app', 'decoy'), { recursive: true })
    writeFileSync(join(data, 'app', 'lab-a', 'host.jsonl'), J({ kind: 'started', pid: 11, t_ms: 0 }, { kind: 'response_started', response_id: 'bound-reply', request_id: 'mic-1', t_ms: 100 }, { kind: 'response_done', response_id: 'bound-reply', status: 'completed', t_ms: 400 }))
    writeFileSync(join(data, 'app', 'decoy', 'host.jsonl'), J({ kind: 'started', pid: 22, t_ms: 0 }, { kind: 'response_started', response_id: 'DECOY-reply', request_id: 'mic-9', t_ms: 100 }))
    process.env.UNFOLD_HOST_JOURNAL = join(data, 'app', 'lab-a', 'host.jsonl')
    const a = await loaded(dir, { journals: { hostFromInstance: true } })
    const s = await a.snap()
    assert.equal(s.sources.host_journal.state, 'following')
    assert.ok(s.sources.host_journal.detail.startsWith(join(data, 'app', 'lab-a', 'host.jsonl')), s.sources.host_journal.detail)
    const text = JSON.stringify(s.rows)
    assert.match(text, /bound-reply/)
    assert.doesNotMatch(text, /DECOY-reply/, 'another instance\'s journal is never read')
    await a.fiber.dispose()

    process.env.UNFOLD_HOST_JOURNAL = join(dir, 'dummy-host-journal') // outside the layout: unsupported
    const b = await loaded(dir, { journals: { hostFromInstance: true } })
    const sb = await b.snap()
    assert.equal(sb.sources.host_journal.state, 'off')
    assert.match(sb.sources.host_journal.detail, /^host journal binding unsupported: .* is not <data>\/app\/<label>\/host\.jsonl/)
    assert.doesNotMatch(JSON.stringify(sb.rows), /bound-reply|DECOY-reply/, 'unsupported: no fallback to any log')
    await b.fiber.dispose()

    process.env.UNFOLD_HOST_JOURNAL = join(data, 'app', 'lab-missing', 'host.jsonl') // in the layout, no journal yet
    const c = await loaded(dir, { journals: { hostFromInstance: true } })
    const sc = await c.snap()
    assert.equal(sc.sources.host_journal.state, 'missing', 'a missing journal is reported, not invented')
    assert.doesNotMatch(JSON.stringify(sc.rows), /bound-reply|DECOY-reply/)
    await c.fiber.dispose()

    assert.equal(process.env.UNFOLD_HOST_JOURNAL, undefined, 'the variable is taken out of the environment at load')
    const d = await loaded(dir, { journals: { hostFromInstance: true } })
    assert.equal((await d.snap()).sources.host_journal.state, 'missing', 'a reload keeps the launch value')
    await d.fiber.dispose()
  } finally {
    if (saved === undefined) delete process.env.UNFOLD_HOST_JOURNAL
    else process.env.UNFOLD_HOST_JOURNAL = saved
    rmSync(dir, { recursive: true, force: true })
  }
})

test('plugin: a bound host journal that is a symbolic link is refused with its reason; its records are never shown', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-mind-link-'))
  const saved = process.env.UNFOLD_HOST_JOURNAL
  try {
    const data = join(dir, 'data')
    mkdirSync(join(data, 'app', 'lab'), { recursive: true })
    writeFileSync(join(dir, 'other.jsonl'), J({ kind: 'started', pid: 33, t_ms: 0 }, { kind: 'response_started', response_id: 'LINKED-reply', request_id: 'mic-3', t_ms: 100 }))
    symlinkSync(join(dir, 'other.jsonl'), join(data, 'app', 'lab', 'host.jsonl'))
    process.env.UNFOLD_HOST_JOURNAL = join(data, 'app', 'lab', 'host.jsonl')
    const a = await loaded(dir, { journals: { hostFromInstance: true } })
    const s = await a.snap()
    assert.equal(s.sources.host_journal.state, 'refused')
    assert.match(s.sources.host_journal.detail, /symbolic link/)
    assert.doesNotMatch(JSON.stringify(s.rows), /LINKED-reply/)
    await a.fiber.dispose()
  } finally {
    if (saved === undefined) delete process.env.UNFOLD_HOST_JOURNAL
    else process.env.UNFOLD_HOST_JOURNAL = saved
    rmSync(dir, { recursive: true, force: true })
  }
})
