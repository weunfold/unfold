// #507 and #530: the explicit-key fallbacks, written from the issues alone. A `? name` line with
// no `:` line still has an indented scalar under it (#507a), an indented non-comment line in the
// gap before the `:` line is a value too (#507b), and a map child under the name carries a value
// (#507c). #530's remaining fallback is the unclosed quoted name with a quote of the other kind
// inside it: the tail after the colon used to be served and remembered as nothing. Every row
// asserts the served text never carries the value, that secretsIn remembers it, and that a later
// bare occurrence is masked. Tokens are joined pieces, so no credential-shaped literal is
// committed.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { REDACTED, redactText, secretsIn } from '../src/redact.js'

const PASS = ['pass', 'word'].join('')
const TAIL = ['pri', 'mary'].join('') // six characters, over the remember minimum
const VALUE = ['N7', 'quartz', 'Finch'].join('')
const GAP = ['sta', 'ging'].join('') // the indented non-comment line before the `:` line
const CAMEL = ['access', 'Token'].join('')
const SNAKE = ['db_', PASS].join('')
const INVISIBLE = `${PASS.slice(0, 4)}\u00ad${PASS.slice(4)}`
const APOS = `${['user', "'", 's_'].join('')}${PASS}` // user's_password
const TWIN = `${['user', '"', 's_'].join('')}${PASS}` // user"s_password
const APOS_VALUE = ['hunt', "'", 'er2'].join('')
const APOS_TAIL = [VALUE, "'", 'x'].join('')
const XS = 'x'.repeat(250)

const later = (text, kept) => kept.reduce((out, v) => out.split(v).join(REDACTED), text)

// The rule every row shares: the served text never carries the value, the value is remembered, and
// a later bare occurrence is masked. Returns the served text and the remembered set for the
// group-specific assertions.
function withheld(input, value) {
  const served = redactText(input)
  assert.ok(!served.includes(value), 'the value is not served')
  const kept = secretsIn(input)
  assert.ok(kept.includes(value), 'the value is remembered')
  assert.ok(!later(`later note: ${value}`, kept).includes(value), 'a later bare occurrence is masked')
  return { served, kept }
}

// A. #507(a): the `? name` line has no `:` line; the indented scalar under it is its value. The
// value is hidden and remembered in every name form, and the `?` line stays.
const A_ROWS = [
  ['a double-quoted name carrying the colon', `? "${PASS}:${TAIL}"\n  ${VALUE}\n`, `? "${PASS}:${TAIL}"`],
  ['an unquoted name carrying the colon', `? ${PASS}:${TAIL}\n  ${VALUE}\n`, `? ${PASS}:`],
  ['a camelCase name', `? ${CAMEL}\n  ${VALUE}\n`, `? ${CAMEL}`],
  ['a list item', `- ? "${PASS}"\n  ${VALUE}\n`, `- ? "${PASS}"`],
  ['an anchored and tagged name', `? &a !!str "${PASS}"\n  ${VALUE}\n`, `? &a !!str "${PASS}"`],
  ['an invisible character inside the name', `? "${INVISIBLE}"\n  ${VALUE}\n`, `? "${PASS}"`],
]
for (const [what, input, head] of A_ROWS) {
  test(`#507(a): no colon line hides the indented scalar (${what})`, () => {
    const { served } = withheld(input, VALUE)
    assert.ok(served.split('\n')[0].startsWith(head), 'the `?` line is served')
    assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the indented scalar is hidden')
  })
}

// B. #507(b): an indented non-comment line sits in the gap before the `:` line. That gap line and
// the real value on the `:` line are hidden and remembered; the `?` line and a block indicator
// stay.
const B_INLINE = [
  ['a double-quoted name', `? "${PASS}"\n  ${GAP}\n: ${VALUE}\n`, `? "${PASS}"`, `: ${REDACTED}`],
  ['an unquoted name', `? ${SNAKE}\n  ${GAP}\n: ${VALUE}\n`, `? ${SNAKE}`, `: ${REDACTED}`],
  ['a camelCase name', `? ${CAMEL}\n  ${GAP}\n: ${VALUE}\n`, `? ${CAMEL}`, `: ${REDACTED}`],
  ['a list item', `- ? "${PASS}"\n  ${GAP}\n  : ${VALUE}\n`, `- ? "${PASS}"`, `  : ${REDACTED}`],
]
for (const [what, input, head, colon] of B_INLINE) {
  test(`#507(b): a gap line before an inline colon value is hidden (${what})`, () => {
    const { served, kept } = withheld(input, VALUE)
    assert.ok(!served.includes(GAP), 'the gap line is hidden')
    assert.ok(kept.includes(GAP), 'the gap line is remembered')
    assert.ok(!later(`later note: ${GAP}`, kept).includes(GAP), 'a later bare gap line is masked')
    assert.equal(served.split('\n')[0], head, 'the `?` line stays')
    assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the gap line is hidden in place')
    assert.equal(served.split('\n')[2], colon, 'the real value on the `:` line is hidden')
  })
}

test('#507(b): a gap line before a block value is hidden, the indicator stays', () => {
  const input = `? "${PASS}"\n  ${GAP}\n: |\n  ${VALUE}\n`
  const { served, kept } = withheld(input, VALUE)
  assert.ok(!served.includes(GAP), 'the gap line is hidden')
  assert.ok(kept.includes(GAP), 'the gap line is remembered')
  assert.equal(served.split('\n')[0], `? "${PASS}"`, 'the `?` line stays')
  assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the gap line is hidden in place')
  assert.equal(served.split('\n')[2], ': |', 'the block indicator stays')
  assert.equal(served.split('\n')[3], `  ${REDACTED}`, 'the block value is hidden')
})

test('#507(b): a gap line before an empty colon is hidden, the colon stays', () => {
  const input = `? "${PASS}"\n  ${GAP}\n:\n  ${VALUE}\n`
  const { served, kept } = withheld(input, VALUE)
  assert.ok(!served.includes(GAP), 'the gap line is hidden')
  assert.ok(kept.includes(GAP), 'the gap line is remembered')
  assert.equal(served.split('\n')[0], `? "${PASS}"`, 'the `?` line stays')
  assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the gap line is hidden in place')
  assert.equal(served.split('\n')[2], ':', 'the empty colon stays')
  assert.equal(served.split('\n')[3], `  ${REDACTED}`, 'the value under the empty colon is hidden')
})

// C. #507(c): a map child under the name. The child's value is hidden and remembered; the line
// label stays under policy B; only its leaf is hidden.
const C_ROWS = [
  ['a double-quoted name', `? "${PASS}"\n  ${GAP}: ${VALUE}\n`, `? "${PASS}"`],
  ['an unquoted name', `? ${SNAKE}\n  ${GAP}: ${VALUE}\n`, `? ${SNAKE}`],
  ['a camelCase name', `? ${CAMEL}\n  ${GAP}: ${VALUE}\n`, `? ${CAMEL}`],
]
for (const [what, input, head] of C_ROWS) {
  test(`#507(c): a map child under the name hides its value (${what})`, () => {
    const { served } = withheld(input, VALUE)
    assert.ok(served.includes(`${GAP}:`), 'policy B keeps the child label')
    assert.equal(served.split('\n')[0], head, 'the `?` line stays')
    assert.equal(served.split('\n')[1], `  ${GAP}: ${REDACTED}`, 'the child leaf is hidden in place')
  })
}

// D. #530: an unclosed quoted name whose name holds a quote of the other kind. The scan used to
// stop at that inner quote, so the tail after the colon was served and remembered as nothing. The
// tail is now hidden to the line end and remembered whole.
const D_ROWS = [
  ['a double-quoted name with an apostrophe', `? "${APOS}:${VALUE}\n`, `? "${APOS}:`, VALUE],
  ['a single-quoted twin with a double quote', `? '${TWIN}:${VALUE}\n`, `? '${TWIN}:`, VALUE],
  ['an apostrophe after the colon', `? "${APOS}:${APOS_TAIL}\n`, `? "${APOS}:`, APOS_TAIL],
  ['an apostrophe inside the value', `? "${APOS}:${APOS_VALUE}\n`, `? "${APOS}:`, APOS_VALUE],
  ['an over-bound name with an apostrophe', `? "${XS}${APOS}:${APOS_VALUE}\n`, `? "${XS}${APOS}:`, APOS_VALUE],
]
for (const [what, input, head, value] of D_ROWS) {
  test(`#530: an unclosed name hides its tail to the line end (${what})`, () => {
    const { served } = withheld(input, value)
    assert.ok(served.startsWith(head), 'the name is served as written')
    assert.ok(served.trimEnd().endsWith(REDACTED), 'the tail is hidden to the line end')
  })
}

// E. #530's listed shapes, pinned: they already fail closed on the base this branch starts from.
// The value on the `:` line is hidden and remembered, and the name's tail is never remembered.
const E_ROWS = [
  ['an unquoted name with a colon', `? ${PASS}:${TAIL}\n: ${VALUE}\n`, `? ${PASS}:${TAIL}`],
  ['an over-bound quoted name', `? "${XS}${PASS}:${TAIL}"\n: ${VALUE}\n`, `? "${XS}${PASS}:${TAIL}"`],
  ['an unclosed quoted name', `? "${PASS}:${TAIL}\n: ${VALUE}\n`, `? "${PASS}:`],
]
for (const [what, input, head] of E_ROWS) {
  test(`#530: the listed fallback hides the colon-line value (${what})`, () => {
    const { served, kept } = withheld(input, VALUE)
    assert.ok(served.split('\n')[0].startsWith(head), 'the name line is served')
    assert.equal(served.split('\n')[1], `: ${REDACTED}`, 'the colon-line value is hidden')
    assert.ok(!kept.includes(TAIL), 'the name tail is not remembered')
    assert.ok(later(`later note: ${TAIL}`, kept).includes(TAIL), 'a later bare name tail is served')
  })
}

// F. Escaped quotes inside a quoted name (the Reviewer's Medium on #507): the name used to end at
// the first quote character, even an escaped one, so `? "pa\"password"` was read as `pa\`, not a
// credential name, and the three #507 shapes served their values. A double-quoted name ends at its
// real closing quote, a single-quoted one at its real doubled `''`, and with no real closing quote
// on the line the rest of the line is the name, so the no-close rows still fail closed.
const ESC_Q = ['\\', '"'].join('') // a backslash-escaped quote inside a double-quoted name
const ESC_A = ['pa', ESC_Q, PASS].join('') // pa\"password
const ESC_B = ['my ', ESC_Q, PASS, ESC_Q].join('') // my \"password\"
const ESC_C = ["it''s ", PASS].join('') // it''s password
const ESC_NAMES = [
  ['a backslash-escaped quote inside a double-quoted name', `? "${ESC_A}"`],
  ['the word between escaped quotes', `? "${ESC_B}"`],
  ['a doubled quote inside a single-quoted name', `? '${ESC_C}'`],
]
const ESC_TWINS = [
  ['a block indicator', ': |'],
  ['an empty colon', ':'],
]

for (const [what, head] of ESC_NAMES) {
  test(`#507(a): an escaped-quote name hides the indented scalar (${what})`, () => {
    const { served } = withheld(`${head}\n  ${VALUE}\n`, VALUE)
    assert.equal(served.split('\n')[0], head, 'the `?` line stays')
    assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the indented scalar is hidden')
  })

  test(`#507(b): an escaped-quote name hides a gap line and its inline value (${what})`, () => {
    const { served, kept } = withheld(`${head}\n  ${GAP}\n: ${VALUE}\n`, VALUE)
    assert.ok(!served.includes(GAP), 'the gap line is hidden')
    assert.ok(kept.includes(GAP), 'the gap line is remembered')
    assert.ok(!later(`later note: ${GAP}`, kept).includes(GAP), 'a later bare gap line is masked')
    assert.equal(served.split('\n')[0], head, 'the `?` line stays')
    assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the gap line is hidden in place')
    assert.equal(served.split('\n')[2], `: ${REDACTED}`, 'the inline value is hidden')
  })

  for (const [twin, colon] of ESC_TWINS) {
    test(`#507(b): an escaped-quote name hides a value under ${twin} (${what})`, () => {
      const { served, kept } = withheld(`${head}\n  ${GAP}\n${colon}\n  ${VALUE}\n`, VALUE)
      assert.ok(!served.includes(GAP), 'the gap line is hidden')
      assert.ok(kept.includes(GAP), 'the gap line is remembered')
      assert.equal(served.split('\n')[0], head, 'the `?` line stays')
      assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the gap line is hidden in place')
      assert.equal(served.split('\n')[2], colon, `the ${twin} stays`)
      assert.equal(served.split('\n')[3], `  ${REDACTED}`, 'the value under it is hidden')
    })
  }

  test(`#507(c): an escaped-quote name hides a map child value (${what})`, () => {
    const { served } = withheld(`${head}\n  ${GAP}: ${VALUE}\n`, VALUE)
    assert.ok(served.includes(`${GAP}:`), 'policy B keeps the child label')
    assert.equal(served.split('\n')[0], head, 'the `?` line stays')
    assert.equal(served.split('\n')[1], `  ${GAP}: ${REDACTED}`, 'the child leaf is hidden in place')
  })
}

// No real closing quote after the escaped one: the rest of the line is the name, so the indented
// scalar and the value on the `:` line are still hidden and remembered (fail closed).
const ESC_OPEN = `? "${ESC_A}`
test('#507(a): an escaped name with no real closing quote hides the indented scalar', () => {
  const { served } = withheld(`${ESC_OPEN}\n  ${VALUE}\n`, VALUE)
  assert.equal(served.split('\n')[0], ESC_OPEN, 'the `?` line stays')
  assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the indented scalar is hidden')
})

test('#507(b): an escaped name with no real closing quote hides the colon-line value', () => {
  const { served, kept } = withheld(`${ESC_OPEN}\n  ${GAP}\n: ${VALUE}\n`, VALUE)
  assert.ok(!served.includes(GAP), 'the gap line is hidden')
  assert.ok(kept.includes(GAP), 'the gap line is remembered')
  assert.equal(served.split('\n')[0], ESC_OPEN, 'the `?` line stays')
  assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the gap line is hidden in place')
  assert.equal(served.split('\n')[2], `: ${REDACTED}`, 'the colon-line value is hidden')
})

// G. L2 list-item memory pin: a list item under an explicit key is remembered as the item, not as
// its `- ` mark. Hiding the line was never enough: a later bare occurrence has to be masked too.
test('#507: a list item under an explicit key remembers the item, not its dash', () => {
  const { served, kept } = withheld(`? ${PASS}\n  - ${VALUE}\n`, VALUE)
  assert.ok(kept.includes(VALUE), 'the item is remembered as the item')
  assert.ok(!kept.includes('-'), 'the dash is not the remembered value')
  assert.equal(served.split('\n')[0], `? ${PASS}`, 'the `?` line stays')
  assert.equal(served.split('\n')[1], `  - ${REDACTED}`, 'the list marker stays and its value is hidden')
})

// H. #530's credential-first plain multi-word name, the Reviewer's Medium on #507: the name has
// several words and the credential word leads (`? password db`), with its value on the `:` line
// right under it and nothing in the gap. The gap rules read the `?` line as one token ending its
// line, so the `:` line has to be taken by the explicit-key scan itself. The three names, then the
// trailing comment, block indicator, empty colon and list item forms. Every row is hidden,
// remembered, and masked in a later bare copy.
const FIRST_NAMED = [['password db', 'password'], ['api_key prod', 'api_key'], ['token for ci', 'token']]
for (const [name, credential] of FIRST_NAMED) {
  test(`#530: a credential-first multi-word name hides its colon line (${name})`, () => {
    const { served } = withheld(`? ${name}\n: ${VALUE}\n`, VALUE)
    assert.equal(served.split('\n')[0], `? ${credential} ${REDACTED}`, 'the credential stays and its question-line tail is hidden')
    assert.equal(served.split('\n')[1], `: ${REDACTED}`, 'the colon-line value is hidden')
  })
}

test('#530: a comment after a credential-first multi-word name hides its colon line', () => {
  const { served } = withheld(`? password db # note\n: ${VALUE}\n`, VALUE)
  assert.equal(served.split('\n')[0], `? password ${REDACTED}`, 'the credential stays and its question-line tail is hidden')
  assert.equal(served.split('\n')[1], `: ${REDACTED}`, 'the colon-line value is hidden')
})

test('#530: a block indicator under a credential-first multi-word name hides its block', () => {
  const { served } = withheld(`? password db\n: |\n  ${VALUE}\n`, VALUE)
  assert.equal(served.split('\n')[0], `? password ${REDACTED}`, 'the credential stays and its question-line tail is hidden')
  assert.equal(served.split('\n')[1], ': |', 'the block indicator stays')
  assert.equal(served.split('\n')[2], `  ${REDACTED}`, 'the block value is hidden')
})

test('#530: an empty colon under a credential-first multi-word name hides its value', () => {
  const { served } = withheld(`? password db\n:\n  ${VALUE}\n`, VALUE)
  assert.equal(served.split('\n')[0], `? password ${REDACTED}`, 'the credential stays and its question-line tail is hidden')
  assert.equal(served.split('\n')[1], ':', 'the empty colon stays')
  assert.equal(served.split('\n')[2], `  ${REDACTED}`, 'the indented value is hidden')
})

test('#530: a list item with a credential-first multi-word name hides its colon line', () => {
  const { served } = withheld(`- ? password db\n  : ${VALUE}\n`, VALUE)
  assert.equal(served.split('\n')[0], `- ? password ${REDACTED}`, 'the list-item credential stays and its tail is hidden')
  assert.equal(served.split('\n')[1], `  : ${REDACTED}`, 'the colon-line value is hidden')
})

// I. The other side of the same form: a plain name of several words whose credential word comes
// later (`? db password`), across the three #507 shapes. The comment and anchor variants ride the
// `?` line, so every shape is rowed with each. Every row is hidden and remembered.
const LATER_NAMED = [
  ['a plain name', 'db password', '? db password'],
  ['a trailing comment', 'db password # note', `? db password ${REDACTED}`],
  ['an anchor', '&a db password', '? &a db password'],
]
for (const [what, name, head] of LATER_NAMED) {
  test(`#507(a): no colon line hides the indented scalar (${what})`, () => {
    const { served } = withheld(`? ${name}\n  ${VALUE}\n`, VALUE)
    assert.equal(served.split('\n')[0], head, 'the credential context stays and its tail is hidden')
    assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the indented scalar is hidden')
  })

  test(`#507(b): a gap line and the colon-line value are hidden (${what})`, () => {
    const { served, kept } = withheld(`? ${name}\n  ${GAP}\n: ${VALUE}\n`, VALUE)
    assert.ok(!served.includes(GAP), 'the gap line is hidden')
    assert.ok(kept.includes(GAP), 'the gap line is remembered')
    assert.equal(served.split('\n')[0], head, 'the credential context stays and its tail is hidden')
    assert.equal(served.split('\n')[1], `  ${REDACTED}`, 'the gap line is hidden in place')
    assert.equal(served.split('\n')[2], `: ${REDACTED}`, 'the colon-line value is hidden')
  })

  test(`#507(c): a map child under the name hides its value (${what})`, () => {
    const { served } = withheld(`? ${name}\n  ${GAP}: ${VALUE}\n`, VALUE)
    assert.ok(served.includes(`${GAP}:`), 'policy B keeps the child label')
    assert.equal(served.split('\n')[0], head, 'the credential context stays and its tail is hidden')
    assert.equal(served.split('\n')[1], `  ${GAP}: ${REDACTED}`, 'the child leaf is hidden in place')
  })
}

// The control the fix is bounded by: `db host` names no credential in either word order, so its
// `:` line is served as written and nothing is remembered.
test('#507: a non-credential multi-word name serves its colon line unchanged', () => {
  const input = '? db host\n: 5432\n'
  assert.equal(redactText(input), input, 'the non-credential name is served unchanged')
  assert.deepEqual(secretsIn(input), [], 'nothing is remembered')
})
