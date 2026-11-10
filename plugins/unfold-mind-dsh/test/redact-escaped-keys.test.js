import assert from 'node:assert/strict'
import { test } from 'node:test'
import * as source from '../src/redact.js'
import * as support from '../../unfold-trajectory-dsh/test/support/redact.js'
import { MindStream } from '../src/mind.js'
import { loadDefinitions } from '../src/definitions.js'
const PATHS = [['Mind', source], ['trajectory', support]]

const CASES = []
for (const key of [String.raw`"\x70assword"`, String.raw`"\u0070assword"`, String.raw`"\U00000070assword"`, String.raw`"pa\x73sword"`, String.raw`"\x74oken"`, String.raw`"title\q"`]) {
  for (const prefix of ['', '? ', '- - ', '- - ? ']) for (const [shape, tail] of [
    ['inline', ': @V@'], ['quoted value', ': "@V@"'],
    ['later value', ':\n      @V@'], ['block', ': |\n      @V@'],
  ]) CASES.push([`${prefix}${key}, ${shape}`, `${prefix}${key}${tail.replace('@V@', ['Cedar', '7Apricot'].join(''))}\nnext: shown`])
}


for (const key of [String.raw`"\x70assword"`, String.raw`"\u0070assword"`]) {
  CASES.push([`JSON sibling ${key}`, `{${key}: "${['Cedar', '7Apricot'].join('')}", "label": "visible"}\nnext: shown`])
  CASES.push([`flow sibling ${key}`, `{label: visible, ${key}: ${['Cedar', '7Apricot'].join('')}, after: visible}\nnext: shown`])
}

const VALUE = ['Cedar', '7Apricot'].join('')
const WRAPS = [
 ['LF', s => s], ['CRLF', s => s.replace(/\n/g, '\r\n')],
 ['CR', s => s.replace(/\n/g, '\r')],
 ['nested', s => 'outer:\n' + s.split('\n').map(line => '  ' + line).join('\n')],
]
for (const [path, redactor] of PATHS) for (const [shape, raw] of CASES) {
  test(`${path}: escaped-keys hides and remembers ${shape} (all line endings)`, () => {
    for (const [ending, wrap] of WRAPS) {
      const input = wrap(raw)
      const served = redactor.redactText(input)
      assert.ok(!served.includes(VALUE), 'the value is hidden: ' + ending)
      assert.ok(served.includes('next: shown'), 'the following sibling stays visible')
      for (let cut = input.indexOf(VALUE) + VALUE.length; cut <= input.length; cut++) {
        assert.ok(!redactor.redactText(input.slice(0, cut), true).includes(VALUE), 'every growing-buffer cut with a complete value hides it')
      }
      if (redactor.secretsIn) {
        const kept = redactor.secretsIn(input)
        assert.ok(kept.includes(VALUE), 'the hidden value is remembered')
        assert.ok(!kept.some(v => /gapword|childname|shown/.test(v)), 'gap words, child names and sibling values are not remembered')
        const open = input.slice(0, input.indexOf(VALUE) + VALUE.length)
        assert.deepEqual(redactor.secretsIn(open, true), [], 'nothing is remembered while the value line is open')
        const later = kept.reduce((text, value) => text.split(value).join('[redacted]'), `later ${VALUE}`)
        assert.ok(!later.includes(VALUE), 'a later bare repeat is masked')
      }
    }
  })
}
for (const [path, redactor] of PATHS) {
  test(`${path}: escaped-keys keeps an ordinary quoted key and its value`, () => {
    const input = 'label: shown\n- - ? "title"\n    : visible\nnext: shown'
    assert.equal(redactor.redactText(input), input)
  })
}

test('escaped-keys: MindStream hides the value and its later bare repeat', () => {
  for (const [, input] of CASES) {
    const mind = new MindStream({ now: () => 1 })
    mind.life = 'lifeZ'
    let seq = 0
    const send = (kind, text = '') => mind.agentd({ v: 2, id: `g:${seq}`, producer: 'g', seq, t_ns: seq++, source: 'guest', kind, response_id: 'rK', text }, { via: 'agentd journal', file: '/j', offset: seq })
    const served = () => [JSON.stringify(mind.snapshot()), ...mind.rows.map(row => JSON.stringify(mind.detail(row.id, loadDefinitions())))].join('\n')
    send('response_started')
    send('agent_delta', input + '\n')
    assert.ok(!served().includes(VALUE), 'the original value is never served')
    send('agent_delta', `later ${VALUE} stays\n`)
    assert.ok(!served().includes(VALUE), 'the later bare repeat is masked')
  }
})
