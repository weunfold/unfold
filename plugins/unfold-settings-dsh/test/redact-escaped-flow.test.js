import assert from 'node:assert/strict'
import { test } from 'node:test'
import * as source from '../src/redact.js'
const PATHS = [['Settings', source]]

const VALUE = ['Cedar', '7Apricot'].join('')
const SHAPES = [
  ['anchor', k => `{${k}: &a ${VALUE}}`, true],
  ['tag', k => `{${k}: !!str ${VALUE}}`, true],
  ['anchor and quote', k => `{${k}: &a "${VALUE}"}`, true],
  ['later quoted value', k => `{${k}:\n  "${VALUE}"}`, true],
  ['later after sibling', k => `{"a": 1, ${k}:\n  "${VALUE}"}`, true],
  ['later inside list', k => `[{${k}:\n  "${VALUE}"}]`, true],
  ['later before sibling line', k => `{${k}:\n  "${VALUE}",\n  "other": 2}`, true],
  ['blank before bare value', k => `{${k}:\n\n  ${VALUE}}`, false],
  ['comment before value', k => `{${k}: # note\n  "${VALUE}"}`, true],
]
const CASES = [String.raw`"\x70assword"`, String.raw`"\u0070assword"`].flatMap(key => SHAPES.map(([name, make, remember]) => [`${key}, ${name}`, make(key) + '\nnext: shown', remember]))
for (const [path, redactor] of PATHS) for (const [shape, input, remember] of CASES) {
  test(`${path}: escaped flow value is hidden (${shape})`, () => {
    for (const ending of ['\n', '\r\n', '\r']) {
      const text = input.replace(/\n/g, ending)
      assert.ok(!redactor.redactText(text).includes(VALUE), 'the original value is hidden')
      assert.ok(redactor.redactText(text).includes('next: shown'), 'the following sibling stays visible')
      if (remember && redactor.secretsIn) {
        const kept = redactor.secretsIn(text)
        assert.ok(kept.includes(VALUE), 'the hidden value is remembered')
        const cut = text.slice(0, text.indexOf(VALUE) + VALUE.length)
        assert.ok(!redactor.secretsIn(cut, true).includes(VALUE), 'an open value is not remembered')
        assert.ok(!kept.reduce((out, v) => out.split(v).join('[redacted]'), `later ${VALUE}`).includes(VALUE), 'the later bare repeat is hidden')
      }
    }
  })
}
for (const [path, redactor] of PATHS) {
  test(`${path}: plain credential flow controls remain hidden`, () => {
    for (const [, make] of SHAPES) assert.ok(!redactor.redactText(make('"password"')).includes(VALUE))
  })
}
