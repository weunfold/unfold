/**
 * The one redaction perf guard for the Unfold plugins (#564).
 *
 * Every plugin judges its redaction the same way:
 *
 * - `sample` reads CPU time (`process.cpuUsage`, user + system), not wall clock. CI runs test
 *   files side by side, and wall clock includes the time a process waits for a CPU. Noise can
 *   add CPU time, never remove it, so a rule that is slow in every run stays slow in every
 *   sample.
 * - `fixedWork` takes all five samples and keeps the best: it sets a bound, so one run that
 *   caught a background compile or a collection must not move the bound.
 * - `timed` keeps the best sample and stops early once it is under the path's ceiling.
 * - The reference is a fixed amount of the same work: the plugin's `redactText` over one 80k
 *   text built from the fuzz's own pieces with a fixed seed. It runs the same code path over a
 *   fixed number of pieces, never a fuzz text, so its cost is linear in that input (measured
 *   100.03, 98.28 and 98.49 ns per character at 40k, 80k and 160k, #553) and every run pays
 *   exactly that work. A slower machine or clock raises the reference and the bound together; it
 *   is not a time constant. A zero (or NaN) reference fails before it can make a bound of 0, and
 *   the 200 ms backstop (#553) fails a reference that high, so a broken clock cannot widen a
 *   bound without limit.
 * - Text paths keep an absolute ceiling (100 ms): they are the plain rule pass over fixed-size
 *   inputs, with no window, stream or served-value walk on top, so no per-path constant scales
 *   them. The slowest measured was 58.9 ms of CPU on CI (#53's rule pass at 80k, against the old
 *   50 ms ceiling); a machine 4x slower lands at 50 ms, and planted quadratic KEY= mutants
 *   measured 125-379.6 ms, so they still fail.
 * - A scaled path (a window, a stream, a served value) costs a measured multiple of the
 *   reference: bound = margin x reference. Each plugin supplies its own measured margin. The
 *   bound is capped at the 400 ms bound backstop: raised from 200 (#553) to 400 in 1354b1c5
 *   because 200 clipped a healthy 235.2 ms stream on a loaded runner, and it binds only when
 *   margin x reference passes it. The 200 ms backstop still guards the reference itself.
 * - Every assertion prints the sampled CPU ms and the ceiling or bound it missed, so a red can
 *   be told from a load spike.
 *
 * The module is dependency-free: `node:assert/strict` only.
 */
import assert from 'node:assert/strict'

/** A text path's absolute ceiling, in CPU ms. */
export const TEXT_CEILING_MS = 100
/** The reference sample's own backstop, in CPU ms (#553). */
export const REFERENCE_BACKSTOP_MS = 200
/** The scaled bound's absolute backstop, in CPU ms (1354b1c5). */
export const BOUND_BACKSTOP_MS = 400
/** How many CPU samples a measurement takes. */
export const SAMPLES = 5
/** A #590 linear path may cost this multiple of its flat same-length reference. */
export const LINEAR_MARGIN = 40

/** One CPU-time sample in ms: user + system, a number noise can only add to. */
export function sample(fn) {
  const t0 = process.cpuUsage()
  fn()
  const used = process.cpuUsage(t0)
  return (used.user + used.system) / 1000
}

/** Best of SAMPLES, every sample taken: the reference sets a bound. */
export function fixedWork(fn) {
  return Math.min(...Array.from({ length: SAMPLES }, () => sample(fn)))
}

/** Best of SAMPLES, stopping once the best is under the ceiling. */
export function timed(fn, ceiling = TEXT_CEILING_MS) {
  let best = Infinity
  for (let i = 0; i < SAMPLES && best >= ceiling; i++) best = Math.min(best, sample(fn))
  return best
}

// The reference pieces are the fuzz's shapes a redaction rule can backtrack on: whitespace runs,
// key names in each form, separators, open and closed quotes, backslashes, escaped quotes, line
// ends and schemes. The text is 80k characters from seed 0x531, built once; every run pays the
// same work. Policy B withholds malformed credential collections as a whole, so random
// flow openers would short-circuit this scalar reference. Their opening delimiters are x instead;
// seed, size, samples, margins and ceilings stay fixed. Measured CPU at 80k: the original
// reference 8.94 ms before the policy and 1.45 ms after it; this scalar reference 8.07/9.35 ms.
// At 160k the scalar pass was 15.88/17.04 ms, retaining the linear work #553 requires.
const REFERENCE_PIECES = [
  ' ', '\t', ' \t', '\n', '\n  ', 'password', 'db_password', 'DB_PASS', 'accessToken',
  'ClientSecret', 'DbPass', 'API_TOKEN', '--token', '-pass', 'token', '=', ': ', ' = ', '":',
  '\\":', ': |\n', ': >-\n', '"', "'", '`', '\\', '\\\\', '\\"', '\\"\\"', 'bearer ',
  'basic ', 'Bearer ', 'x9', 'a', 'word ', 'x"', 'x\\"', '_', '-',
]
const REFERENCE_SEED = 0x531
const REFERENCE_SIZE = 80_000

/** Build one fixed-seed text of `size` characters from `pieces` (default: the guard's). */
export function referenceText(
  pieces = REFERENCE_PIECES,
  { seed = REFERENCE_SEED, size = REFERENCE_SIZE } = {},
) {
  let state = seed >>> 0
  const rand = () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = Math.imul(state ^ (state >>> 15), state | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296
  }
  const parts = []
  for (let n = 0; n < size; ) {
    const piece = pieces[Math.floor(rand() * pieces.length)]
    parts.push(piece)
    n += piece.length
  }
  return parts.join('')
}

const REFERENCE_TEXT = referenceText()

/**
 * Fail closed on a reference sample: zero (or NaN) cannot set a bound, and the 200 ms backstop
 * keeps a misread clock from widening one without limit.
 */
function checkedReference(ms, precision) {
  const shown = ms.toFixed(precision)
  assert.ok(ms > 0, `reference sample is ${shown} ms of CPU`)
  assert.ok(ms <= REFERENCE_BACKSTOP_MS,
    `reference sample is ${shown} ms of CPU, backstop ${REFERENCE_BACKSTOP_MS} ms`)
  return ms
}

/**
 * The fixed-work reference for `redact`: `redact` over the guard's own 80k text. Returns its best
 * CPU ms, failing closed on a zero/NaN or over-backstop sample.
 */
export function measureReference(redact, text = REFERENCE_TEXT) {
  return checkedReference(fixedWork(() => redact(text)), 1)
}

/** The scaled path's bound and the text that explains it. */
export function boundFor(referenceMs, margin) {
  return {
    bound: Math.min(margin * referenceMs, BOUND_BACKSTOP_MS),
    margin: `${margin} x reference ${referenceMs.toFixed(1)} ms`,
  }
}

/** Time one text path against the absolute ceiling, assert, and return its CPU ms. */
export function assertTextPath({ path, shape, run, ceiling = TEXT_CEILING_MS }) {
  const ms = timed(run, ceiling)
  const detail = `${path}: ${shape}: ${ms.toFixed(1)} ms of CPU, ceiling ${ceiling} ms`
  assert.ok(ms < ceiling, detail)
  return ms
}

/** Time one scaled path against `boundFor`, assert, and return its CPU ms. */
export function assertScaledPath({ path, shape, run, referenceMs, margin }) {
  const { bound, margin: shown } = boundFor(referenceMs, margin)
  const ms = timed(run, bound)
  const detail = `${path}: ${shape}: ${ms.toFixed(1)} ms of CPU, ` +
    `bound ${bound.toFixed(1)} ms (${shown}; backstop ${BOUND_BACKSTOP_MS} ms)`
  assert.ok(ms <= bound, detail)
  return ms
}

/**
 * Run a plugin's fuzz cases through the guard.
 *
 * `cases` is `[shape, text]`; `textPaths` is `[name, run]`; `scaledPath` is
 * `[name, run, margin]` or null; `reference` is the plugin's redact function for the fixed
 * reference work (required with a scaled path); `prepare(text, shape)` turns a case text into
 * the input the paths receive (Mind appends its planted credentials); `check(text, shape)` runs
 * the plugin's correctness assertions before the timed pass; `onDiagnostic(line)` receives the
 * reference/bound line and the slowest row per path.
 *
 * Every text path is timed per case against the shared ceiling, then the scaled path against
 * its bound. Warm-up is not measured, but runs every path on every prepared case so a cold
 * process's regex compilation, buffer growth and V8 tiers are not a text's cost.
 */
export function guardCases({
  cases,
  textPaths = [],
  scaledPath = null,
  reference = null,
  prepare = (text) => text,
  check = null,
  onDiagnostic = null,
}) {
  if (scaledPath && !reference) throw new Error('a scaled path needs a reference redact function')
  const inputs = cases.map(([shape, text]) => [shape, prepare(text, shape)])
  for (const [, text] of inputs) {
    for (const [, run] of textPaths) run(text)
    if (scaledPath) scaledPath[1](text)
  }
  const referenceMs = scaledPath ? measureReference(reference) : null
  const limits = scaledPath ? boundFor(referenceMs, scaledPath[2]) : null
  if (limits && onDiagnostic) {
    onDiagnostic(`${scaledPath[0]}: reference ${referenceMs.toFixed(1)} ms of CPU, ` +
      `bound ${limits.bound.toFixed(1)} ms (${limits.margin}; ` +
      `backstop ${BOUND_BACKSTOP_MS} ms)`)
  }
  const slowest = new Map()
  const note = (path, ms, shape) => {
    if (ms > (slowest.get(path)?.ms ?? 0)) slowest.set(path, { ms, shape })
  }
  for (const [shape, text] of inputs) {
    if (check) check(text, shape)
    for (const [path, run] of textPaths) {
      note(path, assertTextPath({ path, shape, run: () => run(text) }), shape)
    }
    if (scaledPath) {
      const [path, run, margin] = scaledPath
      const ms = assertScaledPath({ path, shape, run: () => run(text), referenceMs, margin })
      note(path, ms, shape)
    }
  }
  if (referenceMs && onDiagnostic) {
    for (const [path, { ms, shape }] of slowest) {
      onDiagnostic(`${path}: slowest ${ms.toFixed(1)} ms of CPU, ` +
        `${(ms / referenceMs).toFixed(2)} times the reference — ${shape}`)
    }
  }
  return { referenceMs, bound: limits?.bound ?? null, slowest }
}

/**
 * The #590 check: each path on a fixed text must stay within `factor` of the same redact function
 * over a flat text of the same length, so a rule that backtracks over an unclosed quote run fails
 * even when a slow runner's backstop would hide it in the fuzz's shared bound.
 */
export function assertLinearPaths({ text, reference, runs, factor = LINEAR_MARGIN }) {
  const flat = 'x'.repeat(text.length)
  const referenceMs = checkedReference(fixedWork(() => reference(flat)), 3)
  const bound = factor * referenceMs
  for (const [path, run] of runs) {
    const ms = fixedWork(() => run(text))
    const detail = `${path}: ${ms.toFixed(2)} ms of CPU on the quote run, ` +
      `bound ${bound.toFixed(2)} ms (${factor} x reference ${referenceMs.toFixed(3)} ms)`
    assert.ok(ms <= bound, detail)
  }
}
