import assert from 'node:assert/strict'
import { test } from 'node:test'
import { redactText as settingsText } from '../src/redact.js'

const VALUE = ['Cedar', '7Apricot'].join('')
const CREDENTIAL = ['pass', 'word'].join('')
const FORMS = [['spaced suffix', `? "db" ${CREDENTIAL}`], ['glued suffix', `? "db"_${CREDENTIAL}`]]
const SHAPES = [
  ['indented scalar', (key) => `${key}\n  ${VALUE}\nnext: shown`],
  ['indented gap before colon', (key) => `${key}\n  gap\n: ${VALUE}\nnext: shown`],
  ['map child', (key) => `${key}\n  child: ${VALUE}\nnext: shown`],
]

for (const [path, redactText] of [['Settings', settingsText]]) {
  for (const [form, key] of FORMS) for (const [shape, input] of SHAPES) {
    test(`#785: ${path} hides the value after a malformed quoted key (${form}, ${shape})`, () => {
      const served = redactText(input(key))
      assert.ok(!served.includes(VALUE), 'the value must never be served')
      assert.ok(served.endsWith('next: shown'), 'the following sibling stays visible')
    })
  }
}
