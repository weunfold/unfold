// The #348 table, test/fixtures/redact-348.json: the same file, byte for byte, in unfold-events-dsh
// and unfold-settings-dsh (scripts/unfold-copies.test.ts), so the copies of the redaction cannot
// diverge on it. Invisible characters inside a credential name (NUL, zero-width, BOM, bidi marks,
// raw or as a JSON \u escape) and YAML anchors and tags before its value. A text row serves the
// shape of its plain form; an object row hides the value under a key read without its invisible
// characters and keeps the key as written; a near miss keeps its value.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { loadDefinitions } from '../src/definitions.js'
import { MindStream } from '../src/mind.js'
import { REDACTED, redact, redactText, secretsIn, nameText} from '../src/redact.js'

const TABLE = JSON.parse(readFileSync(new URL('./fixtures/redact-348.json', import.meta.url), 'utf8'))
const VALUE = /hunter2/
// The rows whose value the redaction hides.
const HIDDEN = TABLE.text.filter(([, input, want]) => VALUE.test(input) && !VALUE.test(want))
// A block scalar's key line: a name, its colon, up to two anchors or tags, then an indicator, a
// comment (after a space or glued on) and a CR.
const BLOCK = /:[ \t]*(?:[&!][^\s"'`]*[ \t]+){0,2}[|>][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*\r?$/m
// The rows whose anchor or tag has no value on its line: the next line's key is never the value.
const NEXT_LINE = TABLE.text.filter(([what]) => what.startsWith('next line:'))
// The invalid YAML rows (three or more anchors and tags): left as written, nothing remembered.
const INVALID = TABLE.text.filter(([what]) => what.startsWith('invalid YAML:'))

for (const [what, input, want] of TABLE.text) test(`text: ${what}`, () => assert.equal(redactText(input), want))
for (const [what, input, want] of TABLE.object) test(`object: ${what}`, () => assert.deepEqual(redact(input), want))

test('a hidden value is remembered as the value, not as an anchor, a tag or a name', () => {
  for (const [what, input] of HIDDEN) {
    assert.ok(!secretsIn(`${input} `).some((v) => /^[&!|>]/.test(v)), what)
    if (BLOCK.test(input)) continue // a block scalar's lines are not remembered, as for its plain form
    assert.ok(secretsIn(`${input} `).some((v) => VALUE.test(v)), what)
  }
})

test('an invalid YAML row is remembered as nothing', () => {
  for (const [what, input] of INVALID) assert.deepEqual(secretsIn(`${input} `), [], what)
})

function journal() {
  const mind = new MindStream({ now: () => 1 })
  mind.life = 'lifeZ'
  let seq = 0
  const send = (text) => mind.agentd({ v: 2, id: `g:${++seq}`, producer: 'g', seq, t_ns: seq, source: 'guest', kind: 'agent_delta', response_id: 'rK', text }, { via: 'agentd journal', file: '/j', offset: seq })
  mind.agentd({ v: 2, id: 'g:0', producer: 'g', seq: 0, t_ns: 0, source: 'guest', kind: 'response_started', response_id: 'rK' }, { via: 'agentd journal', file: '/j', offset: 0 })
  const served = () => [JSON.stringify(mind.snapshot()), ...mind.rows.map((x) => JSON.stringify(mind.detail(x.id, loadDefinitions())))].join('\n')
  return { send, served, row: () => mind.rows.find((x) => x.kind === 'agent_delta') }
}

test('served: every row the table hides stays hidden in the stream, whole and a character at a time', () => {
  for (const [what, input] of HIDDEN) {
    const whole = journal()
    whole.send(`the config has ${input}\nand goes on`)
    assert.doesNotMatch(whole.served(), VALUE, what)
    const split = journal()
    for (const ch of `the config has ${input}\nand goes on`) split.send(ch)
    assert.doesNotMatch(split.served(), VALUE, `${what}, a character at a time`)
  }
})

test('served: a value under an invisible name or an anchor that the 4096 cut sweeps through stays hidden', () => {
  for (const head of ['pass​word: ', 'pass\u0000word: ', 'password: &a ', 'password: !!str ', 'password: &a !!str ']) {
    const r = journal()
    r.send(`${head}"${'hunter2 '.repeat(800)}"`)
    r.send(' and goes on')
    assert.doesNotMatch(r.served(), VALUE, JSON.stringify(head))
  }
})

// A block scalar under an anchor or tag before its indicator, longer than the bound, streamed a line
// at a time: no line of it is served, and the text after it is.
const BLOCK_HEADS = ['password: &a |', 'password: !!str >-', 'password: !<tag:yaml.org,2002:str> |', 'dbPassword: &a !!str |+']
const BLOCK_LINES = Array.from({ length: 700 }, (_, i) => `    hunter2 line ${i}\n`)
test('served: a block scalar under an anchor or tag longer than the 4096 keep, a line at a time, stays hidden', () => {
  for (const head of BLOCK_HEADS) {
    const r = journal()
    r.send(`config:\n  ${head}\n`)
    for (const line of BLOCK_LINES) r.send(line)
    assert.doesNotMatch(r.served(), VALUE, head)
    r.send('next: shown')
    assert.match(r.served(), /next: shown/, head)
  }
})

test('an anchor or tag with no value on its line never makes the next line\'s key a remembered value', () => {
  for (const [what, input] of NEXT_LINE) {
    assert.deepEqual(secretsIn(`${input} `).filter((v) => /username|bob|^[&!]/.test(v)), [], what)
    const r = journal()
    r.send(`${input}\n`)
    r.send('later username: carol')
    assert.match(r.served(), /later username: carol/, what)
  }
})

// A key line, then a blank run longer than the 4096 keep before its indicator: the cut lands in the
// run, and the key line is kept, so the block's lines that arrive next are hidden under it.
test('served: a blank run past the 4096 keep between a key line and its block indicator keeps the block hidden', () => {
  for (const [what, head, indicator] of TABLE.blank_run) {
    const r = journal()
    r.send(`config:\n  ${head}${' '.repeat(17_000)}`)
    r.send(`${indicator}\n`)
    for (const line of BLOCK_LINES.slice(0, 5)) r.send(line)
    assert.doesNotMatch(r.served(), VALUE, what)
    r.send('next: shown')
    assert.match(r.served(), /next: shown/, what)
  }
})

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
const SECRET = FILL['{value}']
const CONTINUED = TABLE.continuation.map(([what, input, want]) => [what, fill(input), fill(want)])
const CONTINUED_HIDDEN = CONTINUED.filter(([, input, want]) => input.includes(SECRET) && !want.includes(SECRET))
// The rows that keep the value (a sibling, a child with no credential name, a comment line that
// names a key): it is served, and never remembered.
const KEPT = CONTINUED.filter(([, , want]) => want.includes(SECRET))
// A block indicator on a later line: its lines are not remembered, as under one on the key's line.
const LATER_BLOCK = /^[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}[|>][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*$/m
// The name a row's key line holds, served again after a cut.
const nameOf = (input) => (nameText(input).includes(FILL['{camel}']) ? FILL['{camel}'] : nameText(input).includes(FILL['{name}']) ? FILL['{name}'] : FILL['{parent}'])
// Where a cut can land before the value: the end of each line before it (a CRLF's CR), and right
// before the value. A blank run there keeps each line as it reads: a key line or a line under it
// gains trailing blanks, and a blank line stays blank.
const cutsOf = (input) => {
  const at = input.indexOf(SECRET)
  const cuts = [at]
  for (let i = 0; i < at; i++) if ((input[i] === '\r' || input[i] === '\n') && input[i - 1] !== '\r') cuts.push(i)
  return cuts
}

for (const [what, input, want] of CONTINUED) test(`continuation: ${what}`, () => assert.equal(redactText(input), want))

// A value on a later line is remembered once its line has ended (#406): a blank after it is not
// enough, as a key may hold one and a cut pads with them, so each row ends at a line break here.
test('continuation: a hidden value is remembered, and no anchor, tag, indicator or kept value is', () => {
  for (const [what, input] of CONTINUED_HIDDEN) {
    const kept = secretsIn(`${input}\n`)
    assert.ok(!kept.some((v) => /^[&!|>]/.test(v)), what)
    if (!LATER_BLOCK.test(input)) assert.ok(kept.some((v) => v.includes(SECRET)), what)
  }
  for (const [what, input] of KEPT) assert.ok(!secretsIn(`${input}\n`).some((v) => v.includes(SECRET)), what)
})

// #406 F-C: an explicit key's continued value is remembered where it is hidden, at the end of the
// text too, so a later bare occurrence of it is masked (these rows end at the value; the inline
// rows still need the line break that ends a value line, so a child key still arriving is never
// remembered as its parent's value). A block's first line is remembered for the explicit-key form.
const EXPLICIT = CONTINUED_HIDDEN.filter(([, input]) => /(?:^|\n)[ \t]*(?:-[ \t]+)?\?[ \t]/.test(input))
const masked = (text, kept) => kept.reduce((t, v) => t.split(v).join('[redacted]'), text)

test('continuation: an explicit key\'s continued value is remembered at the end of the text, so a later bare occurrence is masked', () => {
  assert.equal(EXPLICIT.length, 16, 'FA1-FA5, FL1, FB1 and the nine #493 explicit-key rows')
  for (const [what, input] of EXPLICIT) {
    const kept = secretsIn(input)
    assert.ok(kept.some((v) => v.includes(SECRET)), `${what}: remembered`)
    assert.ok(!masked(`later ${SECRET} stays`, kept).includes(SECRET), `${what}: a later bare occurrence is masked`)
    // FA4's value line is closed by the sibling after it; the others end at the value, still arriving.
    if (input.slice(input.lastIndexOf('\n') + 1).includes(SECRET)) assert.deepEqual(secretsIn(input, true), [], `${what}: a growing buffer remembers nothing while that line is open`)
  }
})

// #495: a trailing comment after an explicit key's name ends at its own line end, so the quoted
// block's first line is still hidden in place and remembered, and a later bare occurrence is masked.
// The comment is not part of the value's context: the plain spelling is the same rule, which base
// served with no memory at all (a deliberate widening here). The comment swallows nothing after its
// line, and the sibling key that follows the block is neither hidden nor remembered.
const COMMENT = ['Zq9', 'marker'].join('')
const NEXT_VALUE = ['Shown', '9value'].join('')
const COMMENTED = [
  ['an invisible name', `? "${FILL['{pass}']}\u00ad${FILL['{word}']}" # note\n: |\n  ${COMMENT}`],
  ['a plain name', `? "${FILL['{pass}']}${FILL['{word}']}" # note\n: |\n  ${COMMENT}`],
]
for (const [what, input] of COMMENTED) {
  test(`#495: a quoted explicit-block value under ${what} with a trailing comment is hidden and remembered`, () => {
    const name = `${FILL['{pass}']}${FILL['{word}']}`
    assert.equal(redactText(input), `? "${name}" # note\n: |\n  ${REDACTED}`, 'hidden in place')
    const kept = secretsIn(`${input}\n`)
    assert.ok(kept.includes(COMMENT), 'remembered')
    assert.ok(!masked(`later note: ${COMMENT}`, kept).includes(COMMENT), 'a later bare occurrence is masked')
  })
}

test('#495: a trailing comment swallows nothing after its line and makes no sibling remembered', () => {
  for (const [what, input] of COMMENTED) {
    const text = `${input}\nsibling: ${NEXT_VALUE}\n`
    assert.ok(redactText(text).endsWith(`\nsibling: ${NEXT_VALUE}\n`), `${what}: the next line is served as written`)
    assert.ok(!secretsIn(text).includes(NEXT_VALUE), `${what}: a sibling value is not remembered`)
  }
})

test('served: a value on a later line stays hidden in the stream, whole and a character at a time; a kept one is served', () => {
  for (const [what, input] of CONTINUED_HIDDEN) {
    const whole = journal()
    whole.send(`${input}\nand goes on`)
    assert.ok(!whole.served().includes(SECRET), what)
    const split = journal()
    for (const ch of `${input}\nand goes on`) split.send(ch)
    assert.ok(!split.served().includes(SECRET), `${what}, a character at a time`)
  }
  for (const [what, input] of KEPT) {
    const whole = journal()
    whole.send(`${input}\nand goes on`)
    assert.ok(whole.served().includes(SECRET), what)
    const split = journal()
    for (const ch of `${input}\nand goes on`) split.send(ch)
    assert.ok(split.served().includes(SECRET), `${what}, a character at a time`)
  }
})

// A blank run past the 4096 keep at each place a cut can land before the value (the end of the key
// line, of each blank, comment, anchor, tag or indicator line under it, and right before the value):
// the key line and the lines under it that hold anchors, tags or an indicator are kept, and the
// value that arrives next, a character at a time, is hidden.
test('served: a cut after a key line, in the lines under it or before its value keeps the key and hides the value', () => {
  for (const [what, input] of CONTINUED_HIDDEN) {
    for (const at of cutsOf(input)) {
      const r = journal()
      r.send(`${input.slice(0, at)}${' '.repeat(17_000)}`)
      for (const ch of `${input.slice(at)}\nand goes on`) r.send(ch)
      assert.ok(!r.served().includes(SECRET), `${what}, cut at ${at}`)
      assert.ok(r.served().includes(nameOf(input)), `${what}, cut at ${at}: the key is still served`)
    }
  }
})

// #406, the table's `partial_key` rows: in the stream, a line more indented than a key line with no
// colon on it yet (a child key still arriving; `{cut}` marks where the first part stops) is never
// remembered as the key's value, so later text that names a credential is served as written, and
// once the rest arrives the row is served as its whole text is. Each row is sent whole, a character
// at a time, in two parts split at the cut, and past the 4096 keep (filler before it, so the keep's
// cut lands in the filler, then the rest a character at a time). A blank run past the keep at the cut
// pads the partial line with blanks: nothing on it is remembered either (the padded text is no longer
// the row's, so its value is not checked).
const LATER = fill('\nlater the {name}, {parent} and user words stay')
const PARTIAL = TABLE.partial_key.map(([what, input, want]) => [what, fill(input).split('{cut}'), fill(want)])
const FILLER = 'filler words here\n'.repeat(Math.ceil(4096 / 18))

for (const [what, [head, tail], want] of PARTIAL) test(`partial key: ${what}`, () => assert.equal(redactText(`${head}${tail}`), want))

test('served: a child key still arriving under a key line is never remembered as its value, whole, in parts and past the 4096 keep', () => {
  for (const [what, [head, tail], want] of PARTIAL) {
    const whole = `${head}${tail}${LATER}`
    const partial = head.slice(head.search(/[^\r\n]*$/)).trim()
    const runs = {
      whole: (r) => r.send(whole),
      'a character at a time': (r) => { for (const ch of whole) r.send(ch) },
      'in two parts at the cut': (r) => { r.send(head); r.send(`${tail}${LATER}`) },
      'past the 4096 keep': (r) => { r.send(`${FILLER}${head}`); for (const ch of `${tail}${LATER}`) r.send(ch) },
      'a blank run past the keep at the cut': (r) => { r.send(`${head}${' '.repeat(17_000)}`); for (const ch of `${tail}${LATER}`) r.send(ch) },
    }
    for (const [how, run] of Object.entries(runs)) {
      const r = journal()
      run(r)
      const { secrets, text } = r.row()
      assert.ok(!secrets.some((v) => LATER.includes(v) || (partial && partial.includes(v))), `${what}, ${how}: remembered ${JSON.stringify(secrets)}`)
      assert.ok(text.endsWith(LATER), `${what}, ${how}: the later text is served as written`)
      // The pad is the cut's, not the row's: the line it pads is hidden whole (fail-closed, as no
      // name on it can be read), so only the runs without a pad serve the row's text as it reads.
      if (!how.startsWith('a blank run')) assert.ok(text.endsWith(`${want}${LATER}`), `${what}, ${how}: served as the whole text`)
      if (!want.includes(SECRET)) assert.ok(!r.served().includes(SECRET), `${what}, ${how}: the value is hidden`)
    }
  }
})

// #406 F-B residual: the cut pads an unfinished line with blanks. In a growing buffer that line may
// still be arriving, so a bare token on it is neither hidden nor remembered until its line ends: the
// completion decision needs the colon (a child key) or the line end (its parent's scalar). Until
// then the token is withheld, never served (the one place a pending line is not shown). A whole
// payload keeps the old, fail-closed read of its last line.
test('served: a partial child key padded by a cut is neither hidden nor remembered while its line is open', () => {
  const PADDED = PARTIAL.filter(([, [head]]) => {
    const pending = head.slice(head.search(/[^\r\n]*$/)).trim()
    return pending && !pending.includes(':') && !/^[ \t]*[|>]/m.test(head)
  })
  assert.equal(PADDED.length, 4, 'the anchor line, the anchored child, the spaced child and the plain guard')
  for (const [what, [head]] of PARTIAL) assert.deepEqual(secretsIn(`${head}${' '.repeat(17_000)}`, true), [], what)
  for (const [what, [head]] of PADDED) {
    const pending = head.slice(head.search(/[^\r\n]*$/)).trim()
    const out = redactText(`${head}${' '.repeat(17_000)}`, true)
    assert.ok(!out.includes('[redacted]'), `${what}: not masked while the line is open`)
    assert.ok(!out.includes(pending), `${what}: withheld, never served`)
    assert.ok(secretsIn(head, true).every((v) => !pending.includes(v)), `${what}: not remembered`)
    assert.ok(redactText(head).includes('[redacted]'), `${what}: a whole payload still hides it`)
  }
})

// #406 F-B: the cut pads the unfinished line with blanks. Once the rest of that line arrives it can
// read as anything (the pad hides where its name ended, so the parts of a credential name arrive
// apart), so the line is hidden whole, fail-closed, and the value after the pad's first colon is
// remembered: a later line that holds it bare is masked too. Rows: the padded completion, the same a
// character at a time, and a later note line that holds the value bare.
test('served: a partial child key completed after a cut\'s pad is hidden whole, and a later bare occurrence is masked', () => {
  for (const [what, [head, tail]] of PARTIAL) {
    const pad = `${head}${' '.repeat(17_000)}`
    const runs = {
      'the padded completion': [(r) => { r.send(pad); r.send(`${tail}${LATER}`) }, LATER],
      'the padded completion, a character at a time': [(r) => { r.send(pad); for (const ch of `${tail}${LATER}`) r.send(ch) }, LATER],
      'the padded completion, then a later note line': [(r) => { r.send(pad); r.send(tail); r.send(`\nlater ${SECRET} appears`) }, `later ${REDACTED} appears`],
    }
    for (const [how, [run, end]] of Object.entries(runs)) {
      // A block's lines are not remembered (a later bare occurrence is served, as whole), so only
      // the completion runs apply to a block body under the pad.
      if (how.endsWith('later note line') && LATER_BLOCK.test(head)) continue
      const r = journal()
      run(r)
      const { text } = r.row()
      assert.ok(!r.served().includes(SECRET), `${what}, ${how}: no payload serves the value`)
      assert.ok(text.endsWith(end), `${what}, ${how}: the text after it is served as written`)
    }
  }
})

// #406's last frame: the value that completes the token on a cut-padded line. While that line is
// still arriving no rule can read it (its head is cut, so the name arrives in parts) and the value
// is not remembered until its line ends, so the frame must withhold the rest of the line too: a
// character at a time, no served payload (row, snapshot or detail record) ever holds the value.
// The pad lands mid-line and at a line start (a blank line), hence every row is streamed.
test('served: a cut-padded line never serves the value at the frame that completes it, a character at a time', () => {
  for (const [what, [head, tail]] of PARTIAL) {
    const r = journal()
    r.send(`${head}${' '.repeat(17_000)}`)
    for (const ch of `${tail}${LATER}`) {
      r.send(ch)
      assert.ok(!r.served().includes(SECRET), `${what}: no frame serves the value`)
    }
  }
})

// #406 FAIL 2: an explicit key's block (FB1) is remembered once its entry has ended even while the
// buffer is still growing, as the continued form (FL1) is: a line break after the value, or a blank, a
// tab or a comment that ends it at the end of the text, closes it. While the value is still the last
// thing in the text it may still be arriving, and is not remembered.
const ENDED = ['\nnext: shown', ' ', '\t', ' # note']
test('continuation: an explicit key\'s block is remembered while the buffer grows once its value has ended', () => {
  for (const input of [fill('? {name}\n: |\n  {value}'), fill('- ? {name}\n  :\n    {value}')]) {
    assert.deepEqual(secretsIn(input, true), [], 'a value still arriving at the end of the text is not remembered')
    for (const end of ENDED) {
      const kept = secretsIn(`${input}${end}`, true)
      assert.ok(kept.some((v) => v.includes(SECRET)), `remembered once ${JSON.stringify(end)} ended it`)
      assert.ok(!masked(`later ${SECRET} stays`, kept).includes(SECRET), `a later bare occurrence is masked, ended by ${JSON.stringify(end)}`)
    }
  }
  const r = journal()
  r.send(`${fill('? {name}\n: |\n  {value}')}\nnext: shown\nlater ${SECRET} stays`)
  assert.ok(!r.row().text.includes(SECRET), 'the value, and its later bare occurrence, are not served')
})

// #406 NARROW 4: an explicit key's continued value (F-C) whose line ends at the end of the text is
// remembered there, with a blank, a tab or a comment after it as with a line break: the value has
// ended either way. Only a text that stops at the value itself waits in a growing buffer.
test('continuation: an explicit key\'s continued value is remembered when blanks, a tab or a comment end it at the end of the text', () => {
  const input = fill('? {name}\n:\n    {value}')
  for (const end of ['', ...ENDED]) assert.ok(secretsIn(`${input}${end}`).some((v) => v.includes(SECRET)), `remembered whole, ended by ${JSON.stringify(end)}`)
  for (const end of ENDED) assert.ok(secretsIn(`${input}${end}`, true).some((v) => v.includes(SECRET)), `remembered in a growing buffer, ended by ${JSON.stringify(end)}`)
  assert.deepEqual(secretsIn(input, true), [], 'a growing buffer still waits for the value to end')
  const r = journal()
  r.send(`${input} `)
  r.send(`\nlater ${SECRET} stays`)
  assert.ok(!r.served().includes(SECRET), 'the value, and its later bare occurrence, are not served')
})

// #406 FAIL 3: a pad between an explicit key's question line and its `:` line is longer than the tail
// keep, so the `:` line is cut away with the rest of the text. The question line is a key line too,
// and is kept, so the value that arrives under the `:` line stays hidden. Rows: the plain form, a
// list item's (FL1) and one whose `:` line opens a block (FB1).
const QUESTION_FILLER = 'k: v\n'.repeat(800)
const BETWEEN = [
  ['plain', (name) => `? ${name}\n`, (value) => `:\n  ${value}\n`],
  ['FL1', (name) => `- ? ${name}\n`, (value) => `  :\n    ${value}\n`],
  ['FB1', (name) => `? ${name}\n`, (value) => `: |\n  ${value}\n`],
]
test('served: a cut between a question line and its \':\' line keeps the question line and hides the value', () => {
  for (const [what, head, body] of BETWEEN) {
    const r = journal()
    r.send(`${QUESTION_FILLER}${head(FILL['{name}'])}${' '.repeat(17_000)}\n`)
    r.send(body(SECRET))
    r.send('next: shown')
    assert.ok(!r.served().includes(SECRET), `${what}: no payload serves the value`)
    assert.ok(r.served().includes(`? ${FILL['{name}']}`), `${what}: the question line is still served`)
    assert.ok(r.served().includes('next: shown'), `${what}: the later text is served`)
  }
})
