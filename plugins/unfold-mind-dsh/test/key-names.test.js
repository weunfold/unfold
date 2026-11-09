// Secret key names in any case, with any prefix joined by _ or -, in the three forms a reply
// carries them (KEY=value, JSON, key: value), the value quoted with spaces: no word of the value
// reaches a Mind row, snapshot, audit record or change payload, inside the 4096-char keep or with
// the cut sweeping through it. Words that only contain a key name are not keys. The value is
// built from words at run time. Fields like pass_rate are not secrets, and camelCase words with
// no key part are words.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadDefinitions } from '../src/definitions.js'
import { MindStream } from '../src/mind.js'
import { redact, redactText, secretsIn } from '../src/redact.js'
import { guardCases } from '../../../scripts/redact-fuzz-guard.mjs'

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
const FILL = 5000 // past the 4096-character keep
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
  'YAML block, CRLF': `password: |\r\n${'  a line of the block\r\n'.repeat(3_400)}`,
  'YAML block, CR': `password: |\r${'  a line of the block\r'.repeat(3_700)}`,
  'YAML keys, CRLF': '\r\npassword: |'.repeat(6_200),
  'YAML keys, CR': '\rpassword: |'.repeat(6_700),
  'blank lines, CRLF': `password: |${'\r\n'.repeat(40_000)}`,
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
const FORMS = [
  ['KEY=value', (k) => `${k}="${PHRASE}"`], ['JSON', (k) => `{"${k}": "${PHRASE}"}`], ['key: value', (k) => `${k}: '${PHRASE}'`],
  ['key: `value`', (k) => `${k}: \`${PHRASE}\``], ['key=`value`', (k) => `${k}=\`${PHRASE}\``],
  ['--key=value', (k) => `tool --${k}=${JOINED}`], ['--key="value"', (k) => `tool --${k}="${PHRASE}"`],
  ['key: value, bare', (k) => `${k}: ${JOINED}`],
  ['escaped JSON', (k) => `{\\"${k}\\": \\"${PHRASE}\\"}`],
  // A YAML block with LF, CRLF or lone CR line ends.
  ...[['', '\n'], [', CRLF', '\r\n'], [', CR', '\r']].flatMap(([eol, n]) => [
    [`YAML >${eol}`, (k) => `${n}${k}: >${n}  ${PHRASE}${n}  ${PHRASE}${n}next: shown`],
    [`YAML |${eol}`, (k) => `${n}  ${k}: |-${n}    ${PHRASE}${n}${n}    ${PHRASE}${n}  next: shown`],
  ]),
  // A stray line end the key line does not use, inside the block: part of the value (#346).
  ...[['LF', '\n', 'CR', '\r'], ['CRLF', '\r\n', 'CR', '\r'], ['CRLF', '\r\n', 'LF', '\n'], ['CR', '\r', 'LF', '\n']].flatMap(([eol, n, stray, s]) => [
    [`YAML |, ${eol}, a stray ${stray} mid-line`, (k) => `${n}${k}: |${n}  ${PHRASE}${s}${PHRASE}${n}next: shown`],
    [`YAML >, ${eol}, a stray ${stray} at a line start`, (k) => `${n}${k}: >${n}  ${PHRASE}${n}${s}  ${PHRASE}${n}next: shown`],
    [`YAML |-, ${eol}, a stray ${stray} on a blank line`, (k) => `${n}  ${k}: |-${n}    ${PHRASE}${n}${s}${n}    ${PHRASE}${n}  next: shown`],
  ]),
  ['key = value', (k) => `${k} = ${JOINED}`], ['key : "value"', (k) => `${k} : "${PHRASE}"`], ['{"key" : "value"}', (k) => `{"${k}" : "${PHRASE}"}`],
  // YAML anchors and tags before the value (#348).
  ['key: &a !!str "value"', (k) => `${k}: &a !!str "${PHRASE}"`], ['key: !!str value, bare', (k) => `${k}: !!str ${JOINED}`],
]
const CASES = KEYS.flatMap((k) => FORMS.map(([form, text]) => [`${k} (${form})`, text(k)]))
const leaked = (text) => [...new Set(text.match(/[a-z]{3,}/gi) ?? [])].filter((t) => WORDS.some((w) => w.includes(t.toLowerCase()) || t.toLowerCase().includes(w)))
// The stream includes row, sentence and audit work beyond the reference redaction pass.
// The handover timing row measured 4.0-4.3x on healthy runners; 6x leaves about 40% headroom
// above that range. Doubling the 4.3x workload still exceeds the bound (8.6x), and the shared
// 400 ms absolute backstop and text-path ceilings remain unchanged.
const STREAM_MARGIN = 6

function journal() {
  const mind = new MindStream({ now: () => 1 })
  mind.life = 'lifeZ'
  const seen = []
  mind.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  let seq = 0
  const send = (text) => mind.agentd({ v: 2, id: `g:${++seq}`, producer: 'g', seq, t_ns: seq, source: 'guest', kind: 'agent_delta', response_id: 'rK', text }, { via: 'agentd journal', file: '/j', offset: seq })
  mind.agentd({ v: 2, id: 'g:0', producer: 'g', seq: 0, t_ns: 0, source: 'guest', kind: 'response_started', response_id: 'rK' }, { via: 'agentd journal', file: '/j', offset: 0 })
  const served = () => {
    const defs = loadDefinitions()
    return [JSON.stringify(mind.snapshot()), ...mind.rows.map((x) => JSON.stringify(mind.detail(x.id, defs))), ...seen].join('\n')
  }
  return { send, served }
}

test('every key form inside the keep: no word of its value is served', () => {
  for (const [what, text] of CASES) {
    const r = journal()
    r.send(`the config has ${text} and goes on`)
    assert.deepEqual(leaked(r.served()), [], what)
  }
})

test('every key form the 4096 cut sweeps through: no word of its value is served', () => {
  for (const [what, text] of CASES) {
    for (const lead of [0, 2]) {
      const r = journal()
      r.send(`${text} `)
      r.send('w'.repeat(4096 - text.length - 8 + lead))
      for (let i = 0; i < text.length + 16; i++) r.send('z')
      assert.deepEqual(leaked(r.served()), [], `${what}, lead ${lead}`)
    }
  }
})

test('words that only contain a key name are not keys', () => {
  const prose = 'bypass=open compass: north passenger: aboard tokenizer: fast a-bypass=1 -bypass=1 --bypass=open secret_sauce: butter basic steps first bearer bonds accessLevel: admin clientName: acme passRate: 0.9 maxTokens: 512 tokenCount: 12 primaryKey: id sortKey: name keyboardLayout: us passengerCount: 4 tokenizerName: bpe compassHeading: north my firstPass at it boardingPass alone AccessLevel: admin MaxTokens: 512 PassengerCount: 4 PassRate: 0.9 my FirstPass at it the bearer of bad news'
  assert.equal(redactText(prose), prose)
})

test('a scheme after a key name is hidden in any case, and the scheme word stays', () => {
  for (const text of SCHEMES) {
    const r = journal()
    r.send(`the config has ${text} and goes on`)
    assert.deepEqual(leaked(r.served()), [], text)
    if (!text.startsWith('{')) assert.match(redactText(text), /(bearer|BEARER|basic) \[redacted\]/, text)
  }
})

test('80k characters of adversarial input stay fast, in the rules and in the stream', () => {
  guardCases({
    cases: Object.entries(ADVERSARIAL),
    textPaths: [['redactText', redactText], ['secretsIn', secretsIn]],
    scaledPath: ['the stream', (text) => journal().send(text), STREAM_MARGIN],
    reference: redactText,
    onDiagnostic: console.info,
  })
})

test('fields named for a pass count are kept; fields named for a pass are hidden', () => {
  assert.deepEqual(redact({ pass_rate: 0.9, pass_count: 3 }), { pass_rate: 0.9, pass_count: 3 })
  assert.deepEqual(redact({ pass: 'x1', db_pass: 'x2' }), { pass: '[redacted]', db_pass: '[redacted]' })
})

test('a YAML block longer than the keep, streamed a line at a time: no word of it is served', () => {
  for (const [eol, n] of [['LF', '\n'], ['CRLF', '\r\n'], ['CR', '\r']]) {
    for (const key of ['password', 'dbPassword']) {
      const r = journal()
      r.send(`config:${n}  ${key}: |${n}`)
      for (const line of YAML_LINES) r.send(line.replace(/\n$/, n))
      assert.deepEqual(leaked(r.served()), [], `${key}, ${eol}`)
      r.send('next: shown')
      assert.match(r.served(), /next: shown/, `${key}, ${eol}`)
    }
  }
})

// A stray line end the key line does not use is part of the value, also when a delta ends on one:
// the cut keeps an indented block line, so the unindented rest of that line stays hidden (#346).
test('a YAML block streamed past the keep with stray line ends: no word of it is served', () => {
  for (const [eol, n, s] of [['LF', '\n', '\r'], ['CRLF', '\r\n', '\r'], ['CRLF', '\r\n', '\n'], ['CR', '\r', '\n']]) {
    for (const key of ['password', 'dbPassword']) {
      const what = `${key}, ${eol} key, a stray ${JSON.stringify(s)}`
      const r = journal()
      r.send(`config:${n}  ${key}: |${n}`)
      for (const line of YAML_LINES) r.send(line.replace(/\n$/, s))
      r.send(`${PHRASE}${n}`)
      assert.deepEqual(leaked(r.served()), [], what)
      r.send('next: shown')
      assert.match(r.served(), /next: shown/, what)
    }
  }
})

// A CRLF stream can split a line end between deltas: the \r ends one, the \n starts the next.
test('a YAML block streamed with its CRLF line ends split across deltas: no word of it is served', () => {
  for (const key of ['password', 'dbPassword']) {
    const r = journal()
    r.send(`config:\r\n  ${key}: |\r`)
    for (const line of YAML_LINES) r.send(`\n${line.replace(/\n$/, '\r')}`)
    assert.deepEqual(leaked(r.served()), [], key)
    r.send('\nnext: shown')
    assert.match(r.served(), /next: shown/, key)
  }
})

test('a scheme in any case with no key name hides a token-shaped word, in the keep and across the cut', () => {
  for (const text of ALONE) {
    const r = journal()
    r.send(text)
    assert.deepEqual(leaked(r.served()), [], text)
    for (const lead of [0, 2]) {
      const c = journal()
      c.send(`${text} `)
      c.send('w'.repeat(4096 - text.length - 8 + lead))
      for (let i = 0; i < text.length + 16; i++) c.send('z')
      assert.deepEqual(leaked(c.served()), [], `${text}, lead ${lead}`)
    }
  }
  for (const text of NOT_TOKENS) assert.equal(redactText(`${text}, then more`), `${text}, then more`)
})

test('a cut inside an escaped value that ends mid-escape: the rest of the value stays hidden', () => {
  for (const [what, head, rest] of MID_ESCAPE) {
    const r = journal()
    r.send(head)
    r.send(rest)
    assert.deepEqual(leaked(r.served()), [], what)
    assert.match(r.served(), /done/, what)
  }
})

test('a token after a bare scheme, streamed a character at a time: no prefix of it is served', () => {
  const r = journal()
  for (const ch of LATE) r.send(ch)
  assert.deepEqual(leaked(r.served()), [], LATE)
  const split = journal()
  for (const text of LATE_SPLIT) split.send(text)
  assert.deepEqual(leaked(split.served()), [], LATE_SPLIT.join(' + '))
  for (const text of NOT_TOKENS) {
    const p = journal()
    for (const ch of `${text}, then more`) p.send(ch)
    assert.ok(p.served().includes(`${text}, then more`), text)
  }
})
