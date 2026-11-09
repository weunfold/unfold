// Every redaction path stays fast on a seeded fuzz of mixed inputs: the window's rules, the
// browser copy and its values, and the window itself.
import { test } from 'node:test'
import { loadCatalog } from '../src/catalog.js'
import { EventWindow, redactText } from '../src/window.js'
import { redactText as clientRedactText, secretsIn as clientSecretsIn } from '../src/client/redact.js'
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

// The window runs redactText over every delta on top of its own row work, so it costs a multiple
// of the reference: 2.31-2.54x across runs here (Node 24.13.1), 3.51x once under load, 2.32-2.50x
// on the runner numbers (26.24 ms against 10.5-11.3 ms), 2.36x on the Fixer head and 2.06x through
// 6.32x on CI. The 6.32x is the #513 head ee8595c8 (113.8 ms at an 18.0 ms reference), the
// failure this margin answers; 3.72x and 3.66x are 040a22fb and b6e7041e, the same lane.
// WINDOW_MARGIN 8 sits 1.27x above that worst. The trade: a 2x regression of the window path on a
// quiet machine (4.4-5.1x) now stays inside the bound, while a window-only O(n^2) mutant
// (re-measured 184.4 ms against the 65.8 ms bound here at 8 x 8.2 ms; 308.4 ms against the old
// 5.5 x 8.8 ms bound) still fails. The shared 400 ms bound backstop caps how far a slow reference
// may widen the bound; every observed reference (worst 29.8 ms here) stays below it, so the
// margin sets the bound. A shared rule gone quadratic still fails on the text ceiling.
const WINDOW_MARGIN = 8

const catalog = loadCatalog()
const push = (text) => new EventWindow(catalog, { limit: 20 }).push({ v: 2, id: 'g:1', producer: 'g', seq: 1, t_ns: 1, source: 'guest', kind: 'agent_delta', response_id: 'rK', text })
const TEXT_PATHS = [
  ['the window rules', redactText],
  ['the browser copy', clientRedactText],
  ["the browser copy's values", clientSecretsIn],
]
const WINDOW_PATH = ['the window', push, WINDOW_MARGIN]

test('a seeded fuzz of 200 mixed texts up to 80k characters stays fast on every path', (t) => {
  guardCases({
    cases: fuzzCases(),
    textPaths: TEXT_PATHS,
    scaledPath: WINDOW_PATH,
    reference: redactText,
    onDiagnostic: (line) => t.diagnostic(line),
  })
})

// #590: the window's own rule pass on the padded kept-tail shape, against a flat text of the
// same length. The shared window bound is wide enough that a rule-only regression hides in it.
const WINDOW_QUOTE_RUN = '{"\\'.repeat(5462)
const WINDOW_PAD_ROW = `${WINDOW_QUOTE_RUN.slice(0, 8192)}${' '.repeat(4096)}` +
  `${WINDOW_QUOTE_RUN.slice(0, 8176)}=ClientSecret: |\n DB_PASS`
test('the window rules stay linear on an unclosed quote run around a blank pad', () => {
  assertLinearPaths({
    text: WINDOW_PAD_ROW,
    reference: redactText,
    runs: [['the window rules', redactText]],
  })
})
