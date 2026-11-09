import assert from 'node:assert/strict'
import { test } from 'node:test'
import * as source from '../src/redact.js'
import * as support from '../../unfold-trajectory-dsh/test/support/redact.js'

const VALUE = ['Olive', '9Peach'].join('')
const NAMES = [['hex escape', String.raw`? "\x70assword"`], ['unicode escape', String.raw`? "\u0070assword"`]]
const SHAPES = [['indented scalar', (key) => `${key}\n  ${VALUE}`], ['gap before colon', (key) => `${key}\n  gapword\n: ${VALUE}`], ['map child', (key) => `${key}\n  childname: ${VALUE}`], ['colon line', (key) => `${key}\n: ${VALUE}`]]
const CASES = NAMES.flatMap(([name, key]) => SHAPES.map(([shape, input]) => [`${name}, ${shape}`, input(key)]))

for (const [path, redactor] of [['Mind', source], ['trajectory support', support]]) for (const [shape, input] of CASES) {
  test('#794: ' + path + ' hides and remembers the value (' + shape + ')', () => {
    const served = redactor.redactText(`${input}\nnext: shown`)
    assert.ok(!served.includes(VALUE), 'the value must never be served')
    assert.ok(served.endsWith('next: shown'), 'the following sibling stays visible')
    if (redactor.secretsIn) {
      const kept = redactor.secretsIn(input)
      assert.ok(kept.includes(VALUE), 'the hidden value is remembered')
      assert.ok(!kept.reduce((text, value) => text.split(value).join('[redacted]'), `later ${VALUE}`).includes(VALUE), 'a later bare repeat is masked')
    }
  })
}
