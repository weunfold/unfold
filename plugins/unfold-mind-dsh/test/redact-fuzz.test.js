// Every redaction path hides a planted secret and stays fast on a seeded fuzz of mixed inputs: the
// text rules, the values a cut tail still masks, and Mind's stream.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { MindStream } from '../src/mind.js'
import { redactText, secretsIn } from '../src/redact.js'
import { assertLinearPaths, guardCases } from '../../../scripts/redact-fuzz-guard.mjs'

// A seeded fuzz: 200 texts of up to 80k characters, each a random mix of the pieces a rule could
// backtrack on (whitespace runs, key names in each case and form, separators, open and closed
// quotes, backslashes, escaped quotes, newlines, schemes), most with a run repeated to fill the
// size. A third are the shapes a rule once backtracked on, with random runs, keys and tails:
// whitespace before a YAML block key, a word of upper-case names then `=` and a failing value, an
// unbroken run then a separator and whitespace, and a key name run then a failing value. The same
// seed gives the same texts, and each text comes with a short shape that names it.
const SEED = 0x5eed124
const PIECES = [' ', '\t', ' \t', '\n', '\n  ', 'password', 'db_password', 'DB_PASS', 'accessToken', 'ClientSecret', 'DbPass', 'API_TOKEN', '--token', '-pass', 'token', '=', ': ', ' = ', '":', '\\":', ': |\n', ': >-\n', '"', "'", '`', '\\', '\\\\', '\\"', '\\"\\"', 'bearer ', 'basic ', 'Bearer ', 'x9', 'a', 'word ', '{"', '{\\"', '_', '-']
const SIZES = [1_000, 4_000, 16_000, 40_000, 80_000]
const SHAPES = [
  [[' ', '\t', ' \t', '\t '], ['password', 'accessToken', 'db_password', 'ClientSecret'], [': |\n', ': >-\n', ': |', ':  >']],
  [['TOKEN', 'API_KEY_', 'SECRET', 'PASSWORD_', 'X9_', 'APIKEY'], [''], ['=', '= x', '="', "='", '=`', '=x', ' =', '']],
  [['a', 'DB_PASSaClientSecret', 'x9-', 'word'], [''], [': = \t', ': =\\": \t', ' \t', ':\n ']],
  [['password', 'accessToken', 'db_pass ', '--token'], [''], ['=', ': ', '\\": \\"', '="', ' = ']],
]
function fuzzCases(seed = SEED, count = 200) {
  let state = seed >>> 0
  const rand = () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = Math.imul(state ^ (state >>> 15), state | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296
  }
  const pick = (list) => list[Math.floor(rand() * list.length)]
  const unit = () => Array.from({ length: 1 + Math.floor(rand() * 4) }, () => pick(PIECES)).join('')
  const fill = (piece, size) => piece.repeat(Math.max(1, Math.floor(size / piece.length)))
  const cases = Array.from({ length: count }, (_, i) => {
    const size = pick(SIZES)
    if (i % 3 === 2) {
      const [runs, keys, tails] = pick(SHAPES)
      const [run, key, tail] = [pick(runs), pick(keys), pick(tails)]
      return [`#${i} ${JSON.stringify(run)} to ${size}, then ${JSON.stringify(key + tail)}`, `${fill(run, size)}${key}${tail}`]
    }
    if (i % 3 === 0) {
      const parts = []
      for (let n = 0; n < size; ) {
        const piece = pick(PIECES)
        parts.push(piece)
        n += piece.length
      }
      return [`#${i} ${size} random pieces`, parts.join('')]
    }
    const [lead, run, tail] = [unit(), unit(), unit()]
    const share = rand()
    const text = `${fill(lead, size * share)}${fill(run, size * (1 - share))}${tail}`
    return [`#${i} ${JSON.stringify(lead)} then ${JSON.stringify(run)} repeated, then ${JSON.stringify(tail)}`, text]
  })
  // The shape that once backtracked over a whole blank run: 80k of " \t", then a name and a block.
  cases.push(['fixed: 80000 " \\t" then accessToken: >', `${' \t'.repeat(40_000)}accessToken: >\n`])
  cases.push(['fixed: 16384 of {"\\ runs', '{"\\'.repeat(5462).slice(0, 16_384)])
  // The shape Mind's padded kept tail makes from the row above: the unclosed run, a 4 KiB blank
  // pad, then the run again (#590). Its cost must stay one linear pass of its own.
  cases.push(['fixed: {"\\ runs around a 4096 blank pad',
    `${'{"\\'.repeat(5462).slice(0, 8192)}${' '.repeat(4096)}` +
    `${'{"\\'.repeat(5462).slice(0, 8176)}=ClientSecret: |\n DB_PASS`])
  cases.push(['fixed: ? " + 80000 alnum + :x', `? "${'z'.repeat(80_000)}:x\n`])
  return cases
}

// Each text ends with two planted credentials: `SVCTOKEN=`, which only the upper-case KEY= rule
// knows (no `_` or `-` before its TOKEN), and a lower-case `db_password=`. Every path must hide
// both (redactText's output and the stream's snapshot never hold them) and secretsIn must find
// both, so a rule that stops matching fails here on any case. They are bare: after a text's own
// open quote they are part of its value, and hidden with it.
const [UPPER, LOWER] = ['s3cretPlantedUpper9', 's3cretPlantedLower9']
const PLANTED = ` SVCTOKEN=${UPPER} db_password=${LOWER}`

// The stream runs redactText over each delta and builds its row, sentence and audit work on top,
// so it costs a multiple of the reference: 3.56-4.24x measured here (worst of the 200 cases across
// 24.13.1) and 3.49x, 3.55x, 3.56x, 3.76x, 3.92x, 5.84x, 6.00x, 7.02x, 7.18x, 7.45x, 7.62x and
// 10.5x across CI runs, then 3.62x, 3.66x, 4.26x and 4.51x on the #599 and #513 lanes after it.
// The 10.5x one measured 118.7 ms against a 11.3 ms reference and failed the old 10 x bound by
// 5% (#513's b6e7041e head); it was a margin overrun, never a hiding failure. STREAM_MARGIN 15
// sits 1.43x above that worst CI reading. The spread is runner-driven: the reference moved
// 11.3-33.5 ms while the ratio moved 3.6-10.5x, so a bound of two same-run references would track
// it better; that is #590's deeper rework, not this pass. The trade: a 2x regression of the stream
// on a quiet machine (8.0x) now stays inside the bound, while a stream-only O(n^2) mutant
// (measured 351.9 ms against the 126.4 ms bound here) still fails. The shared 400 ms bound
// backstop caps how far a slow reference may widen the bound: it sits 1.7x above the worst
// observed stream reading (235.2 ms, 7.02x) and binds when 15 x reference passes it, a reference
// above 26.7 ms; at the worst observed reference (33.5 ms) the cap sets the bound and that
// 235.2 ms reading still passes. A shared rule gone quadratic still fails on the text ceiling.
const STREAM_MARGIN = 15

function push(text) {
  const mind = new MindStream({ now: () => 1 })
  mind.life = 'lifeZ'
  mind.agentd({ v: 2, id: 'g:0', producer: 'g', seq: 0, t_ns: 0, source: 'guest', kind: 'response_started', response_id: 'rK' }, { via: 'agentd journal', file: '/j', offset: 0 })
  mind.agentd({ v: 2, id: 'g:1', producer: 'g', seq: 1, t_ns: 1, source: 'guest', kind: 'agent_delta', response_id: 'rK', text }, { via: 'agentd journal', file: '/j', offset: 1 })
  return mind
}
const TEXT_PATHS = [
  ['redactText', redactText],
  ['secretsIn', secretsIn],
]
const STREAM_PATH = ['the stream', push, STREAM_MARGIN]

test('a seeded fuzz of 200 mixed texts up to 80k characters hides every planted secret, fast, on every path', (t) => {
  guardCases({
    cases: fuzzCases(),
    textPaths: TEXT_PATHS,
    scaledPath: STREAM_PATH,
    reference: redactText,
    prepare: (plain) => plain + PLANTED,
    check: (text, shape) => {
      const hides = (path, out) => {
        for (const secret of [UPPER, LOWER]) {
          assert.ok(!out.includes(secret), `${path}: ${shape} shows ${secret}`)
        }
      }
      hides('redactText', redactText(text))
      hides('the stream', JSON.stringify(push(text).snapshot()))
      const found = secretsIn(text)
      for (const secret of [UPPER, LOWER]) {
        const misses = `secretsIn: ${shape} misses ${secret}`
        assert.ok(found.some((v) => v.includes(secret)), misses)
      }
    },
    onDiagnostic: (line) => t.diagnostic(line),
  })
})

// #590: the padded kept-tail shape of #172 as its own row, with its own reference. The reference
// is a flat text of the same length: a linear scan over the row must stay a small multiple of it,
// so a rule that backtracks over the unclosed quote run fails even when a slow host's backstop
// would hide it in the fuzz's shared bound. The old rule measured ~34 ms on the row against
// ~0.1 ms here (340x); the linear one costs ~1 ms (about 10x).
const QUOTE_RUN = '{"\\'.repeat(5462)
const QUOTE_PAD_ROW = `${QUOTE_RUN.slice(0, 8192)}${' '.repeat(4096)}` +
  `${QUOTE_RUN.slice(0, 8176)}=ClientSecret: |\n DB_PASS`
test('an unclosed quote run around a blank pad is linear against its own reference', () => {
  assertLinearPaths({
    text: QUOTE_PAD_ROW,
    reference: redactText,
    runs: [['redactText', redactText], ['secretsIn', secretsIn]],
  })
})
