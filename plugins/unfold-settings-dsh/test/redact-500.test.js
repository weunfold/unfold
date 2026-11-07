// #500: the explicit-key reader's gap branch (#496) took a comment's own colon as the separator.
// A credential-shaped value inside the trailing comment after the name was served verbatim (the
// comment sat inside the match and was never rescanned), and a comment line before the `:` line
// was masked as the value. The gap form now runs after the plain name rules, so the comment's own
// text is read first, and its comment runs are whole: each is terminated by its own line end, so
// it matches only whole, and the `:` that ends a gap must be the first non-blank of its own line.
// Settings' redaction has no `secretsIn`; these rows are the text it serves. Rows are built from
// pieces, so no credential-shaped text is committed.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { REDACTED, redactText } from '../src/redact.js'

const PASS = ['pass', 'word'].join('')
const KEY = ['api', '_key'].join('')
const VALUE = ['Kx7', 'plumTree'].join('')
const LEAK = ['hunter', '42x'].join('')
const OWNER = ['Owner', '4Ada'].join('')
const SHOWN = ['Shown', '9value'].join('')

// A. A credential-shaped value inside the trailing comment after the explicit name is redacted in
// place; the real value on the `:` line is hidden too.
const LEAKY_COMMENT = [
  ['a quoted name', `? "${PASS}" # old ${PASS}: ${LEAK}\n: ${VALUE}\n`, `? "${PASS}" # old ${PASS}: ${REDACTED}\n: ${REDACTED}\n`],
  ['an unquoted api_key', `? ${KEY} # old ${KEY}: ${LEAK}\n: ${VALUE}\n`, `? ${KEY} # old ${KEY}: ${REDACTED}\n: ${REDACTED}\n`],
]
for (const [what, input, want] of LEAKY_COMMENT) {
  test(`#500: a value in the trailing comment is redacted (${what})`, () => {
    assert.equal(redactText(input), want, 'the comment value and the real value are hidden')
  })
}

test('#500: a trailing comment swallows nothing after its line', () => {
  const text = `? ${PASS} # old ${PASS}: ${LEAK}\n: ${VALUE}\nsibling: ${SHOWN}\n`
  assert.ok(redactText(text).endsWith(`sibling: ${SHOWN}\n`), 'the sibling line is served as written')
})

// B. A comment line before the `:` line is never the value: its text stays as written, and the
// real value under the block or the empty colon is hidden.
const COMMENT_LINES = [
  ['a block', `? ${PASS}\n# owner: ${OWNER}\n: |\n  ${VALUE}\n`],
  ['an empty colon', `? ${PASS}\n# owner: ${OWNER}\n:\n  ${VALUE}\n`],
]
for (const [what, input] of COMMENT_LINES) {
  test(`#500: a comment line before the ':' line is not masked (${what})`, () => {
    const served = redactText(input)
    assert.ok(served.includes(`# owner: ${OWNER}`), 'the comment line is served as written')
    assert.ok(!served.includes(VALUE), 'the real value is hidden')
  })
}

test('#500: a gap line does not swallow the sibling after the value', () => {
  const input = `? ${PASS}\n# owner: ${OWNER}\n:\n  ${VALUE}\nsibling: ${SHOWN}\n`
  assert.ok(redactText(input).endsWith(`sibling: ${SHOWN}\n`), 'the sibling line is served as written')
})
