// #406 Design A: the split-form table (`split_forms` in test/fixtures/redact-348.json, the same file
// byte for byte in unfold-mind-dsh, unfold-events-dsh and unfold-settings-dsh). These tests have a
// file of their own, kept small: under the package script's --test-force-exit the tail of a large
// file's report can be lost with its plan still matching (#422), and these are read by name. The
// placeholders are filled here from pieces, so no credential-shaped text is committed.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { redactText } from '../src/redact.js'

const TABLE = JSON.parse(readFileSync(new URL('./fixtures/redact-348.json', import.meta.url), 'utf8'))
const FILL = { '{name}': ['pass', 'word'].join(''), '{parent}': ['sec', 'ret'].join(''), '{value}': ['hunter', 2].join('') }
const fill = (s) => s.replace(/\{(?:name|parent|value)\}/g, (k) => FILL[k])
const SECRET = FILL['{value}']


// #406 Design A, the class rule: in a growing buffer the suffix after the last line terminator is
// never matched, served or remembered until its terminator arrives, however many deltas it came in,
// and no bound starts inside that open line. The table's `split_forms` (one table, byte-identical in
// mind/events/settings): a cut-padded key line, plain, an explicit key's block, a list item's
// explicit key, an explicit key's plain value and its list item, each with the value on the line
// under the pad (and the plain key line completed on its own line). Two tests per form,
// synchronous, each cell run once (the 1-char cells at pads 4096+ in the second, CI only): every pad (1000, under the 4096 keep; 4096, at it; 5000 and 17000, past it,
// and 17000 past the window's 16 KiB bound too) and every split (1, 10 and 100 characters, and the
// whole payload as one delta). From the frame that brings the value's first character on (no frame
// before it holds the value), no served frame holds the value, and the copies that remember values
// remember it only from the frame that ends its line on; at rest it is hidden and remembered.
const SPLIT_PADS = [1000, 4096, 5000, 17000]
const SPLIT_SIZES = [1, 10, 100, 'one delta']
// One redaction per delta: 1-character deltas over a pad at or past the 4096 keep cost minutes on a
// developer machine, so those cells are their own test, run in CI only (GitHub Actions sets CI);
// elsewhere it is skipped with this reason, never left out. Under the package script's
// --test-force-exit the tail of a file's report can be lost with its plan still matching (#422), so
// read these tests by name in a run's output, not by its plan.
const SLOW_SPLIT = (pad, size) => size === 1 && pad >= 4096
const SLOW_SKIP = process.env.CI ? false : 'CI only: 1-character deltas over a pad at or past the 4096 keep, 18 cells per package (set CI=1 to run them)'
const SLOW_NAME = '1-char deltas at pads 4096, 5000 and 17000 (CI only)'
const splitPayload = (template, pad) => fill(template).replace('{pad}', ' '.repeat(pad)).replace('{later}', `later the ${FILL['{name}']} and user words stay`)
// Each cell of a form: its row, pad and split, the payload, where the value starts and the
// terminator that ends its line.
function* splitCells(rows, slow) {
  for (const [what, template] of rows) for (const pad of SPLIT_PADS) for (const size of SPLIT_SIZES) {
    if (SLOW_SPLIT(pad, size) !== slow) continue
    const payload = splitPayload(template, pad)
    const at = payload.indexOf(SECRET)
    const end = payload.indexOf('\n', at + SECRET.length)
    yield { name: `${what}, pad ${pad}, ${size === 'one delta' ? 'one delta' : `${size}-char deltas`}`, payload, at, end, step: size === 'one delta' ? payload.length : size }
  }
}

// Settings keeps no stream and no remembered values: each frame is its growing buffer read whole.
function splitCell(cell) {
  const { payload, at, step } = cell
  for (let i = 0; i < payload.length; i += step) {
    if (i + step <= at) continue // the value has not arrived
    const upto = Math.min(i + step, payload.length)
    assert.ok(!redactText(payload.slice(0, upto), true).includes(SECRET), `${cell.name}, the frame ending at ${upto}: the growing buffer hides the value`)
  }
  assert.ok(!redactText(payload).includes(SECRET), `${cell.name}: the completed text hides the value`)
  assert.ok(redactText(payload).endsWith('user words stay'), `${cell.name}: the text after it is served`)
}
for (const [form, rows] of TABLE.split_forms) {
  test(`served however the deltas were split (#406 Design A): ${form}`, () => {
    for (const cell of splitCells(rows, false)) splitCell(cell)
  })
  test(`served however the deltas were split (#406 Design A): ${form}, ${SLOW_NAME}`, { skip: SLOW_SKIP }, () => {
    for (const cell of splitCells(rows, true)) splitCell(cell)
  })
}
