// The composer's call button: what it may show comes only from the owner's
// receipts (`host.call`, `host.mic`), a click is a pending request until a receipt answers it,
// and the DSH routes pass requests to the native owner without deciding anything themselves.
import test from 'node:test'
import assert from 'node:assert/strict'
import http from 'node:http'
import { LEASE_MS, PENDING_MS, callView, createCallReader, muteView, nextRead, pollMs } from '../src/call-view.mjs'
import { HostClient } from '../src/host-client.mjs'
import { callStatus, fetchRoutes, hostStopConfig } from '../index.mjs'

const read = (state, extra = {}, mic = 'unmuted') => ({ call: { state, desired: ['starting', 'active'].includes(state) ? 'on' : 'off', reason: `${state} reason`, error: null, ...extra }, mic: { state: mic }, link: 'up', at: 1 })

test('each owner state maps to one button, and only receipts say what the call is', () => {
  assert.deepEqual(['off', 'starting', 'active', 'ending', 'error'].map((s) => [callView(read(s), null, 0).label, callView(read(s), null, 0).action]), [
    ['Start call', 'start'],
    ['Starting call…', 'end'],
    ['End call', 'end'],
    ['Ending call…', null],
    ['Start call', 'start'],
  ])
  const err = callView(read('error', { desired: 'on', error: 'microphone permission denied' }), null, 0)
  assert.equal(err.reason, 'microphone permission denied')
  assert.equal(err.tone, 'error')
  assert.equal(err.desired, 'on', 'desired and actual apart: wanted on, actually error')
  assert.equal(callView(null, null, 0).state, 'unknown')
  assert.equal(callView(null, null, 0).action, null)
  assert.equal(callView({ unavailable: 'owner status 0' }, null, 0).action, null)
  const old = callView({ call: null, mic: { state: 'unmuted' }, link: 'up' }, null, 0)
  assert.equal(old.label, 'Call unavailable', 'an rth-host that predates calls is never shown as off')
  assert.match(callView({ ...read('off'), link: 'down' }, null, 0).title, /agent link: down/)
})

test('a click is pending until a receipt answers it, then the receipt shows; no receipt is said', () => {
  const p = { want: 'start', at: 1000 }
  const waiting = callView(read('off'), p, 1500)
  assert.equal(waiting.label, 'Start call', 'the button keeps the receipt state, never the click')
  assert.equal(waiting.action, null, 'no second request while one waits')
  assert.equal(waiting.busy, true)
  assert.equal(waiting.pending, 'waiting')
  assert.equal(callView(read('starting'), p, 1500).pending, null, 'desired on answers a start')
  const late = callView(read('off'), p, 1000 + PENDING_MS + 1)
  assert.equal(late.pending, 'no_receipt')
  assert.equal(late.action, 'start', 'after the timeout the button works again')
  assert.match(late.title, /no receipt/)
  assert.equal(callView(read('active'), { want: 'end', at: 0 }, 10).pending, 'waiting', 'an end is answered by desired off only')
})

test('mute shows beside an active call only, from the applied fence', () => {
  assert.equal(callView(read('off'), null, 0).mute, null)
  assert.equal(callView(read('starting'), null, 0).mute, null)
  assert.deepEqual([callView(read('active'), null, 0).mute.label, callView(read('active'), null, 0).mute.action], ['Mute', true])
  assert.deepEqual([callView(read('active', {}, 'muted'), null, 0).mute.label, callView(read('active', {}, 'muted'), null, 0).mute.action], ['Unmute', false])
  assert.equal(muteView({ state: 'muting' }).action, null, 'a request not applied yet is never shown as muted')
  assert.equal(muteView({ state: 'muting' }).muted, false)
  assert.equal(muteView(undefined), null)
  assert.equal(pollMs(callView(read('starting'), null, 0)), 600)
  assert.equal(pollMs(callView(read('active'), null, 0)), 3000)
  assert.equal(pollMs(callView(read('off'), null, 0)), 5000)
})

function fakeHost(answers = {}) {
  const calls = []
  return {
    calls,
    async status() {
      calls.push({ op: 'status' })
      return answers.status ?? { status: 200, body: { host: { call: { state: 'off', desired: 'off' }, mic: { state: 'unmuted' } }, link: { state: 'up' }, deployment: null } }
    },
    async call(action) {
      calls.push({ op: 'call', action })
      return answers.call ?? { status: 202, body: { request: action, desired: action === 'start' ? 'on' : 'off' } }
    },
    async mute(muted) {
      calls.push({ op: 'mute', muted })
      return answers.mute ?? { status: 202, body: { request: { muted } } }
    },
    async cancel() {
      calls.push({ op: 'cancel' })
      return { status: 200, body: { ok: true } }
    },
  }
}

const route = (host, path) => fetchRoutes({ faultsRoute: false }, '/api/rth-session', host).find((r) => r.path === path)
const post = (body) => new Request('http://x/', { method: 'POST', body: typeof body === 'string' ? body : JSON.stringify(body) })

test('the call route passes requests to the owner and reads only its receipts', async () => {
  const host = fakeHost()
  const r = route(host, '/api/rth-session/call')
  assert.deepEqual(r.methods, ['GET', 'POST'])
  const got = await (await r.fetch(new Request('http://x/'))).json()
  assert.deepEqual([got.call.state, got.mic.state, got.link], ['off', 'unmuted', 'up'])
  const res = await r.fetch(post({ call: 'start' }))
  assert.equal(res.status, 202)
  assert.equal((await res.json()).desired, 'on')
  assert.equal((await r.fetch(post({ call: 'dial' }))).status, 400)
  assert.equal((await r.fetch(post('not json'))).status, 400)
  assert.deepEqual(host.calls.filter((c) => c.op === 'call'), [{ op: 'call', action: 'start' }], 'a bad body never reaches the owner')
  // An owner without calls, and an unreachable one, are passed on as what they are.
  assert.equal((await route(fakeHost({ call: { status: 404, body: { error: 'no such route' } } }), '/api/rth-session/call').fetch(post({ call: 'end' }))).status, 404)
  assert.equal((await route(fakeHost({ call: { status: 0, body: { error: 'ECONNREFUSED', retry: true } } }), '/api/rth-session/call').fetch(post({ call: 'end' }))).status, 502)
  assert.deepEqual(await callStatus(fakeHost({ status: { status: 0, body: { error: 'ECONNREFUSED' } } })), { unavailable: 'ECONNREFUSED' })
})

test('the mute route is a request with a boolean only', async () => {
  const host = fakeHost()
  const r = route(host, '/api/rth-session/mute')
  assert.equal((await r.fetch(post({ muted: true }))).status, 202)
  assert.equal((await r.fetch(post({ muted: 'yes' }))).status, 400)
  assert.deepEqual(host.calls, [{ op: 'mute', muted: true }])
})

test('the host client sends the call and mute requests with the bearer token', async () => {
  const seen = []
  const server = http.createServer((req, res) => {
    let body = ''
    req.on('data', (c) => (body += c))
    req.on('end', () => {
      seen.push({ method: req.method, url: req.url, auth: req.headers.authorization, body: JSON.parse(body) })
      res.writeHead(202, { 'content-type': 'application/json' }).end(JSON.stringify({ ok: true }))
    })
  })
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const host = new HostClient({ url: `http://127.0.0.1:${server.address().port}`, token: 't0k' })
  assert.equal((await host.call('start')).status, 202)
  assert.equal((await host.mute(false)).status, 202)
  server.close()
  assert.deepEqual(seen, [
    { method: 'POST', url: '/v1/call', auth: 'Bearer t0k', body: { call: 'start' } },
    { method: 'POST', url: '/v1/mute', auth: 'Bearer t0k', body: { muted: false } },
  ])
})

// No stale authority while a read hangs or answers late.

const ok = (body) => ({ ok: true, status: 200, json: async () => body })
const activeMuted = { call: { state: 'active', desired: 'on', reason: 'r' }, mic: { state: 'muted' }, link: 'up' }

test('a receipt is evidence only for its local lease: then the call and mute are unknown', () => {
  const v = callView({ ...activeMuted, receivedAt: 0 }, null, LEASE_MS)
  assert.deepEqual([v.label, v.mute?.label], ['End call', 'Unmute'])
  const stale = callView({ ...activeMuted, receivedAt: 0 }, null, LEASE_MS + 1)
  assert.equal(stale.state, 'unknown')
  assert.equal(stale.mute, null, 'no mute claim from a stale read')
  assert.equal(stale.action, 'end', 'End stays offered as a best-effort escape')
  assert.equal(stale.label, 'End call (state unknown)')
  assert.match(stale.title, /no fresh receipt/)
  assert.equal(pollMs(stale), 600)
  // The server's clock (`at`) is never the lease: an hour-old read is unknown whatever `at` says.
  assert.equal(callView({ ...activeMuted, at: Date.now(), receivedAt: 0 }, null, 3_600_000).state, 'unknown')
  const offStale = callView({ call: { state: 'off', desired: 'off' }, mic: { state: 'unmuted' }, receivedAt: 0 }, null, LEASE_MS + 1)
  assert.equal(offStale.action, null, 'no Start from an unknown state')
})

test('a hung GET is aborted, and the view goes unknown while it hangs', async () => {
  let t = 0
  const reader = createCallReader({ fetch: async () => ok(activeMuted), now: () => t, timeoutMs: 50 })
  let read = nextRead(null, await reader.load())
  assert.equal(callView(read, null, t).label, 'End call')
  // The next GET never answers: it is aborted at the timeout, and is no receipt.
  const hung = createCallReader({
    fetch: (_url, { signal }) => new Promise((_, reject) => signal.addEventListener('abort', () => reject(new Error('aborted')))),
    now: () => t,
    timeoutMs: 50,
  })
  t = LEASE_MS + 1 // the lease runs out while the read is still pending
  assert.equal(callView(read, null, t).state, 'unknown', 'unknown during the hang, before any answer')
  const got = await hung.load()
  assert.match(got.unavailable, /no answer within/)
  read = nextRead(read, got)
  const v = callView(read, null, t)
  assert.deepEqual([v.state, v.label, v.action, v.mute], ['unknown', 'End call (state unknown)', 'end', null], 'a timed-out read keeps no state, only the End escape')
  assert.match(v.title, /cannot be read: no answer within/)
})

test('a late answer from an older request never overwrites a newer view', async () => {
  const answers = []
  const reader = createCallReader({ fetch: () => new Promise((resolve) => answers.push(resolve)), now: () => 1 })
  const first = reader.load()
  const second = reader.load()
  answers[1](ok({ call: { state: 'off', desired: 'off' }, mic: { state: 'unmuted' } }))
  let read = nextRead(null, await second)
  answers[0](ok(activeMuted))
  const late = await first
  assert.equal(late.stale, true)
  read = nextRead(read, late)
  assert.equal(callView(read, null, 1).label, 'Start call', 'the newer receipt (off) stands')
  assert.equal(reader.latest().call.state, 'off')
})

test('a failed read is unknown, and the next good read recovers', async () => {
  let fail = true
  const reader = createCallReader({ fetch: async () => (fail ? { ok: false, status: 502, json: async () => ({}) } : ok(activeMuted)), now: () => 5 })
  let read = nextRead(null, await reader.load())
  assert.equal(callView(read, null, 5).label, 'Call unavailable')
  assert.equal(callView(read, null, 5).action, null, 'no escape without a good receipt that said a call was up')
  fail = false
  read = nextRead(read, await reader.load())
  assert.equal(callView(read, null, 5).label, 'End call')
  fail = true
  read = nextRead(read, await reader.load())
  assert.deepEqual([callView(read, null, 5).state, callView(read, null, 5).label, callView(read, null, 5).action], ['unknown', 'End call (state unknown)', 'end'])
})

// ── the page itself says the owner stopped, and offers Restart ──
test('an rth-host that stopped on its own is shown from the app\'s exit record, and Restart is a request to the app', async () => {
  const { mkdtempSync, writeFileSync } = await import('node:fs')
  const { tmpdir } = await import('node:os')
  const { join } = await import('node:path')
  const dir = mkdtempSync(join(tmpdir(), 'rth-host-stopped-'))
  const exitFile = join(dir, 'last-exit.json')
  const dead = () => fakeHost({ status: { status: 0, body: { error: 'connect ECONNREFUSED 127.0.0.1:7781' } } })
  const stop = { exitFile, restart: true, since: 1_000 }
  // No record yet, or one from before this DSH started: just unavailable.
  assert.deepEqual(await callStatus(dead(), stop), { unavailable: 'connect ECONNREFUSED 127.0.0.1:7781' })
  writeFileSync(exitFile, JSON.stringify({ kind: 'host_exit', pid: 4242, exit: 'signal: 6 (SIGABRT)', panic: 'panic on thread tokio-rt-worker at atomic_waker.rs:336:17: assertion failed', utc_ms: 999 }))
  assert.equal((await callStatus(dead(), stop)).hostStopped, undefined, 'an older host')
  writeFileSync(exitFile, JSON.stringify({ kind: 'host_exit', pid: 4242, exit: 'signal: 6 (SIGABRT)', panic: 'panic on thread tokio-rt-worker at atomic_waker.rs:336:17: assertion failed', utc_ms: 2_000 }))
  const got = await callStatus(dead(), stop)
  assert.deepEqual(got, { unavailable: 'connect ECONNREFUSED 127.0.0.1:7781', hostStopped: { pid: 4242, exit: 'signal: 6 (SIGABRT)', panic: 'panic on thread tokio-rt-worker at atomic_waker.rs:336:17: assertion failed', at: 2_000 }, restart: true })
  // A readable owner wins over any record.
  assert.equal((await callStatus(fakeHost(), stop)).hostStopped, undefined)
  // The view: what stopped, and Restart only when the app takes it from here.
  const v = callView({ ...got, receivedAt: 0 }, null, 0)
  assert.deepEqual([v.state, v.label, v.action, v.tone], ['stopped', 'Restart voice host', 'restart', 'error'])
  assert.match(v.reason, /^rth-host stopped \(signal: 6 \(SIGABRT\); panic on thread tokio-rt-worker/)
  const old = callView({ ...got, restart: false, receivedAt: 0 }, null, 0)
  assert.deepEqual([old.label, old.action], ['Voice host stopped', null])
  assert.match(old.title, /Unfold menu or the tray/)
  const waiting = callView({ ...got, receivedAt: 0 }, { want: 'restart', at: 0 }, 1_000)
  assert.deepEqual([waiting.busy, waiting.action, waiting.pending], [true, null, 'waiting'])
  assert.equal(callView({ ...got, receivedAt: 29_000 }, { want: 'restart', at: 0 }, 30_000).pending, 'no_receipt', 'a fresh read, an old request')
  assert.equal(callView({ call: { state: 'off', desired: 'off' }, mic: { state: 'unmuted' }, link: 'up', receivedAt: 0 }, { want: 'restart', at: 0 }, 1).state, 'off', 'answered by a readable owner')

  // The route: one stdout line, only while the owner is not answering, only for an app that takes it.
  const lines = []
  const restartRoute = (host, s) => fetchRoutes({ faultsRoute: false }, '/api/rth-session', host, { stop: s, emit: (l) => lines.push(l) }).find((r) => r.path === '/api/rth-session/restart-host')
  const res = await restartRoute(dead(), stop).fetch(post({ restart: true }))
  assert.equal(res.status, 202)
  assert.deepEqual(lines, ['rth-app: restart-host'])
  assert.equal((await restartRoute(fakeHost(), stop).fetch(post({}))).status, 409, 'a running owner is not restarted')
  assert.equal((await restartRoute(dead(), { ...stop, restart: false }).fetch(post({}))).status, 409, 'an older app build: the menu or the tray')
  assert.deepEqual(lines, ['rth-app: restart-host'], 'refusals print nothing')
  assert.equal(hostStopConfig({}).exitFile, null)
  assert.equal(hostStopConfig({}).restart, false, 'no env (an app that predates this): nothing changes')
  assert.deepEqual([hostStopConfig({ RTH_HOST_EXIT_FILE: exitFile, RTH_HOST_RESTART: 'dsh-stdout' }).exitFile, hostStopConfig({ RTH_HOST_RESTART: 'dsh-stdout' }).restart], [exitFile, true])
})

// ── a death receipt's lease and owner, and the redactor ──
test('a death receipt is current only within the lease and only for the last live owner; an unreadable successor is unknown, never stopped or off', async () => {
  const { mkdtempSync, writeFileSync } = await import('node:fs')
  const { tmpdir } = await import('node:os')
  const { join } = await import('node:path')
  const { voiceControl } = await import('../../dsh-events/src/avatar.mjs')
  const dir = mkdtempSync(join(tmpdir(), 'rth-host-lease-'))
  const exitFile = join(dir, 'last-exit.json')
  writeFileSync(exitFile, JSON.stringify({ kind: 'host_exit', pid: 111, exit: 'signal: 6 (SIGABRT)', panic: null, utc_ms: 2_000 }))
  let live = null // null: unreadable; else the live owner's pid
  const host = { status: async () => (live ? { status: 200, body: { host: { pid: live, call: { state: 'active', desired: 'on' }, mic: { state: 'unmuted' } }, link: { state: 'up' } } } : { status: 0, body: { error: 'timeout' } }) }
  const stop = { exitFile, restart: true, since: 1_000 }
  const route = fetchRoutes({}, '/api/rth-session', host, { stop }).find((r) => r.path.endsWith('/call'))
  const read = async () => JSON.parse(await (await route.fetch(new Request('http://localhost/api/rth-session/call'))).text())
  // Expired: a stopped read is evidence only within the lease.
  const first = { ...(await read()), receivedAt: 0 }
  assert.equal(voiceControl(first, null, 0).actual, 'stopped', 'fresh: the death is shown')
  const expired = voiceControl(first, null, LEASE_MS + 1)
  assert.notEqual(expired.actual, 'stopped', 'expired: not asserted as current')
  assert.notEqual(expired.action, 'restart')
  // Host 222 comes up live (the restart), then its status read times out: host 111's old death
  // must not come back as the current state.
  live = 222
  assert.equal((await read()).call.state, 'active')
  live = null
  const later = await read()
  assert.equal(later.hostStopped, undefined, 'the record names 111, the last live owner is 222')
  const v = callView({ ...later, receivedAt: 30_000 }, null, 30_000)
  assert.deepEqual([v.state, v.action === 'restart'], ['unknown', false])
  const a = voiceControl({ ...later, receivedAt: 30_000 }, { want: 'restart', at: 29_000 }, 30_000)
  assert.equal(a.actual, 'unknown', 'a pending restart cannot turn the uncertainty into stopped')
  assert.notEqual(a.status, 'Voice asleep', 'nor into off')
  // The successor itself dies: its own record is current again.
  writeFileSync(exitFile, JSON.stringify({ kind: 'host_exit', pid: 222, exit: 'signal: 11 (SIGSEGV)', panic: null, utc_ms: 40_000 }))
  assert.equal((await read()).hostStopped?.pid, 222)
})

test('a panic text reaches the page only through the credential redactor, applied before it is trimmed', async () => {
  const { mkdtempSync, writeFileSync } = await import('node:fs')
  const { tmpdir } = await import('node:os')
  const { join } = await import('node:path')
  const { voiceControl } = await import('../../dsh-events/src/avatar.mjs')
  const dir = mkdtempSync(join(tmpdir(), 'rth-host-redact-'))
  const exitFile = join(dir, 'last-exit.json')
  const canary = 'sk-proj-SYNTHETIC-PROBE-ONLY-0123456789abcdefgh'
  const host = { status: async () => ({ status: 0, body: { error: 'timeout' } }) }
  const route = fetchRoutes({}, '/api/rth-session', host, { stop: { exitFile, restart: true, since: 1_000 } }).find((r) => r.path.endsWith('/call'))
  // An assignment, a bare key, and a bare key straddling the 400-character trim.
  for (const panic of [`API_KEY=${canary}`, `boom: ${canary} after`, `${'x'.repeat(390)} ${canary}`, `{"api_key": "${canary}"}`]) {
    writeFileSync(exitFile, JSON.stringify({ kind: 'host_exit', pid: 7, exit: `signal: 6 (${canary})`, panic, utc_ms: 2_000 }))
    const wire = await (await route.fetch(new Request('http://localhost/api/rth-session/call'))).text()
    const shown = voiceControl({ ...JSON.parse(wire), receivedAt: 0 }, null, 0)
    for (const text of [wire, shown.reason ?? '', shown.title ?? '']) {
      assert.ok(!text.includes('sk-proj-SYN'), `no fragment of the canary leaves: ${text.slice(0, 120)}…`)
    }
    assert.equal(shown.actual, 'stopped', 'the death is still shown')
  }
})

// ── a NEW app owner retires the old death ──
test('once the app starts a new owner, the old death is history even if DSH never read the successor; a failed restart keeps the same death, never a new one', async () => {
  const { mkdtempSync, writeFileSync } = await import('node:fs')
  const { tmpdir } = await import('node:os')
  const { join } = await import('node:path')
  const { voiceControl } = await import('../../dsh-events/src/avatar.mjs')
  const dir = mkdtempSync(join(tmpdir(), 'rth-host-r1b-'))
  const exitFile = join(dir, 'last-exit.json'), ownerFile = join(dir, 'current-owner.json')
  const owner = (pid, gen) => writeFileSync(ownerFile, JSON.stringify({ kind: 'host_owner', pid, gen, utc_ms: 1_500 + gen }))
  let live = 111
  const host = { status: async () => (live ? { status: 200, body: { host: { pid: live, call: { state: 'off', desired: 'off' }, mic: { state: 'unmuted' } }, link: { state: 'up' } } } : { status: 0, body: { error: 'timeout' } }) }
  const stop = { exitFile, ownerFile, restart: true, since: 1_000 }
  const route = fetchRoutes({}, '/api/rth-session', host, { stop }).find((r) => r.path.endsWith('/call'))
  const read = async () => JSON.parse(await (await route.fetch(new Request('http://localhost/api/rth-session/call'))).text())
  owner(111, 1)
  assert.equal((await read()).call.state, 'off', 'DSH sees 111 live')
  // 111 dies; the app records it. The page says stopped.
  live = null
  writeFileSync(exitFile, JSON.stringify({ kind: 'host_exit', pid: 111, exit: 'signal: 6 (SIGABRT)', panic: null, utc_ms: 2_000 }))
  assert.equal((await read()).hostStopped?.pid, 111)
  // A failed restart (nothing spawned): the owner record still names 111, the same death, not a new one.
  const same = await read()
  assert.deepEqual([same.hostStopped?.pid, same.hostStopped?.at], [111, 2_000])
  // The app restarts 222 and enables it (menu Wake works); DSH NEVER reads 222 successfully.
  owner(222, 2)
  const later = await read()
  assert.equal(later.hostStopped, undefined, 'the death names 111, the current owner is 222: history, not now')
  const v = voiceControl({ ...later, receivedAt: 3_000 }, null, 3_000)
  assert.deepEqual([v.actual === 'stopped', v.action === 'restart', v.status === 'Voice asleep'], [false, false, false], 'unknown: neither stopped, nor a restart, nor off')
  // Retirement cannot be proven (the owner record is unreadable): unknown, not stopped.
  writeFileSync(ownerFile, '{"torn')
  assert.equal((await read()).hostStopped, undefined)
  // 222 dies: its own record is current.
  owner(222, 2)
  writeFileSync(exitFile, JSON.stringify({ kind: 'host_exit', pid: 222, exit: 'signal: 11 (SIGSEGV)', panic: null, utc_ms: 4_000 }))
  assert.equal((await read()).hostStopped?.pid, 222)
})
