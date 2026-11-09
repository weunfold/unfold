import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'
import { Context } from '@deepseek-ai/cordis'
import * as plugin from '../index.js'
import { PanelModel, shown, viewLines } from '../src/display.js'
import { REDACTED, redact } from '../src/redact.js'
import { Facts, project } from '../src/settings.js'
import { readContextBounds, readPolicies, setupIdentity, verifyPrompt, parseReady } from '../src/sources.js'

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), 'utf8'))
// GET /v1/status from a running host owner (2026-10-21), read-only; it carries no secret.
const RUNNING = fixture('host-status-running.json')
// The same plus host.mic and host.voice_device in the shapes host.rs / vpio.rs write today.
const VOICE = fixture('host-status-voice-synthetic.json')
const BOOT = RUNNING.link.boot_id
const NOW = 1_792_615_000_000
// The ready lines a live voice trial recorded, with the setup ids the replica computes over the
// fixture policy text (test/fixtures/gemini.rs). The replica is the first Unfold plugins' one, which
// reproduced the trial's own ids over the agent's real policy text; these pin it unchanged.
const READY_SERVER = 'gemini ready gen=1 model=gemini-3.8-live ws_connect_ms=281.9 setup_ms=373.4 setup_to_ready_ms=655.3 resumed=false vad=server policy=ptbr-addressee-v1 proactive=false transcription_langs=- system_extra=false setup_id=bab5b0aed6d89b26'
const READY_LOW = 'gemini ready gen=1 model=gemini-3.8-live ws_connect_ms=274.0 setup_ms=406.3 setup_to_ready_ms=680.3 resumed=false vad=low-v1 policy=ptbr-addressee-v1 proactive=false transcription_langs=- system_extra=false setup_id=3deb324405afb5a2'
// Stand-ins for the agent's source files the panel reads; the plugin takes their paths from config.
const SOURCES = { gemini: fileURLToPath(new URL('./fixtures/gemini.rs', import.meta.url)), context: fileURLToPath(new URL('./fixtures/context.rs', import.meta.url)) }
const policies = readPolicies(SOURCES.gemini)
const bounds = readContextBounds(SOURCES.context)

const CONTEXT = { status: 'not_restored', method: 'none', lineage_since: 40960, turns: 0, tail_records: 0, truncated_turns: 0, detail: 'no snapshot; context starts empty, history is retained' }
const helloFields = (boot = BOOT, ctx = CONTEXT) => ({
  protocol: 2, agent: 'agentd', version: '0.1.0', boot_id: boot, utc_ms_at_origin: NOW - 60_000, journal: '/data/journal.jsonl', backend: 'gemini-live@v1',
  deployment: RUNNING.link.deployment,
  instance: { instance_id: RUNNING.link.instance_id, session_id: RUNNING.link.session_id, state_schema: 1, journal_bytes_at_boot: 40960, context: ctx?.status ?? 'history_retained_model_context_not_restored', ...(ctx ? { context_restore: ctx } : {}) },
  command_generation: 6,
})
const row = (wire, fields, boot = BOOT, n = 1) => ({ id: `guest-${boot}:${wire}:${n}`, producer: `guest-${boot}`, source: 'guest', wire, fields })
const facts = (...rows) => {
  const f = new Facts()
  for (const r of rows) f.ingest(r, NOW - 5_000)
  return f
}
const currentFacts = (msg = READY_SERVER) => facts(row('hello', helloFields()), row('log', { level: 'info', msg }), row('context_restore', CONTEXT))
const liveHost = (body = RUNNING, readAgo = 1_000) => ({ body, read_at: NOW - readAgo, ok: true, error: null })
const attached = { state: 'attached', detail: 'observer' }
const view = (over = {}) => project({ host: liveHost(), facts: currentFacts(), eventsLink: attached, bot: { memoryReview: false }, policies, bounds, now: NOW, ...over })
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
/** SSE frames from a fetch Response, in order; `next(event)` skips frames of other events. */
function sseReader(response) {
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  const queue = []
  let buf = ''
  const pump = async () => {
    const { value, done } = await reader.read()
    if (done) return false
    buf += decoder.decode(value, { stream: true })
    for (let i; (i = buf.indexOf('\n\n')) >= 0; buf = buf.slice(i + 2)) {
      const block = buf.slice(0, i)
      queue.push({ event: /^event: (.*)$/m.exec(block)?.[1], id: /^id: (.*)$/m.exec(block)?.[1] ?? null, data: JSON.parse(/^data: (.*)$/m.exec(block)[1]) })
    }
    return true
  }
  const take = async (event) => {
    for (;;) {
      while (queue.length) {
        const fr = queue.shift()
        if (!event || fr.event === event) return fr
      }
      if (!(await pump())) return null
    }
  }
  return {
    /**
     * The next frame, failing after `ms`. The wait holds a ref'd timer: an in-memory stream holds
     * no handle and the plugin's heartbeat timer is unref'd, so without it the loop could drain.
     */
    async next(event, ms = 2_000) {
      if (ms === Infinity) return take(event)
      let timer
      const late = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`no ${event ?? 'frame'} in ${ms} ms`)), ms) })
      try {
        return await Promise.race([take(event), late])
      } finally {
        clearTimeout(timer)
      }
    },
    /** Every frame that arrives within `ms`. */
    async during(ms) {
      const out = []
      const until = Date.now() + ms
      while (Date.now() < until) {
        const fr = await Promise.race([this.next(undefined, Infinity), sleep(Math.max(1, until - Date.now())).then(() => undefined)])
        if (fr === undefined) break
        if (fr === null) break
        out.push(fr)
      }
      return out
    },
  }
}
/**
 * A stand-in for unfold-events' event window with only what this plugin reads: `rows`, `onChange`,
 * and the row `push` makes of an envelope (`wire` is its kind, the rest of it is `fields`), which is
 * the shape the real window's catalog gives every row.
 */
class EventWindow {
  rows = []
  listeners = new Set()
  onChange(listener) {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }
  push(env) {
    const { v, id, producer, seq, t_ns, source, kind, cause, ...fields } = env
    const row = { v, id, producer, seq, t_ns, source, cause: cause ?? null, wire: kind, fields }
    this.rows.push(row)
    return this.emit('added', row)
  }
  emit(change, payload) {
    for (const listener of this.listeners) listener(change, payload)
    return payload
  }
}
const leaf = (v, id) => v.sections.flatMap((s) => s.leaves).find((l) => l.id === id) ?? assert.fail(`no leaf ${id}`)

test('the setup_id replica reproduces both setup ids of the ready lines, from the policy text in the source file', () => {
  const text = policies.policies['ptbr-addressee-v1'].text
  assert.match(text, /^\[unfold-voice-policy ptbr-addressee-v1\]\nYou are Unfold, a voice assistant\./)
  assert.match(text, /only when someone is talking to you: a name said in passing is not "being addressed"\.$/, 'escapes and line continuations unescaped')
  assert.deepEqual(Object.keys(policies.policies), ['ptbr-addressee-v1'], 'a constant with no policy tag is not a policy')
  const base = { model: 'gemini-3.8-live', proactive: false, langs: [], system: text, resumption: true, compression: true }
  assert.equal(setupIdentity({ ...base, vad: 'server' }), 'bab5b0aed6d89b26')
  assert.equal(setupIdentity({ ...base, vad: 'low-v1' }), '3deb324405afb5a2')
  const low = verifyPrompt(parseReady(READY_LOW), policies.policies)
  assert.deepEqual([low.status, low.resumption, low.compression], ['verified', true, true])
  assert.deepEqual(bounds.bounds, { MAX_TURNS: 128, MAX_CONTEXT_BYTES: 32 * 1024, MAX_TURN_BYTES: 4 * 1024 })
})

test('renders the running host status and current-boot agent facts: every value with its source, the four revisions apart', () => {
  const v = view()
  assert.equal(shown(leaf(v, 'backend')), 'gemini-live@v1')
  assert.equal(leaf(v, 'backend').source, 'host /v1/status link.backend')
  assert.equal(shown(leaf(v, 'model')), 'gemini-3.8-live')
  assert.match(leaf(v, 'model').source, /gemini ready/)
  assert.equal(shown(leaf(v, 'provider_generation')), '1')
  assert.equal(shown(leaf(v, 'command_generation')), '6')
  assert.equal(shown(leaf(v, 'record_generation')), '16')
  assert.equal(shown(leaf(v, 'deployment_agree')), 'yes')
  assert.equal(shown(leaf(v, 'self_repo_head')), '970a132c409a496cb782ec69eda1b51cf89709ed')
  assert.deepEqual(leaf(v, 'self_repo_dirty').value, { dirty: false, dirty_paths: [] })
  assert.equal(shown(leaf(v, 'barge_in')), 'provider')
  assert.equal(shown(leaf(v, 'talk')), 'open')
  // The running host predates host.mic: unknown, and it says why.
  assert.equal(leaf(v, 'mic_state').state, 'unknown')
  assert.match(leaf(v, 'mic_state').note, /absent from this host build/)
  // voice_device: null is a reported value, not a missing one.
  assert.equal(shown(leaf(v, 'device')), 'none reported')
  assert.equal(shown(leaf(v, 'deployment_pending')), 'none reported', 'deployment.pending: null is reported, not missing')
  // The prompt is shown because the replica proved it, with resumption and compression.
  assert.equal(leaf(v, 'prompt_proof').value, 'verified')
  assert.equal(leaf(v, 'prompt_text').value, policies.policies['ptbr-addressee-v1'].text)
  assert.equal(leaf(v, 'prompt_text').revision, policies.policies['ptbr-addressee-v1'].sha256)
  assert.deepEqual([shown(leaf(v, 'resumption')), shown(leaf(v, 'compression'))], ['on', 'on'])
  assert.equal(shown(leaf(v, 'bot_memory_review')), 'off')
  assert.equal(shown(leaf(v, 'context_status')), 'not_restored')
  assert.equal(shown(leaf(v, 'context_link')), 'not_restored')
  assert.equal(leaf(v, 'max_turns').state, 'source')
  assert.equal(shown(leaf(v, 'max_context_bytes')), '32768')
  const r = v.revisions
  assert.deepEqual(
    [r.working_tree_head.value, r.active_deployment.value, r.config_revision.value, r.context_revision.value, r.context_revision.watermark],
    ['970a132c409a496cb782ec69eda1b51cf89709ed', RUNNING.deployment.active.artifact_id, 'bab5b0aed6d89b26', 40960, null],
  )
  const text = viewLines(v)
  assert.ok(text.includes('# Runtime') && text.includes('# Voice') && text.includes('# Prompt and instructions') && text.includes('# Context'))
  assert.ok(text.some((l) => l.startsWith('Model: gemini-3.8-live [live]')))
  assert.ok(v.gaps.some((g) => g.id === 'model_in_status'))
  assert.deepEqual(v.edits, { pending: [], applied: [], rejected: [], note: 'read-only: no write path exists' })
})

test('renders mute requested and applied apart, and the device route and mic mode, when the host reports them', () => {
  const v = view({ host: liveHost(VOICE) })
  assert.equal(shown(leaf(v, 'mic_state')), 'muting')
  assert.deepEqual([leaf(v, 'mic_requested').value.muted, leaf(v, 'mic_applied').value.muted], [true, false])
  assert.equal(shown(leaf(v, 'device')), 'running')
  assert.equal(leaf(v, 'device_route').value.default_devices_at_start.input.name, 'MacBook Pro Microphone')
  assert.equal(leaf(v, 'mic_mode').value.active, 'voice_isolation')
})

test('unknown when a source is missing: no services, no host read, no ready line, no context report', () => {
  const none = project({ now: NOW, policies, bounds })
  for (const id of ['backend', 'model', 'prompt_text', 'context_status', 'bot_memory_review', 'events_link', 'mic_state']) {
    assert.equal(shown(leaf(none, id)), 'unknown', id)
  }
  assert.match(leaf(none, 'backend').note, /unfold-session is not loaded/)
  assert.match(leaf(none, 'model').note, /unfold-events is not loaded/)
  assert.match(none.sources.host.error, /unfold-session is not loaded/)

  const notRead = project({ host: { body: null, read_at: null, ok: false, error: 'host token unavailable: ENOENT' }, now: NOW })
  assert.match(leaf(notRead, 'backend').note, /host status not read: host token unavailable/)

  const noReady = view({ facts: facts(row('hello', helloFields())) })
  assert.equal(shown(leaf(noReady, 'model')), 'unknown')
  assert.match(leaf(noReady, 'model').note, /no gemini ready line observed/)
  assert.equal(leaf(noReady, 'prompt_text').value, null)
  assert.equal(shown(leaf(noReady, 'resumption')), 'unknown')
  // The Hello's own context report stands in for a missing context_restore event.
  assert.equal(shown(leaf(noReady, 'context_turns')), '0')
  assert.equal(leaf(noReady, 'context_turns').source, 'agentd hello instance.context_restore turns')

  const oldAgent = view({ facts: facts(row('hello', helloFields(BOOT, null))) })
  assert.equal(shown(leaf(oldAgent, 'context_status')), 'unknown')
  assert.match(leaf(oldAgent, 'context_status').note, /carries no context_restore/)
})

test('unknown when stale: past the host lease, a failed read, a previous boot, or a dropped observer link', () => {
  const late = view({ host: liveHost(RUNNING, 60_000) })
  assert.equal(leaf(late, 'backend').state, 'stale')
  assert.equal(leaf(late, 'backend').value, null)
  assert.equal(leaf(late, 'backend').last, 'gemini-live@v1')
  assert.equal(shown(leaf(late, 'backend')), 'unknown (stale)')
  assert.match(leaf(late, 'backend').note, /past the 15 s lease/)
  assert.equal(late.revisions.working_tree_head.value, null)

  const failed = view({ host: { ...liveHost(), ok: false, error: 'status 502' } })
  assert.equal(shown(leaf(failed, 'talk')), 'unknown (stale)')
  assert.match(leaf(failed, 'talk').note, /last read failed \(status 502\)/)

  const prevBoot = view({ facts: facts(row('hello', helloFields('0000-1'), '0000-1'), row('log', { level: 'info', msg: READY_SERVER }, '0000-1')) })
  assert.equal(shown(leaf(prevBoot, 'model')), 'unknown (stale)')
  assert.match(leaf(prevBoot, 'model').note, new RegExp(`the host reports boot ${BOOT}, the observer's Hello says 0000-1`))
  assert.equal(leaf(prevBoot, 'prompt_text').value, null, 'a previous boot proves nothing about this one')

  const dropped = view({ eventsLink: { state: 'waiting', detail: 1000 } })
  assert.equal(shown(leaf(dropped, 'setup_id')), 'unknown (stale)')
  assert.match(leaf(dropped, 'setup_id').note, /observer link is waiting/)
})

test('the prompt is never shown unproven: a changed setup_id, an extra instruction, or an unmodelled knob', () => {
  const tampered = view({ facts: currentFacts(READY_SERVER.replace('bab5b0aed6d89b26', '0123456789abcdef')) })
  assert.equal(leaf(tampered, 'prompt_proof').value, 'mismatch')
  assert.equal(leaf(tampered, 'prompt_text').value, null)
  assert.equal(shown(leaf(tampered, 'prompt_text')), 'unknown')
  assert.equal(shown(leaf(tampered, 'compression')), 'unknown')
  const extra = view({ facts: currentFacts(READY_SERVER.replace('system_extra=false', 'system_extra=true')) })
  assert.equal(leaf(extra, 'prompt_proof').value, 'unverifiable')
  assert.match(leaf(extra, 'prompt_proof').note, /the agent's environment or the build/)
  const knob = view({ facts: currentFacts(`${READY_SERVER} temperature=0.4`) })
  assert.match(leaf(knob, 'prompt_proof').note, /does not model: temperature/)
})

// Key-shaped values are built at runtime, so no file spells one (the scan below checks it).
const made = (...parts) => parts.join('')
const GOOGLE_KEY = made('AIza', 'SyD-abcdefghijklmnopqrstuvwxyz012345')

test('secret values never appear: by key at any depth, and inside strings; hashes and ids stay', () => {
  const r = redact({
    api_key: GOOGLE_KEY,
    nested: { token: 'tok-1', Authorization: 'Bearer abc.def', password: 'p', apiKey: 'k', ok: true, has_token: false },
    msg: `connect with Authorization: Bearer eyJhbGciOi.x.y and GEMINI_API_KEY=${GOOGLE_KEY} then x-goog-api-key: zzz`,
    artifact_id: RUNNING.deployment.active.artifact_id,
    manifest_sha256: RUNNING.deployment.active.manifest_sha256,
  })
  assert.equal(r.api_key, REDACTED)
  assert.deepEqual(r.nested, { token: REDACTED, Authorization: REDACTED, password: REDACTED, apiKey: REDACTED, ok: true, has_token: false })
  assert.doesNotMatch(r.msg, /eyJhbGciOi|AIzaSy|zzz/)
  assert.match(r.msg, /Bearer \[redacted\].*GEMINI_API_KEY=\[redacted\].*x-goog-api-key: \[redacted\]/)
  assert.equal(r.artifact_id, RUNNING.deployment.active.artifact_id)
  assert.equal(r.manifest_sha256, RUNNING.deployment.active.manifest_sha256)
})

const SECRET = made('AIza', 'SyQ-THIS-MUST-NEVER-SHOW-0123456789abc')
const leakyStatus = () => {
  const s = structuredClone(VOICE)
  s.host.voice_policy.vad.token = 'vad-secret-token-value'
  s.link.agent = `127.0.0.1:49152?token=link-secret-value`
  s.host.backend_capabilities.context_truncation.note = `slidingWindow only GEMINI_API_KEY=${SECRET}`
  return s
}

test('loads as a Cordis plugin on DSH connection routes, GET only, reads through unfold-session and unfold-events, and redacts what it serves', async () => {
  const ctx = new Context()
  const routes = []
  let reads = 0
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  ctx.provide('unfoldSession', { status: async () => (reads++, { owner: leakyStatus() }) })
  const window = new EventWindow()
  let link = { state: 'attached', detail: 'observer' }
  ctx.provide('unfoldEvents', { window, snapshot: () => ({ link }) })
  ctx.provide('omniTeam', { config: { memoryReview: false, home: '/x', apiKey: 'omni-secret-value' } })
  let seq = 0
  const env = (kind, fields) => ({ v: 2, id: `guest-${BOOT}:${++seq}`, producer: `guest-${BOOT}`, seq, t_ns: seq * 1000, source: 'guest', kind, ...fields })
  window.push(env('hello', helloFields())) // already in the window when the plugin loads

  const fiber = ctx.plugin(plugin, { sources: SOURCES })
  await fiber
  await new Promise((r) => setImmediate(r))
  assert.deepEqual(routes.map((r) => [r.path, r.methods]), [['/api/unfold-settings', ['GET']], ['/api/unfold-settings/stream', ['GET']], ['/api/unfold-settings/config', ['GET']]])

  const res = await routes[0].fetch(new Request('http://h/api/unfold-settings'))
  const body = await res.text()
  const v = JSON.parse(body)
  assert.equal(reads, 1, 'a GET reads the host once')
  assert.equal(shown(leaf(v, 'backend')), 'gemini-live@v1')
  assert.equal(shown(leaf(v, 'model')), 'unknown', 'no gemini ready line yet')
  assert.equal(leaf(v, 'context_status').note, 'already in the event window when this panel loaded')
  assert.equal(shown(leaf(v, 'bot_memory_review')), 'off')
  for (const s of [SECRET, 'vad-secret-token-value', 'link-secret-value', 'omni-secret-value']) assert.ok(!body.includes(s), `served ${s}`)
  assert.equal(leaf(v, 'local_vad').value.token, REDACTED)
  assert.equal(shown(leaf(v, 'agent')), '127.0.0.1:49152?token=[redacted]')

  // The stream: the view on open, then a push when an agentd fact arrives.
  const abort = new AbortController()
  const sse = sseReader(await routes[1].fetch(new Request('http://h/api/unfold-settings/stream', { signal: abort.signal })))
  const next = async () => (await sse.next('settings')).data
  assert.equal(shown(leaf(await next(), 'model')), 'unknown')
  window.push(env('log', { level: 'info', msg: READY_SERVER }))
  const pushed = await next()
  assert.equal(shown(leaf(pushed, 'model')), 'gemini-3.8-live')
  assert.equal(leaf(pushed, 'prompt_proof').value, 'verified')
  link = { state: 'waiting', detail: 500 }
  window.emit('link', link)
  assert.equal(shown(leaf(await next(), 'model')), 'unknown (stale)')
  abort.abort()

  await fiber.dispose()
  assert.equal(routes.length, 0)
})

test('without unfold-session or unfold-events the plugin still serves, and says unknown', async () => {
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  const fiber = ctx.plugin(plugin, {})
  await fiber
  await new Promise((r) => setImmediate(r))
  const v = await (await routes[0].fetch(new Request('http://h/api/unfold-settings'))).json()
  assert.equal(shown(leaf(v, 'backend')), 'unknown')
  assert.equal(shown(leaf(v, 'model')), 'unknown')
  assert.equal(v.sources.agentd.error, 'unfold-events is not loaded: no agentd events')
  await fiber.dispose()
})

test('the browser bundle is built in DSH client format and names the panel', () => {
  const path = new URL('../client.js', import.meta.url)
  assert.ok(existsSync(path), 'run: node scripts/build-client.mjs')
  const text = readFileSync(path, 'utf8')
  assert.match(text, /^\/\/ Generated from src\/client .*\nwindow\.__ModuleLoader__\.load\(\{\n {2}id: '@automagik\/unfold-settings-dsh',/)
  assert.match(text, /Unfold settings/)
  assert.match(text, /unknown \(stale\)/, 'the display rule is bundled from src/display.js')
  // Both ways in: the left panel-list entry (plain profiles) and the right-sidebar tab (the Bot surface
  // hides the left panel list), with the unfold-events seats and an optional right-sidebar inject.
  assert.match(text, /name: "sidebar\.panellist", id: PANEL_ID/)
  for (const seat of ['sidebar.right.pane.tab', 'sidebar.right.pane.tab.title']) assert.ok(text.includes(`name: "${seat}", key: UNFOLD_SETTINGS_TAB_ID`), seat)
  assert.match(text, /UNFOLD_SETTINGS_TAB_ID = "@automagik\/unfold-settings-dsh"/)
  assert.match(text, /UNFOLD_SETTINGS_TAB_KIND = "unfold\.settings"/)
  assert.match(text, /ctx\.inject\(\["sidebarRightTabs", "sidebarRight"\]/)
})

// unfold-sse-dsh's index.js as released with the first Unfold plugins: this package carries it as
// src/sse.js, byte for byte. Change the hub there, then copy it here and update the hash.
const SSE_SHA256 = 'a969339dcce3085ea65242dda857c272a0cc3261dea7fadafd8331d07fce2ae2'
const shipped = () => ['index.js', 'client.js', ...readdirSync(new URL('../src/', import.meta.url), { recursive: true })
  .filter((f) => /\.(js|jsx)$/.test(f)).map((f) => `src/${f}`)]
const source = (file) => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8')

test('src/sse.js is unfold-sse-dsh\'s index.js, byte for byte', () => {
  assert.equal(createHash('sha256').update(readFileSync(new URL('../src/sse.js', import.meta.url))).digest('hex'), SSE_SHA256)
})

// unfold-events-dsh's src/client/ui.jsx as released with the first Unfold plugins: the look every
// Unfold sidebar tab shares. Change it there, then copy it here and update the hash.
const UI_SHA256 = 'c9f820717b4b5d0b236dd32d4ce794b3fab29b4f86d2d2cf84483216677f4d21'

test('src/client/ui.jsx is unfold-events-dsh\'s copy, byte for byte', () => {
  assert.equal(createHash('sha256').update(readFileSync(new URL('../src/client/ui.jsx', import.meta.url))).digest('hex'), UI_SHA256)
})

// Secret scanners and push protection match a key's shape, not its intent, so a test that needs a
// key-shaped value builds it at runtime and no file in the package spells one.
const KEY_SHAPES = [
  /AIza[0-9A-Za-z_-]{35}/, /\bsk-[A-Za-z0-9]{20,}/, /\bgh[pousr]_[A-Za-z0-9]{20,}/, /github_pat_[A-Za-z0-9_]{20,}/,
  /\bxox[abprs]-[A-Za-z0-9-]{10,}/, /\bAKIA[0-9A-Z]{16}\b/, /-----BEGIN [A-Z ]*PRIVATE KEY-----/,
]

test('no file in the package spells a key-shaped value', () => {
  const files = ['index.js', 'client.js', 'README.md', ...['src', 'test'].flatMap((dir) =>
    readdirSync(new URL(`../${dir}/`, import.meta.url), { recursive: true })
      .filter((f) => /\.(js|jsx|mjs|json)$/.test(f)).map((f) => `${dir}/${f}`))]
  for (const file of files) for (const shape of KEY_SHAPES) assert.doesNotMatch(source(file), shape, file)
})

test('the package names no port, no address and no default source path: it reads only what its config names', () => {
  for (const file of shipped()) {
    const src = source(file)
    // The two fixed ports the first Unfold plugins used (agentd observer, host owner), spelled so
    // that a scan for them finds only shipped code.
    for (const port of [77_00, 77_81]) assert.doesNotMatch(src, new RegExp(`\\b${port}\\b`), file)
    assert.doesNotMatch(src, /127\.0\.0\.1|localhost|\[::1\]|0\.0\.0\.0/, file)
    assert.doesNotMatch(src, /node:net|node:http\b|\.listen\(/, file)
    assert.doesNotMatch(src, /\.\.\/\.\.\/|import\.meta\.url|process\.env/, `${file}: a path or setting that does not come from config`)
  }
})

test('no write route: GET only, and nothing in the package writes', async () => {
  for (const file of shipped()) assert.doesNotMatch(source(file), /writeFile|appendFile|unlink|rmSync|mkdir|createWriteStream|renameSync/, file)
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  const fiber = ctx.plugin(plugin, {})
  await fiber
  await new Promise((r) => setImmediate(r))
  assert.deepEqual(routes.map((r) => [r.path, r.methods]), [['/api/unfold-settings', ['GET']], ['/api/unfold-settings/stream', ['GET']], ['/api/unfold-settings/config', ['GET']]])
  const web = plugin.routes({ refresh: async () => ({}) }, '/api/unfold-settings')
  for (const method of ['POST', 'PUT', 'PATCH', 'DELETE']) {
    let head = null
    await web[0].handler({ method }, { writeHead: (status, headers) => { head = [status, headers] }, end() {} })
    assert.deepEqual(head, [405, { allow: 'GET' }], method)
  }
  await fiber.dispose()
})

test('with no source paths configured the prompt is never shown and the bounds are unknown', async () => {
  assert.deepEqual(readPolicies(undefined), { path: null, error: 'not configured', policies: {} })
  assert.deepEqual(readContextBounds(undefined), { path: null, error: 'not configured', bounds: {} })
  const p = await loadPlugin({ sources: {} }, { session: { status: async () => ({ owner: RUNNING }) }, rows: [['hello', helloFields()], ['log', { level: 'info', msg: READY_SERVER }]] })
  const v = await p.get()
  assert.equal(shown(leaf(v, 'model')), 'gemini-3.8-live', 'the agent facts still show')
  assert.notEqual(leaf(v, 'prompt_proof').value, 'verified')
  assert.equal(leaf(v, 'prompt_text').value, null)
  assert.notEqual(leaf(v, 'max_turns').state, 'source')
  await p.fiber.dispose()
})

test('an unfold-session with no host status (Unfold\'s own instance) leaves every host value unknown, and says why', async () => {
  const p = await loadPlugin({}, { session: { submit: async () => ({}) }, rows: [['hello', helloFields()]] })
  const v = await p.get()
  assert.equal(shown(leaf(v, 'backend')), 'unknown')
  assert.match(leaf(v, 'backend').note, /unfold-session reports no host status/)
  assert.equal(v.sources.host.error, 'unfold-session reports no host status')
  await p.fiber.dispose()
})

test('an owner the session reports unavailable is read once per load of unfold-session: no heartbeat or GET reads it again', async () => {
  let reads = 0
  const session = { status: async () => (reads++, { owner: { unavailable: 'x' } }) }
  const p = await loadPlugin({ leaseMs: 10_000, heartbeatMs: 5 }, { rows: [['hello', helloFields()]] })
  const off = p.ctx.provide('unfoldSession', session)
  await new Promise((r) => setImmediate(r))
  const { sse, close } = await p.open()
  const first = (await sse.next('settings')).data
  assert.equal(first.sources.host.error, 'x')
  for (let i = 0; i < 5; i++) await sse.next('heartbeat')
  const got = await p.get()
  assert.equal(got.sources.host.error, 'x')
  assert.equal(reads, 1, 'one read, then neither 5 heartbeats nor a GET read again')
  close()
  // unfold-session reloads on the same ctx, with the same service object: it is read once more.
  await off()
  p.ctx.provide('unfoldSession', session)
  await new Promise((r) => setImmediate(r))
  assert.equal((await p.get()).sources.host.error, 'x')
  await p.get()
  assert.equal(reads, 2, 'a reload of unfold-session reads once more, and only once')
  await p.fiber.dispose()
})

// ── Freshness: what the panel DISPLAYS as time passes and the stream fails ──

const drawnLeaf = (drawn, id) => drawn.sections.flatMap((s) => s.leaves).find((d) => d.leaf.id === id) ?? assert.fail(`no leaf ${id}`)
const drawnRev = (drawn, key) => drawn.revisions.find((r) => r.key === key)
async function loadPlugin(config, { session, rows = [], link = attached } = {}) {
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  if (session) ctx.provide('unfoldSession', session)
  const window = new EventWindow()
  const state = { link }
  ctx.provide('unfoldEvents', { window, snapshot: () => ({ link: state.link }) })
  let seq = 0
  const push = (kind, fields, boot = BOOT) => window.push({ v: 2, id: `guest-${boot}:${++seq}`, producer: `guest-${boot}`, seq, t_ns: seq * 1000, source: 'guest', kind, ...fields })
  for (const [kind, fields] of rows) push(kind, fields)
  const fiber = ctx.plugin(plugin, { sources: SOURCES, ...config })
  await fiber
  await new Promise((r) => setImmediate(r))
  const setLink = (l) => {
    state.link = l
    window.emit('link', l)
  }
  const get = async () => (await routes[0].fetch(new Request('http://h/api/unfold-settings'))).json()
  const open = async () => {
    const abort = new AbortController()
    const sse = sseReader(await routes[1].fetch(new Request('http://h/api/unfold-settings/stream', { signal: abort.signal })))
    return { sse, close: () => abort.abort() }
  }
  return { ctx, fiber, routes, window, push, setLink, get, open, service: ctx.get('unfoldSettings') }
}

test('an idle panel expires host values on its own clock (30 ms lease, no events at all)', async () => {
  const p = await loadPlugin({ leaseMs: 30, heartbeatMs: 0 }, { session: { status: async () => ({ owner: RUNNING }) } })
  const { sse, close } = await p.open()
  const m = new PanelModel()
  m.setView((await sse.next('settings')).data, Date.now())
  m.streamOpen()
  let drawn = m.render(Date.now())
  assert.deepEqual([drawnLeaf(drawn, 'backend').text, drawnLeaf(drawn, 'backend').state], ['gemini-live@v1', 'live'])
  assert.equal(drawnRev(drawn, 'working_tree_head').state, 'live')
  const next = m.nextChange(Date.now())
  assert.ok(next !== null && next - Date.now() <= 40, 'the panel schedules its own redraw at the expiry')
  await sleep(100) // no SSE frame arrives: nothing tells the browser anything
  drawn = m.render(Date.now())
  assert.deepEqual([drawnLeaf(drawn, 'backend').text, drawnLeaf(drawn, 'backend').state], ['unknown (stale)', 'stale'])
  assert.match(drawnLeaf(drawn, 'backend').note, /older than its lease/)
  assert.deepEqual([drawnRev(drawn, 'working_tree_head').text, drawnRev(drawn, 'active_deployment').state], ['unknown (stale)', 'stale'])
  assert.equal(drawnLeaf(drawn, 'max_turns').text, '128', 'working-tree constants are not live claims and do not expire')
  assert.equal(leaf(p.service.snapshot(), 'backend').state, 'stale', 'and the server agrees')
  const after = Date.now()
  const again = m.nextChange(after)
  assert.ok(again === null || again > after, 'an expiry already past schedules no immediate redraw (no render loop)')
  close()
  await p.fiber.dispose()
})

test('heartbeats confirm event-derived values but never renew host values; a silent stall stops them', async () => {
  let reads = 0
  const p = await loadPlugin({ leaseMs: 10_000, heartbeatMs: 20 }, {
    session: { status: async () => (reads++, { owner: RUNNING }) },
    rows: [['hello', helloFields()], ['log', { level: 'info', msg: READY_SERVER }]],
  })
  const { sse, close } = await p.open()
  const m = new PanelModel()
  m.setView((await sse.next('settings')).data, Date.now())
  m.streamOpen()
  const frames = await sse.during(130)
  for (const fr of frames) fr.event === 'heartbeat' ? m.heartbeat(fr.data, Date.now()) : m.setView(fr.data, Date.now())
  const beats = frames.filter((fr) => fr.event === 'heartbeat')
  assert.ok(beats.length >= 3, `heartbeats arrive while connected (${beats.length})`)
  assert.ok(beats.every((b) => !('sections' in b.data) && !('sources' in b.data)), 'a heartbeat carries no host sample')
  assert.equal(reads, 1, 'heartbeats never read the host while its read is fresh')
  let drawn = m.render(Date.now())
  assert.deepEqual([drawnLeaf(drawn, 'model').text, drawnLeaf(drawn, 'backend').text], ['gemini-3.8-live', 'gemini-live@v1'])
  // Connected but silent: the socket stays open and nothing more is read from it.
  await sleep(100)
  drawn = m.render(Date.now())
  assert.equal(drawnLeaf(drawn, 'model').text, 'unknown (stale)')
  assert.match(drawnLeaf(drawn, 'model').note, /no word from the settings stream/)
  assert.equal(drawnRev(drawn, 'config_revision').state, 'stale')
  assert.equal(drawnLeaf(drawn, 'backend').state, 'live', 'a host value keeps its own lease (10 s), neither extended nor cut by heartbeats')
  close()
  await p.fiber.dispose()
})

test('while a stream is open the host is renewed by a bounded read, and a failed read shows as stale', async () => {
  let reads = 0
  const p = await loadPlugin({ leaseMs: 60, heartbeatMs: 20 }, {
    session: { status: async () => { reads++; if (reads > 3) throw new Error('host down'); return { owner: RUNNING } } },
  })
  const { sse, close } = await p.open()
  const m = new PanelModel()
  m.setView((await sse.next('settings')).data, Date.now())
  const frames = await sse.during(220)
  const views = frames.filter((fr) => fr.event === 'settings').map((fr) => fr.data)
  const readAts = views.map((v) => v.sources.host.read_at).filter(Boolean)
  assert.ok(new Set(readAts).size >= 2, 'renewed views carry newer host reads')
  assert.ok(reads >= 4 && reads <= 14, `bounded: one read per beat at most (${reads})`)
  for (const v of views) m.setView(v, Date.now())
  const drawn = m.render(Date.now())
  assert.equal(drawnLeaf(drawn, 'backend').text, 'unknown (stale)')
  assert.match(drawnLeaf(drawn, 'backend').note, /last read failed \(host down\)/)
  close()
  await p.fiber.dispose()
})

test('a lost stream stops event-derived values at once, a failed Refresh stops everything, and a new view recovers', async () => {
  const T = NOW
  const v = view()
  const m = new PanelModel()
  m.setView(v, T)
  m.streamOpen()
  let drawn = m.render(T)
  assert.deepEqual([drawnLeaf(drawn, 'model').state, drawnLeaf(drawn, 'backend').state], ['live', 'live'])
  m.streamLost(T + 1)
  drawn = m.render(T + 2)
  assert.deepEqual([drawnLeaf(drawn, 'model').text, drawnLeaf(drawn, 'prompt_text').text], ['unknown (stale)', 'unknown (stale)'])
  assert.match(drawnLeaf(drawn, 'model').note, /stream was lost/)
  assert.equal(drawnRev(drawn, 'config_revision').state, 'stale')
  assert.equal(drawnRev(drawn, 'working_tree_head').state, 'live', 'a host chip keeps its own lease')
  assert.equal(drawn.stream, 'reconnecting')
  m.heartbeat({ at: v.at + 3, rev: 1 }, T + 3)
  assert.equal(drawnLeaf(m.render(T + 3), 'model').state, 'stale', 'a beat does not undo a loss; only a new view does')
  m.streamOpen()
  m.setView({ ...v, at: v.at + 4 }, T + 4)
  assert.equal(drawnLeaf(m.render(T + 4), 'model').text, 'gemini-3.8-live', 'recovered by the new view')
  m.refreshFailed('status 502')
  drawn = m.render(T + 5)
  assert.deepEqual([drawnLeaf(drawn, 'backend').text, drawnLeaf(drawn, 'model').text], ['unknown (stale)', 'unknown (stale)'])
  assert.match(drawnLeaf(drawn, 'backend').note, /last refresh failed \(status 502\)/)
  assert.equal(drawn.refreshError, 'status 502')
  assert.equal(drawnLeaf(drawn, 'max_context_bytes').text, '32768')
  assert.equal(m.nextChange(T + 5), null, 'nothing left to expire')
  m.setView({ ...v, at: v.at + 6 }, T + 6)
  drawn = m.render(T + 6)
  assert.deepEqual([drawnLeaf(drawn, 'backend').text, drawn.refreshError], ['gemini-live@v1', null], 'a successful refresh recovers')
})

// ── Observation epochs ──

test('a same-boot reattach does not revive provider facts it may have missed; a new ready line does', async () => {
  const p = await loadPlugin({}, { session: { status: async () => ({ owner: RUNNING }) }, rows: [['hello', helloFields()], ['log', { level: 'info', msg: READY_SERVER }]] })
  let v = await p.get()
  assert.deepEqual([shown(leaf(v, 'model')), shown(leaf(v, 'provider_generation'))], ['gemini-3.8-live', '1'])
  p.setLink({ state: 'waiting', detail: 500 })
  v = await p.get()
  assert.equal(shown(leaf(v, 'model')), 'unknown (stale)')
  // Reattached with no Hello seen yet: the boot is not positively known.
  p.setLink({ state: 'attached', detail: 'observer' })
  v = await p.get()
  assert.equal(shown(leaf(v, 'model')), 'unknown (stale)', 'reattachment alone re-establishes nothing')
  assert.equal(v.sources.agentd.boot, null)
  // The observer's real order: drop, Hello on the new attachment, then attached.
  p.setLink({ state: 'connecting', detail: null })
  p.push('hello', helloFields())
  p.setLink({ state: 'attached', detail: 'observer' })
  v = await p.get()
  assert.equal(shown(leaf(v, 'model')), 'unknown (stale)', 'same boot, but the ready line was before the gap')
  assert.match(leaf(v, 'model').note, /before the observer link was interrupted/)
  assert.equal(leaf(v, 'prompt_text').value, null)
  assert.equal(shown(leaf(v, 'context_status')), 'not_restored', 'the new Hello re-establishes the context report')
  assert.equal(v.sources.agentd.boot, BOOT)
  // The provider changed during the gap (gen 2, another VAD knob): only new evidence shows it.
  p.push('log', { level: 'info', msg: READY_LOW.replace('gen=1', 'gen=2').replace('resumed=false', 'resumed=true') })
  v = await p.get()
  assert.deepEqual(
    [shown(leaf(v, 'provider_generation')), shown(leaf(v, 'provider_vad')), shown(leaf(v, 'setup_id')), leaf(v, 'prompt_proof').value, shown(leaf(v, 'resumed'))],
    ['2', 'low-v1', '3deb324405afb5a2', 'verified', 'true'],
  )
  await p.fiber.dispose()
})

test('a current claim needs a Hello from this attachment that the host does not contradict; a new session replaces the context', () => {
  const noHello = view({ facts: facts(row('log', { level: 'info', msg: READY_SERVER })) })
  assert.equal(shown(leaf(noHello, 'model')), 'unknown (stale)')
  assert.match(leaf(noHello, 'model').note, /no Hello observed in this observer attachment/)
  const other = { ...CONTEXT, turns: 3, lineage_since: 90000, detail: 'restored as text history' }
  const f = facts(row('hello', helloFields()), row('context_restore', CONTEXT), row('log', { level: 'info', msg: READY_SERVER }))
  f.ingest(row('hello', { ...helloFields(BOOT, other), instance: { ...helloFields(BOOT, other).instance, session_id: 'sess-new' } }), NOW)
  const v = view({ facts: f })
  assert.deepEqual([shown(leaf(v, 'context_turns')), shown(leaf(v, 'context_lineage'))], ['3', '90000'], 'the new Hello wins over the old epoch\'s report')
  assert.equal(shown(leaf(v, 'model')), 'unknown (stale)', 'the old session\'s ready line is history')
})

test('a delayed host read from a replaced unfold-session never revalidates the view', async () => {
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  let resolveA
  const offA = ctx.provide('unfoldSession', { status: () => new Promise((r) => { resolveA = r }) })
  const fiber = ctx.plugin(plugin, { sources: SOURCES })
  await fiber
  await new Promise((r) => setImmediate(r))
  const pending = routes[0].fetch(new Request('http://h/api/unfold-settings'))
  await new Promise((r) => setImmediate(r))
  assert.equal(typeof resolveA, 'function', 'the read on session A is in flight')
  await offA()
  const B = structuredClone(RUNNING)
  B.link.backend = 'fake@v1'
  ctx.provide('unfoldSession', { status: async () => ({ owner: B }) })
  await new Promise((r) => setImmediate(r))
  resolveA({ owner: RUNNING }) // A answers late, after it was replaced
  const late = await (await pending).json()
  assert.notEqual(shown(leaf(late, 'backend')), 'gemini-live@v1', 'A\'s late answer is dropped')
  assert.notEqual(shown(leaf(ctx.get('unfoldSettings').snapshot(), 'backend')), 'gemini-live@v1')
  const fresh = await (await routes[0].fetch(new Request('http://h/api/unfold-settings'))).json()
  assert.equal(shown(leaf(fresh, 'backend')), 'fake@v1', 'B is read')
  await fiber.dispose()
})

test('expiry is scheduled from the next FUTURE deadline; an expired leaf redraws once, not in a loop', async (t) => {
  // One live host leaf (expires_at 10) and one live event leaf (stream lease 50), view at 0.
  const v = {
    at: 0, stream_lease_ms: 50,
    revisions: {},
    sections: [{ id: 's', title: 'S', leaves: [
      { id: 'h', label: 'host', value: 'x', state: 'live', basis: 'host', source: 'host', expires_at: 10 },
      { id: 'e', label: 'event', value: 'y', state: 'live', basis: 'events', source: 'events' },
    ] }],
    sources: {}, edits: { pending: [], applied: [], rejected: [] }, gaps: [],
  }
  const m = new PanelModel()
  m.setView(v, 0)
  m.streamOpen()
  assert.equal(m.nextChange(0), 11, 'first the host expiry')
  for (const now of [11, 12, 13, 30]) assert.equal(m.nextChange(now), 51, `at ${now}: the expired host deadline no longer counts; the later event lease is kept`)
  assert.equal(drawnLeaf(m.render(12), 'h').state, 'stale')
  assert.equal(drawnLeaf(m.render(12), 'e').state, 'live')
  for (const now of [51, 52, 1000]) assert.equal(m.nextChange(now), null, `at ${now}: all expired, nothing can change without new evidence`)

  // The client's loop (client/index.jsx): render, then one timer to nextChange; the timer redraws.
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'], now: 0 })
  const m2 = new PanelModel()
  m2.setView(v, 0)
  m2.streamOpen()
  let renders = 0
  let timer = null
  const draw = () => {
    renders++
    m2.render(Date.now())
    clearTimeout(timer)
    const next = m2.nextChange(Date.now())
    timer = next === null ? null : setTimeout(draw, Math.max(0, next - Date.now()))
  }
  draw()
  for (let i = 0; i < 1000; i++) t.mock.timers.tick(10) // 10 s in 10 ms steps
  assert.equal(renders, 3, 'initial draw, one at the host expiry, one at the event expiry; then none')
  assert.equal(timer, null)
})

// ── Ordering: one snapshot identity for GET and SSE ──

const idView = (base, { rev, at, serving = 'S1', started = 1, model = 'live' }) => {
  const v = structuredClone(base)
  for (const s of v.sections) for (const l of s.leaves) if (l.id === 'model') Object.assign(l, model === 'live' ? { state: 'live' } : { state: 'stale', value: null, note: 'observed before the observer link was interrupted' })
  for (const s of v.sections) for (const l of s.leaves) if (l.basis === 'host' && l.expires_at != null) l.expires_at = at + 3000
  return { ...v, serving, serving_started_at: started, rev, at, stream_lease_ms: 15_000 }
}

test('a late Refresh cannot undo a newer post-gap view, and a heartbeat confirms only the revision it names', () => {
  const base = view()
  const m = new PanelModel()
  // 1. view at 1000, live with the old model
  assert.equal(m.setView(idView(base, { rev: 10, at: 1000 }), 1000), true)
  m.streamOpen()
  assert.equal(drawnLeaf(m.render(1000), 'model').text, 'gemini-3.8-live')
  // 2. stream loss at 2000
  m.streamLost(2000)
  // 3. the current SSE view after the gap, at 2100: the model is stale (not re-observed)
  assert.equal(m.setView(idView(base, { rev: 15, at: 2100, model: 'stale' }), 2100), true)
  assert.equal(drawnLeaf(m.render(2100), 'model').text, 'unknown (stale)')
  // 4. a delayed GET answer served at 1000 arrives at client 3000: refused
  assert.equal(m.setView(idView(base, { rev: 10, at: 1000 }), 3000), false, 'superseded by rev 15')
  // 5. a heartbeat at 4000 naming rev 20: it does not confirm rev 15, and asks for the new view
  assert.equal(m.heartbeat({ at: 4000, serving: 'S1', serving_started_at: 1, rev: 20 }, 4000), false)
  assert.equal(m.needsSnapshot, true)
  let drawn = m.render(4000)
  assert.equal(drawnLeaf(drawn, 'model').text, 'unknown (stale)', 'never the old model')
  assert.equal(drawnLeaf(drawn, 'setup_id').state, 'stale')
  assert.match(drawnLeaf(drawn, 'setup_id').note, /newer settings than this view/)
  // Other superseded forms: the same revision earlier, an older serving.
  assert.equal(m.setView(idView(base, { rev: 15, at: 2000, model: 'stale' }), 4000), false, 'same revision, earlier')
  assert.equal(m.setView(idView(base, { rev: 99, at: 3900, serving: 'S0', started: 0 }), 4000), false, 'an older serving instance')
  // Recovery: the rev-20 view (the model re-observed), then a matching beat confirms it.
  assert.equal(m.setView(idView(base, { rev: 20, at: 4000 }), 4000), true)
  assert.equal(m.needsSnapshot, false)
  assert.equal(m.heartbeat({ at: 5000, serving: 'S1', serving_started_at: 1, rev: 20 }, 5000), true)
  assert.equal(drawnLeaf(m.render(5000), 'model').text, 'gemini-3.8-live')
  assert.equal(drawnLeaf(m.render(5000 + 15_000 + 1), 'model').state, 'stale', 'and it still needs beats to stay live')
  // A newer serving (the plugin reloaded) replaces the old one.
  assert.equal(m.setView(idView(base, { rev: 1, at: 6000, serving: 'S2', started: 5500 }), 6000), true)
})

test('a delayed view keeps its real age (the clock offset never shrinks on a late arrival)', () => {
  const base = view()
  const m = new PanelModel()
  m.setView(idView(base, { rev: 1, at: 1000 }), 1000) // offset 0
  m.streamOpen()
  // rev 2 was served at 1500 (host lease to 4500) but reaches the browser at 9000.
  assert.equal(m.setView(idView(base, { rev: 2, at: 1500 }), 9000), true)
  const drawn = m.render(9000)
  assert.equal(drawnLeaf(drawn, 'backend').state, 'stale', 'the host read is 7.5 s old, past its 3 s lease')
  assert.equal(drawnLeaf(drawn, 'model').state, 'live', 'still inside the 15 s stream lease from 1500')
  assert.equal(drawnLeaf(m.render(1500 + 15_000 + 1), 'model').state, 'stale')
})

test('GET and SSE share one rising revision; a change is pushed; each heartbeat names the current revision', async () => {
  let owner = RUNNING
  const p = await loadPlugin({ leaseMs: 10_000, heartbeatMs: 20 }, {
    session: { status: async () => ({ owner }) },
    rows: [['hello', helloFields()], ['log', { level: 'info', msg: READY_SERVER }]],
  })
  const { sse, close } = await p.open()
  const first = (await sse.next('settings')).data
  assert.ok(first.serving && first.serving_started_at && Number.isInteger(first.rev))
  // The host's next read reports a new chunk size, so the content changes whatever the clock says.
  owner = { ...RUNNING, host: { ...RUNNING.host, voice_policy: { ...RUNNING.host.voice_policy, chunk_ms: 60 } } }
  const got = await p.get() // a GET reads the host
  assert.equal(got.serving, first.serving)
  assert.ok(got.rev > first.rev, `GET rev ${got.rev} > stream rev ${first.rev}`)
  const pushed = (await sse.next('settings')).data
  assert.equal(pushed.rev, got.rev, 'the GET\'s change reached the open stream')
  assert.equal(leaf(pushed, 'chunk_ms').value, 60, 'the pushed view carries the new host value')
  const beat = (await sse.next('heartbeat')).data
  assert.deepEqual([beat.serving, beat.rev], [got.serving, got.rev], 'the beat confirms exactly that revision')
  const m = new PanelModel()
  m.setView(first, Date.now())
  assert.equal(m.heartbeat(beat, Date.now()), false, 'an older view is not confirmed by a newer beat')
  assert.equal(m.needsSnapshot, true)
  m.setView(got, Date.now())
  assert.equal(m.heartbeat(beat, Date.now()), true)
  close()
  await p.fiber.dispose()
})

test('a stale revision target does not survive a newer serving instance', () => {
  const base = view()
  const m = new PanelModel()
  m.setView(idView(base, { rev: 10, at: 1000, serving: 'S1', started: 1 }), 1000)
  m.streamOpen()
  m.heartbeat({ at: 1100, serving: 'S1', serving_started_at: 1, rev: 20 }, 1100)
  assert.equal(m.needsSnapshot, true)
  // The plugin reloads: S2 starts again at rev 1.
  assert.equal(m.setView(idView(base, { rev: 1, at: 1200, serving: 'S2', started: 1150 }), 1200), true)
  assert.equal(m.needsSnapshot, false, 'the S1 rev-20 target is retired by the newer serving')
  assert.equal(m.heartbeat({ at: 1300, serving: 'S2', serving_started_at: 1150, rev: 1 }, 1300), true)
  assert.equal(drawnLeaf(m.render(1300), 'model').text, 'gemini-3.8-live', 'the fresh value is shown live')
})
