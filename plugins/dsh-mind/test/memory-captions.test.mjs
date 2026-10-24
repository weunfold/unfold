// A recall receipt is never described as memory handed to, received by or used by the model.
// Through both actual renderers: the event catalog (dsh-events) and Mind's sentence. Memory contents never enter the plain sentence.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { Catalog } from '../../dsh-events/src/catalog.mjs'
import { sentence } from '../src/sentences.mjs'

const catalog = new Catalog(JSON.parse(readFileSync(new URL('../../../catalog/events.v3.json', import.meta.url), 'utf8')))
const recall = (status, count) => ({ call_id: 'command:probe', count, total: count, recall: { status, items: Array.from({ length: count }, (_, i) => ({ id: `rm-${i + 1}`, revision: 1, text: `SECRET memory ${i}` })) } })
const both = (fields) => ({ catalog: catalog.describe({ kind: 'memory_recall', ...fields }).summary, mind: sentence({ lane: 'agentd', kind: 'memory_recall', fields }) })
const DELIVERY = /handed|given|received|used by|to the model/i

test('backend_closed with three selected items is a failure, never memory handed to the model', () => {
  const r = both(recall('backend_closed', 3))
  assert.doesNotMatch(r.catalog, DELIVERY)
  assert.doesNotMatch(r.mind.sentence, DELIVERY)
  assert.match(r.mind.sentence, /not delivered|closed/i)
  assert.equal(r.mind.status, 'failed')
})

test('held and queued are pending or attempted; dropped is stopped; no status claims delivery', () => {
  assert.equal(both(recall('held', 2)).mind.status, 'pending')
  assert.equal(both(recall('enqueued', 2)).mind.status, 'attempted')
  for (const s of ['dropped_context_changed', 'dropped_context_changed_at_backend']) assert.equal(both(recall(s, 2)).mind.status, 'stopped', s)
  for (const s of ['held', 'enqueued', 'backend_closed', 'dropped_context_changed', 'dropped_context_changed_at_backend', 'selected_by_backend', 'some_future_status']) {
    const r = both(recall(s, s === 'selected_by_backend' ? 0 : 2))
    assert.doesNotMatch(r.catalog, DELIVERY, `catalog ${s}`)
    assert.doesNotMatch(r.mind.sentence, DELIVERY, `mind ${s}`)
    assert.doesNotMatch(`${r.catalog} ${r.mind.sentence}`, /SECRET/, 'memory contents never enter the sentence')
  }
})

test('control: selected_by_backend says only that the backend selected the text, with no count of zero memories', () => {
  const r = both(recall('selected_by_backend', 0))
  assert.match(r.mind.sentence, /backend selected/i)
  assert.doesNotMatch(r.mind.sentence, /\b0 of 0\b/)
  assert.equal(r.mind.status, 'observed')
  const u = both(recall('some_future_status', 1)).mind
  assert.equal(u.status, 'unknown')
  assert.match(u.sentence, /some_future_status/, 'an unknown status is shown literally')
})
