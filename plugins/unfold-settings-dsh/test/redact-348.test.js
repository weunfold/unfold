// The #348 table, test/fixtures/redact-348.json: the same file, byte for byte, in unfold-mind-dsh
// and unfold-events-dsh (scripts/unfold-copies.test.ts), so the copies of the redaction cannot
// diverge on it. Invisible characters inside a credential name (NUL, zero-width, BOM, bidi marks,
// raw or as a JSON \u escape) and YAML anchors and tags before its value. A text row serves the
// shape of its plain form; an object row hides the value under a key read without its invisible
// characters and keeps the key as written; a near miss keeps its value.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { redact, redactText } from '../src/redact.js'

const TABLE = JSON.parse(readFileSync(new URL('./fixtures/redact-348.json', import.meta.url), 'utf8'))

for (const [what, input, want] of TABLE.text) test(`text: ${what}`, () => assert.equal(redactText(input), want))
for (const [what, input, want] of TABLE.object) test(`object: ${what}`, () => assert.deepEqual(redact(input), want))

// #396 and #410, the table's `continuation` rows (one table, read the same way by the Mind, window
// and settings copies): a credential key line that ends at its colon, or at up to two anchors and
// tags, a comment after them or not, continues on a later line. Blank and comment lines are
// skipped, and so are lines of anchors and tags indented under the key; then the first line strictly
// more indented is the value (hidden with its continued lines, or a block indicator, whose lines are
// hidden), a more indented mapping line is a child key, and a line at the key's indent or less is a
// sibling. The key line and each blank, comment, anchor and tag line are kept as written. The rows
// hold placeholders, filled here from pieces, so no credential-shaped text is committed.
const FILL = { '{name}': ['pass', 'word'].join(''), '{camel}': ['api', 'Key'].join(''), '{parent}': ['sec', 'ret'].join(''), '{value}': ['hunter', 2].join(''), '{pass}': 'pass', '{word}': 'word' }
const fill = (s) => s.replace(/\{(?:name|camel|parent|value|pass|word)\}/g, (k) => FILL[k])

for (const [what, input, want] of TABLE.continuation) test(`continuation: ${what}`, () => assert.equal(redactText(fill(input)), fill(want)))

// #406, the table's `partial_key` rows read whole (`{cut}` marks where a stream's first part stops).
for (const [what, input, want] of TABLE.partial_key) test(`partial key: ${what}`, () => assert.equal(redactText(fill(input).split('{cut}').join('')), fill(want)))

// #406 F-B residual: a cut pads the unfinished line with blanks. In a growing buffer that line may
// still be arriving, so a bare token on it is neither hidden nor remembered until its line ends (a
// colon makes it a child key, a line end its parent's scalar); until then it is withheld, never
// served. A whole payload keeps the old, fail-closed read of its last line.
const PARTIAL = TABLE.partial_key.map(([what, input]) => [what, fill(input).split('{cut}')])
const PADDED = PARTIAL.filter(([, [head]]) => {
  const pending = head.slice(head.search(/[^\r\n]*$/)).trim()
  return pending && !pending.includes(':') && !/^[ \t]*[|>]/m.test(head)
})
test('a partial child key padded by a cut is neither hidden nor served while its line is open', () => {
  assert.equal(PADDED.length, 4, 'the anchor line, the anchored child, the spaced child and the plain guard')
  for (const [what, [head]] of PADDED) {
    const pending = head.slice(head.search(/[^\r\n]*$/)).trim()
    const out = redactText(`${head}${' '.repeat(17_000)}`, true)
    assert.ok(!out.includes('[redacted]'), `${what}: not masked while the line is open`)
    assert.ok(!out.includes(pending), `${what}: withheld, never served`)
    assert.ok(redactText(head).includes('[redacted]'), `${what}: a whole payload still hides it`)
  }
})

// #406's last frame: the value that completes the token on a cut-padded line. While that line is
// still arriving no rule can read it (its head is cut, so the name arrives in parts) and the value
// is not remembered until its line ends, so the frame must withhold the rest of the line too: on
// every prefix of the growing buffer, the value is never served.
test('a cut-padded line never serves the value at the frame that completes it', () => {
  const VALUE = /hunter2/
  for (const [what, [head, tail]] of PARTIAL) {
    let grown = `${head}${' '.repeat(17_000)}`
    for (const ch of `${tail}\nand goes on`) {
      grown += ch
      assert.doesNotMatch(redactText(grown, true), VALUE, `${what}: no frame serves the value`)
    }
  }
})

// #406 F-B: a line a cut padded with blanks is hidden whole, as its name cannot be read across the
// pad (the parts of a credential name arrive apart). The same rule is in the window and the browser
// copy, byte for byte (scripts/unfold-copies.test.ts).
test('a padded line is hidden whole', () => {
  const VALUE = /hunter2/
  const padded = `${fill('{parent}: &d\n  {pass}')}${' '.repeat(17_000)}${fill('{word}: {value}')}\n`
  assert.doesNotMatch(redactText(padded), VALUE, 'whole payload')
  assert.doesNotMatch(redactText(padded, true), VALUE, 'growing buffer')
})
