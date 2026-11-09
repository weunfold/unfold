import assert from 'node:assert/strict'
import { test } from 'node:test'
import * as source from '../src/redact.js'
const PATHS = [['Settings', source]]

const CASES = []
for (const key of ['- - ? "password"', '- - ? "db" password', '- - ? "db"_password', "- - ? 'token' x", '- - - ? "password" extra', '-   -\t? "password"']) {
  for (const [shape, tail] of [
    ['colon at marker', '\n: @V@'], ['colon at question', '\n    : @V@'],
    ['scalar', '\n      @V@'], ['child', '\n      childname: @V@'],
    ['gap', '\n      gapword\n    : @V@'], ['block', '\n    : |\n      @V@'],
    ['empty colon', '\n    :\n      @V@'], ['blank and comment', '\n\n    # note\n    : @V@'],
  ]) CASES.push([`${key}, ${shape}`, `${key}${tail.replace('@V@', ['Cedar', '7Apricot'].join(''))}\nnext: shown`])
}

const VALUE = ['Cedar', '7Apricot'].join('')
const WRAPS = [
 ['LF', s => s], ['CRLF', s => s.replace(/\n/g, '\r\n')],
 ['CR', s => s.replace(/\n/g, '\r')],
 ['nested', s => 'outer:\n' + s.split('\n').map(line => '  ' + line).join('\n')],
]
for (const [path, redactor] of PATHS) for (const [shape, raw] of CASES) {
  test(`${path}: nested-lists hides and remembers ${shape} (all line endings)`, () => {
    for (const [ending, wrap] of WRAPS) {
      const input = wrap(raw)
      const served = redactor.redactText(input)
      assert.ok(!served.includes(VALUE), 'the value is hidden: ' + ending)
      assert.ok(served.includes('next: shown'), 'the following sibling stays visible')
      for (let cut = input.indexOf(VALUE) + VALUE.length; cut <= input.length; cut++) {
        assert.ok(!redactor.redactText(input.slice(0, cut), true).includes(VALUE), 'every growing-buffer cut with a complete value hides it')
      }
      if (redactor.secretsIn) {
        const kept = redactor.secretsIn(input)
        assert.ok(kept.includes(VALUE), 'the hidden value is remembered')
        assert.ok(!kept.some(v => /childname|shown/.test(v)), 'child names and sibling values are not remembered')
        const open = input.slice(0, input.indexOf(VALUE) + VALUE.length)
        assert.ok(!redactor.secretsIn(open, true).includes(VALUE), 'the value is not remembered while its line is open')
        const later = kept.reduce((text, value) => text.split(value).join('[redacted]'), `later ${VALUE}`)
        assert.ok(!later.includes(VALUE), 'a later bare repeat is masked')
      }
    }
  })
}
for (const [path, redactor] of PATHS) {
  test(`${path}: nested-lists keeps an ordinary quoted key and its value`, () => {
    const input = 'label: shown\n- - ? "title"\n    : visible\nnext: shown'
    assert.equal(redactor.redactText(input), input)
  })
}
