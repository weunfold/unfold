import assert from 'node:assert/strict'
import { test } from 'node:test'
import { redactText as clientText, secretsIn as clientSecrets } from '../src/client/redact.js'
import { redactText as windowText } from '../src/window.js'

const VALUE = ['Cedar', '7Apricot'].join('')
const CREDENTIAL = ['pass', 'word'].join('')
const FORMS = [['spaced suffix', `? "db" ${CREDENTIAL}`], ['glued suffix', `? "db"_${CREDENTIAL}`]]
const SHAPES = [
  ['indented scalar', (key) => `${key}\n  ${VALUE}\nnext: shown`],
  ['indented gap before colon', (key) => `${key}\n  gap\n: ${VALUE}\nnext: shown`],
  ['map child', (key) => `${key}\n  child: ${VALUE}\nnext: shown`],
]

for (const [path, redactText] of [['Events browser', clientText], ['Events window', windowText]]) {
  for (const [form, key] of FORMS) for (const [shape, input] of SHAPES) {
    test(`#785: ${path} hides the value after a malformed quoted key (${form}, ${shape})`, () => {
      const served = redactText(input(key))
      assert.ok(!served.includes(VALUE), 'the value must never be served')
      assert.ok(served.endsWith('next: shown'), 'the following sibling stays visible')
    })
  }
}

const MEMORY_SHAPES = [
  ['indented scalar', (key) => `${key}\n  ${VALUE}`],
  ['indented gap before colon', (key) => `${key}\n  gapword\n: ${VALUE}`],
  ['map child', (key) => `${key}\n  childname: ${VALUE}`],
]
const MEMORY_ROWS = [
  ...FORMS.flatMap(([form, key]) => MEMORY_SHAPES.map(([shape, input]) => [form, key, shape, input])),
  ...[['glued suffix', `? "db"_${CREDENTIAL}`], ['quoted credential name', `? "${CREDENTIAL}" extra`]].map(
    ([form, key]) => [form, key, 'deeper colon', (name) => `${name}\n  : ${VALUE}`],
  ),
]
const masked = (text, kept) => kept.reduce((out, value) => out.split(value).join('[redacted]'), text)

for (const [path, secretsIn] of [['Events browser', clientSecrets]]) {
  for (const [form, key, shape, input] of MEMORY_ROWS) {
    test(`#785: ${path} remembers only an ended value (${form}, ${shape})`, () => {
      const text = input(key)
      const kept = secretsIn(text)
      assert.ok(kept.includes(VALUE), 'the hidden value is remembered')
      assert.ok(!kept.includes('gapword'), 'the gap word is never remembered')
      assert.ok(!kept.some((value) => value.includes('childname')), 'the child name is never remembered')
      assert.deepEqual(secretsIn(text, true), [], 'an open value line is not remembered')
      assert.deepEqual(secretsIn(`${text}   `, true), [], 'trailing spaces do not close the line')
      assert.ok(secretsIn(`${text}\n`, true).includes(VALUE), 'a line break closes the value')
      assert.ok(!masked(`later ${VALUE} stays`, kept).includes(VALUE), 'a bare repeat is masked')
    })
  }
}
