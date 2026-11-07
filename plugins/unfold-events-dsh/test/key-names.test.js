// Secret key names in any case, with any prefix joined by _ or -, in the three forms a reply
// carries them (KEY=value, JSON, key: value), the value quoted with spaces: no word of the value
// reaches the event window's rows or change payloads, whole or across its private 16 KiB bound,
// and the browser's copy of the redaction hides it too. Words that only contain a key name are
// not keys. The value is built from words at run time. Fields like pass_rate are not secrets, and
// camelCase words with no key part are words.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadCatalog } from '../src/catalog.js'
import { EventWindow, redact, redactText } from '../src/window.js'
import { redact as clientRedact, redactText as clientRedactText, secretsIn as clientSecretsIn } from '../src/client/redact.js'

const WORDS = ['correct', 'horse', 'battery', 'staple']
const PHRASE = WORDS.join(' ')
const JOINED = WORDS.join('')
const KEYS = ['password', 'PASSWD', 'pass', 'Secret', 'client_secret', 'passphrase', 'TOKEN', 'api_key', 'private_key', 'db_password', 'DB_PASS', 'x-api-key', 'secret_key', 'SECRET_KEY', 'aws_secret_access_key', 'AWS_SECRET_ACCESS_KEY', 'accessToken', 'clientSecret', 'dbPassword', 'apiKey', 'privateKey', 'authToken', 'dbPass', 'userPass', 'adminPass', 'ClientSecret', 'AccessToken', 'DbPassword', 'DbPass',
  // Invisible characters inside the name, read as the name (#348).
  'pass\u200bword', 'pass\u0000word', 'db\u200bPassword', 'API\ufeff_KEY']
const SCHEMES = [`authorization: bearer ${JOINED}`, `AUTHORIZATION: BEARER ${JOINED}`, `Authorization: basic ${JOINED}`, `{"authorization": "bearer ${PHRASE}"}`]
// 80k characters of each input that could make a rule quadratic: a start after - or --, an open
// quote, a camelCase run, a YAML block, an open escaped JSON value, a prefix run, many line starts,
// many escaped JSON values, many YAML keys, many names mid-line, bearer runs, a word of upper-case names, long indents before a YAML key or
// any key, repeated names, keys and flags, open quotes of each kind, and backslash and escaped
// quote runs, and a word of upper-case names then `=` and a failing value, at 80k and 160k.
// A scheme in any case with no key name before it, before a token-shaped word (16+ characters of the
// token alphabet with a digit), built from words at run time.
const SHAPED = WORDS.join('9')
const ALONE = [`bearer ${SHAPED}`, `BEARER ${SHAPED}`, `basic ${SHAPED}`, `the header was bEaReR ${SHAPED}, then more`]
const NOT_TOKENS = ['bearer bonds', 'basic steps', 'the bearer of bad news', 'bearer v2 of the api']
// A token after a bare scheme whose digit comes last, and the same token in two deltas.
const LATE = `use bearer ${JOINED}9 now`
const LATE_SPLIT = [`use bearer ${WORDS.slice(0, 2).join('')}`, `${WORDS.slice(2).join('')}9 now`]
const FILL = 17_000 // past the private 16 KiB bound
// An escaped JSON value still open where the keep is cut, ending inside an escape: two backslashes
// (an escape begun one level down) or three (and one begun here). The next delta must still be read
// as part of the value, so the words after the escaped quote stay hidden.
const MID_ESCAPE = [2, 3].map((run) => [`${run} backslashes`, `{\\"password\\": \\"${'x'.repeat(FILL)}${'\\'.repeat(run)}`, `${run === 2 ? '\\"' : '"'}${PHRASE}\\"} done`])
// Spaces, tabs or both before a YAML block key: 80k of them, and 16k (the window's bound).
const INDENTED = [80_000, 16_000].flatMap((n) => [' ', '\t', ' \t'].flatMap((run) => ['password', 'accessToken'].map((key) => [`${n} ${JSON.stringify(run)} before ${key}: |`, `${run.repeat(n / run.length)}${key}: |\n`])))
const ADVERSARIAL = {
  'a- repeat': 'a-'.repeat(40_000),
  'open quote': `password: "${'x '.repeat(40_000)}`,
  'camelCase run': `a${'Bc'.repeat(40_000)}Token: x`,
  'camelCase words': 'aBc '.repeat(20_000),
  'YAML block': `password: |\n${'  a line of the block\n'.repeat(3_700)}`,
  'escaped JSON': `{\\"password\\": \\"${'\\\\'.repeat(40_000)}`,
  'prefix run': `${'a_'.repeat(40_000)}token: x`,
  'line starts': '\n '.repeat(40_000),
  'escaped JSON run': '\\"token\\": \\"x\\" '.repeat(5_000),
  'YAML keys': '\npassword: |'.repeat(6_700),
  'YAML block, LF, stray CRs': `password: |\n  ${'a line\r'.repeat(10_000)}`,
  'YAML block, CRLF, stray CRs and LFs': `password: |\r\n  ${'a line\ra line\n'.repeat(5_000)}`,
  'YAML block, CR, stray LFs': `password: |\r  ${'a line\n'.repeat(10_000)}`,
  'YAML block, LF, stray CRs at line starts': `password: |\n${'\r  a line\n'.repeat(7_000)}`,
  'YAML block, CR, stray LF blank lines': `password: |${'\r\n'.repeat(40_000)}`,
  'names mid-line': 'a password: '.repeat(6_700),
  'an explicit key line of names, then its colon line': `? ${'a password '.repeat(6_700)}\n:\n`,
  'explicit key lines': '? password\n:\n'.repeat(8_000),
  'camelCase names mid-line': 'x dbPassword: '.repeat(5_700),
  'bearer runs': 'bearer x '.repeat(9_000),
  'bearer long run': `bearer ${'a'.repeat(80_000)}`,
  'upper names in one word': `${'TOKEN'.repeat(16_000)} =`,
  'upper names then =': `${'TOKEN'.repeat(16_000)}=`,
  'upper names then = and a space': `${'TOKEN'.repeat(16_000)}= x`,
  'upper names then = and an open quote': `${'TOKEN'.repeat(16_000)}="`,
  'upper names then = and no quote': `${'TOKEN'.repeat(16_000)}=x`,
  'upper names then =, 160k': `${'TOKEN'.repeat(32_000)}=`,
  'upper names then = and a space, 160k': `${'TOKEN'.repeat(32_000)}= x`,
  'upper names then = and an open quote, 160k': `${'TOKEN'.repeat(32_000)}="`,
  'upper names then = and no quote, 160k': `${'TOKEN'.repeat(32_000)}=x`,
  'leading spaces, then a key': `${' '.repeat(80_000)}password: x`,
  'leading tabs, then KEY=': `${'\t'.repeat(80_000)}API_TOKEN="x`,
  'leading mixed runs, then a camelCase key': `${' \t'.repeat(40_000)}accessToken: x`,
  'repeated names': 'password '.repeat(8_900),
  'repeated camelCase keys': 'accessToken: '.repeat(6_200),
  'repeated PascalCase keys': 'ClientSecret='.repeat(6_200),
  'repeated flags': '--token='.repeat(10_000),
  'repeated KEY=': 'API_TOKEN='.repeat(8_000),
  'open single quote': `password: '${'x '.repeat(40_000)}`,
  'open backtick': `password: \`${'x '.repeat(40_000)}`,
  'quote runs': `${'"'.repeat(27_000)}${"'".repeat(27_000)}${'`'.repeat(26_000)}`,
  'backslash run': '\\'.repeat(80_000),
  'backslashes in an open value': `password: "${'\\'.repeat(80_000)}`,
  'escaped quote pairs': '\\"'.repeat(40_000),
  'escaped quote pairs in an open value': `password: "${'\\"'.repeat(40_000)}`,
  'escaped quote pairs in an escaped value': `{\\"password\\": \\"${'\\"'.repeat(40_000)}`,
  ...Object.fromEntries(INDENTED),
  // Invisible characters, raw and escaped, in and around names, and anchors after names (#348).
  'invisibles inside a word': 'a\u200b'.repeat(40_000),
  'invisibles before colons': '\u200b:'.repeat(40_000),
  'a BOM run': '\ufeff'.repeat(80_000),
  'a name, a BOM run, no separator': `pass\u200bword${'\ufeff'.repeat(80_000)}`,
  'a name, then spaces': `pass\u200bword${' '.repeat(80_000)}`,
  'escaped invisibles inside a word': 'a\\u0000'.repeat(13_000),
  'escaped invisibles after backslashes': '\\\\u200b'.repeat(11_000),
  'names with invisibles mid-line': 'a pass\u200bword: '.repeat(4_400),
  'anchors after a name': 'password: & '.repeat(6_700),
  'tags after a name, no value': 'password: !!str'.repeat(5_300),
  // Key lines whose value comes on a later line (#396, #410): each read once, forward from its line.
  'key lines with comments': 'password: &a # c\n'.repeat(5_000),
  'bare key lines': 'password:\n'.repeat(8_000),
  'a key line, a comment run, then a child': `password:\n${'# c\n'.repeat(20_000)}  user: x`,
  'a key line, an anchor run, then a sibling': `password:\n${'  &a\n'.repeat(16_000)}next: x`,
  'comment lines naming keys': '# password: &a\n'.repeat(6_000),
  'a key line, then a value with a space run': `password:\n  x${' '.repeat(80_000)}#`,
  'a key line, then a long word run': `password:\n  ${'a '.repeat(40_000)}`,
  'key lines ending at a lone CR': 'password: &a\r'.repeat(8_000),
  'a key line, then a CRLF blank run': `password:${'\r\n'.repeat(40_000)}x`,
  'parents and children': 'secret: &d\n  user: x\n'.repeat(4_000),
  'open quotes under key lines': 'password:\n  "x\n'.repeat(5_000),
}
const YAML_LINES = Array.from({ length: 400 }, (_, i) => `    ${PHRASE} ${i}\n`) // more than either keep
// A bare value is one unbroken run (only its first word would be hidden), so it never takes the
// spaced value the 16 KiB row sends.
const FORMS = [
  ['KEY=value', (k, v) => `${k}="${v}"`], ['JSON', (k, v) => `{"${k}": "${v}"}`], ['key: value', (k, v) => `${k}: '${v}'`],
  ['key: `value`', (k, v) => `${k}: \`${v}\``], ['key=`value`', (k, v) => `${k}=\`${v}\``],
  ['--key=value', (k) => `tool --${k}=${JOINED}`, 'bare'], ['--key="value"', (k, v) => `tool --${k}="${v}"`],
  ['key: value, bare', (k) => `${k}: ${JOINED}`, 'bare'],
  ['escaped JSON', (k, v) => `{\\"${k}\\": \\"${v}\\"}`],
  // A YAML block with LF, CRLF or lone CR line ends.
  ...[['', '\n'], [', CRLF', '\r\n'], [', CR', '\r']].flatMap(([eol, n]) => [
    [`YAML >${eol}`, (k, v) => `${n}${k}: >${n}  ${v}${n}  ${PHRASE}${n}next: shown`],
    [`YAML |${eol}`, (k, v) => `${n}  ${k}: |-${n}    ${v}${n}${n}    ${PHRASE}${n}  next: shown`],
  ]),
  // A stray line end the key line does not use, inside the block: part of the value (#346).
  ...[['LF', '\n', 'CR', '\r'], ['CRLF', '\r\n', 'CR', '\r'], ['CRLF', '\r\n', 'LF', '\n'], ['CR', '\r', 'LF', '\n']].flatMap(([eol, n, stray, s]) => [
    [`YAML |, ${eol}, a stray ${stray} mid-line`, (k, v) => `${n}${k}: |${n}  ${v}${s}${PHRASE}${n}next: shown`],
    [`YAML >, ${eol}, a stray ${stray} at a line start`, (k, v) => `${n}${k}: >${n}  ${v}${n}${s}  ${PHRASE}${n}next: shown`],
    [`YAML |-, ${eol}, a stray ${stray} on a blank line`, (k, v) => `${n}  ${k}: |-${n}    ${v}${n}${s}${n}    ${PHRASE}${n}  next: shown`],
  ]),
  ['key = value', (k) => `${k} = ${JOINED}`, 'bare'], ['key : "value"', (k, v) => `${k} : "${v}"`], ['{"key" : "value"}', (k, v) => `{"${k}" : "${v}"}`],
  // YAML anchors and tags before the value (#348).
  ['key: &a !!str "value"', (k, v) => `${k}: &a !!str "${v}"`], ['key: !!str value, bare', (k) => `${k}: !!str ${JOINED}`, 'bare'],
]
const CASES = KEYS.flatMap((k) => FORMS.map(([form, text, bare]) => [`${k} (${form})`, (v = PHRASE) => text(k, v), bare]))
const leaked = (text) => [...new Set(text.match(/[a-z]{3,}/gi) ?? [])].filter((t) => WORDS.some((w) => w.includes(t.toLowerCase()) || t.toLowerCase().includes(w)))
// A sample uses CPU time, not wall clock: CI runs this package's test files side by side, and a
// wall-clock sample includes the time this process waits for a CPU. The best of five runs drops one
// that caught a background compile or a collection; noise can add CPU time, never remove it, so a
// rule that is slow in every run stays slow in every sample.
// The text paths keep the absolute ceiling: 100 ms sits 7.9x above the slowest healthy input here
// (12.6 ms of CPU measured, Node 24.13.1), and a machine 4x slower lands at 50 ms.
const FAST_MS = 100
const FAST_TRIES = 5
const sample = (fn) => {
  const t0 = process.cpuUsage()
  fn()
  const used = process.cpuUsage(t0)
  return (used.user + used.system) / 1000
}
const timed = (fn, ceiling = FAST_MS) => {
  let best = Infinity
  for (let i = 0; i < FAST_TRIES && best >= ceiling; i++) best = Math.min(best, sample(fn))
  return best
}
// The reference sets a bound, so it always takes all five samples.
const fixedWork = (fn) => Math.min(...Array.from({ length: FAST_TRIES }, () => sample(fn)))

// The reference is a fixed amount of the same work: redactText over one 80k text of the fuzz's own
// pieces, built once from a fixed seed. It runs the same code path over a fixed N pieces, never the
// adversarial texts, so its cost is linear in that input (40k, 80k and 160k characters measured
// 115, 112 and 110 ns per character). A slower machine or a slower clock raises the reference and
// the window's bound together: it is not a time constant.
const REFERENCE_PIECES = [
  ' ', '\t', ' \t', '\n', '\n  ', 'password', 'db_password', 'DB_PASS', 'accessToken',
  'ClientSecret', 'DbPass', 'API_TOKEN', '--token', '-pass', 'token', '=', ': ', ' = ', '":',
  '\\":', ': |\n', ': >-\n', '"', "'", '`', '\\', '\\\\', '\\"', '\\"\\"', 'bearer ',
  'basic ', 'Bearer ', 'x9', 'a', 'word ', '{"', '{\\"', '_', '-',
]
const REFERENCE_TEXT = (() => {
  let state = 0x531 >>> 0
  const rand = () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = Math.imul(state ^ (state >>> 15), state | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296
  }
  const parts = []
  for (let n = 0; n < 80_000; ) {
    const piece = REFERENCE_PIECES[Math.floor(rand() * REFERENCE_PIECES.length)]
    parts.push(piece)
    n += piece.length
  }
  return parts.join('')
})()

// The window runs redactText over each delta and puts its rows and change payloads together on top,
// so it costs a multiple of the reference: 1.29x measured here (worst of this adversarial set, Node
// 24.13.1). The window reads only its keep, so a 160k input costs no more than a 16k one.
// WINDOW_MARGIN matches the fuzz window's 8, the runner-driven constant: this path is the same
// window, the fuzz window's worst CI reading is 6.32x (#513 head ee8595c8), and this path's own
// measured 1.29x leaves 6.2x of headroom. The absolute backstop (400 ms, as in the fuzz guards)
// caps how far a slow reference may widen the bound; a quadratic rule at 80k characters is 6.4e9
// steps, seconds of CPU, far past the bound or the backstop. The text paths keep the absolute
// ceiling: they are the plain rule pass over fixed-size inputs, with no per-path constant to scale.
const WINDOW_MARGIN = 8
const BACKSTOP_MS = 400

const catalog = loadCatalog()
let seq = 0
function rig() {
  const w = new EventWindow(catalog, { limit: 20 })
  const seen = []
  w.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  const push = (text) => w.push({ v: 2, id: `g:${++seq}`, producer: 'g', seq, t_ns: seq, source: 'guest', kind: 'agent_delta', response_id: 'rK', text })
  return { push, served: () => [...seen, JSON.stringify(w.rows)].join('\n') }
}

test('every key form in one reply: no word of its value is in a row or a change', () => {
  for (const [what, text] of CASES) {
    const r = rig()
    r.push(`the config has ${text()} and goes on`)
    assert.deepEqual(leaked(r.served()), [], what)
  }
})

test('every key form whose quoted value crosses the private 16 KiB bound: no word of it is shown', () => {
  for (const [what, text] of CASES.filter(([, , bare]) => !bare)) {
    const r = rig()
    r.push('ordinary start ')
    r.push(text(`${' '.repeat(17000)}${PHRASE}`))
    r.push(' and the reply goes on')
    assert.deepEqual(leaked(r.served()), [], what)
  }
})

test('the browser copy and the window agree, and hide every key form', () => {
  for (const [what, text] of CASES) {
    assert.deepEqual(leaked(clientRedactText(text())), [], what)
    assert.equal(clientRedactText(text()), redactText(text()), what)
  }
})

test('words that only contain a key name are not keys', () => {
  const prose = 'bypass=open compass: north passenger: aboard tokenizer: fast a-bypass=1 -bypass=1 --bypass=open secret_sauce: butter basic steps first bearer bonds accessLevel: admin clientName: acme passRate: 0.9 maxTokens: 512 tokenCount: 12 primaryKey: id sortKey: name keyboardLayout: us passengerCount: 4 tokenizerName: bpe compassHeading: north my firstPass at it boardingPass alone AccessLevel: admin MaxTokens: 512 PassengerCount: 4 PassRate: 0.9 my FirstPass at it the bearer of bad news'
  assert.equal(redactText(prose), prose)
  assert.equal(clientRedactText(prose), prose)
})

test('a scheme after a key name is hidden in any case, in the window and the browser copy', () => {
  for (const text of SCHEMES) {
    const r = rig()
    r.push(`the config has ${text} and goes on`)
    assert.deepEqual(leaked(r.served()), [], text)
    assert.deepEqual(leaked(clientRedactText(text)), [], text)
    if (!text.startsWith('{')) assert.match(redactText(text), /(bearer|BEARER|basic) \[redacted\]/, text)
  }
})

test('80k characters of adversarial input stay fast, in the window and the browser copy', () => {
  const inputs = Object.entries(ADVERSARIAL)
  // Warm-up, not measured: a cold process pays to compile the rules' regular expressions and climb
  // V8's tiers once, and that setup is not an input's cost. The same inputs then run measured.
  for (const [, text] of inputs) {
    redactText(text)
    clientRedactText(text)
    clientSecretsIn(text)
    rig().push(text)
  }
  const referenceMs = fixedWork(() => redactText(REFERENCE_TEXT))
  // A zero sample would make the bound zero and pass a window that measured nothing.
  assert.ok(referenceMs > 0, `reference sample is ${referenceMs} ms of CPU`)
  const bound = Math.min(WINDOW_MARGIN * referenceMs, BACKSTOP_MS)
  const margin = `${WINDOW_MARGIN} x reference ${referenceMs.toFixed(1)} ms`
  const textPaths = [
    ['the window rules', redactText],
    ['the browser copy', clientRedactText],
    ["the browser copy's values", clientSecretsIn],
  ]
  for (const [what, text] of inputs) {
    for (const [path, run] of textPaths) {
      const ms = timed(() => run(text))
      const detail = `${what}: ${path}: ${ms.toFixed(1)} ms of CPU, ceiling ${FAST_MS} ms`
      assert.ok(ms < FAST_MS, detail)
    }
    const windowMs = timed(() => rig().push(text), bound)
    const detail = `${what}: the window: ${windowMs.toFixed(1)} ms of CPU, ` +
      `bound ${bound.toFixed(1)} ms (${margin}; backstop ${BACKSTOP_MS} ms)`
    assert.ok(windowMs <= bound, detail)
  }
})

test('fields named for a pass count are kept; fields named for a pass are hidden', () => {
  for (const redactOf of [redact, clientRedact]) {
    assert.deepEqual(redactOf({ pass_rate: 0.9, pass_count: 3 }), { pass_rate: 0.9, pass_count: 3 })
    assert.deepEqual(redactOf({ pass: 'x1', db_pass: 'x2' }), { pass: '[redacted]', db_pass: '[redacted]' })
  }
})

test('a YAML block longer than the 16 KiB bound, streamed a line at a time: no word of it is shown', () => {
  for (const [eol, n] of [['LF', '\n'], ['CRLF', '\r\n'], ['CR', '\r']]) {
    for (const key of ['password', 'dbPassword']) {
      const r = rig()
      r.push(`config:${n}  ${key}: |${n}`)
      for (const line of [...YAML_LINES, ...YAML_LINES]) r.push(line.replace(/\n$/, n))
      assert.deepEqual(leaked(r.served()), [], `${key}, ${eol}`)
      r.push('next: shown')
      assert.match(r.served(), /next: shown/, `${key}, ${eol}`)
    }
  }
})

// A stray line end the key line does not use is part of the value, also when a delta ends on one:
// the bound keeps an indented block line, so the unindented rest of that line stays hidden (#346).
test('a YAML block streamed past the 16 KiB bound with stray line ends: no word of it is shown', () => {
  for (const [eol, n, s] of [['LF', '\n', '\r'], ['CRLF', '\r\n', '\r'], ['CRLF', '\r\n', '\n'], ['CR', '\r', '\n']]) {
    for (const key of ['password', 'dbPassword']) {
      const what = `${key}, ${eol} key, a stray ${JSON.stringify(s)}`
      const r = rig()
      r.push(`config:${n}  ${key}: |${n}`)
      for (const line of [...YAML_LINES, ...YAML_LINES]) r.push(line.replace(/\n$/, s))
      r.push(`${PHRASE}${n}`)
      assert.deepEqual(leaked(r.served()), [], what)
      r.push('next: shown')
      assert.match(r.served(), /next: shown/, what)
    }
  }
})

test('a scheme in any case with no key name hides a token-shaped word, in the window and the browser copy', () => {
  for (const text of ALONE) {
    const r = rig()
    r.push(text)
    assert.deepEqual(leaked(r.served()), [], text)
    assert.deepEqual(leaked(clientRedactText(text)), [], text)
  }
  for (const text of NOT_TOKENS) {
    assert.equal(redactText(`${text}, then more`), `${text}, then more`)
    assert.equal(clientRedactText(`${text}, then more`), `${text}, then more`)
  }
})

test('a bound inside an escaped value that ends mid-escape: the rest of the value stays hidden', () => {
  for (const [what, head, rest] of MID_ESCAPE) {
    const r = rig()
    r.push(head)
    r.push(rest)
    assert.deepEqual(leaked(r.served()), [], what)
    assert.match(r.served(), /done/, what)
  }
})

test('a token after a bare scheme, streamed a character at a time: no prefix of it is shown', () => {
  const r = rig()
  for (const ch of LATE) r.push(ch)
  assert.deepEqual(leaked(r.served()), [], LATE)
  const split = rig()
  for (const text of LATE_SPLIT) split.push(text)
  assert.deepEqual(leaked(split.served()), [], LATE_SPLIT.join(' + '))
  assert.deepEqual(leaked(clientRedactText(LATE_SPLIT[0])), [], LATE_SPLIT[0])
  for (const text of NOT_TOKENS) {
    const p = rig()
    for (const ch of `${text}, then more`) p.push(ch)
    assert.ok(p.served().includes(`${text}, then more`), text)
  }
})

test('one push of an 80k unbroken run past the 16 KiB bound, then a separator and whitespace, stays fast', () => {
  const ms = timed(() => rig().push(`${'a'.repeat(80_000)}: = \t`))
  assert.ok(ms < FAST_MS, `${ms.toFixed(1)} ms of CPU, ceiling ${FAST_MS} ms`)
})
