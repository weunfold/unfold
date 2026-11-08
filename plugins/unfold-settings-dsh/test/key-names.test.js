// Secret key names in any case, with any prefix joined by _ or -, in the three forms a served
// value carries them (KEY=value, JSON, key: value), the value quoted with spaces: no word of the
// value is left in a served string, at any depth. Words that only contain a key name are not keys.
// The value is built from words at run time. Fields like pass_rate are not secrets, and camelCase
// words with no key part are words.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { redact, redactText } from '../src/redact.js'

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
const FORMS = [
  ['KEY=value', (k) => `${k}="${PHRASE}"`], ['JSON', (k) => `{"${k}": "${PHRASE}"}`], ['key: value', (k) => `${k}: '${PHRASE}'`],
  ['key: `value`', (k) => `${k}: \`${PHRASE}\``], ['key=`value`', (k) => `${k}=\`${PHRASE}\``],
  ['--key=value', (k) => `tool --${k}=${JOINED}`], ['--key="value"', (k) => `tool --${k}="${PHRASE}"`],
  ['key: value, bare', (k) => `${k}: ${JOINED}`],
  ['escaped JSON', (k) => `{\\"${k}\\": \\"${PHRASE}\\"}`],
  ['YAML >', (k) => `\n${k}: >\n  ${PHRASE}\n  ${PHRASE}\nnext: shown`], ['YAML |', (k) => `\n  ${k}: |-\n    ${PHRASE}\n\n    ${PHRASE}\n  next: shown`],
  ['key = value', (k) => `${k} = ${JOINED}`], ['key : "value"', (k) => `${k} : "${PHRASE}"`], ['{"key" : "value"}', (k) => `{"${k}" : "${PHRASE}"}`],
  // YAML anchors and tags before the value (#348).
  ['key: &a !!str "value"', (k) => `${k}: &a !!str "${PHRASE}"`], ['key: !!str value, bare', (k) => `${k}: !!str ${JOINED}`],
]
const CASES = KEYS.flatMap((k) => FORMS.map(([form, text]) => [`${k} (${form})`, text(k)]))
const leaked = (text) => [...new Set(text.match(/[a-z]{3,}/gi) ?? [])].filter((t) => WORDS.some((w) => w.includes(t.toLowerCase()) || t.toLowerCase().includes(w)))
// A sample uses CPU time, not wall clock: CI runs this package's test files side by side, and a
// wall-clock sample includes the time this process waits for a CPU. The best of five runs drops one
// that caught a background compile or a collection; noise can add CPU time, never remove it, so a
// rule that is slow in every run stays slow in every sample.
// These inputs keep the absolute ceiling: every path here is the plain redactText rule pass over
// fixed-size inputs (80k and 160k characters), never a served-value walk or a stream, so the cost
// has no per-path constant to scale. 100 ms sits 9.5x above the slowest healthy input here (10.5 ms
// of CPU measured, Node 24.13.1), and a quadratic rule on 80k characters is 6.5x over it (about
// 650 ms, from a 20k run that grew 3.8x when doubled). A machine 4x slower lands at 42 ms, still
// under it. The fuzz test, whose served value is a multiple of the rule pass, bounds that path by a
// measured reference.
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

test('every key form in a served string, at any depth: no word of its value is left', () => {
  for (const [what, text] of CASES) {
    assert.deepEqual(leaked(redactText(`the config has ${text} and goes on`)), [], what)
    assert.deepEqual(leaked(JSON.stringify(redact({ host: { note: text }, list: [text] }))), [], what)
  }
})

test('a field named for a secret is hidden whatever its value', () => {
  for (const key of ['passphrase', 'db_pass', 'pass']) assert.deepEqual(leaked(JSON.stringify(redact({ [key]: PHRASE }))), [], key)
  assert.deepEqual(redact({ compass: 'north', bypass: 'open', pass_rate: 0.9, pass_count: 3 }), { compass: 'north', bypass: 'open', pass_rate: 0.9, pass_count: 3 })
})

test('words that only contain a key name are not keys', () => {
  const prose = 'bypass=open compass: north passenger: aboard tokenizer: fast a-bypass=1 -bypass=1 --bypass=open secret_sauce: butter basic steps first bearer bonds accessLevel: admin clientName: acme passRate: 0.9 maxTokens: 512 tokenCount: 12 primaryKey: id sortKey: name keyboardLayout: us passengerCount: 4 tokenizerName: bpe compassHeading: north my firstPass at it boardingPass alone AccessLevel: admin MaxTokens: 512 PassengerCount: 4 PassRate: 0.9 my FirstPass at it the bearer of bad news'
  assert.equal(redactText(prose), prose)
})

test('a scheme after a key name is hidden in any case, and the scheme word stays', () => {
  for (const text of SCHEMES) {
    assert.deepEqual(leaked(redactText(text)), [], text)
    if (!text.startsWith('{')) assert.match(redactText(text), /(bearer|BEARER|basic) \[redacted\]/, text)
  }
})

test('80k characters of adversarial input stay fast', () => {
  const inputs = Object.entries(ADVERSARIAL)
  // Warm-up, not measured: a cold process pays to compile the rules' regular expressions and climb
  // V8's tiers once, and that setup is not an input's cost. The same inputs then run measured.
  for (const [, text] of inputs) redactText(text)
  for (const [what, text] of inputs) {
    const ms = timed(() => redactText(text))
    assert.ok(ms < FAST_MS, `${what}: ${ms.toFixed(1)} ms of CPU, ceiling ${FAST_MS} ms`)
  }
})

test('a scheme in any case with no key name hides a token-shaped word', () => {
  for (const text of ALONE) assert.deepEqual(leaked(redactText(text)), [], text)
  for (const text of NOT_TOKENS) assert.equal(redactText(`${text}, then more`), `${text}, then more`)
  for (const text of [LATE, LATE_SPLIT[0]]) assert.deepEqual(leaked(redactText(text)), [], text)
})
