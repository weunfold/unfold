// The avatar (RTH presence tab): the scene is a pure function of the presence view and the ACTUAL
// voice receipt; a tap is a request through rth-session's call route, never a state; the mute
// glyph is the applied fence, apart from the creature; stale receipts are unknown.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { createRequire } from 'node:module'
import { CALL_ROUTE, CP_PREVIEW, FPS, MUTE_ROUTE, PENDING_MS, RESTART_ROUTE, createVoiceController, grainBudget, muteGlyph, sceneFor, toggleRequest, voiceControl } from '../src/avatar.mjs'
import { LEASE_MS } from '../../dsh-session/src/call-view.mjs'
import { POSES } from '../src/presence.mjs'

const NOW = 100_000
const read = (state, mic = 'unmuted', extra = {}) => ({
  call: { state, desired: ['starting', 'active'].includes(state) ? 'on' : 'off', reason: `${state} reason`, error: state === 'error' ? 'no voice devices configured' : null, ...extra },
  mic: { state: mic },
  link: 'up',
  receivedAt: NOW,
})
const pv = (pose, speech = 'none') => ({ pose, label: pose, because: `${pose} because`, layers: { link: { state: pose === 'unknown' ? 'unknown' : 'live' }, speech: { state: speech } }, faults: [] })
const voice = (state, mic) => voiceControl(read(state, mic), null, NOW)

test('scene: every voice state x creature pose maps to one honest scene', () => {
  const row = (p, v, speech) => {
    const s = sceneFor(pv(p, speech), voice(v))
    return [s.form, s.face, s.tint]
  }
  // Voice asleep: dim, sparse drifting dust; RTH itself stays on (a typed request still orbits).
  assert.deepEqual(row('idle', 'off'), ['drift', null, null])
  assert.ok(sceneFor(pv('idle'), voice('off')).density < 0.5, 'sparse')
  assert.ok(sceneFor(pv('idle'), voice('off')).dim < 0.5, 'dim')
  assert.match(sceneFor(pv('idle'), voice('off')).status, /Voice asleep · Unfold stays on/)
  assert.deepEqual(row('pending', 'off'), ['orbit', null, null])
  assert.deepEqual(row('idle', 'error'), ['drift', null, null])
  assert.deepEqual(row('idle', 'off', 'candidate'), ['drift', null, null], 'no listening face while voice is asleep')
  // Waking / going to sleep: the dust gathers / disperses.
  assert.deepEqual(row('idle', 'starting'), ['gather', null, null])
  assert.deepEqual(row('idle', 'ending'), ['disperse', null, null])
  // Voice awake.
  assert.deepEqual(row('idle', 'active'), ['drift', null, null], 'idle: drifting dust and no face')
  assert.deepEqual(row('idle', 'active', 'candidate'), ['face', 'listen', null], 'a speech candidate forms the face')
  assert.deepEqual(row('pending', 'active', 'candidate'), ['face', 'listen', null], 'over pending too (the fake opens its reply at once)')
  assert.deepEqual(row('pending', 'active'), ['orbit', null, null], 'pending: circling dust, never thinking')
  assert.deepEqual(row('speaking', 'active'), ['face', 'speak', null], 'speaking only from the reducer pose (device-rendered audio)')
  assert.deepEqual(row('sleepy', 'active'), ['slow', null, null])
  assert.deepEqual(row('asleep', 'active'), ['settle', null, null])
  assert.deepEqual(row('wince', 'active'), ['fault', 'wince', 'warm'])
  // Unknown or stale: scattered grey, whatever else is true.
  for (const v of ['off', 'active', 'starting']) assert.deepEqual(row('unknown', v), ['scatter', null, 'grey'])
  assert.deepEqual(sceneFor(pv('idle'), voiceControl(null, null, NOW)).tint, 'grey', 'no voice receipt yet: unknown')
  assert.deepEqual(sceneFor({ pose: 'unknown', label: 'Unknown', because: 'the presence stream is not open', layers: null, faults: [] }, voice('active')).form, 'scatter', 'a stale presence stream is unknown')
  // No scene claims thinking, a mood or that the mic is closed.
  for (const p of POSES) for (const v of ['off', 'starting', 'active', 'ending', 'error']) {
    const s = sceneFor(pv(p), voice(v))
    assert.doesNotMatch(s.status, /(?<!not )thinking|mood|confiden|mic(rophone)? (is )?(off|closed)/i, `${p}/${v}: ${s.status}`)
    // Only what is happening moves (a fault, a transition, an open request, device-rendered speech);
    // idle is a still frame (the measured native cost); unknown never moves.
    const happening = p !== 'unknown' && (p === 'wince' || v === 'starting' || v === 'ending' || p === 'pending' || (v === 'active' && p === 'speaking'))
    assert.equal(FPS[s.motion] > 0, happening, `${p}/${v}: ${s.motion}`)
    assert.equal(typeof s.key, 'string')
  }
})

test('scene: speaking is never drawn without the reducer saying device audio is rendering', () => {
  for (const p of POSES.filter((x) => x !== 'speaking')) for (const speech of ['none', 'candidate', 'unknown']) {
    assert.notEqual(sceneFor(pv(p, speech), voice('active')).face, 'speak', `${p}/${speech}`)
  }
})

test('toggle: off/error wake, active and starting sleep, ending and unknown send nothing (except the escape)', () => {
  const req = (v) => toggleRequest(v)?.body.call ?? null
  assert.deepEqual(['off', 'error', 'starting', 'active', 'ending'].map((s) => [s, voice(s).label, req(voice(s))]), [
    ['off', 'Wake voice', 'start'],
    ['error', 'Try waking voice again', 'start'],
    ['starting', 'Put voice back to sleep', 'end'],
    ['active', 'Put voice to sleep', 'end'],
    ['ending', 'Going to sleep…', null],
  ])
  assert.equal(toggleRequest(voice('off')).url, CALL_ROUTE, 'the same authenticated route the composer button used')
  assert.equal(req(voiceControl(null, null, NOW)), null, 'nothing before the first receipt')
  assert.equal(voiceControl(null, null, NOW).label, 'Checking voice…')
  assert.equal(voiceControl(null, null, NOW).status, 'Checking…', 'no receipt yet: no state is claimed')
  // Stale after an active receipt: a best-effort Sleep escape, labelled as such; after off: nothing.
  const staleActive = voiceControl(read('active'), null, NOW + LEASE_MS + 1)
  assert.equal(staleActive.actual, 'unknown')
  assert.deepEqual([staleActive.label, req(staleActive)], ['Put voice to sleep (not sure it is awake)', 'end'])
  assert.equal(req(voiceControl(read('off'), null, NOW + LEASE_MS + 1)), null)
  assert.equal(req(voiceControl({ call: null, mic: { state: 'unmuted' }, link: 'up', receivedAt: NOW }, null, NOW)), null, 'an rth-host without a call owner offers no Wake')
})

test('displayed state follows receipts, never the tap; desired and actual are shown apart while it settles', () => {
  const tap = { want: 'start', at: NOW }
  const waiting = voiceControl(read('off'), tap, NOW + 500)
  assert.equal(waiting.actual, 'off', 'the tap is not the state')
  assert.equal(waiting.action, null, 'no second request while one waits')
  assert.match(waiting.settling, /^Waking… waiting for Unfold to confirm$/)
  assert.equal(waiting.status, 'Voice asleep', 'the headline keeps the actual state while a tap waits')
  assert.equal(sceneFor(pv('idle'), waiting).form, 'drift', 'the creature stays asleep until the receipt')
  assert.notEqual(sceneFor(pv('idle'), waiting).form, 'gather')
  const answered = voiceControl(read('starting'), tap, NOW + 600)
  assert.equal(answered.actual, 'starting')
  assert.equal(sceneFor(pv('idle'), answered).form, 'gather')
  const late = voiceControl(read('off'), tap, NOW + PENDING_MS + 1)
  assert.match(late.settling, /^Unfold didn't confirm the wake within 6 s\. Voice is asleep\.$/)
  assert.equal(late.action, 'start', 'after the timeout the avatar works again')
  // A failed start: the receipt says error; no active glow.
  const failed = voiceControl(read('error', 'unmuted', { desired: 'on' }), null, NOW)
  assert.equal(failed.actual, 'error')
  assert.equal(failed.status, "Voice didn't wake")
  assert.equal(failed.reason, 'no voice devices configured', 'the reason is shown on its own line')
  assert.equal(failed.desired, 'on', 'desired and actual stay apart: wanted on, actually error')
  assert.equal(failed.hint, 'Tap to try again')
  assert.notEqual(sceneFor(pv('idle'), failed).form, 'gather')
  assert.equal(failed.settling, null, 'an error is not shown as still settling')
})

test('stale or unreadable receipts go to unknown, and a fresh one recovers', () => {
  const at = (r, now) => voiceControl(r, null, now)
  assert.equal(at(read('active'), NOW + LEASE_MS).actual, 'active')
  assert.equal(at(read('active'), NOW + LEASE_MS + 1).actual, 'unknown')
  assert.equal(at({ unavailable: 'connect ECONNREFUSED', receivedAt: NOW }, NOW).actual, 'unknown')
  assert.equal(at({ unavailable: 'x', receivedAt: NOW }, NOW).mute.state, 'unknown', 'the mute glyph goes unknown too')
  assert.equal(at(read('active'), NOW + LEASE_MS + 1).mute.state, 'unknown')
  assert.equal(sceneFor(pv('idle'), at(read('active'), NOW + LEASE_MS + 1)).tint, 'grey')
  assert.equal(at(read('off'), NOW + 1).actual, 'off', 'recovered')
})

test('the mute glyph is the applied fence, independent of the creature and of voice sleep/wake', () => {
  for (const state of ['off', 'starting', 'active', 'ending', 'error']) {
    for (const mic of ['muted', 'unmuted', 'muting', 'unmuting']) {
      const v = voice(state, mic)
      assert.equal(v.mute.state, mic, `${state}/${mic}`)
      assert.equal(v.mute.muted, mic === 'muted' || mic === 'unmuting', 'muting is not yet muted')
      // The creature is identical whatever the mute says: no pose implies a closed mic.
      for (const p of POSES) assert.equal(sceneFor(pv(p), v).key, sceneFor(pv(p), voice(state, 'unmuted')).key, `${p}/${state}/${mic}`)
    }
  }
  assert.deepEqual([voice('off', 'unmuted').mute.label, voice('off', 'unmuted').mute.action], ['Mute microphone', true], 'mute works while voice sleeps (it survives wake)')
  assert.deepEqual([voice('active', 'muted').mute.label, voice('active', 'muted').mute.action], ['Unmute microphone', false])
  assert.equal(voice('active', 'muting').mute.action, null, 'no request while one is being applied')
  assert.equal(muteGlyph({ ...read('active'), mic: {} }, NOW).state, 'unknown', 'a host with no mute state is unknown')
})

/** A fake rth-session: GET answers with the current receipt; POSTs are recorded and answered 202. */
function fakeRoutes(state = 'off', mic = 'unmuted') {
  const posts = []
  let gets = 0
  const now = { t: NOW }
  const routes = {
    posts,
    get gets() { return gets },
    now,
    set: (s, m = mic) => { state = s; mic = m },
    fetch: async (url, init = {}) => {
      if (init.method === 'POST') {
        posts.push({ url, body: JSON.parse(init.body) })
        return { ok: false, status: 202, json: async () => ({}) }
      }
      gets += 1
      const body = read(state, mic)
      delete body.receivedAt
      return { ok: true, status: 200, json: async () => body }
    },
  }
  return routes
}

test('the controller: mounting, reading, remounting and disposing send no request; a tap sends exactly one', async () => {
  const r = fakeRoutes('off')
  for (let i = 0; i < 3; i++) {
    const c = createVoiceController({ fetch: r.fetch, now: () => r.now.t })
    await c.load()
    await c.load()
    c.view()
    c.dispose()
  }
  assert.equal(r.posts.length, 0, 'display never changes devices')
  assert.ok(r.gets >= 6)
  const c = createVoiceController({ fetch: r.fetch, now: () => r.now.t })
  await c.load()
  await c.toggle()
  assert.deepEqual(r.posts, [{ url: CALL_ROUTE, body: { call: 'start' } }])
  assert.equal(c.view().actual, 'off', 'the tap did not change what is shown')
  r.set('active')
  await c.load()
  assert.equal(c.view().actual, 'active')
  assert.equal(c.view().pending, null, 'the receipt (desired on) answered the tap')
  await c.toggle()
  assert.deepEqual(r.posts.at(-1), { url: CALL_ROUTE, body: { call: 'end' } })
  r.set('ending')
  await c.load()
  assert.equal(await c.toggle(), null, 'ending: nothing to send')
  await c.mute()
  assert.deepEqual(r.posts.at(-1), { url: MUTE_ROUTE, body: { muted: true } }, 'mute is its own request')
  assert.equal(r.posts.filter((p) => p.url === CALL_ROUTE).length, 2)
})

test('the controller: a refused or failed request is said, and the receipt still decides', async () => {
  const r = fakeRoutes('off')
  const refuse = async (url, init = {}) => (init.method === 'POST' ? { ok: false, status: 409, json: async () => ({ refused: 'no call owner' }) } : r.fetch(url, init))
  const c = createVoiceController({ fetch: refuse, now: () => r.now.t })
  await c.load()
  await c.toggle()
  assert.match(c.view().note, /Wake voice not delivered: no call owner/)
  assert.equal(c.view().actual, 'off')
  assert.equal(c.view().pending, null)
})

test('cosmic points are a labelled preview, and the grain budget follows the tab size', () => {
  assert.deepEqual({ ...CP_PREVIEW }, { cp: 100, level: 1, stage: 'Seed', colours: 1, preview: true })
  assert.deepEqual(grainBudget(300, 400), { cap: 30_000, grains: 10_000 }, '1 CP = 100 grains: 100 CP draws 10k')
  assert.equal(grainBudget(100, 100).cap, 20_000)
  assert.equal(grainBudget(1000, 1000).cap, 40_000)
  assert.equal(grainBudget(1000, 1000, 1000).grains, 40_000, 'never above the cap')
})

test('the composer has no call button, and typed conversation and Stop stay', () => {
  const src = readFileSync(new URL('../../dsh-session/client/index.jsx', import.meta.url), 'utf8')
  assert.doesNotMatch(src, /conversation\.input\.right/)
  assert.doesNotMatch(src, /CallControls|call-button|data-rth="call"|Start call'|End call'/)
  assert.match(src, /openSession\(ROOT_SESSION\)/, 'the root Session (the composer) still opens')
  assert.match(src, /status-stop/, 'Stop speaking stays')
  assert.match(src, /bt-mic,\.bt-voice/, 'the Bot browser mic stays hidden (one speech owner)')
})

// Evaluate freshly built browser halves (the same esbuild call as plugins/scripts/build-client.mjs,
// in memory: the live lib/client.js files are never written) with stub React.
async function bundle(dir) {
  const require = createRequire(new URL('../../package.json', import.meta.url))
  const { build } = require('esbuild')
  const r = await build({ entryPoints: [new URL(`../../${dir}/client/index.jsx`, import.meta.url).pathname], bundle: true, write: false, format: 'cjs', platform: 'browser', target: 'es2022', jsx: 'automatic', external: ['react', 'react/jsx-runtime', '@deepseek-ai/*'], logLevel: 'silent' })
  const jsx = (type, props) => ({ type, props })
  class Component { constructor(props) { this.props = props; this.state = {} } }
  const hooks = { Component, Fragment: 'F', useState: (i) => [typeof i === 'function' ? i() : i, () => {}], useReducer: (_f, i) => [i, () => {}], useRef: (v) => ({ current: v }), useEffect: () => {}, useCallback: (f) => f, useMemo: (f) => f(), useId: () => ':r0:' }
  const mod = { exports: {} }
  new Function('module', 'exports', 'require', r.outputFiles[0].text)(mod, mod.exports, (n) => (n === 'react' ? hooks : n === 'react/jsx-runtime' ? { jsx, jsxs: jsx, Fragment: 'F' } : {}))
  const regs = []
  const ctx = {
    effect: (f) => { try { f() } catch {} },
    inject: () => {},
    slots: { inject: (_n, f) => f(), register: (opts, comp) => { regs.push({ opts, comp }); return () => {} } },
    sidebarRightTabs: { register: (t) => { regs.push({ opts: { tab: t.id } }); return () => {} } },
    sidebarRight: { openTabs: { getSnapshot: () => [] } },
  }
  return { mod, regs, ctx }
}
// Stub render: function components are called, class components (the error boundary) rendered.
const expand = (node) => {
  if (Array.isArray(node)) return node.map(expand)
  if (!node || typeof node !== 'object') return node
  if (typeof node.type === 'function') {
    const out = node.type.prototype?.render ? new node.type(node.props).render() : node.type(node.props)
    return expand(out)
  }
  return { ...node, props: { ...node.props, children: expand(node.props?.children) } }
}
const find = (node, key, out = []) => {
  if (!node || typeof node !== 'object') return out
  if (node.props?.['data-rth'] === key) out.push(node)
  for (const c of [].concat(node.props?.children ?? [])) find(c, key, out)
  return out
}

test('bundles: dsh-session registers nothing by the composer; the presence tab renders the avatar without a request', async () => {
  const session = await bundle('dsh-session')
  globalThis.document = { createElement: () => ({ dataset: {} }), head: { append() {} }, getElementById: () => null }
  session.mod.exports.apply(session.ctx)
  assert.deepEqual(session.regs.filter((r) => r.opts.name === 'conversation.input.right'), [])
  assert.equal(session.mod.exports.RTH_CALL_ID, undefined)
  const events = await bundle('dsh-events')
  events.mod.exports.apply(events.ctx)
  const body = events.regs.find((r) => r.opts.key === '@rth/dsh-events/presence' && r.opts.name === 'sidebar.right.pane.tab')
  const calls = []
  globalThis.fetch = (...a) => { calls.push(a); return new Promise(() => {}) }
  globalThis.matchMedia = () => ({ matches: false })
  const tree = expand(body.comp({}))
  const [avatar] = find(tree, 'avatar')
  assert.equal(avatar.type, 'button', 'a real button: Enter and Space work natively')
  assert.equal(avatar.props['aria-label'], 'Checking voice…')
  assert.equal(avatar.props['aria-disabled'], true)
  assert.equal(avatar.props['aria-describedby'], ':r0:')
  const [glyph] = find(tree, 'avatar-mute')
  assert.equal(glyph.props['data-state'], 'unknown')
  assert.equal(glyph.props['aria-label'], 'Microphone: not sure if muted')
  assert.equal(find(tree, 'avatar-voice')[0].props.children, 'Checking…', 'before any receipt the headline claims nothing')
  const src = readFileSync(new URL('../client/index.jsx', import.meta.url), 'utf8')
  assert.match(src, /fetch\('api\/rth-presence\/growth'/, 'cosmic points come only from the read-only CP ledger route')
  assert.doesNotMatch(src, /preview: nothing is earned yet|CP_PREVIEW\.(stage|level)/, 'no hardcoded preview level on the surface')
  assert.equal(calls.length, 0, 'rendering sends nothing (reads start in an effect; requests only on a tap)')
  delete globalThis.fetch
  delete globalThis.matchMedia
  delete globalThis.document
})

test('plain words: one quiet line from the presence only; never thinking, listening or "you said"', async () => {
  const { presenceWords, presenceFacts } = await import('../src/avatar.mjs')
  const lay = (over = {}) => ({ link: { state: 'live' }, request: { state: 'none', label: 'requests: none open' }, audio: { state: 'not_observed' }, mic: { state: 'not_muted' }, ...over })
  const pv2 = (pose, layers = lay()) => ({ pose, label: pose, because: '', layers, faults: [] })
  const line = (p, v, layers) => presenceWords(pv2(p, layers), voice(v))?.line ?? null
  assert.equal(line('idle', 'active'), null, 'idle and awake: nothing more to say')
  assert.equal(line('idle', 'off'), 'You can still type to Unfold', 'voice asleep is not Unfold asleep')
  assert.equal(line('speaking', 'active'), 'Speaking')
  assert.equal(line('pending', 'active', lay({ request: { state: 'pending', label: 'pending: command dsh-1 admitted, not finished' } })), 'Got a message, no reply yet')
  assert.equal(line('pending', 'active', lay({ request: { state: 'pending', label: 'pending: reply r1 started, not finished' } })), 'Replying…')
  assert.equal(presenceWords(pv2('idle'), voice('active', 'muted')).line, 'Your microphone is muted')
  assert.equal(presenceWords({ pose: 'unknown', layers: null, faults: [] }, voice('active')).line, 'Reconnecting…')
  const all = []
  for (const p of POSES) for (const v of ['off', 'starting', 'active', 'ending', 'error']) {
    all.push(line(p, v) ?? '', voice(v).status, voice(v).hint ?? '', voice(v).label)
    for (const [, val] of presenceFacts(pv2(p), voice(v))) all.push(val)
  }
  const text = all.join(' | ')
  assert.doesNotMatch(text, /think|listening|you said|mood|innie|outtie/i)
  assert.doesNotMatch(text, /rth-host|receipt|desired|actual/, 'no plumbing words on the surface')
  // The microphone fact never claims capture: "not muted" is what the host reports.
  const facts = Object.fromEntries(presenceFacts(pv2('idle'), voice('active')))
  assert.equal(facts.Microphone, 'Not muted')
  assert.equal(Object.fromEntries(presenceFacts(pv2('idle'), voice('off'))).Microphone, 'Closed (voice asleep)')
  assert.equal(Object.fromEntries(presenceFacts(pv2('idle'), voiceControl(read('active'), null, NOW + LEASE_MS + 1))).Voice, 'Unknown', 'a stale receipt is unknown')
})

test('unknown presence shows no life, even with voice awake: known -> unknown -> known', async () => {
  const { lifeFor } = await import('../src/avatar.mjs')
  // The renderer on a synthetic canvas (2D path, stub context), with rAF on timers.
  const ctx2d = new Proxy({}, { get: (t, k) => (k in t ? t[k] : () => {}), set: (t, k, v) => { t[k] = v; return true } })
  const canvas = { width: 0, height: 0, dataset: {}, getBoundingClientRect: () => ({ width: 200, height: 300 }), getContext: (k) => (k === '2d' ? ctx2d : null) }
  globalThis.requestAnimationFrame = (f) => setTimeout(() => f(performance.now()), 8)
  globalThis.cancelAnimationFrame = (id) => clearTimeout(id)
  const { dustRenderer } = await import('../client/dust-renderer.js')
  const css = readFileSync(new URL('../client/index.jsx', import.meta.url), 'utf8')
  assert.match(css, /\.rth-stage\[data-alive=true\] \.rth-tube::after\{animation:rth-breathe/, 'the breathing glow is keyed on data-alive')
  assert.doesNotMatch(css, /\.rth-stage\[data-voice=active\] \.rth-tube::after\{animation/, 'never on the voice receipt alone')
  assert.match(css, /data-alive=\{String\(lifeFor\(view, voice\)\)\}/)
  const r = dustRenderer(canvas, { grains: 2000 })
  const awake = voice('active')
  const known = pv('pending') // something happening (an open request): the dust moves at 8 fps
  const unknown = { pose: 'unknown', label: 'Unknown', because: 'the presence stream is not open', layers: null, faults: [] }
  const run = async (view) => {
    const scene = sceneFor(view, awake)
    r.draw(scene)
    const before = r.stats().frames
    await new Promise((res) => setTimeout(res, 650))
    return { alive: lifeFor(view, awake), motion: scene.motion, fps: FPS[scene.motion], frames: r.stats().frames - before, running: r.stats().running, label: awake.status }
  }
  try {
  const a = await run(known)
  assert.deepEqual([a.alive, a.motion, a.running], [true, 'calm', true], 'known + awake + a request open: alive, the dust orbits')
  assert.equal(sceneFor(pv('idle'), awake).motion, 'rest', 'known + awake + idle: a still frame (the glow breathes)')
  assert.equal(lifeFor(pv('idle'), awake), true)
  assert.ok(a.frames >= 2, `known: ${a.frames} frames in 650 ms`)
  const b = await run(unknown)
  assert.deepEqual([b.alive, b.motion, b.fps, b.running], [false, 'still', 0, false], 'unknown + awake: no breathing, no loop')
  assert.ok(b.frames <= 1, `unknown: one still frame at most, got ${b.frames}`)
  assert.equal(b.label, 'Voice awake', 'the honest voice label stays')
  assert.equal(toggleRequest(awake).body.call, 'end', 'and Sleep still works')
  const c = await run(known)
  assert.deepEqual([c.alive, c.running], [true, true], 'known again: life returns')
  assert.ok(c.frames >= 2)
  // Unknown voice receipt with known presence: also still.
  assert.equal(sceneFor(known, voiceControl(read('active'), null, NOW + LEASE_MS + 1)).motion, 'still')
  assert.equal(lifeFor(known, voiceControl(read('active'), null, NOW + LEASE_MS + 1)), false)
  // Hidden stays at zero frames; dispose leaves nothing behind.
  r.draw(sceneFor(known, awake))
  r.setVisible(false)
  const h = r.stats().frames
  await new Promise((res) => setTimeout(res, 200))
  assert.equal(r.stats().frames, h, 'hidden: no frames')
  } finally {
    r.dispose() // always: a failed assertion must not leave the loop's timer running
  }
  const dbg = globalThis.__rthAvatar
  assert.deepEqual([dbg.live, dbg.rafs, dbg.timeouts, dbg.observers], [0, 0, 0, 0])
  delete globalThis.requestAnimationFrame
  delete globalThis.cancelAnimationFrame
})

test('reduced motion stops pseudo-element animation too (the capsule breathe lives on ::after)', () => {
  const ui = readFileSync(new URL('../../rth-ui/ui.jsx', import.meta.url), 'utf8')
  assert.match(ui, /@media \(prefers-reduced-motion: reduce\)\{\.rth-ui \*,\.rth-ui \*::before,\.rth-ui \*::after\{transition:none!important;animation:none!important\}\}/)
})

test('Events draws every summary through redactText (a credential in a note is masked on the surface)', async () => {
  const src = readFileSync(new URL('../client/index.jsx', import.meta.url), 'utf8')
  assert.match(src, /import \{ redactText \} from '\.\.\/\.\.\/dsh-mind\/src\/redact\.mjs'/)
  assert.match(src, /<span className="rth-ev-sum">\{redactText\(row\.summary\)\}<\/span>/)
  assert.doesNotMatch(src, /<span className="rth-ev-sum">\{row\.summary\}/)
  const { redactText } = await import('../../dsh-mind/src/redact.mjs')
  const fake = 'AIzaSyUXDEV' + 'FAKE0123456789abcdefghijklmnopq'
  assert.doesNotMatch(redactText(`info: config key=${fake} loaded`), /AIzaSyUXDEVFAKE/)
})

test('voice reads never stall on a page that says hidden: the first read always runs, then a slow cadence (native WKWebView launched in the background)', async () => {
  const { shouldRead, HIDDEN_READ_MS } = await import('../src/avatar.mjs')
  assert.equal(shouldRead({ hidden: true, sinceLast: Infinity, delay: 5000 }), true, 'first read even if the page claims hidden')
  assert.equal(shouldRead({ hidden: true, sinceLast: 2000, delay: 600 }), false, 'hidden: no fast polling')
  assert.equal(shouldRead({ hidden: true, sinceLast: HIDDEN_READ_MS, delay: 600 }), true, 'hidden: a slow keepalive read')
  assert.ok(HIDDEN_READ_MS < LEASE_MS, `the hidden cadence (${HIDDEN_READ_MS} ms) stays under the receipt lease (${LEASE_MS} ms): a page wrongly reporting hidden keeps a known state`)
  assert.equal(shouldRead({ hidden: false, sinceLast: 600, delay: 600 }), true)
  assert.equal(shouldRead({ hidden: false, sinceLast: 100, delay: 600 }), false)
  const src = readFileSync(new URL('../client/index.jsx', import.meta.url), 'utf8')
  assert.match(src, /if \(!shouldRead\(\{ hidden: document\.visibilityState === 'hidden', sinceLast: performance\.now\(\) - last, delay: readDelay\(c\.view\(\)\) \}\)\) return/)
})

test('no phone frame: the creature floats in a capsule in the tab', () => {
  const src = readFileSync(new URL('../client/index.jsx', import.meta.url), 'utf8')
  assert.doesNotMatch(src, /rth-phone|rth-notch|rth-home|bezel/, 'no device frame, notch, home bar or bezel')
  assert.match(src, /className="rth-stage" data-rth="avatar-frame"/)
  assert.match(src, /className="rth-tube"[\s\S]{0,200}data-rth="avatar"/, 'the capsule is still the one tap control')
})

// ── A stopped rth-host on the avatar, and its Restart ──
test('a stopped rth-host: the avatar says so, offers Restart (a request), and settles once the new owner answers', async () => {
  const stoppedRead = (restart = true) => ({ unavailable: 'connect ECONNREFUSED', hostStopped: { pid: 4242, exit: 'signal: 11 (SIGSEGV)', panic: null, at: 1 }, restart, receivedAt: NOW })
  const v = voiceControl(stoppedRead(), null, NOW)
  assert.deepEqual([v.actual, v.status, v.label, v.action, v.hint], ['stopped', 'Voice stopped', 'Restart voice', 'restart', 'Tap to restart voice (it comes back asleep)'])
  assert.equal(v.reason, 'rth-host stopped (signal: 11 (SIGSEGV))')
  assert.deepEqual(toggleRequest(v), { url: RESTART_ROUTE, body: { restart: true } })
  assert.deepEqual([sceneFor(pv('idle'), v).form, sceneFor(pv('idle'), v).tint], ['fault', 'warm'])
  const old = voiceControl(stoppedRead(false), null, NOW)
  assert.deepEqual([old.action, toggleRequest(old), old.label], [null, null, 'Restart from the Unfold menu or the tray'])

  // The controller: one POST to the restart route; waiting until a readable owner answers.
  let dead = true
  const posts = []
  const fetch = async (url, init = {}) => {
    if (init.method === 'POST') { posts.push({ url, body: JSON.parse(init.body) }); return { ok: false, status: 202, json: async () => ({}) } }
    const body = dead ? stoppedRead() : read('off')
    delete body.receivedAt
    return { ok: true, status: 200, json: async () => body }
  }
  const t = { t: NOW }
  const c = createVoiceController({ fetch, now: () => t.t })
  await c.load()
  await c.toggle()
  assert.deepEqual(posts, [{ url: RESTART_ROUTE, body: { restart: true } }])
  assert.deepEqual([c.view().actual, c.view().pending, c.view().action], ['stopped', 'waiting', null], 'a request, not a state')
  assert.match(c.view().settling, /^Restarting… waiting for Unfold to confirm/)
  dead = false
  await c.load()
  assert.deepEqual([c.view().actual, c.view().pending, c.view().label], ['off', null, 'Wake voice'], 'the restarted owner is asleep')
  c.dispose()
})
