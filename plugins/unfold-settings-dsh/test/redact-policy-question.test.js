import assert from 'node:assert/strict'
import { test } from 'node:test'
import * as source from '../src/redact.js'
const PATHS = [['Settings', source]]

const VALUE = ['Cedar', '7Apricot'].join('')
const SECOND = ['Olive', '9Peach'].join('')
const WRAPS = [['LF', s => s], ['CRLF', s => s.replace(/\n/g, '\r\n')], ['CR', s => s.replace(/\n/g, '\r')], ['nested', s => 'outer:\n' + s.split('\n').map(line => '  ' + line).join('\n')]]

const HEADS = [
 ['quoted name then credential word', '? "db" password', '? "db" password'],
 ['credential quoted name', '? "password"', '? "password"'],
 ['single-quoted credential', "? 'token'", "? 'token'"],
 ['prefixed credential word', '? "db" db_password', '? "db" db_password'],
 ['camel credential word', '? "db" accessToken', '? "db" accessToken'],
 ['word before credential', '? "db" prod password', '? "db" prod password'],
 ['anchor and tag', '? &a !t "db" password', '? &a !t "db" password'],
 ['nested list markers', '- - ? "db" password', '- - ? "db" password'],
 ['escaped quote in name', String.raw`? "d\"b" password`, String.raw`? "d\"b" password`],
 ['quoted credential word', '? "db" "password"', '? "db" "password"'],
 ['bracketed credential word', '? "db" [password]', '? "db" [password]'],
 ['invisible credential name', '? "db" pass\u200bword', '? "db" password'],
]
const CASES = HEADS.map(([name, head, keep]) => [name, `${head} ${VALUE} more words\n: ${SECOND}\nnext: shown`, keep])
CASES.push(['quoted value', `? "password" "${VALUE}"\nnext: shown`, '? "password"'])
for (const [path, redactor] of PATHS) for (const [shape, raw, keep] of CASES) {
  test(`${path}: question-line value is hidden and remembered (${shape})`, () => {
    for (const [ending, wrap] of WRAPS) {
      const input = wrap(raw)
      const out = redactor.redactText(input)
      assert.ok(!out.includes(VALUE), 'the question-line value is hidden: ' + ending)
      assert.ok(!out.includes(SECOND), 'the later value remains protected')
      assert.ok(out.replace(/\u200b/g, '').includes(keep), 'the credential name/word stays as context')
      assert.ok(out.includes('next: shown'), 'the outside sibling stays visible')
      for (let cut = input.indexOf(VALUE) + VALUE.length; cut <= input.length; cut++) assert.ok(!redactor.redactText(input.slice(0, cut), true).includes(VALUE), 'a growing-buffer prefix cannot expose the complete value')
      if (redactor.secretsIn) {
        const kept = redactor.secretsIn(input)
        assert.ok(kept.includes(VALUE), 'the hidden question-line value is remembered')
        assert.ok(!kept.includes('password') && !kept.includes('shown'), 'credential words and outside siblings are not learned')
        const open = input.slice(0, input.indexOf(VALUE) + VALUE.length)
        assert.ok(!redactor.secretsIn(open, true).includes(VALUE), 'the current open value is not remembered')
      }
    }
  })
}
for (const [path, redactor] of PATHS) {
  test(`${path}: the tail after a credential word is hidden whole, including a comment`, () => {
    const out = redactor.redactText('? "db" password # note\n: ' + VALUE + '\nnext: shown')
    assert.ok(!out.includes('note'))
    assert.ok(out.includes('? "db" password'))
    assert.ok(!out.includes(VALUE))
  })

  test(`${path}: question-line noncredential prose stays visible`, () => {
    const text = `? "title" ${VALUE}\nnext: shown`
    assert.equal(redactor.redactText(text), text)
  })
  test(`${path}: a credential tail word without a value is retained`, () => {
    assert.ok(redactor.redactText(`? "db" password\n: ${VALUE}\nnext: shown`).startsWith('? "db" password\n'))
  })
}
