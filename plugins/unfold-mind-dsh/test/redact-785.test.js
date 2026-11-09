import assert from 'node:assert/strict'
import { test } from 'node:test'
import { MindStream } from '../src/mind.js'
import { loadDefinitions } from '../src/definitions.js'
import { redactText as mindText, secretsIn as mindSecrets } from '../src/redact.js'
import { redactText as trajectoryText, secretsIn as trajectorySecrets } from '../../unfold-trajectory-dsh/test/support/redact.js'

const VALUE = ['Cedar', '7Apricot'].join('')
const CREDENTIAL = ['pass', 'word'].join('')
const FORMS = [['spaced suffix', `? "db" ${CREDENTIAL}`], ['glued suffix', `? "db"_${CREDENTIAL}`]]
const SHAPES = [
  ['indented scalar', (key) => `${key}\n  ${VALUE}\nnext: shown`],
  ['indented gap before colon', (key) => `${key}\n  gap\n: ${VALUE}\nnext: shown`],
  ['map child', (key) => `${key}\n  child: ${VALUE}\nnext: shown`],
]

for (const [path, redactText] of [['Mind', mindText], ['trajectory support', trajectoryText]]) {
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
const masked = (text, kept) => kept.reduce((out, value) => out.split(value).join('[redacted]'), text)

for (const [path, secretsIn] of [['Mind', mindSecrets], ['trajectory support', trajectorySecrets]]) {
  for (const [form, key] of FORMS) for (const [shape, input] of MEMORY_SHAPES) {
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


test('#785: MindStream masks a later bare repeat after a malformed key', () => {
  for (const [, key] of FORMS) for (const [, input] of MEMORY_SHAPES) {
    const mind = new MindStream({ now: () => 1 })
    mind.life = 'lifeZ'
    let seq = 0
    const send = (kind, text = '') => mind.agentd({ v: 2, id: `g:${seq}`, producer: 'g', seq, t_ns: seq++, source: 'guest', kind, response_id: 'rK', text }, { via: 'agentd journal', file: '/j', offset: seq })
    send('response_started')
    send('agent_delta', `${input(key)}\n`)
    send('agent_delta', `later ${VALUE} stays\n`)
    const served = [JSON.stringify(mind.snapshot()), ...mind.rows.map((row) => JSON.stringify(mind.detail(row.id, loadDefinitions())))].join('\n')
    assert.ok(!served.includes(VALUE), 'the repeat is never served')
  }
})
