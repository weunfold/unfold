// #500: the explicit-key reader's gap branch (#496) took a comment's own colon as the separator.
// A credential-shaped value inside the trailing comment after the name was served verbatim (the
// comment sat inside the match and was never rescanned) and forgotten, and a comment line before
// the `:` line was masked as the value and its text remembered. The gap form now runs after the
// plain name rules, so the comment's own text is read first, and its comment runs are whole: each
// is terminated by its own line end, so it matches only whole, and the `:` that ends a gap must be
// the first non-blank of its own line. Rows are built from pieces, so no credential-shaped text is
// committed.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { REDACTED, redactText, secretsIn } from '../src/redact.js'

const PASS = ['pass', 'word'].join('')
const KEY = ['api', '_key'].join('')
const VALUE = ['Kx7', 'plumTree'].join('')
const LEAK = ['hunter', '42x'].join('')
const OWNER = ['Owner', '4Ada'].join('')
const SHOWN = ['Shown', '9value'].join('')
const masked = (text, kept) => kept.reduce((t, v) => t.split(v).join(REDACTED), text)

// A. A credential-shaped value inside the trailing comment after the explicit name is redacted in
// place and remembered, as it was before #500; the real value on the `:` line is hidden too.
const LEAKY_COMMENT = [
  ['a quoted name', `? "${PASS}" # old ${PASS}: ${LEAK}\n: ${VALUE}\n`, `? "${PASS}" # old ${PASS}: ${REDACTED}\n: ${REDACTED}\n`],
  ['an unquoted api_key', `? ${KEY} # old ${KEY}: ${LEAK}\n: ${VALUE}\n`, `? ${KEY} # old ${KEY}: ${REDACTED}\n: ${REDACTED}\n`],
]
for (const [what, input, want] of LEAKY_COMMENT) {
  test(`#500: a value in the trailing comment is redacted and remembered (${what})`, () => {
    assert.equal(redactText(input), want, 'the comment value and the real value are hidden')
    const kept = secretsIn(input)
    assert.ok(kept.includes(LEAK), 'the comment value is remembered')
    assert.ok(kept.includes(VALUE), 'the real value is remembered')
    assert.ok(!masked(`later note: ${LEAK}`, kept).includes(LEAK), 'a later bare occurrence of the comment value is masked')
  })
}

test('#500: a trailing comment swallows nothing after its line and is never remembered itself', () => {
  const text = `? ${PASS} # old ${PASS}: ${LEAK}\n: ${VALUE}\nsibling: ${SHOWN}\n`
  assert.ok(redactText(text).endsWith(`sibling: ${SHOWN}\n`), 'the sibling line is served as written')
  const kept = secretsIn(text)
  assert.ok(kept.includes(VALUE), 'the real value is remembered')
  assert.ok(!kept.includes(SHOWN), 'a sibling value is not remembered')
  assert.deepEqual(secretsIn(`? ${PASS} # ${VALUE}\n: ${SHOWN}\n`), [SHOWN], "a comment's own text is never remembered")
})

// B. A comment line before the `:` line is never the value: its text stays as written and is not
// remembered, and the real value under the block or the empty colon is hidden and remembered.
const COMMENT_LINES = [
  ['a block', `? ${PASS}\n# owner: ${OWNER}\n: |\n  ${VALUE}\n`],
  ['an empty colon', `? ${PASS}\n# owner: ${OWNER}\n:\n  ${VALUE}\n`],
]
for (const [what, input] of COMMENT_LINES) {
  test(`#500: a comment line before the ':' line is neither masked nor remembered (${what})`, () => {
    const served = redactText(input)
    assert.ok(served.includes(`# owner: ${OWNER}`), 'the comment line is served as written')
    assert.ok(!served.includes(VALUE), 'the real value is hidden')
    const kept = secretsIn(input)
    assert.ok(!kept.includes(OWNER), "a comment's text is not remembered")
    assert.ok(kept.includes(VALUE), 'the real value is remembered')
  })
}

test('#500: a gap line neither captures nor remembers the sibling after the value', () => {
  const input = `? ${PASS}\n# owner: ${OWNER}\n:\n  ${VALUE}\nsibling: ${SHOWN}\n`
  assert.ok(redactText(input).endsWith(`sibling: ${SHOWN}\n`), 'the sibling line is served as written')
  assert.ok(!secretsIn(input).includes(SHOWN), 'a sibling value is not remembered')
})
