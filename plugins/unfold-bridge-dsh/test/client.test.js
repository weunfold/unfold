import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { runInNewContext } from 'node:vm'
import { bundleClient } from '../scripts/build-client.mjs'
import { checkCopy } from '../scripts/copy-check.mjs'
import { DRAFT_TEXT, EMOJI_TEXT, HIDDEN_TEXT, OTHER, SESSION, TOKEN, fixtureView } from './fixture-view.js'
import { ALLOWED, REFUSED } from './hidden-vectors.js'
import { byData, expand, find, findAll, html, loadClient, text } from './render.js'


const client = await loadClient()
const en = client.translator('en')
const pt = client.translator('pt-BR')
const actions = { send: client.sendDraft, cancel: client.cancelDraft, watch: client.watchSession, drop: client.dropSubscription, undo: client.undoChange }
const h = (View, props) => ({ type: View, props: { t: en, actions, ...props } })
const render = (View, props) => expand(h(View, props))
const tick = () => new Promise(resolve => setImmediate(resolve))

// Every request the client makes, as the bridge would read it.
function recordFetch(status = 200, answer = { ok: true }) {
  const calls = []
  client.fetch = async (url, init) => {
    calls.push({ url, method: init.method, type: init.headers['content-type'], body: init.body })
    return { ok: status < 400, status, json: async () => answer }
  }
  return calls
}

test('client.js is the build of src/client', async () => {
  assert.equal(await bundleClient(), readFileSync(new URL('../client.js', import.meta.url), 'utf8'), 'client.js is stale: run `npm run build` and commit it')
})

// DSH runs client.js as a classic script that hands a factory to its module loader.
test('the client registers seven slots and never conversation.approval.detail', () => {
  const loads = []
  runInNewContext(readFileSync(new URL('../client.js', import.meta.url), 'utf8'), { window: { __ModuleLoader__: { load: entry => loads.push(entry) } } })
  assert.equal(loads.length, 1)
  assert.equal(loads[0].id, '@automagik/unfold-bridge-dsh')
  const requested = new Set()
  const exports = loads[0].factory(name => {
    requested.add(name)
    if (name !== 'react') throw new Error(`no platform module ${name}`)
    return { createElement: () => null, useState: initial => [initial, () => {}], useSyncExternalStore: (_s, get) => get() }
  })
  assert.deepEqual([...requested], ['react'])
  assert.deepEqual(Array.from(exports.inject), ['slots'])
  const registered = []
  const effects = []
  exports.apply({
    get: () => undefined,
    effect: (_run, label) => effects.push(label),
    slots: {
      inject: (name, register) => register(),
      register: (options, component) => { registered.push({ ...options, component: typeof component }); return () => {} },
    },
  })
  assert.deepEqual(effects, ['unfold-bridge: client styles', 'unfold-bridge: the view stream', 'unfold-bridge: the install offer'])
  assert.deepEqual(registered, [
    { name: 'sidebar.workspaces.session.row.action', id: 'unfold-watch', order: 300, component: 'function' },
    { name: 'sidebar.session.row.leading', id: 'unfold-watched', order: 100, component: 'function' },
    { name: 'sidebar.footer.action', id: 'unfold-board', order: 100, component: 'function' },
    { name: 'conversation.input.dock', id: 'unfold-dock', order: 50, component: 'function' },
    { name: 'settings.section', id: 'unfold-bridge', order: 900, label: 'Unfold', component: 'function' },
    { name: 'settings.section.icon', id: 'unfold-bridge', order: 900, component: 'function' },
    { name: 'sidebar.footer.action', id: 'unfold-bridge.install', order: 110, component: 'function' },
  ])
  assert.ok(!registered.some(entry => entry.name === 'conversation.approval.detail'))
})

test('the slots render from a fixture view', () => {
  const snap = fixtureView({ draft: true, approval: true })
  const row = render(client.RowAction, { snap, sessionId: SESSION })
  assert.equal(row.props['aria-pressed'], 'true', 'the row has its own subscription')
  assert.equal(row.props['aria-label'], 'Stop watching with Unfold · Working')
  const board = text(render(client.BoardPanel, { snap }))
  for (const words of ['builder · app', 'writer · docs', 'Unfold connected', "Unfold's read of this agent's messages", 'Draft for builder · app', 'Changes by Unfold', 'Unfold, at your request: added workspace docs (turns)', 'Unfold: removed agent helper (turns)']) assert.ok(board.includes(words), words)
  const dock = render(client.Dock, { snap, sessionId: SESSION })
  assert.equal(text(find(dock, byData('data-unfold-draft-to'))), 'To: builder · app')
  assert.ok(text(dock).includes("Unfold's read of this agent's messages"))
  const settings = text(render(client.SettingsSection, { snap }))
  for (const words of ['Unfold connected', 'by you', 'by Unfold', 'Undo', 'Unfold, at your request: added workspace docs (turns)', 'You: added session builder · app (turns, approvals)']) assert.ok(settings.includes(words), words)
})

test('a subscription shows who added it only when the view says so', () => {
  const badges = snap => findAll(render(client.SettingsSection, { snap }), byData('data-unfold-by')).map(node => node.props['data-unfold-by'])
  assert.deepEqual(badges(fixtureView()), ['person', 'unfold'])
  const rows = findAll(render(client.SettingsSection, { snap: fixtureView() }), byData('data-unfold-sub'))
  assert.ok(text(rows.find(row => row.props['data-unfold-sub'] === 'sub-row')).includes('by you'))
  assert.ok(text(rows.find(row => row.props['data-unfold-sub'] === 'sub-docs')).includes('by Unfold'))
  // No badge is guessed from the change log: it is truncated, so it would badge only some.
  const plain = render(client.SettingsSection, { snap: fixtureView({ badges: false }) })
  assert.deepEqual(badges(fixtureView({ badges: false })), [])
  assert.ok(!text(plain).includes('by you') && !text(plain).includes('by Unfold'))
  assert.equal(findAll(plain, byData('data-unfold-sub')).length, 2, 'both subscriptions still listed, each with Remove')
})

test('Send posts exactly {token} and never the draft text', async () => {
  const calls = recordFetch()
  const snap = fixtureView({ draft: true })
  const send = find(render(client.Dock, { snap, sessionId: SESSION }), byData('data-unfold-send'))
  send.props.onClick()
  await tick()
  assert.equal(calls.length, 1)
  const [call] = calls
  assert.equal(call.url, 'api/unfold-bridge/drafts/send?effect_id=e-1')
  assert.equal(call.method, 'POST')
  assert.equal(call.type, 'application/json')
  assert.deepEqual(Object.keys(JSON.parse(call.body)), ['token'], 'the body is {token} and nothing else')
  assert.deepEqual(JSON.parse(call.body), { token: TOKEN })
  assert.ok(!call.body.includes('Run the tests'), 'no draft text is sent')
})

test('Cancel posts exactly {token}', async () => {
  const calls = recordFetch()
  find(render(client.Dock, { snap: fixtureView({ draft: true }), sessionId: SESSION }), byData('data-unfold-cancel')).props.onClick()
  await tick()
  assert.deepEqual(calls.map(c => [c.url, c.method, c.body]), [['api/unfold-bridge/drafts/cancel?effect_id=e-1', 'POST', JSON.stringify({ token: TOKEN })]])
})

test('the shown draft is the held text byte for byte', () => {
  assert.equal(DRAFT_TEXT.length, 600)
  assert.ok(DRAFT_TEXT.split('\n').length > 10)
  for (const t of [en, pt]) {
    const shown = find(render(client.Dock, { snap: fixtureView({ draft: true }), sessionId: SESSION, t }), byData('data-unfold-draft-text'))
    assert.equal(shown.props.children.length, 1, 'one text node, unsplit')
    assert.equal(shown.props.children[0], DRAFT_TEXT)
    assert.equal(Buffer.compare(Buffer.from(shown.props.children[0]), Buffer.from(DRAFT_TEXT)), 0)
  }
  assert.ok(DRAFT_TEXT.includes('e\u0301') && DRAFT_TEXT.includes('\uFB01'), 'the fixture holds what NFC or NFKC would change')
  // The style keeps every space and line: no clamp, no ellipsis, no hidden overflow, on the text
  // or on anything around it.
  assert.match(client.STYLES.match(/\.ub-draft-text \{([^}]*)\}/)[1], /white-space: pre-wrap/)
  for (const selector of ['.ub-draft-text', '.ub-draft', '.ub-dock', '.ub-actions']) {
    for (const rule of client.STYLES.match(new RegExp(`(^|[\\s,])${selector.replace('.', '\\.')}[^{]*\\{[^}]*\\}`, 'gm')) ?? []) {
      assert.doesNotMatch(rule, /max-height|line-clamp|overflow: hidden|text-overflow|height:/, selector)
    }
  }
})

test("a draft shows only in its own Session's composer, and the board shows no draft text", () => {
  const snap = fixtureView({ draft: true })
  assert.equal(render(client.Dock, { snap, sessionId: OTHER }), null)
  const board = render(client.BoardPanel, { snap })
  assert.ok(!text(board).includes(DRAFT_TEXT.trim().slice(0, 20)))
  assert.equal(find(board, byData('data-unfold-send')), null, 'no Send outside the composer')
})

test('a replaced draft removes the old Send', () => {
  const first = fixtureView({ draft: true })
  const second = { ...first, drafts: [{ ...first.drafts[0], effect_id: 'e-2', token: 'test-token-second-not-a-secret', text: 'a newer draft', at: 4500 }] }
  const before = findAll(render(client.Dock, { snap: first, sessionId: SESSION }), byData('data-unfold-send')).map(node => node.props['data-unfold-send'])
  const after = findAll(render(client.Dock, { snap: second, sessionId: SESSION }), byData('data-unfold-send')).map(node => node.props['data-unfold-send'])
  assert.deepEqual(before, ['e-1'])
  assert.deepEqual(after, ['e-2'])
})

test('no rendered slot carries a draft token', () => {
  const snap = fixtureView({ draft: true, approval: true })
  const out = [
    html(render(client.RowAction, { snap, sessionId: SESSION })),
    html(render(client.BoardPanel, { snap })),
    html(render(client.Dock, { snap, sessionId: SESSION })),
    html(render(client.SettingsSection, { snap })),
  ].join('\n')
  assert.ok(out.includes('Run the tests again'))
  assert.ok(!out.includes(TOKEN))
})

test('the row toggle adds a subscription for its Session or removes its own one', async () => {
  const calls = recordFetch()
  const watched = fixtureView()
  render(client.RowAction, { snap: watched, sessionId: SESSION }).props.onClick()
  // The other Session is watched only through Unfold's workspace subscription: its toggle adds
  // its own subscription and leaves Unfold's alone.
  const other = render(client.RowAction, { snap: watched, sessionId: OTHER })
  assert.equal(other.props['aria-pressed'], 'false')
  assert.match(other.props.title, /through another subscription/)
  other.props.onClick()
  await tick()
  assert.deepEqual(calls.map(c => [c.method, c.url, c.body]), [
    ['DELETE', 'api/unfold-bridge/subscriptions?id=sub-row', '{}'],
    ['POST', 'api/unfold-bridge/subscriptions', JSON.stringify({ session_id: OTHER })],
  ])
})

test('Undo posts {} for that change', async () => {
  const calls = recordFetch()
  const settings = render(client.SettingsSection, { snap: fixtureView() })
  const undo = findAll(settings, byData('data-unfold-change')).find(node => node.props['data-unfold-change'] === 'ch-2')
  find(undo, node => node.type === 'button').props.onClick()
  await tick()
  assert.deepEqual(calls.map(c => [c.method, c.url, c.body]), [['POST', 'api/unfold-bridge/undo?change_id=ch-2', '{}']])
})

test("a refused tap carries the bridge's code, which the slot words in the person's language", async () => {
  assert.equal(pt('error.bad_token'), 'Este rascunho mudou. Leia o novo antes de enviar.')
  assert.equal(en('error.other', { code: 'http_500' }), 'That did not work (http_500).')
  recordFetch(403, { error: { code: 'bad_token' } })
  await assert.rejects(client.sendDraft({ effect_id: 'e-1', token: TOKEN }), error => error.code === 'bad_token')
})

test("advice shows only while that Session's approval is pending, with its label and the instructions mark", () => {
  const marked = fixtureView({ approval: true, marked: true })
  const dock = text(render(client.Dock, { snap: marked, sessionId: SESSION }))
  assert.ok(dock.includes("Unfold's read of this agent's messages"))
  assert.ok(dock.includes("This agent's text contains instructions"))
  assert.ok(dock.includes('The command deletes only the build folder'))
  const plain = text(render(client.Dock, { snap: fixtureView({ approval: true }), sessionId: SESSION }))
  assert.ok(!plain.includes('contains instructions'))
  // Decided: the approval left the pending list, so its advice goes even if the view still has it.
  const decided = { ...marked, approvals: [] }
  assert.equal(render(client.Dock, { snap: decided, sessionId: SESSION }), null)
  assert.equal(render(client.Dock, { snap: marked, sessionId: OTHER }), null)
})

test('the first run suggests the Session with the most recent turn', () => {
  const fresh = fixtureView({ subscriptions: false, log: false })
  assert.equal(client.suggestion(fresh).session_id, OTHER)
  const board = render(client.BoardPanel, { snap: fresh })
  assert.equal(find(board, byData('data-unfold-suggest')).props['data-unfold-suggest'], OTHER)
  assert.ok(text(board).includes('Start with writer · docs?'))
  assert.equal(client.suggestion(fixtureView()), null, 'nothing is suggested once something is watched')
})

test('the link state reads Unfold connected, not connected, or the bridge not there yet', () => {
  const link = snap => find(render(client.LinkState, { snap, t: pt }), byData('data-unfold-link')).props['data-unfold-link']
  assert.equal(link(fixtureView()), 'on')
  assert.equal(link(fixtureView({ connected: false })), 'off')
  assert.equal(link(null), 'down')
  assert.equal(text(render(client.LinkState, { snap: fixtureView({ connected: false }), t: pt })), 'Conexão com Unfold: inativaSeus cliques são entregues quando a conexão com Unfold voltar.')
})

test('the copy check passes over EN and pt-BR and catches a bad line', () => {
  assert.deepEqual(checkCopy(), [])
  assert.deepEqual(Object.keys(client.DICTIONARIES.en).sort(), Object.keys(client.DICTIONARIES['pt-BR']).sort())
  const dir = mkdtempSync(join(tmpdir(), 'copy-check-'))
  writeFileSync(join(dir, 'en.json'), JSON.stringify({ a: 'Unfold runs locally, offline' }))
  writeFileSync(join(dir, 'pt.json'), JSON.stringify({ a: 'O Unfold está pronto' }))
  const findings = checkCopy({ en: join(dir, 'en.json'), 'pt-BR': join(dir, 'pt.json') })
  assert.ok(findings.some(line => line.includes('local-only')))
  assert.ok(findings.some(line => line.includes('offline')))
  assert.ok(findings.some(line => line.includes('article before Unfold')))
  assert.ok(findings.some(line => line.includes('gendered word')))
  // the words the copy never uses: a click is a click, and Unfold's DSH is never a sandbox
  writeFileSync(join(dir, 'en.json'), JSON.stringify({ a: 'Tap Send', b: 'It runs in a sandbox', c: 'After two taps' }))
  writeFileSync(join(dir, 'pt.json'), JSON.stringify({ a: 'Toque em Enviar', b: 'quando você toca em Enviar', c: 'Roda numa sandbox' }))
  const words = checkCopy({ en: join(dir, 'en.json'), 'pt-BR': join(dir, 'pt.json') })
  for (const key of ['en a', 'en b', 'en c', 'pt-BR a', 'pt-BR b', 'pt-BR c']) assert.ok(words.some(line => line.startsWith(`${key}: `)), key)
})

test('the client holds no key', () => {
  const sources = readdirSync(new URL('../src/client/', import.meta.url), { recursive: true })
    .filter(name => /\.(js|json)$/.test(name))
    .map(name => readFileSync(new URL(`../src/client/${name}`, import.meta.url), 'utf8'))
  for (const source of [...sources, readFileSync(new URL('../client.js', import.meta.url), 'utf8')]) {
    // The bridge's key lives in the host half and its endpoint file; nothing here reads or sends one.
    assert.doesNotMatch(source, /authorization|bearer|x-api-key|endpoint-file|endpoint\.json/i)
  }
})

test('a draft with hidden characters is shown as held, with a warning, and Send stays off', async () => {
  const calls = recordFetch()
  const snap = fixtureView({ draft: true, hidden: true })
  const dock = render(client.Dock, { snap, sessionId: SESSION })
  const shown = find(dock, byData('data-unfold-draft-text'))
  assert.deepEqual(shown.props.children, [HIDDEN_TEXT], 'the held text, unchanged')
  const warning = find(dock, byData('data-unfold-hidden'))
  assert.equal(warning.props.className, 'ub-marked')
  const line = text(warning)
  assert.match(line, /^\d+ hidden characters: /)
  for (const name of ['U+200B', 'U+202E', 'U+202C', 'U+E0074']) assert.ok(line.includes(name), name)
  assert.ok(text(dock).includes("Send stays off while a draft holds characters you can't see."), 'the hold hint replaces the usual one')
  assert.ok(!text(dock).includes('Send passes exactly this text'))
  const send = find(dock, byData('data-unfold-send'))
  const cancel = find(dock, byData('data-unfold-cancel'))
  assert.equal(send.props.disabled, true)
  // A disabled button looks disabled: the primary style must not keep it looking live.
  assert.match(client.STYLES, /\.ub-button:disabled \{[^}]*opacity: 0\.5[^}]*cursor: not-allowed/)
  assert.equal(cancel.props.disabled, false)
  send.props.onClick()
  await tick()
  assert.equal(calls.length, 0, 'a click on the held Send sends nothing')
  cancel.props.onClick()
  await tick()
  assert.deepEqual(calls.map(c => c.url), ['api/unfold-bridge/drafts/cancel?effect_id=e-1'])
  // The text, its recipient line and Unfold's notes keep their own direction.
  for (const selector of ['.ub-draft-text', '.ub-note', '.ub-to']) assert.match(client.STYLES.match(new RegExp(`\\${selector} \\{([^}]*)\\}`))[1], /unicode-bidi: plaintext/, selector)
})

test('hidden characters in a recipient label or in advice are flagged too', () => {
  const snap = fixtureView({ draft: true, approval: true, hidden: true })
  snap.drafts[0] = { ...snap.drafts[0], text: 'plain', name: 'build\u200Fer' }
  const dock = render(client.Dock, { snap, sessionId: SESSION })
  const flags = findAll(dock, byData('data-unfold-hidden')).map(text)
  assert.equal(flags.length, 2, 'one in the advice, one on the draft')
  assert.ok(flags.some(line => line.startsWith('2 hidden characters: U+200B, U+202E')))
  assert.ok(flags.some(line => line === '1 hidden character: U+200F'))
  assert.equal(find(dock, byData('data-unfold-send')).props.disabled, true)
  assert.equal(pt('hidden.many', { count: 3, list: 'U+200B' }), '3 caracteres ocultos: U+200B')
})

test('emoji joiners and presentation selectors are not hidden characters', () => {
  assert.equal(client.hiddenIn(EMOJI_TEXT).length, 0)
  const snap = fixtureView({ draft: true })
  snap.drafts[0] = { ...snap.drafts[0], text: EMOJI_TEXT }
  const dock = render(client.Dock, { snap, sessionId: SESSION })
  assert.equal(find(dock, byData('data-unfold-hidden')), null)
  assert.equal(find(dock, byData('data-unfold-send')).props.disabled, false)
  // Outside an emoji sequence they are hidden.
  assert.deepEqual(Array.from(client.hiddenIn('a\u200Db x\uFE0F \u00AD\x07\x9B\tok\r\n'), client.codeName), ['U+200D', 'U+FE0F', 'U+00AD', 'U+0007', 'U+009B'])
})

test("a change Unfold made at the person's request says so", () => {
  const changes = client.changesOf(fixtureView())
  assert.deepEqual(changes.map(entry => [entry.change_id, entry.cause]), [['ch-3', 'Event'], ['ch-2', 'Person'], ['ch-1', 'Person']])
  assert.deepEqual(changes.map(client.changedBy), ['log.byUnfold', 'log.byUnfoldAsked', 'log.byYou'])
  assert.equal(pt('log.byUnfoldAsked'), 'Unfold, a seu pedido')
  assert.ok(client.describeChange(changes[1], fixtureView(), pt).startsWith('Unfold, a seu pedido: adicionou espaço de trabalho docs'))
  // The board's notices are Unfold's own changes only.
  assert.deepEqual(client.noticesOf(fixtureView()).map(entry => entry.change_id), ['ch-3', 'ch-2'])
})

test('advice shows only under its own call and Session', () => {
  const snap = fixtureView({ approval: true })
  snap.view.advice = [
    { call_id: 'call-1', session_id: OTHER, note: 'about another Session', marked: false, at: 3100 },
    { call_id: 'call-old', session_id: SESSION, note: 'about a decided call', marked: false, at: 3200 },
  ]
  assert.equal(render(client.Dock, { snap, sessionId: SESSION }), null)
  assert.ok(!text(render(client.BoardPanel, { snap })).includes('about'))
})

test('the newest draft for a Session wins, and the row toggle removes its own lone subscription', () => {
  const snap = fixtureView({ draft: true })
  snap.drafts = [{ ...snap.drafts[0], effect_id: 'e-new', at: 9000 }, { ...snap.drafts[0], effect_id: 'e-old', at: 1000 }]
  assert.equal(client.draftFor(snap, SESSION).effect_id, 'e-new')
  snap.view.subscriptions = [...snap.view.subscriptions, { id: 'sub-wide', match: { session_ids: [SESSION, OTHER], names: [], workspaces: [] }, kinds: ['turn_done'] }]
  assert.equal(client.rowSubscription(snap, SESSION).id, 'sub-row')
})

test('nothing renders a request: taps happen only on a click', async () => {
  let calls = 0
  client.fetch = async () => { calls += 1; return { ok: true, status: 200, json: async () => ({}) } }
  for (const t of [en, pt]) for (const connected of [true, false]) for (const draft of [true, false]) for (const approval of [true, false]) for (const subscriptions of [true, false]) {
    for (const snap of [fixtureView({ connected, draft, approval, subscriptions, log: subscriptions }), null]) {
      for (const [View, extra] of [[client.RowAction, { sessionId: SESSION }], [client.RowAction, { sessionId: OTHER }], [client.BoardPanel, {}], [client.BoardButton, { wide: true }], [client.Dock, { sessionId: SESSION }], [client.SettingsSection, {}]]) render(View, { snap, t, ...extra })
    }
  }
  await tick()
  assert.equal(calls, 0)
})

test('no console line carries a draft token', async () => {
  client.logs.length = 0
  recordFetch(502, { error: { code: 'send_failed' } })
  const dock = render(client.Dock, { snap: fixtureView({ draft: true }), sessionId: SESSION })
  find(dock, byData('data-unfold-send')).props.onClick()
  find(dock, byData('data-unfold-cancel')).props.onClick()
  await assert.rejects(client.sendDraft({ effect_id: 'e-1', token: TOKEN }))
  await tick()
  assert.ok(!client.logs.some(args => args.some(arg => String(arg).includes(TOKEN) || JSON.stringify(arg ?? null)?.includes(TOKEN))))
})

test("a 401 from DSH's admission reads as unauthorized", async () => {
  client.fetch = async () => ({ ok: false, status: 401, json: async () => { throw new Error('not json') } })
  await assert.rejects(client.undoChange('ch-1'), error => error.code === 'unauthorized')
})

test('the view stream reopens with a growing pause after DSH closes it', () => {
  const sources = []
  class FakeSource {
    constructor(url) { Object.assign(this, { url, readyState: 0, listeners: {}, closed: false }); sources.push(this) }
    addEventListener(name, fn) { this.listeners[name] = fn }
    close() { this.closed = true; this.readyState = 2 }
  }
  const timers = []
  const seen = []
  const stop = client.openView(snap => seen.push(snap), { EventSourceImpl: FakeSource, later: (fn, ms) => { timers.push({ fn, ms }); return timers.length }, cancel: () => {} })
  assert.equal(sources[0].url, 'api/unfold-bridge/view')
  // A dropped stream EventSource retries itself (CONNECTING): no timer of ours.
  sources[0].readyState = 0
  sources[0].listeners.error()
  assert.deepEqual([seen, timers.length], [[null], 0])
  // A non-200 answer closes it for good: reopened after 2 s, then 4 s, …
  sources[0].readyState = 2
  sources[0].listeners.error()
  assert.equal(timers[0].ms, 2000)
  timers[0].fn()
  sources[1].readyState = 2
  sources[1].listeners.error()
  assert.equal(timers[1].ms, 4000)
  timers[1].fn()
  sources[2].listeners.view({ data: JSON.stringify({ connected: true }) })
  assert.equal(seen.at(-1).connected, true)
  // A view resets the pause: the next close waits 2 s again, not 8 s.
  sources[2].readyState = 2
  sources[2].listeners.error()
  assert.equal(timers[2].ms, 2000)
  stop()
  assert.equal(sources[2].closed, true)
})

test('while nothing is known, settings and the board show the link state and no empty lists', () => {
  for (const snap of [null, fixtureView({ connected: false, view: false })]) {
    const settings = text(render(client.SettingsSection, { snap }))
    assert.ok(settings.includes('Unfold not connected'))
    assert.ok(!settings.includes('watches nothing') && !settings.includes('No changes yet'))
  }
  const board = text(render(client.BoardPanel, { snap: null }))
  assert.ok(board.includes('Unfold not connected') && !board.includes('No sessions yet'))
})

test('the board button names what waits, and the row names its status', () => {
  const button = find(render(client.BoardButton, { snap: fixtureView({ draft: true, approval: true }), wide: true }), byData('data-unfold-board-button'))
  assert.equal(button.props['aria-label'], 'Unfold, 2 waiting')
  assert.equal(find(render(client.BoardButton, { snap: fixtureView(), wide: true }), byData('data-unfold-board-button')).props['aria-label'], 'Unfold')
  assert.equal(render(client.RowAction, { snap: fixtureView(), sessionId: OTHER }).props['aria-label'], 'Watch with Unfold · Idle')
})

test("DSH's locale picks the language: Portuguese for any pt locale, else English", () => {
  assert.deepEqual(['pt-BR', 'pt', 'PT-pt', 'en', 'zh', undefined].map(client.languageOf), ['pt-BR', 'pt-BR', 'pt-BR', 'en', 'en', 'en'])
})

test('a malformed change log entry does not take the settings down', () => {
  const snap = fixtureView()
  snap.view.log_tail = [...snap.view.log_tail, { change_id: 'ch-bad', added: [{ match: { session_ids: 'not-a-list', names: [7] }, kinds: 'x' }], by: 'unfold' }]
  assert.ok(text(render(client.SettingsSection, { snap })).includes('Undo'))
})

test("the wish's hidden-character vectors: Allowed sends, Refused holds Send", () => {
  const dockFor = text => {
    const snap = fixtureView({ draft: true })
    snap.drafts[0] = { ...snap.drafts[0], text }
    return render(client.Dock, { snap, sessionId: SESSION })
  }
  for (const text of ALLOWED) {
    const dock = dockFor(text)
    assert.equal(client.hiddenIn(text).length, 0, JSON.stringify(text))
    assert.equal(find(dock, byData('data-unfold-hidden')), null, JSON.stringify(text))
    assert.equal(find(dock, byData('data-unfold-send')).props.disabled, false, JSON.stringify(text))
  }
  for (const text of REFUSED) {
    const dock = dockFor(text)
    assert.ok(client.hiddenIn(text).length > 0, JSON.stringify(text))
    assert.ok(find(dock, byData('data-unfold-hidden')), JSON.stringify(text))
    assert.equal(find(dock, byData('data-unfold-send')).props.disabled, true, JSON.stringify(text))
    assert.deepEqual(find(dock, byData('data-unfold-draft-text')).props.children, [text], 'shown as held')
  }
})

test('a watched Session carries an at-rest mark in its row, with nothing to click', () => {
  const snap = fixtureView()
  const own = render(client.WatchedMark, { snap, sessionId: SESSION })
  assert.equal(own.props.role, 'img')
  assert.equal(own.props['aria-label'], 'Unfold watches this session')
  assert.equal(own.props.onClick, undefined)
  assert.equal(findAll(own, node => node.type === 'button').length, 0)
  // Watched through Unfold's workspace subscription counts too.
  assert.equal(render(client.WatchedMark, { snap, sessionId: OTHER }).props['aria-label'], 'Unfold watches this session')
  assert.equal(render(client.WatchedMark, { snap: fixtureView({ subscriptions: false }), sessionId: SESSION }), null)
  assert.equal(render(client.WatchedMark, { snap: null, sessionId: SESSION }), null)
  assert.equal(pt('row.watchedMark'), 'Unfold acompanha esta sessão')
})

test('the client flags the Arabic letter mark and a joiner after a letter', () => {
  assert.deepEqual(Array.from(client.hiddenIn('a\u061Cb'), client.codeName), ['U+061C'])
  assert.deepEqual(Array.from(client.hiddenIn('a\u200D\u{1F44D}'), client.codeName), ['U+200D'])
})

test('Escape closes the board', () => {
  let closed = 0
  const board = render(client.BoardPanel, { snap: fixtureView(), onClose: () => { closed += 1 } })
  board.props.onKeyDown({ key: 'Enter' })
  assert.equal(closed, 0)
  board.props.onKeyDown({ key: 'Escape' })
  assert.equal(closed, 1)
})

// A browser window as the install offer sees it: its events, its display mode and its address.
// `prompts`: the browser has the install event (Chrome, Edge), whether or not it fired yet.
function fakeWindow({ hash = '', standalone = false, fullscreen = false, iosStandalone, prompts = true } = {}) {
  const listeners = {}
  return {
    ...(prompts ? { onbeforeinstallprompt: null } : {}),
    location: { hash },
    navigator: iosStandalone === undefined ? {} : { standalone: iosStandalone },
    matchMedia: query => ({ matches: (query === '(display-mode: standalone)' && standalone) || (query === '(display-mode: fullscreen)' && fullscreen) }),
    addEventListener: (type, fn) => { (listeners[type] ??= new Set()).add(fn) },
    removeEventListener: (type, fn) => { listeners[type]?.delete(fn) },
    fire: (type, event = {}) => { for (const fn of Array.from(listeners[type] ?? [])) fn(event) },
    count: type => listeners[type]?.size ?? 0,
  }
}
const promptEvent = () => {
  const seen = { prevented: 0, prompts: 0 }
  return { seen, preventDefault: () => { seen.prevented++ }, prompt: async () => { seen.prompts++; return { outcome: 'accepted' } } }
}
const installView = (install, t = en) => render(client.InstallAction, { offer: install.get(), onInstall: install.prompt, t, wide: true })

test('a beforeinstallprompt shows Install, and a click calls prompt() once', () => {
  const win = fakeWindow()
  const install = client.createInstall(win)
  install.watch()
  assert.equal(installView(install), null, 'nothing to offer before the event')
  const event = promptEvent()
  win.fire('beforeinstallprompt', event)
  assert.equal(event.seen.prevented, 1, 'the browser keeps its own bar to itself')
  const button = find(installView(install), byData('data-unfold-install-button'))
  assert.equal(text(button), 'Install as app')
  button.props.onClick()
  button.props.onClick()
  assert.equal(event.seen.prompts, 1, 'one prompt per event')
  assert.equal(installView(install), null, 'spent: the offer goes')
  // the browser may offer again later, after a dismissal
  const again = promptEvent()
  win.fire('beforeinstallprompt', again)
  find(installView(install), byData('data-unfold-install-button')).props.onClick()
  assert.equal(again.seen.prompts, 1)
})

test('#install opens the offer at load: Install when the browser can, else the Safari line', () => {
  const safari = client.createInstall(fakeWindow({ hash: '#install', prompts: false }))
  safari.watch()
  const panel = find(installView(safari), byData('data-unfold-install-panel'))
  assert.ok(panel, 'open from the address')
  assert.equal(panel.props.role, 'dialog')
  assert.equal(panel.props.tabIndex, -1, 'it takes the focus when it opens')
  assert.equal(text(find(panel, byData('data-unfold-install-safari'))), 'In Safari, choose File › Add to Dock.')
  assert.equal(find(panel, byData('data-unfold-install')), null, 'no Install button without the event')
  assert.ok(text(find(render(client.InstallAction, { offer: safari.get(), onInstall: safari.prompt, t: pt }), byData('data-unfold-install-safari'))).includes('Arquivo › Adicionar ao Dock'))

  const win = fakeWindow({ hash: '#install' })
  const chrome = client.createInstall(win)
  chrome.watch()
  const event = promptEvent()
  win.fire('beforeinstallprompt', event)
  const open = find(installView(chrome), byData('data-unfold-install-panel'))
  assert.equal(find(open, byData('data-unfold-install-safari')), null)
  find(open, byData('data-unfold-install')).props.onClick()
  assert.equal(event.seen.prompts, 1)

  assert.equal(installView(client.createInstall(fakeWindow({ hash: '#other', prompts: false }))), null, 'another fragment opens nothing')
})

test('Chrome or Edge opened with #install never shows the Safari line, and opens on the event', () => {
  const win = fakeWindow({ hash: '#install' })
  const install = client.createInstall(win)
  install.watch()
  // not installable yet, or installed already from another tab: nothing to say
  assert.equal(installView(install), null)
  const event = promptEvent()
  win.fire('beforeinstallprompt', event)
  const open = find(installView(install), byData('data-unfold-install-panel'))
  assert.ok(open, 'the address asked for it, so it opens when the browser can install')
  assert.equal(find(open, byData('data-unfold-install-safari')), null)
  find(open, byData('data-unfold-install')).props.onClick()
  assert.equal(event.seen.prompts, 1)
})

test('an installed app never shows the offer', () => {
  for (const options of [{ standalone: true }, { fullscreen: true }, { iosStandalone: true, prompts: false }]) {
    const win = fakeWindow({ hash: '#install', ...options })
    const install = client.createInstall(win)
    install.watch()
    win.fire('beforeinstallprompt', promptEvent())
    assert.equal(installView(install), null, JSON.stringify(options))
  }
  // installed from this tab: the tab stops offering
  const win = fakeWindow()
  const install = client.createInstall(win)
  install.watch()
  win.fire('beforeinstallprompt', promptEvent())
  win.fire('appinstalled')
  assert.equal(installView(install), null)
})

test('the install offer listens only while watched, and works with no window at all', () => {
  const win = fakeWindow()
  const install = client.createInstall(win)
  const stop = install.watch()
  install.watch()
  assert.equal(win.count('beforeinstallprompt'), 1, 'watching twice listens once')
  stop()
  assert.equal(win.count('beforeinstallprompt'), 0)
  assert.equal(win.count('appinstalled'), 0)
  const bare = client.createInstall(undefined)
  bare.watch()()
  assert.equal(installView(bare), null)
})
