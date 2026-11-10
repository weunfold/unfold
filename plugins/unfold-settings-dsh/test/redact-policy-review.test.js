import assert from 'node:assert/strict'
import { test } from 'node:test'
import * as source from '../src/redact.js'
const PATHS = [['Settings',source]]

const VALUE = ['Cedar', '7Apricot'].join('')
const SK = ['sk-', 'Cedar9Apricot7Olive5Peach3'].join('')
const KEPT = [
 ['comment line', `password:\n  # ${SK}\n  childname: ${VALUE}\nnext: shown`],
 ['trailing comment', `password:\n  childname: ${VALUE} # ${SK}\nnext: shown`],
 ['trailing assignment comment', `password:\n  childname: ${VALUE} # api_key=${SK}\nnext: shown`],
 ['flow label', `password: {${SK}: ${VALUE}}\nnext: shown`],
 ['block label', `password:\n  ${SK}: ${VALUE}\nnext: shown`],
 ['Bearer label', `password: {Bearer ${SK}: ${VALUE}}\nnext: shown`],
]
const MALFORMED = [
 ['multiline map', `password: {a: 1\napi_key: ${VALUE}\n}\nnext: shown`],
 ['multiline map plus tail', `password: {a: 1\napi_key: ${VALUE}\n} done\nnext: shown`],
 ['multiline sequence', `password: [x\ntoken: ${VALUE}\n]\nnext: shown`],
 ['extra close', `password: {a: 1, token: ${VALUE}}}\nnext: shown`],
]
const UNCLOSED = [
 ['map', `password: {a: 1\napi_key: ${VALUE}\n`],
 ['sequence', `password: [x\ntoken: ${VALUE}\n`],
 ['JSON', `{"password": {"token": "${VALUE}",`],
 ['scalar at EOF', `password: {a: 1, token: ${VALUE}`],
]
for (const [path, redactor] of PATHS) {
  for (const [shape, input] of KEPT) test(`${path}: collection kept spans apply standalone rules (${shape})`, () => {
    assert.ok(!redactor.redactText(input).includes(SK), 'a standalone credential in a kept span is hidden')
    assert.ok(!redactor.redactText(input).includes(VALUE), 'the collection leaf stays hidden')
    assert.ok(redactor.redactText(input).includes('next: shown'), 'the outside sibling stays')
    if (redactor.secretsIn) assert.ok(redactor.secretsIn(input).includes(SK), 'the kept-span credential is remembered')
  })
  if (redactor.secretsIn) {
    test(`${path}: collection scalar memory waits for a real value-line end`, () => {
      const input = `password: {a: 1\napi_key:\n  ${VALUE}`
      assert.ok(!redactor.secretsIn(input, true).includes(VALUE), 'buffer EOF does not terminate the arriving value line')
      assert.ok(redactor.secretsIn(input + '\n', true).includes(VALUE), 'a real terminator permits independent scalar memory')
    })
    for (const [shape, input] of MALFORMED) test(`${path}: malformed collection preserves ended scalar memory (${shape})`, () => {
      for (const open of [false, true]) assert.ok(redactor.secretsIn(input, open).includes(VALUE), 'an ended malformed region retains scalar memory')
      assert.ok(!redactor.redactText(input).includes(VALUE), 'the original value is hidden')
    })
    for (const [shape, input] of UNCLOSED) test(`${path}: closed payload retains malformed scalar memory (${shape})`, () => {
      assert.ok(redactor.secretsIn(input).includes(VALUE), 'the closed payload retains scalar memory')
    })
  }
}
