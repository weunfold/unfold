// Mind follows THIS app instance's host journal,
// bound from the RTH_HOST_TOKEN_FILE path (<data>/host/<label>/token -> <data>/app/<label>/rth-host.jsonl).
// Pure path parsing (never the token's contents), off-with-a-reason for any unsupported binding,
// never another instance's log, and the default launcher mapping checked offline only.
import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { Context } from '@deepseek-ai/cordis'
import { loadCatalog } from '../../dsh-events/src/catalog.mjs'
import { EventWindow } from '../../dsh-events/src/window.mjs'
import * as plugin from '../index.mjs'
import { hostJournalFor, resolveHostJournal } from '../src/host-binding.mjs'
import { needRth, rthFile } from '../../../test/rth-checkout.mjs'

test('resolver: the app layout binds data root and label; the token is a path, never read', () => {
  assert.deepEqual(hostJournalFor('/srv/rth-proof/host/native-proof-2/token'), { path: '/srv/rth-proof/app/native-proof-2/rth-host.jsonl', data: '/srv/rth-proof', label: 'native-proof-2', from: '/srv/rth-proof/host/native-proof-2/token' })
  assert.equal(hostJournalFor('/a/b/../c/host/x.y-1/token').path, '/a/c/app/x.y-1/rth-host.jsonl', 'normalized')
  const missing = '/definitely/not/here/host/lab/token'
  assert.equal(hostJournalFor(missing).path, '/definitely/not/here/app/lab/rth-host.jsonl', 'binding is pure: nothing on disk is consulted')
  const src = readFileSync(new URL('../src/host-binding.mjs', import.meta.url), 'utf8')
  assert.doesNotMatch(src, /from 'node:fs'|readFile|statSync|existsSync|openSync/, 'the resolver module imports no filesystem access')
})

test('resolver: every unsupported binding is off with a reason, never a guess', () => {
  for (const v of [undefined, '', '   ']) assert.match(hostJournalFor(v).why, /RTH_HOST_TOKEN_FILE is not set/)
  assert.match(hostJournalFor('data/host/lab/token').why, /not an absolute path/)
  for (const v of ['/x/host/lab/token.txt', '/x/hosts/lab/token', '/x/host/token', '/x/host/la b/token', '/x/host/lab/sub/token', '/tmp/dummy-host-token', '/x/app/lab/token']) {
    const r = hostJournalFor(v)
    assert.equal(r.path, null, v)
    assert.match(r.why, /is not <data>\/host\/<label>\/token/, v)
  }
})

test('config: explicit journals.host wins; the flag binds only when it is empty; off by default', () => {
  const env = { RTH_HOST_TOKEN_FILE: '/srv/d/host/lab/token' }
  assert.equal(resolveHostJournal({ host: '/explicit.jsonl', hostFromTokenFile: true }, env).path, '/explicit.jsonl')
  assert.equal(resolveHostJournal({ host: '', hostFromTokenFile: true }, env).path, '/srv/d/app/lab/rth-host.jsonl')
  assert.match(resolveHostJournal({ host: '', hostFromTokenFile: true }, env).detail, /bound to this app instance: data \/srv\/d, label lab, from the RTH_HOST_TOKEN_FILE path; the token is never read/)
  assert.equal(resolveHostJournal({ host: '' }, env).path, null, 'the flag defaults off: the live profile behaves as before until it opts in')
  assert.match(resolveHostJournal({ host: '', hostFromTokenFile: true }, { RTH_HOST_TOKEN_FILE: '/tmp/dummy-host-token' }).detail, /^host journal binding unsupported: /)
  assert.match(resolveHostJournal({ host: '', hostFromTokenFile: true }, {}).detail, /^host journal binding unsupported: RTH_HOST_TOKEN_FILE is not set/)
})

test('offline: the default launcher instance (port 7700) maps to its own host-7700 journal (no :7700 contact, no file read)', needRth('app/src-tauri/src/main.rs'), () => {
  // The defaults as written in the sources (checked as text, nothing run): the app's data root is
  // RTH_DATA_DIR or ~/.local/share/rth and its label host-<agent port>; app.mjs builds the same token path.
  const main = readFileSync(rthFile('app/src-tauri/src/main.rs'), 'utf8')
  const appMjs = readFileSync(new URL('../../../shell/app.mjs', import.meta.url), 'utf8')
  assert.match(main, /let host_dir = cfg\.data\.join\("host"\)\.join\(&cfg\.label\);\s*let token_file = host_dir\.join\("token"\);\s*let app_dir = cfg\.data\.join\("app"\)\.join\(&cfg\.label\);/)
  assert.match(main, /\.args\(\["--label", &cfg\.label, "--log"\]\)\s*\.arg\(app_dir\.join\("rth-host\.jsonl"\)\)/)
  assert.match(main, /label: get\("label"\)\.unwrap_or_else\(\|\| format!\("host-\{\}", agent\.map\(\|a\| a\.port\(\)\)\.unwrap_or\(0\)\)\)/)
  assert.match(main, /home\(\)\.join\("\.local\/share\/rth"\)/)
  assert.match(appMjs, /RTH_HOST_TOKEN_FILE: tokenFile/)
  assert.match(appMjs, /join\(data, 'host', `host-\$\{agentPort\}`, 'token'\)/)
  const home = '/home/example' // a placeholder: the real home is never read here
  assert.equal(hostJournalFor(`${home}/.local/share/rth/host/host-7700/token`).path, `${home}/.local/share/rth/app/host-7700/rth-host.jsonl`)
})

// ---- through the real plugin: one Mind instance, the bound journal followed, a decoy never read ----

const catalog = loadCatalog()
const J = (...recs) => recs.map((r) => `${JSON.stringify(r)}\n`).join('')
async function loaded(dir, config) {
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  ctx.provide('rthEvents', { window: new EventWindow(catalog, { limit: 200 }), catalog, snapshot: () => ({ link: { state: 'attached', detail: 'observer' } }) })
  const fiber = ctx.plugin(plugin, { dataDir: join(dir, 'mind-data'), ...config })
  await fiber
  await new Promise((r) => setTimeout(r, 50))
  const snap = async () => (await (await routes.find((r) => r.path === '/api/rth-mind').fetch(new Request('http://h/api/rth-mind'))).json())
  return { fiber, snap }
}

test('plugin: the bound instance journal is followed; a decoy under another label is never ingested; unsupported is off with its reason', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-n1-'))
  const saved = process.env.RTH_HOST_TOKEN_FILE
  try {
    const data = join(dir, 'data')
    mkdirSync(join(data, 'app', 'lab-a'), { recursive: true })
    mkdirSync(join(data, 'app', 'decoy'), { recursive: true })
    writeFileSync(join(data, 'app', 'lab-a', 'rth-host.jsonl'), J({ kind: 'started', pid: 11, t_ms: 0 }, { kind: 'response_started', response_id: 'bound-reply', request_id: 'mic-1', t_ms: 100 }, { kind: 'response_done', response_id: 'bound-reply', status: 'completed', t_ms: 400 }))
    writeFileSync(join(data, 'app', 'decoy', 'rth-host.jsonl'), J({ kind: 'started', pid: 22, t_ms: 0 }, { kind: 'response_started', response_id: 'DECOY-reply', request_id: 'mic-9', t_ms: 100 }))
    process.env.RTH_HOST_TOKEN_FILE = join(data, 'host', 'lab-a', 'token') // never created: the path is all it takes
    const a = await loaded(dir, { journals: { hostFromTokenFile: true } })
    const s = await a.snap()
    assert.equal(s.sources.host_journal.state, 'following')
    assert.ok(s.sources.host_journal.detail.startsWith(join(data, 'app', 'lab-a', 'rth-host.jsonl')), s.sources.host_journal.detail)
    const text = JSON.stringify(s.rows)
    assert.match(text, /bound-reply/)
    assert.doesNotMatch(text, /DECOY-reply/, 'another instance\'s journal is never read')
    await a.fiber.dispose()

    process.env.RTH_HOST_TOKEN_FILE = join(dir, 'dummy-host-token') // the shell-load check's layout: unsupported
    const b = await loaded(dir, { journals: { hostFromTokenFile: true } })
    const sb = await b.snap()
    assert.equal(sb.sources.host_journal.state, 'off')
    assert.match(sb.sources.host_journal.detail, /^host journal binding unsupported: .* is not <data>\/host\/<label>\/token/)
    assert.doesNotMatch(JSON.stringify(sb.rows), /bound-reply|DECOY-reply/, 'unsupported: no fallback to any log')
    await b.fiber.dispose()

    process.env.RTH_HOST_TOKEN_FILE = join(data, 'host', 'lab-missing', 'token') // supported layout, no journal yet
    const c = await loaded(dir, { journals: { hostFromTokenFile: true } })
    const sc = await c.snap()
    assert.equal(sc.sources.host_journal.state, 'missing', 'a missing journal is reported, not invented')
    assert.doesNotMatch(JSON.stringify(sc.rows), /bound-reply|DECOY-reply/)
    await c.fiber.dispose()

    delete process.env.RTH_HOST_TOKEN_FILE
    const d = await loaded(dir, { journals: { hostFromTokenFile: true } })
    assert.match((await d.snap()).sources.host_journal.detail, /RTH_HOST_TOKEN_FILE is not set/)
    await d.fiber.dispose()
  } finally {
    if (saved === undefined) delete process.env.RTH_HOST_TOKEN_FILE
    else process.env.RTH_HOST_TOKEN_FILE = saved
    rmSync(dir, { recursive: true, force: true })
  }
})
