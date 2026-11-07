// A quoted credential value with spaces in it (a passphrase) on the direct journal path: no word
// of it reaches a Mind row, snapshot, audit record or change payload, whether the text stays
// inside the 4096-char keep or the keep's cut sweeps through it. Synthetic value only.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadDefinitions } from '../src/definitions.js'
import { MindStream } from '../src/mind.js'

const P = 'guest-b0'
const PASSPHRASE = 'correct horse battery staple'
const ASSIGNMENT = `DB_PASSWORD="${PASSPHRASE}"`
const WORDS = PASSPHRASE.split(' ')

function journal() {
  const mind = new MindStream({ now: () => 1 })
  mind.life = 'lifeZ'
  const seen = []
  mind.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  let seq = 0
  const send = (kind, f) => mind.agentd({ v: 2, id: `${P}:${++seq}`, producer: P, seq, t_ns: seq, source: 'guest', kind, ...f }, { via: 'agentd journal', file: '/j', offset: seq })
  send('response_started', { response_id: 'rK' })
  const served = () => {
    const defs = loadDefinitions()
    return [JSON.stringify(mind.snapshot()), ...mind.rows.map((x) => JSON.stringify(mind.detail(x.id, defs))), ...seen].join('\n')
  }
  return { send, served }
}

// A served word that is a passphrase word or a piece of one (3+ letters): a cut may leave any part.
const piecesIn = (text) => [...new Set(text.match(/[a-z]{3,}/gi) ?? [])].filter((t) => WORDS.some((w) => w.includes(t.toLowerCase())))

test('a spaced passphrase well inside the keep: none of its words is served', () => {
  const r = journal()
  r.send('agent_delta', { response_id: 'rK', text: `the config sets ${ASSIGNMENT} and goes on` })
  assert.deepEqual(piecesIn(r.served()), [])
})

test('a spaced passphrase the 4096 cut sweeps through: none of its words is served', () => {
  for (const lead of [0, 1, 2, 3]) {
    const r = journal()
    r.send('agent_delta', { response_id: 'rK', text: `${ASSIGNMENT} ` })
    r.send('agent_delta', { response_id: 'rK', text: 'w'.repeat(4096 - ASSIGNMENT.length - 8 + lead) })
    // One character per delta: the cut moves one place each time, from before the value to past it.
    for (let i = 0; i < ASSIGNMENT.length + 16; i++) r.send('agent_delta', { response_id: 'rK', text: 'z' })
    assert.deepEqual(piecesIn(r.served()), [], `lead ${lead}`)
  }
})

test('a spaced passphrase still open when the 4096 cut passes its key: none of its words is served', () => {
  const r = journal()
  r.send('agent_delta', { response_id: 'rK', text: `DB_PASSWORD="${WORDS[0]} ${WORDS[1]} ` })
  r.send('agent_delta', { response_id: 'rK', text: 'w'.repeat(4096) })
  r.send('agent_delta', { response_id: 'rK', text: ` ${WORDS[2]} ${WORDS[3]}" and the reply goes on` })
  assert.deepEqual(piecesIn(r.served()), [])
})
