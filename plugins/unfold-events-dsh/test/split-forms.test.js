// #406 Design A: the split-form table (`split_forms` in test/fixtures/redact-348.json, the same file
// byte for byte in unfold-mind-dsh, unfold-events-dsh and unfold-settings-dsh). These tests have a
// file of their own, kept small: under the package script's --test-force-exit the tail of a large
// file's report can be lost with its plan still matching (#422), and these are read by name. The
// placeholders are filled here from pieces, so no credential-shaped text is committed.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { loadCatalog } from '../src/catalog.js'
import { EventWindow, redactText } from '../src/window.js'
import { redactText as clientRedactText, secretsIn as clientSecretsIn } from '../src/client/redact.js'

const TABLE = JSON.parse(readFileSync(new URL('./fixtures/redact-348.json', import.meta.url), 'utf8'))
const FILL = { '{name}': ['pass', 'word'].join(''), '{parent}': ['sec', 'ret'].join(''), '{value}': ['hunter', 2].join('') }
const fill = (s) => s.replace(/\{(?:name|parent|value)\}/g, (k) => FILL[k])
const SECRET = FILL['{value}']
const catalog = loadCatalog()
let seq = 0

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
// remember a scalar from its ended line; policy B waits for a collection to close.
// At rest the completed values are hidden and remembered.
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
    yield { name: `${what}, pad ${pad}, ${size === 'one delta' ? 'one delta' : `${size}-char deltas`}`, payload, at, end, collection: template.includes('{parent}') || template.includes('\n  - {value}'), step: size === 'one delta' ? payload.length : size }
  }
}
// The value on the key's own line (the plain form's second row) is remembered once it has closed
// on that line, as a same-line value always was: by the frame that ends its line at the latest.
const sameLine = (cell) => !cell.payload.slice(0, cell.at).endsWith('\n') && !/\n[ \t]*-?[ \t]*$/.test(cell.payload.slice(0, cell.at))

// The window's frames: each payload it emits for a delta, read as it is emitted.
function splitRig() {
  const w = new EventWindow(catalog, { limit: 20 })
  let frame = []
  w.onChange((change, payload) => frame.push(JSON.stringify(payload)))
  const push = (text) => {
    frame = []
    w.push({ v: 2, id: `g:${++seq}`, producer: 'g', seq, t_ns: seq, source: 'guest', kind: 'agent_delta', response_id: 'rK', text })
    return [...frame, JSON.stringify(w.rows)].join('\n')
  }
  return { push, text: () => w.rows.at(-1).text }
}
function splitCell(cell) {
  const { payload, at, end, step } = cell
  const r = splitRig()
  for (let i = 0; i < payload.length; i += step) {
    const served = r.push(payload.slice(i, i + step))
    if (i + step <= at) continue // the value has not arrived
    const upto = Math.min(i + step, payload.length)
    const frame = `${cell.name}, the frame ending at ${upto}`
    assert.ok(!served.includes(SECRET), `${frame}: no payload the window emits holds the value`)
    // The browser copy reads the same growing buffer.
    assert.ok(!clientRedactText(payload.slice(0, upto), true).includes(SECRET), `${frame}: the browser copy hides it`)
    const kept = clientSecretsIn(payload.slice(0, upto), true).includes(SECRET)
    if (cell.collection && upto <= end + 1) assert.ok(!kept, `${frame}: policy B waits for the collection to close`)
    else if (i + step > end) assert.ok(kept, `${frame}: the browser copy remembers it once its line has ended`)
    else if (!sameLine(cell)) assert.ok(!kept, `${frame}: the browser copy does not remember it while its line is open`)
  }
  for (const [copy, text] of [['window', redactText], ['browser copy', clientRedactText]]) {
    assert.ok(!text(payload).includes(SECRET), `${cell.name}, ${copy}: the completed text hides the value`)
    assert.ok(!text(payload, true).includes(SECRET), `${cell.name}, ${copy}: the growing buffer hides it at rest`)
  }
  assert.ok(clientSecretsIn(payload).includes(SECRET), `${cell.name}: the browser copy remembers it at rest`)
  assert.ok(r.text().endsWith('user words stay'), `${cell.name}: the text after it is served`)
}
for (const [form, rows] of TABLE.split_forms) {
  test(`served and remembered however the deltas were split (#406 Design A): ${form}`, () => {
    for (const cell of splitCells(rows, false)) splitCell(cell)
  })
  test(`served and remembered however the deltas were split (#406 Design A): ${form}, ${SLOW_NAME}`, { skip: SLOW_SKIP }, () => {
    for (const cell of splitCells(rows, true)) splitCell(cell)
  })
}
