import assert from 'node:assert/strict'
import { test } from 'node:test'
import * as source from '../src/redact.js'

const VALUE = ['Olive', '9Peach'].join('')
const CREDENTIAL = ['pass', 'word'].join('')
const CASES = [['plain multiword key', `  ? ${CREDENTIAL} db\n: ${VALUE}`], ['quoted key with a gap', `  ? "db ${CREDENTIAL}"\n    gapword\n: ${VALUE}`]]
for (const [name, key] of [['valid sequence key', `- ? "db ${CREDENTIAL}"`], ['malformed sequence key', `- ? "db" ${CREDENTIAL}`]]) {
  CASES.push([`${name}, scalar at column four`, `${key}\n    ${VALUE}`])
  CASES.push([`${name}, map child at column four`, `${key}\n    childname: ${VALUE}`])
}

for (const [name, key] of [['quoted credential', `  ? "${CREDENTIAL}" extra`], ['glued quoted name', `  ? "db"_${CREDENTIAL}`]]) {
  CASES.push([`less-indented malformed ${name}`, `${key}\n: ${VALUE}`])
}


for (const [path, redactor] of [['Settings', source]]) for (const [shape, input] of CASES) {
  test('#804: ' + path + ' hides and remembers the value (' + shape + ')', () => {
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
