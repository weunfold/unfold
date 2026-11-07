// #509: with a comment between an explicit key's quoted name and its `:` line, the plain
// `name:value` rules read a colon inside the quoted name as a separator, so the name's tail was
// masked although nothing was served. The plain name rules (and the gap rules) now take the first
// quoted token of an explicit key as one while its quote closes on its line: the name is served
// whole and the value on the `:` line is still hidden. Settings' redaction has no `secretsIn`;
// these rows are the text it serves. A, B and the empty-colon twin (the #500 rows) are unchanged.
// Rows are built from pieces, so no credential-shaped text is committed.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { REDACTED, redactText } from '../src/redact.js'

const PASS = ['pass', 'word'].join('')
const KEY = ['api', '_key'].join('')
const VALUE = ['Kx7', 'plumTree'].join('')
const LEAK = ['hunter', '42x'].join('')
const OWNER = ['Owner', '4Ada'].join('')
const TAIL = ['pri', 'mary'].join('') // six characters, over the four-character remember minimum
const SHORT = ['d', 'b'].join('') // two characters, under it
const SHY = `${PASS.slice(0, 4)}\u00ad${PASS.slice(4)}` // an invisible character inside the name

// 1. The Fixer repro, in every name form: the quoted name stays whole and the value on the `:`
// line is hidden.
const NAMES = [
  ['a double-quoted name', `? "${PASS}:${TAIL}" # note\n: ${VALUE}\n`, `? "${PASS}:${TAIL}" # note\n: ${REDACTED}\n`],
  ['a single-quoted name', `? '${KEY}:${TAIL}' # note\n: ${VALUE}\n`, `? '${KEY}:${TAIL}' # note\n: ${REDACTED}\n`],
  ['a name after an anchor and a tag', `? &a !!str "${PASS}:${TAIL}" # note\n: ${VALUE}\n`, `? &a !!str "${PASS}:${TAIL}" # note\n: ${REDACTED}\n`],
  ['a list item', `- ? "${PASS}:${TAIL}" # note\n  : ${VALUE}\n`, `- ? "${PASS}:${TAIL}" # note\n  : ${REDACTED}\n`],
  ['an invisible character in the name', `? "${SHY}:${TAIL}" # note\n: ${VALUE}\n`, `? "${PASS}:${TAIL}" # note\n: ${REDACTED}\n`],
]
for (const [what, input, want] of NAMES) {
  test(`#509: a comment gap keeps a quoted explicit name whole (${what})`, () => {
    assert.equal(redactText(input), want, 'the name is served whole and the value hidden')
  })
}

// 2. The boundaries: a two-character tail (under the remember minimum) is still part of the name
// and never read as a value, and an unquoted name has no tail to lose.
const BOUNDARIES = [
  ['a two-character tail', `? "${PASS}:${SHORT}" # note\n: ${VALUE}\n`, `? "${PASS}:${SHORT}" # note\n: ${REDACTED}\n`],
  ['an unquoted name', `? ${PASS} # note\n: ${VALUE}\n`, `? ${PASS} # note\n: ${REDACTED}\n`],
]
for (const [what, input, want] of BOUNDARIES) {
  test(`#509: a comment gap keeps the name whole at the boundary (${what})`, () => {
    assert.equal(redactText(input), want, 'the name is served as written and the value hidden')
  })
}

// 3. Every gap kind: no gap (the value on the name's line), a blank line and a comment line. The
// name stays whole in all of them.
const GAPS = [
  ['no gap', `? "${PASS}:${TAIL}": ${VALUE}\n`],
  ['a blank line', `? "${PASS}:${TAIL}"\n\n: ${VALUE}\n`],
  ['a comment line', `? "${PASS}:${TAIL}"\n# owner: ${OWNER}\n: ${VALUE}\n`],
]
for (const [what, input] of GAPS) {
  test(`#509: a quoted explicit name is taken whole with ${what}`, () => {
    const served = redactText(input)
    assert.ok(served.includes(`"${PASS}:${TAIL}"`), 'the name is served whole')
    assert.ok(!served.includes(VALUE), 'the value is hidden')
  })
}

// 4. A, its unquoted twin, B and the empty-colon twin (#500) still hold with a quoted colon name:
// a credential inside the trailing comment is masked, the name stays whole, and a comment line
// before the value is served.
test('#509: a credential in the trailing comment is still masked', () => {
  const input = `? "${PASS}:${TAIL}" # old ${PASS}: ${LEAK}\n: ${VALUE}\n`
  assert.equal(redactText(input), `? "${PASS}:${TAIL}" # old ${PASS}: ${REDACTED}\n: ${REDACTED}\n`)
})

test('#509: the unquoted A twin is unchanged', () => {
  const input = `? ${KEY} # old ${KEY}: ${LEAK}\n: ${VALUE}\n`
  assert.equal(redactText(input), `? ${KEY} # old ${KEY}: ${REDACTED}\n: ${REDACTED}\n`)
})

const B_ROWS = [
  ['a block', `? db_${PASS}\n# owner: ${OWNER}\n: |\n  ${VALUE}\n`],
  ['an empty colon', `? db_${PASS}\n# owner: ${OWNER}\n: ${VALUE}\n`],
]
for (const [what, input] of B_ROWS) {
  test(`#509: a comment line before the value is served (${what})`, () => {
    const served = redactText(input)
    assert.ok(served.includes(`# owner: ${OWNER}`), 'the comment line is served as written')
    assert.ok(!served.includes(VALUE), 'the real value is hidden')
  })
}

// 5. The guard's edges over-hide, they never serve: an unquoted explicit name with a colon in it is
// one token (the colon is not a separator), a `? ` line's long name (a colon far from the stem, or
// none) is taken whole by a fail-closed branch, and a quote that closes past the bound, or not at
// all, still hides the value on the `:` line, inline or under a block indicator. Before this fix
// all of them served.
const EDGES = [
  ['an unquoted name with a colon', `? ${PASS}:${TAIL}\n: ${VALUE}\n`, `? ${PASS}:${TAIL}\n: ${REDACTED}\n`],
  ['an unquoted name with a colon, then a comment', `? ${PASS}:${TAIL} # note\n: ${VALUE}\n`, `? ${PASS}:${TAIL} # note\n: ${REDACTED}\n`],
  ['an unquoted name with a colon after an anchor and a tag', `? &a !!str ${PASS}:${TAIL} # note\n: ${VALUE}\n`, `? &a !!str ${PASS}:${TAIL} # note\n: ${REDACTED}\n`],
  ['an unquoted name with a colon in a list item', `- ? ${PASS}:${TAIL} # note\n  : ${VALUE}\n`, `- ? ${PASS}:${TAIL} # note\n  : ${REDACTED}\n`],
  ['an unquoted name with a colon, block', `? ${PASS}:${TAIL}\n: |\n  ${VALUE}\n`, `? ${PASS}:${TAIL}\n: |\n  ${REDACTED}`],
  ['a quote past the bound', `? "${'x'.repeat(250)}${PASS}:${TAIL}"\n: ${VALUE}\n`, `? "${'x'.repeat(250)}${PASS}:${TAIL}"\n: ${REDACTED}\n`],
  ['a quote past the bound, block', `? "${'x'.repeat(250)}${PASS}:${TAIL}"\n: |\n  ${VALUE}\n`, `? "${'x'.repeat(250)}${PASS}:${TAIL}"\n: |\n  ${REDACTED}`],
  ['a colon 16 after the stem', `? ${PASS}${'x'.repeat(15)}:${TAIL}\n: ${VALUE}\n`, `? ${PASS}${'x'.repeat(15)}:${TAIL}\n: ${REDACTED}\n`],
  ['a colon 16 after the stem, block', `? ${PASS}${'x'.repeat(15)}:${TAIL}\n: |\n  ${VALUE}\n`, `? ${PASS}${'x'.repeat(15)}:${TAIL}\n: |\n  ${REDACTED}`],
  ['a colon 17 after the stem', `? ${PASS}${'x'.repeat(16)}:${TAIL}\n: ${VALUE}\n`, `? ${PASS}${'x'.repeat(16)}:${TAIL}\n: ${REDACTED}\n`],
  ['a colon 17 after the stem, block', `? ${PASS}${'x'.repeat(16)}:${TAIL}\n: |\n  ${VALUE}\n`, `? ${PASS}${'x'.repeat(16)}:${TAIL}\n: |\n  ${REDACTED}`],
  ['a colon 40 after the stem', `? ${PASS}${'x'.repeat(39)}:${TAIL}\n: ${VALUE}\n`, `? ${PASS}${'x'.repeat(39)}:${TAIL}\n: ${REDACTED}\n`],
  ['a colon 40 after the stem, block', `? ${PASS}${'x'.repeat(39)}:${TAIL}\n: |\n  ${VALUE}\n`, `? ${PASS}${'x'.repeat(39)}:${TAIL}\n: |\n  ${REDACTED}`],
  ['a long plain name', `? db_${PASS}_production_${TAIL}\n: ${VALUE}\n`, `? db_${PASS}_production_${TAIL}\n: ${REDACTED}\n`],
  ['a long plain name, block', `? db_${PASS}_production_${TAIL}\n: |\n  ${VALUE}\n`, `? db_${PASS}_production_${TAIL}\n: |\n  ${REDACTED}`],
  ['an unclosed quote past 4096', `? "${PASS}${'x'.repeat(5000)}\n: ${VALUE}\n`, `? "${PASS}${'x'.repeat(5000)}\n: ${REDACTED}\n`],
  ['an unclosed quote past 4096, block', `? "${PASS}${'x'.repeat(5000)}\n: |\n  ${VALUE}\n`, `? "${PASS}${'x'.repeat(5000)}\n: |\n  ${REDACTED}`],
  ['41 blanks after ? then a long name', `? ${' '.repeat(41)}${PASS}${'x'.repeat(17)}\n: ${VALUE}\n`, `? ${' '.repeat(41)}${PASS}${'x'.repeat(17)}\n: ${REDACTED}\n`],
  ['41 blanks after ? then a long name, block', `? ${' '.repeat(41)}${PASS}${'x'.repeat(17)}\n: |\n  ${VALUE}\n`, `? ${' '.repeat(41)}${PASS}${'x'.repeat(17)}\n: |\n  ${REDACTED}`],
  ['an unclosed quote with an inline value', `? "${PASS}: ${VALUE}\n`, `? "${PASS}: ${REDACTED}\n`],
]
for (const [what, input, want] of EDGES) {
  test(`#509: the guard's edge over-hides, never serves (${what})`, () => {
    assert.equal(redactText(input), want, 'the name is served as written and the value hidden')
  })
}

for (const [what, valueLine] of [['an empty colon', `: ${VALUE}\n`], ['a block', `: |\n  ${VALUE}\n`]]) {
  test(`#509: an unclosed quote never serves the value (${what})`, () => {
    const served = redactText(`? "${PASS}:${TAIL} # note\n${valueLine}`)
    assert.ok(!served.includes(VALUE), 'the value on the `:` line is hidden')
  })
}

test('#509: a mid-line ? is not an explicit key', () => {
  const input = `What? ${PASS}:${LEAK}\n: note\n`
  assert.equal(redactText(input), `What? ${PASS}:${REDACTED}\n: note\n`)
})

test('#509: an unclosed no-space colon tail is masked', () => {
  for (const [what, input, want] of [
    ['double quoted', `? "${PASS}:${LEAK}\n`, `? "${PASS}:${REDACTED}\n`],
    ['single quoted', `? '${PASS}:${LEAK}\n`, `? '${PASS}:${REDACTED}\n`],
    ['a 5000-char tail', `? "${PASS}:${'y'.repeat(5000)}${LEAK}\n`, `? "${PASS}:${REDACTED}\n`],
    ['a 5000-char glued name', `? "${PASS}${'x'.repeat(5000)}:${VALUE}\n`, `? "${PASS}${'x'.repeat(5000)}:${REDACTED}\n`],
    ['a quote after the colon', `? "${PASS}:${LEAK}"x\n`, `? "${PASS}:${REDACTED}\n`],
    ['an apostrophe after the colon', `? "${PASS}:${LEAK} 'x\n`, `? "${PASS}:${REDACTED}\n`],
    ['an apostrophe in the value', `? "${PASS}:hunt'er2\n`, `? "${PASS}:${REDACTED}\n`],
    ['a value beginning with an apostrophe', `? "${PASS}:'${VALUE}\n`, `? "${PASS}:${REDACTED}\n`],
    ['a value beginning with a double quote', `? "${PASS}:"${VALUE}\n`, `? "${PASS}:${REDACTED}\n`],
    ['a value with doubled apostrophes', `? "${PASS}:abc''def\n`, `? "${PASS}:${REDACTED}\n`],
    ['a single-quoted twin with a double quote', `? '${PASS}:${LEAK} "x\n`, `? '${PASS}:${REDACTED}\n`],
    ['a trailing comment apostrophe', `? "${PASS}:${LEAK} # c'mon\n`, `? "${PASS}:${REDACTED}\n`],
    ['a mismatched closing quote', `? "${PASS}:${LEAK}'\n`, `? "${PASS}:${REDACTED}\n`],
    ['a mismatched closing quote before a comment', `? "${PASS}:${LEAK} ' # x\n`, `? "${PASS}:${REDACTED}\n`],
    ['a mismatched closing double quote', `? '${PASS}:${LEAK}"\n`, `? '${PASS}:${REDACTED}\n`],
  ]) {
    assert.equal(redactText(input), want, what)
  }
})

test('#509: a matching closing quote with no separator is served whole', () => {
  const input = `? "${PASS}:${LEAK}"\n`
  assert.equal(redactText(input), input)
})


test('#509: a non-credential quoted name is not over-hidden', () => {
  assert.equal(redactText('? "compass:north\n'), '? "compass:north\n')
})
