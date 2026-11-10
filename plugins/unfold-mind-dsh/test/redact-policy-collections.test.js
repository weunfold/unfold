import assert from 'node:assert/strict'
import { test } from 'node:test'
import * as source from '../src/redact.js'
import * as support from '../../unfold-trajectory-dsh/test/support/redact.js'
import { MindStream } from '../src/mind.js'
import { loadDefinitions } from '../src/definitions.js'
const PATHS = [['Mind', source], ['trajectory', support]]

const VALUE = ['Cedar', '7Apricot'].join('')
const SECOND = ['Olive', '9Peach'].join('')
const WRAPS = [['LF', s => s], ['CRLF', s => s.replace(/\n/g, '\r\n')], ['CR', s => s.replace(/\n/g, '\r')], ['nested', s => 'outer:\n' + s.split('\n').map(line => '  ' + line).join('\n')]]

const CASES = [
 ['flow map', `password: {childname: ${VALUE}, othername: public}\nnext: shown`, [VALUE, 'public'], ['childname', 'othername']],
 ['multiword parent', `db password: {childname: ${VALUE}}\nnext: shown`, [VALUE], ['childname']],
 ['anchored key', `&a password: {childname: ${VALUE}}\nnext: shown`, [VALUE], ['childname']],
 ['quoted map', `"password": {"childname": "${VALUE}", "othername": "public"}\nnext: shown`, [VALUE, 'public'], ['childname', 'othername']],
 ['nested flow map', `db_password: {childname: {othername: ${VALUE}}, labelname: [public, "${SECOND}"]}\nnext: shown`, [VALUE, SECOND, 'public'], ['childname', 'othername', 'labelname']],
 ['flow sequence', `token: [${VALUE}, public]\nnext: shown`, [VALUE, 'public'], []],
 ['quoted sequence', `apiKey: ["${VALUE}", "public"]\nnext: shown`, [VALUE, 'public'], []],
 ['sequence of maps', `password: [{childname: ${VALUE}}, {othername: public}]\nnext: shown`, [VALUE, 'public'], ['childname', 'othername']],
 ['child map', `password:\n  childname: ${VALUE}\n  othername: public\nnext: shown`, [VALUE, 'public'], ['childname', 'othername']],
 ['child value on a later line', `password:\n  childname:\n\n    ${VALUE}\nnext: shown`, [VALUE], ['childname']],
 ['nested child map', `password:\n  childname:\n    othername: ${VALUE}\nnext: shown`, [VALUE], ['childname', 'othername']],
 ['child sequence', `password:\n  - ${VALUE}\n  - public\nnext: shown`, [VALUE, 'public'], []],
 ['child sequence of maps', `password:\n  - childname: ${VALUE}\n    othername: public\nnext: shown`, [VALUE, 'public'], ['childname', 'othername']],
 ['anchored collection', `password: &a {childname: ${VALUE}}\nnext: shown`, [VALUE], ['childname']],
 ['flow inside a log object', `{"settings": {"password": {"childname": "${VALUE}"}}, "next": "shown"}\nnext: shown`, [VALUE], ['settings', 'childname']],
]
for (const [path, redactor] of PATHS) for (const [shape, raw, values, labels] of CASES) {
  test(`${path}: credential collections keep labels and hide leaves (${shape})`, () => {
    for (const [ending, wrap] of WRAPS) {
      const input = wrap(raw), out = redactor.redactText(input)
      assert.equal(redactor.redactText(out), out, 'collection redaction is idempotent')
      for (const value of values) assert.ok(!out.includes(value), 'a credential leaf is hidden: ' + ending)
      for (const label of labels) assert.ok(out.includes(label), 'the structural label stays: ' + label)
      assert.ok(out.includes('next: shown'), 'the outside sibling stays visible')
      for (let cut = 1; cut <= input.length; cut++) {
        const prefix = redactor.redactText(input.slice(0, cut), true)
        for (const value of values) assert.ok(!prefix.includes(value), 'a growing-buffer prefix cannot expose a leaf')
      }
      if (redactor.secretsIn) {
        const kept = redactor.secretsIn(input)
        for (const value of values) assert.ok(kept.includes(value), 'each completed leaf is remembered')
        for (const label of [...labels, 'shown']) assert.ok(!kept.some(v => v.includes(label)), 'labels/outside siblings are not learned')
      }
    }
  })
}
const BAD = [
 ['unclosed map', `password: {childname: ${VALUE}`],
 ['unclosed sequence', `password: [${VALUE}, {childname: public}`],
 ['flow followed by a stray value', `password: {childname: public} ${VALUE}\nnext: shown`],
 ['marker followed by a stray value', `password: [redacted] ${VALUE}\nnext: shown`],
 ['child flow followed by a stray value', `password:\n  childname: {othername: public} ${VALUE}\nnext: shown`],
 ['missing map value', `password: {childname: ${VALUE}, invalid}`],
 ['unclosed quoted leaf', `password: {childname: "${VALUE}`],
]
for (const [path, redactor] of PATHS) {
  test(`${path}: malformed credential collections hide the whole subtree`, () => {
    for (const [shape, input] of BAD) for (const open of [false, true]) assert.ok(!redactor.redactText(input, open).includes(VALUE), shape)
  })
  test(`${path}: unfinished structures never learn their arriving leaves`, () => {
    for (const input of [`password: {childname: ${VALUE}\n`, `password:\n  childname: ${VALUE}\n`]) {
      assert.ok(!redactor.redactText(input, true).includes(VALUE))
      if (redactor.secretsIn) assert.ok(!redactor.secretsIn(input, true).includes(VALUE), 'no memory before the structure closes')
    }
  })
  test(`${path}: an ordinary parent collection stays visible`, () => {
    const input = `settings: {childname: ${VALUE}}\nnext: shown`
    assert.equal(redactor.redactText(input), input)
  })
}

test('policy-collections: MindStream hides each original value and its later repeat', () => {
  for (const row of CASES) {
    const [shape, input] = row
    const values = row[2]
    const mind = new MindStream({ now: () => 1 })
    mind.life = 'lifeZ'
    let seq = 0
    const send = (kind, text = '') => mind.agentd({ v: 2, id: `g:${seq}`, producer: 'g', seq, t_ns: seq++, source: 'guest', kind, response_id: 'rK', text }, { via: 'agentd journal', file: '/j', offset: seq })
    const served = () => [JSON.stringify(mind.snapshot()), ...mind.rows.map(row => JSON.stringify(mind.detail(row.id, loadDefinitions())))].join('\n')
    send('response_started')
    for (const part of input) {
      send('agent_delta', part)
      for (const value of values) assert.ok(!served().includes(value), shape + ': original leaf never appears')
    }
    send('agent_delta', '\n')
    send('agent_delta', 'later ' + values.join(' then ') + ' stays\n')
    for (const value of values) assert.ok(!served().includes(value), shape + ': later repeat remains hidden')
  }
})

const BOUND_SHAPES = [
 ['map', 'password:\n' + '  childname: public\n'.repeat(1800) + `  othername: ${VALUE}\nnext: shown\n`],
 ['sequence', 'password:\n' + '  - public\n'.repeat(3000) + `  - ${VALUE}\nnext: shown\n`],
 ['flow', 'password: {\n' + '  childname: public,\n'.repeat(1800) + `  othername: ${VALUE}\n}\nnext: shown\n`],
]

test('policy-collections: MindStream retains collection privacy past its raw bound', () => {
  for (const [shape, input] of BOUND_SHAPES) {
    const mind = new MindStream({ now: () => 1 }); mind.life = 'lifeZ'
    let seq = 0
    const send = (kind, text = '') => mind.agentd({ v: 2, id: `g:${seq}`, producer: 'g', seq, t_ns: seq++, source: 'guest', kind, response_id: 'rK', text }, { via: 'agentd journal', file: '/j', offset: seq })
    send('response_started')
    for (let at = 0; at < input.length; at += 137) {
      send('agent_delta', input.slice(at, at + 137))
      assert.ok(!JSON.stringify(mind.snapshot()).includes(VALUE), shape + ': no orphaned leaf')
    }
    assert.ok(mind.rows.some(row => row.text?.includes('next: shown')), shape + ': the outside sibling returns')
  }
})
