// #507: an explicit key's value on the indented lines under its `?` line was served, in three
// shapes: an indented scalar under a name with no `:` line, an indented non-comment line in the
// gap before the `:` line (whose real value was served too), and a map child under the name. The
// redaction now reads the more-indented lines under a credential explicit key as its value,
// hides them and remembers what it hides; a clean `? name` ... `:` gap is still the gap rules'.
// #530's listed unquoted and over-bound/unclosed shapes already fail closed on this base, so they
// are pinned; its remaining fallback, an unclosed quoted name holding a quote of the other kind
// (`? "user's_...`, `? 'user"...`), served that tail and remembered nothing, and now hides and
// remembers it. These rows run against the trajectory's carried copy of Mind's redaction. Rows
// are built from pieces, so no credential-shaped text is committed.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { REDACTED, redactText, secretsIn } from './support/redact.js'

const PASS = ['pass', 'word'].join('')
const CAMEL = ['db', 'Password'].join('')
const VALUE = ['Kx7', 'plumTree'].join('')
const INDENTED = ['Maple', '83Tulip'].join('')
const TAIL = ['pri', 'mary'].join('') // six characters, over the four-character remember minimum
const OWNER = ['Owner', '4Ada'].join('')
const SHY = `${PASS.slice(0, 4)}\u00ad${PASS.slice(4)}` // an invisible character inside the name
const masked = (text, kept) => kept.reduce((t, v) => t.split(v).join(REDACTED), text)
const later = (value) => `later note: ${value}`

// 1. #507 (a): a `? name` line with no `:` line under it. The indented scalar is the key's value:
// hidden and remembered, in every name form. The `?` line stays as each form writes it.
const NO_COLON = [
  ['a quoted name with a colon', `? "${PASS}:${TAIL}"\n  ${INDENTED}\n`],
  ['an unquoted name with a colon', `? ${PASS}:${TAIL}\n  ${INDENTED}\n`],
  ['a camelCase name', `? ${CAMEL}\n  ${INDENTED}\n`],
  ['a list item', `- ? "${PASS}"\n  ${INDENTED}\n`],
  ['an anchor and a tag', `? &a !!str "${PASS}"\n  ${INDENTED}\n`],
  ['an invisible name', `? "${SHY}"\n  ${INDENTED}\n`],
]
for (const [what, input] of NO_COLON) {
  test(`#507: an indented scalar under an explicit name with no ':' line is hidden (${what})`, () => {
    const served = redactText(input)
    assert.ok(!served.includes(INDENTED), 'the indented value is hidden')
    assert.ok(served.includes('?') || served.includes('- ?'), 'the question line is kept')
    const kept = secretsIn(input)
    assert.ok(kept.includes(INDENTED), 'the indented value is remembered')
    assert.ok(!masked(later(INDENTED), kept).includes(INDENTED), 'a later bare occurrence is masked')
  })
}

test('#507: the quoted name with a colon stays whole and no part of it is remembered', () => {
  const input = `? "${PASS}:${TAIL}"\n  ${INDENTED}\n`
  assert.equal(redactText(input), `? "${PASS}:${TAIL}"\n  ${REDACTED}\n`, 'the name is served whole')
  assert.ok(!secretsIn(input).includes(TAIL), 'no part of the name is remembered')
})

// 2. #507 (b): an indented non-comment line sits in the gap before the `:` line. It is the
// value, and the real value on the `:` line (inline, under a block indicator, or under an empty
// colon) is hidden with it and remembered; the `?` line stays.
const GAP = [
  ['a quoted name, the value on the colon line', `? "${PASS}"\n  ${INDENTED}\n: ${VALUE}\n`],
  ['a quoted name, a block', `? "${PASS}"\n  ${INDENTED}\n: |\n  ${VALUE}\n`],
  ['a quoted name, an empty colon', `? "${PASS}"\n  ${INDENTED}\n:\n  ${VALUE}\n`],
  ['an unquoted name', `? ${PASS}\n  ${INDENTED}\n: ${VALUE}\n`],
  ['a camelCase name, a block', `? ${CAMEL}\n  ${INDENTED}\n: |\n  ${VALUE}\n`],
  ['a list item', `- ? "${PASS}"\n  ${INDENTED}\n  : ${VALUE}\n`],
  ['an anchor and a tag', `? &a !!str ${PASS}\n  ${INDENTED}\n: ${VALUE}\n`],
]
for (const [what, input] of GAP) {
  test(`#507: an indented gap line and the ':' line value are hidden and remembered (${what})`, () => {
    const served = redactText(input)
    assert.ok(!served.includes(INDENTED), 'the indented value is hidden')
    assert.ok(!served.includes(VALUE), 'the value on the colon line is hidden')
    const kept = secretsIn(input)
    assert.ok(kept.includes(INDENTED), 'the indented value is remembered')
    assert.ok(kept.includes(VALUE), 'the value on the colon line is remembered')
    assert.ok(!masked(later(INDENTED), kept).includes(INDENTED), 'a later bare occurrence is masked')
    assert.ok(!masked(later(VALUE), kept).includes(VALUE), 'a later bare occurrence is masked')
  })
}

test('#507: the gap keeps its shape: the block indicator and the question line are served', () => {
  const input = `? "${PASS}"\n  ${INDENTED}\n: |\n  ${VALUE}\n`
  assert.equal(redactText(input), `? "${PASS}"\n  ${REDACTED}\n: |\n  ${REDACTED}\n`)
})

// 3. #507 (c): a map child under an explicit credential name. The child's value is the key's
// value too: the child line is hidden with it and the value is remembered.
const CHILDREN = [
  ['a quoted name', `? "${PASS}"\n  staging: ${VALUE}\n`],
  ['an unquoted name', `? ${PASS}\n  staging: ${VALUE}\n`],
  ['a camelCase name', `? ${CAMEL}\n  staging: ${VALUE}\n`],
]
for (const [what, input] of CHILDREN) {
  test(`#507: a map child under an explicit credential name is hidden and remembered (${what})`, () => {
    const served = redactText(input)
    assert.ok(!served.includes(VALUE), 'the child value is hidden')
    assert.ok(served.includes('?'), 'the question line is kept')
    const kept = secretsIn(input)
    assert.ok(kept.includes(VALUE), 'the child value is remembered')
    assert.ok(!masked(later(VALUE), kept).includes(VALUE), 'a later bare occurrence is masked')
  })
}

// 4. The clean gap shapes the #496/#500 rows cover are unchanged: a blank, comment or indented
// comment line before the `:` line is not a value; the value under it is hidden and remembered.
const CLEAN = [
  ['a comment line', `? ${PASS}\n# owner: ${OWNER}\n: |\n  ${VALUE}\n`, `? ${PASS}\n# owner: ${OWNER}\n: |\n  ${REDACTED}`],
  ['an empty colon under a list item', `- ? ${PASS}\n  :\n    ${VALUE}\n`, `- ? ${PASS}\n  :\n    ${REDACTED}\n`],
  ['an indented comment line', `? ${PASS}\n  # note\n: ${VALUE}\n`, `? ${PASS}\n  # note\n: ${REDACTED}\n`],
]
for (const [what, input, want] of CLEAN) {
  test(`#507: a clean explicit-key gap is unchanged (${what})`, () => {
    assert.equal(redactText(input), want)
    const kept = secretsIn(input)
    assert.ok(kept.includes(VALUE), 'the value under the colon line is remembered')
    assert.ok(!kept.includes(OWNER), "a comment's text is not remembered")
  })
}

test('#507: a non-credential explicit name is not over-hidden', () => {
  const input = '? compass\n  north\n'
  assert.equal(redactText(input), input)
  assert.deepEqual(secretsIn(input), [])
})

// 5. #530: the two listed shapes already fail closed on this base. They are pinned here so a
// later change cannot serve them, and the value is still remembered.
test('#530: an unquoted explicit name with a colon hides and remembers the colon-line value', () => {
  const input = `? ${PASS}:${TAIL}\n: ${VALUE}\n`
  assert.equal(redactText(input), `? ${PASS}:${TAIL}\n: ${REDACTED}\n`)
  const kept = secretsIn(input)
  assert.ok(kept.includes(VALUE), 'the value is remembered')
  assert.ok(!kept.includes(TAIL), 'the name tail is not remembered')
})

test('#530: an over-bound quoted name hides and remembers the colon-line value', () => {
  const name = `"${'x'.repeat(250)}${PASS}:${TAIL}"`
  const input = `? ${name}\n: ${VALUE}\n`
  assert.equal(redactText(input), `? ${name}\n: ${REDACTED}\n`)
  const kept = secretsIn(input)
  assert.ok(kept.includes(VALUE), 'the value is remembered')
  assert.ok(!kept.includes(TAIL), 'the name tail is not remembered')
})

test('#530: an unclosed quoted name hides and remembers the colon-line value', () => {
  const input = `? "${PASS}:${TAIL}\n: ${VALUE}\n`
  assert.equal(redactText(input), `? "${PASS}:${REDACTED}\n: ${REDACTED}\n`)
  assert.ok(secretsIn(input).includes(VALUE), 'the value is remembered')
})

// 6. #530's remaining fallback: the quoted name does not close, and a quote of the other kind
// sits inside it, so the old no-space scan stopped before the credential and served the tail. It
// is hidden to the line end now and remembered; the single-quote twin, an apostrophe after the
// colon and one inside the value are the same rule.
const APOSTROPHE = [
  ['an apostrophe in a double-quoted name', `? "user's_${PASS}:${VALUE}\n`, `? "user's_${PASS}:${REDACTED}\n`, VALUE],
  ['an apostrophe mid-name', `? "pass'word:${VALUE}\n`, `? "pass'word:${REDACTED}\n`, VALUE],
  ['a double quote in a single-quoted name', `? 'user"s_${PASS}:${VALUE}\n`, `? 'user"s_${PASS}:${REDACTED}\n`, VALUE],
  ['a double quote mid-name', `? 'pass"word:${VALUE}\n`, `? 'pass"word:${REDACTED}\n`, VALUE],
  ['an apostrophe after the colon', `? "${PASS}:'${VALUE}\n`, `? "${PASS}:${REDACTED}\n`, VALUE],
  ["an apostrophe inside the value", `? "${PASS}:ab'cd${VALUE}\n`, `? "${PASS}:${REDACTED}\n`, `ab'cd${VALUE}`],
]
for (const [what, input, want, remembered] of APOSTROPHE) {
  test(`#530: an unclosed quoted name with a mismatched quote hides its tail (${what})`, () => {
    assert.equal(redactText(input), want, 'the tail is hidden to the line end')
    const kept = secretsIn(input)
    assert.ok(kept.includes(remembered), 'the tail is remembered')
    assert.ok(!masked(later(remembered), kept).includes(remembered), 'a later bare occurrence is masked')
  })
}

test('#530: an over-bound name with an apostrophe still hides and remembers the tail', () => {
  const input = `? "${'x'.repeat(250)}user's_${PASS}:${VALUE}\n`
  const served = redactText(input)
  assert.ok(!served.includes(VALUE), 'the tail is hidden')
  assert.ok(served.startsWith(`? "${'x'.repeat(250)}user's_${PASS}:`), 'the name line is served as written')
  assert.ok(secretsIn(input).includes(VALUE), 'the tail is remembered')
})

test('#530: an apostrophe name with a colon line still hides the colon-line value', () => {
  const input = `? "user's_${PASS}\n: ${VALUE}\n`
  assert.equal(redactText(input), `? "user's_${PASS}\n: ${REDACTED}\n`)
  assert.ok(secretsIn(input).includes(VALUE), 'the value is remembered')
})

test('#530: a matching closing quote with no separator is still served whole', () => {
  const input = `? "${PASS}:${VALUE}"\n`
  assert.equal(redactText(input), input)
  assert.deepEqual(secretsIn(input), [])
})

// 7. An escaped quote inside a quoted name: a `\"` in double quotes and a doubled `''` in single
// quotes do not end the name, so the credential word after them still claims the value, in each
// #507 shape.
const ESCAPED = [
  ['an escaped quote mid-name', `"pa\\"${PASS}"`],
  ['escaped quotes around the word', `"my \\"${PASS}\\""`],
  ['a doubled single quote', `'it''s ${PASS}'`],
]
for (const [what, name] of ESCAPED) {
  const shapes = [
    ['no colon line', `? ${name}\n  ${INDENTED}\n`, [INDENTED]],
    ['a gap line', `? ${name}\n  ${INDENTED}\n: ${VALUE}\n`, [INDENTED, VALUE]],
    ['a map child', `? ${name}\n  staging: ${VALUE}\n`, [VALUE]],
  ]
  for (const [shape, input, values] of shapes) {
    test(`#507: an escaped quote in a quoted name still hides and remembers (${what}, ${shape})`, () => {
      const served = redactText(input)
      const kept = secretsIn(input)
      for (const value of values) {
        assert.ok(!served.includes(value), 'the value is hidden')
        assert.ok(kept.includes(value), 'the value is remembered')
      }
    })
  }
}

// 8. A list item under an explicit credential name: the item is the value, not its `- ` mark.
test('#507: a list item under an explicit credential name is remembered without its dash', () => {
  const input = `? ${PASS}\n  - ${INDENTED}\n`
  assert.ok(!redactText(input).includes(INDENTED), 'the item is hidden')
  const kept = secretsIn(input)
  assert.ok(kept.includes(INDENTED), 'the item is remembered')
  assert.ok(!masked(later(INDENTED), kept).includes(INDENTED), 'a later bare occurrence is masked')
})

// 9. A plain name of several words: the credential word is not the first one, so the rest of the
// `?` line after its props, minus a trailing comment, claims the value, in each #507 shape.
const WORDS = [
  ['a two-word name', `db ${PASS}`],
  ['an anchor and a two-word name', `&a db ${PASS}`],
  ['a two-word name and a comment', `db ${PASS} # note`],
]
for (const [what, name] of WORDS) {
  const shapes = [
    ['no colon line', `? ${name}\n  ${INDENTED}\n`, [INDENTED]],
    ['a gap line', `? ${name}\n  ${INDENTED}\n: ${VALUE}\n`, [INDENTED, VALUE]],
    ['a map child', `? ${name}\n  staging: ${VALUE}\n`, [VALUE]],
  ]
  for (const [shape, input, values] of shapes) {
    test(`#507: a plain multi-word credential name hides and remembers (${what}, ${shape})`, () => {
      const served = redactText(input)
      const kept = secretsIn(input)
      for (const value of values) {
        assert.ok(!served.includes(value), 'the value is hidden')
        assert.ok(kept.includes(value), 'the value is remembered')
        assert.ok(!masked(later(value), kept).includes(value), 'a later bare occurrence is masked')
      }
    })
  }
}

test('#507: a plain multi-word non-credential name still serves its indented line', () => {
  const input = '? db host\n  north\n'
  assert.equal(redactText(input), input)
  assert.deepEqual(secretsIn(input), [])
})

// 10. #530: a plain name of several words whose credential word comes first, with its value on
// the `:` line right under it and nothing in the gap: the value is hidden and remembered.
const FIRST = ['password db', 'api_key prod', 'token for ci']
for (const name of FIRST) {
  test(`#530: a multi-word name with the credential word first hides and remembers its ':' line (${name})`, () => {
    const input = `? ${name}\n: ${VALUE}\n`
    const served = redactText(input)
    const kept = secretsIn(input)
    assert.ok(!served.includes(VALUE), 'the value is hidden')
    assert.ok(served.includes(`? ${name}\n`), 'the question line is kept')
    assert.ok(kept.includes(VALUE), 'the value is remembered')
    assert.ok(!masked(later(VALUE), kept).includes(VALUE), 'a later bare occurrence is masked')
  })
}

test('#530: a plain multi-word non-credential name still serves its colon line', () => {
  const input = '? db host\n: 5432\n'
  assert.equal(redactText(input), input)
  assert.deepEqual(secretsIn(input), [])
})
