import assert from 'node:assert/strict'
import { test } from 'node:test'
import { MindStream } from '../src/mind.js'
import { loadDefinitions } from '../src/definitions.js'
import * as source from '../src/redact.js'
import * as support from '../../unfold-trajectory-dsh/test/support/redact.js'

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

const DASH_CASES = [
  ['quoted credential', `- ? "${CREDENTIAL}" extra\n: ${VALUE}`],
  ['glued quoted name', `- ? "db"_${CREDENTIAL}\n: ${VALUE}`],
  ['single-quoted credential', `- ? 'token' x\n: ${VALUE}`],
  ['two trailing words', `- ? "${CREDENTIAL}" db primary\n: ${VALUE}`],
  ['nested item', `outer:\n  - ? "${CREDENTIAL}" extra\n  : ${VALUE}`],
  ['trailing comment', `- ? "${CREDENTIAL}" extra # note\n: ${VALUE}`],
  ['blank gap', `- ? "${CREDENTIAL}" extra\n\n: ${VALUE}`],
  ['empty colon', `- ? "${CREDENTIAL}" extra\n:\n  ${VALUE}`],
  ['colon block', `- ? "${CREDENTIAL}" extra\n: |\n  ${VALUE}`],
  ['extra marker spacing', `-   ? "${CREDENTIAL}" extra\n: ${VALUE}`],
]
for (const [shape, input] of DASH_CASES) CASES.push([`dash-column ${shape}`, input])


for (const [path, redactor] of [['Mind', source], ['trajectory support', support]]) for (const [shape, input] of CASES) {
  test('#804: ' + path + ' hides and remembers the value (' + shape + ')', () => {
    const served = redactor.redactText(`${input}\nnext: shown`)
    assert.ok(!served.includes(VALUE), 'the value must never be served')
    assert.ok(served.endsWith('next: shown'), 'the following sibling stays visible')
    if (redactor.secretsIn) {
      const kept = redactor.secretsIn(input)
      assert.ok(kept.includes(VALUE), 'the hidden value is remembered')
      if (shape.startsWith('dash-column')) assert.deepEqual(redactor.secretsIn(input, true), input.includes('"password" extra') ? ['extra'] : [], 'an open value line is not remembered')
      assert.ok(!kept.reduce((text, value) => text.split(value).join('[redacted]'), `later ${VALUE}`).includes(VALUE), 'a later bare repeat is masked')
    }
  })
}


test('#804: MindStream masks a dash-column value and its later bare repeat', () => {
  for (const [, input] of DASH_CASES) {
    const mind = new MindStream({ now: () => 1 })
    mind.life = 'lifeZ'
    let seq = 0
    const send = (kind, text = '') => mind.agentd({ v: 2, id: `g:${seq}`, producer: 'g', seq, t_ns: seq++, source: 'guest', kind, response_id: 'rK', text }, { via: 'agentd journal', file: '/j', offset: seq })
    const served = () => [JSON.stringify(mind.snapshot()), ...mind.rows.map((row) => JSON.stringify(mind.detail(row.id, loadDefinitions())))].join('\n')
    send('response_started')
    send('agent_delta', `${input}\n`)
    assert.ok(!served().includes(VALUE), 'the value under the key is never served')
    send('agent_delta', `later ${VALUE} stays\n`)
    assert.ok(!served().includes(VALUE), 'the later bare repeat is masked')
  }
})
