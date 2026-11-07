// #496: an explicit key's value was served when its name ended at a trailing comment and the value
// was on the `:` line, when a colon inside its quoted name kept the name from being read whole, and
// when a comment or blank line sat between the key line and its `:` line (hidden in place, never
// remembered). The explicit-key reader now takes a quoted name whole (any colon in it), allows a
// trailing comment before the line end and blank or comment lines before the `:` line, and the
// value is hidden in place and remembered, so a later bare occurrence is masked. Rows are built
// from pieces, so no credential-shaped text is committed.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { REDACTED, redactText, secretsIn } from '../src/redact.js'

const PASS = ['pass', 'word'].join('')
const SHY = `${PASS.slice(0, 4)}\u00ad${PASS.slice(4)}` // `pass\u00adword`, the #493 spelling
const INVIS = `${PASS.slice(0, 2)}\u00ad${PASS.slice(2)}` // `pa\u00adssword`, the same name
const VALUE = ['Zq9', 'marker'].join('')
const SHOWN = ['Shown', '9value'].join('')
const masked = (text, kept) => kept.reduce((t, v) => t.split(v).join(REDACTED), text)

// 1. A trailing comment after the name, the value on the `:` line. The comment ends at its line.
const COMMENTED_VALUE = [
  ['a quoted invisible name', `? "${SHY}" # note\n: ${VALUE}`, `? "${PASS}" # note\n: ${REDACTED}`],
  ['a quoted plain name', `? "${PASS}" # note\n: ${VALUE}`, `? "${PASS}" # note\n: ${REDACTED}`],
  ['an unquoted plain name', `? ${PASS} # note\n: ${VALUE}`, `? ${PASS} # note\n: ${REDACTED}`],
  ['a list item', `- ? "${PASS}" # note\n  : ${VALUE}`, `- ? "${PASS}" # note\n  : ${REDACTED}`],
]
for (const [what, input, want] of COMMENTED_VALUE) {
  test(`#496: an explicit value under a trailing comment is hidden and remembered (${what})`, () => {
    assert.equal(redactText(input), want, 'hidden in place')
    const kept = secretsIn(`${input}\n`)
    assert.ok(kept.includes(VALUE), 'remembered')
    assert.ok(!masked(`later note: ${VALUE}`, kept).includes(VALUE), 'a later bare occurrence is masked')
  })
}

test('#496: a trailing comment swallows nothing after its line and is never remembered itself', () => {
  const text = `? "${PASS}" # note\n: ${VALUE}\nsibling: ${SHOWN}\n`
  assert.ok(redactText(text).endsWith(`sibling: ${SHOWN}\n`), 'the sibling line is served as written')
  const kept = secretsIn(text)
  assert.ok(kept.includes(VALUE), 'the value is remembered')
  assert.ok(!kept.includes(SHOWN), 'a sibling value is not remembered')
  assert.deepEqual(secretsIn(`? "${PASS}" # ${VALUE}\n: ${SHOWN}\n`), [SHOWN], "a comment's own text is never remembered")
})

// 2. A quoted name is taken whole, a colon inside it included: its value is hidden and remembered,
// and the name stays as written (it is not read as a name and a value on the `?` line).
const COLON_NAMES = [
  ['a quoted plain name, the value on its line', `? "${PASS}:db": ${VALUE}`],
  ['a quoted invisible name, the value on its line', `? "${INVIS}:db": ${VALUE}`],
  ['a quoted plain name, the value on its \':\' line', `? "${PASS}:db"\n: ${VALUE}`],
  ['a quoted invisible name, the value on its \':\' line', `? "${INVIS}:db"\n: ${VALUE}`],
]
for (const [what, input] of COLON_NAMES) {
  test(`#496: a quoted explicit name is taken whole, a colon included (${what})`, () => {
    const name = `${PASS}:db`
    assert.ok(redactText(input).includes(`"${name}"`), 'the name is served as written')
    assert.ok(!redactText(input).includes(`${PASS}:[redacted]`), 'the name is not read as a name and a value')
    assert.ok(redactText(input).endsWith(REDACTED), 'the value is hidden in place')
    const kept = secretsIn(`${input}\n`)
    assert.ok(kept.includes(VALUE), 'remembered')
    assert.ok(!masked(`later note: ${VALUE}`, kept).includes(VALUE), 'a later bare occurrence is masked')
  })
}

test('#496: a quoted name with a colon and no credential stem is served and remembers nothing', () => {
  const input = `? "user:name": ${VALUE}`
  assert.equal(redactText(input), input)
  assert.deepEqual(secretsIn(`${input}\n`), [])
})

// 3. A comment or blank line between the key line and its `:` line: the block is hidden in place
// (the block reader already read the gap) and its first line is remembered, so a later bare
// occurrence is masked.
const GAPS = [
  ['an invisible name, a comment after it and a comment line', `? "${SHY}" # note\n# full\n: |\n  ${VALUE}`, `? "${PASS}" # note\n# full\n: |\n  ${REDACTED}`],
  ['a plain name, a comment line', `? ${PASS}\n# full\n: |\n  ${VALUE}`, `? ${PASS}\n# full\n: |\n  ${REDACTED}`],
  ['a blank line', `? ${PASS}\n\n: |\n  ${VALUE}`, `? ${PASS}\n\n: |\n  ${REDACTED}`],
  ['an indented comment line', `? ${PASS}\n  # full\n: |\n  ${VALUE}`, `? ${PASS}\n  # full\n: |\n  ${REDACTED}`],
  ['a value on the \':\' line past a comment line', `? ${PASS}\n# full\n: ${VALUE}`, `? ${PASS}\n# full\n: ${REDACTED}`],
]
for (const [what, input, want] of GAPS) {
  test(`#496: a gap line between an explicit key and its ':' line is read (${what})`, () => {
    assert.equal(redactText(input), want, 'hidden in place')
    const kept = secretsIn(`${input}\n`)
    assert.ok(kept.includes(VALUE), 'remembered')
    assert.ok(!masked(`later note: ${VALUE}`, kept).includes(VALUE), 'a later bare occurrence is masked')
  })
}

test('#496: a gap line neither captures nor remembers the sibling after the block', () => {
  const input = `? ${PASS}\n# full\n: |\n  ${VALUE}\nsibling: ${SHOWN}\n`
  assert.ok(redactText(input).endsWith(`sibling: ${SHOWN}\n`), 'the sibling line is served as written')
  assert.ok(!secretsIn(input).includes(SHOWN), 'a sibling value is not remembered')
})
