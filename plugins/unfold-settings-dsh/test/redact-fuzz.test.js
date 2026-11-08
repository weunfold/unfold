// Every redaction path stays fast on a seeded fuzz of mixed inputs: the text rules, and a served
// value at depth.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { redact, redactText } from '../src/redact.js'

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

// A sample uses CPU time, not wall clock: CI runs this package's test files side by side, and a
// wall-clock sample includes the time this process waits for a CPU. Five wall-clock runs of the
// same 80k text spanned 8 ms to 211 ms here while their CPU time stayed within a millisecond. The
// best of five runs drops one that caught a background compile or a collection; noise can add CPU
// time, never remove it, so a rule that is slow in every run stays slow in every sample.
// The text path is the plain rule pass over one fixed-size piece, with no served-value walk on
// top, so it keeps an absolute ceiling: 100 ms. The slowest measured was 58.9 ms of CPU on CI
// (events' #53 rule pass at 80k, against the old 50 ms ceiling); the browser copy costs 1.15-1.27x
// the rule pass here, projecting 68-75 ms under the same inflation, so 100 keeps 1.3-1.5x
// headroom. A machine 4x slower lands at 50 ms, and planted quadratic KEY= mutants measured
// 125-379.6 ms, so they still fail.
const TEXT_CEILING_MS = 100
const FAST_TRIES = 5
const sample = (fn) => {
  const t0 = process.cpuUsage()
  fn()
  const used = process.cpuUsage(t0)
  return (used.user + used.system) / 1000
}
const timed = (fn, ceiling = TEXT_CEILING_MS) => {
  let best = Infinity
  for (let i = 0; i < FAST_TRIES && best >= ceiling; i++) best = Math.min(best, sample(fn))
  return best
}
// The reference sets a bound, so it always takes all five samples.
const fixedWork = (fn) => Math.min(...Array.from({ length: FAST_TRIES }, () => sample(fn)))

// The reference is a fixed amount of the same work: redactText over one 80k text of the fuzz's own
// pieces, built once from a fixed seed. It runs the same code path over a fixed N pieces, never the
// fuzz texts, so its cost is linear in that input (40k, 80k and 160k characters measured 114, 110
// and 107 ns per character) and every run pays exactly that work. A slower machine or a slower
// clock raises the reference and the served value's bound together: it is not a time constant.
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
    const piece = PIECES[Math.floor(rand() * PIECES.length)]
    parts.push(piece)
    n += piece.length
  }
  return parts.join('')
})()

// `redact` walks a served value and sends its strings through redactText, so it costs a small
// multiple of the reference: 1.37–1.63x measured here over five runs (worst of the 200 cases over
// those runs, Node 24.13.1), 2.02x on the CI run that failed (#513: 20.8 ms against a 10.3 ms
// reference) and 1.62x, 1.82x and 1.89x on the #599 and #513 lanes after it. SERVED_MARGIN 3 sits
// 1.49x above that CI reading and 1.84x above the local worst: a 2x regression of either reading
// (3.26x local, 4.04x on CI) still fails, while a regression smaller than 1.49x of the CI reading
// is the trade for clearing the runner's spread.
// The absolute backstop caps how far a slow reference may widen the bound: 400 ms sits 1.7x above
// the worst observed runner reading (mind's stream 225.8-233.2 ms at a 30.3-30.6 ms reference) and
// only binds when 3 x reference passes it. A shared rule gone quadratic still fails: the text
// ceiling catches it on the rule pass, and the served path runs past the cap.
const SERVED_MARGIN = 3
const BACKSTOP_MS = 400

const PATHS = [['redactText', redactText], ['redact', (text) => redact({ host: { note: text } })]]

test('a seeded fuzz of 200 mixed texts up to 80k characters stays fast on every path', (t) => {
  const cases = fuzzCases()
  // Warm-up, not measured: a cold process pays to compile each rule's regular expressions, grow
  // its buffers and climb V8's tiers once, and that setup is not a text's cost. The pass runs the
  // same texts through the same paths, so a rule that is slow on a shape is still slow on the
  // measured pass; only the one-time cold cost of a new process is left out.
  for (const [, text] of cases) for (const [, run] of PATHS) run(text)
  const [servedPath, servedRun] = PATHS[PATHS.length - 1]
  const textPaths = PATHS.slice(0, -1)
  const referenceMs = fixedWork(() => redactText(REFERENCE_TEXT))
  // A zero sample would make the bound zero and pass a served value that measured nothing.
  assert.ok(referenceMs > 0, `reference sample is ${referenceMs.toFixed(1)} ms of CPU`)
  const bound = Math.min(SERVED_MARGIN * referenceMs, BACKSTOP_MS)
  const margin = `${SERVED_MARGIN} x reference ${referenceMs.toFixed(1)} ms`
  const slowest = new Map()
  const noteSlowest = (path, ms, shape) => {
    if (ms > (slowest.get(path)?.ms ?? 0)) slowest.set(path, { ms, shape })
  }
  for (const [shape, text] of cases) {
    for (const [path, run] of textPaths) {
      const ms = timed(() => run(text))
      const detail = `${path}: ${shape}: ${ms.toFixed(1)} ms of CPU, ceiling ${TEXT_CEILING_MS} ms`
      assert.ok(ms < TEXT_CEILING_MS, detail)
      noteSlowest(path, ms, shape)
    }
    const servedMs = timed(() => servedRun(text), bound)
    const detail = `${servedPath}: ${shape}: ${servedMs.toFixed(1)} ms of CPU, ` +
      `bound ${bound.toFixed(1)} ms (${margin}; backstop ${BACKSTOP_MS} ms)`
    assert.ok(servedMs <= bound, detail)
    noteSlowest(servedPath, servedMs, shape)
  }
  for (const [path, { ms, shape }] of slowest) {
    t.diagnostic(`${path}: slowest ${ms.toFixed(1)} ms of CPU, ` +
      `${(ms / referenceMs).toFixed(2)} times the reference — ${shape}`)
  }
})
