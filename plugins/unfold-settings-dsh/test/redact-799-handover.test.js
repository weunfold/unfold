import assert from 'node:assert/strict'
import { test } from 'node:test'
import * as source from '../src/redact.js'

const VALUE = ['Olive', '9Peach'].join('')
const CREDENTIAL = ['pass', 'word'].join('')
const CASES = [['one trailing word', `? "${CREDENTIAL}" db\n: ${VALUE}`], ['two trailing words', `? "${CREDENTIAL}" db primary\n: ${VALUE}`]]

for (const [path, redactor] of [['Settings', source]]) for (const [shape, input] of CASES) {
  test('#799: ' + path + ' hides and remembers the value (' + shape + ')', () => {
    const served = redactor.redactText(`${input}\nnext: shown`)
    assert.ok(!served.includes(VALUE), 'the value must never be served')
    assert.ok(served.endsWith('next: shown'), 'the following sibling stays visible')
    if (redactor.secretsIn) {
      const kept = redactor.secretsIn(input)
      assert.ok(kept.includes(VALUE), 'the hidden value is remembered')
      assert.ok(!kept.some((value) => /gapword|childname/.test(value)), 'names and gap words are not values')
      assert.ok(!kept.reduce((text, value) => text.split(value).join('[redacted]'), `later ${VALUE}`).includes(VALUE), 'a later bare repeat is masked')
    }
  })
}
