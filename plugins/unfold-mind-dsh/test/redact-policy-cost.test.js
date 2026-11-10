import assert from 'node:assert/strict'
import { test } from 'node:test'
import { redactText, secretsIn } from '../src/redact.js'
import { boundFor, fixedWork, measureReference } from '../../../scripts/redact-fuzz-guard.mjs'

const VALUE = ['Cedar', '7Apricot'].join('')
const SHAPES = [
 ...[['space',' '],['tab','\t'],['mixed',' \t']].map(([name,run]) => [`${name} run before a failed collection key`, 'a' + run.repeat(80_000 / run.length) + 'b {x}']),
 ['long question tail', '? "db" password ' + (VALUE + ' ').repeat(10_000)],
 ['wide flow map', 'password: {' + Array.from({ length: 6_000 }, (_, i) => `child${i}: "${VALUE}"`).join(',') + '}'],
 ['wide sequence', 'password: [' + (`"${VALUE}",`).repeat(12_000) + '"end"]'],
 ['deep collections', 'password: ' + '[{childname: '.repeat(8_000) + `"${VALUE}"` + '}]'.repeat(8_000)],
]
for (const [shape, input] of SHAPES) for (const [path, run] of [['redactText', redactText], ['secretsIn', secretsIn]]) {
  test(`${path}: policy scan remains within its 6x CPU bound (${shape})`, t => {
    assert.ok(input.length >= 80_000)
    run(input)
    const reference = measureReference(redactText), { bound } = boundFor(reference, 6)
    const ms = fixedWork(() => run(input))
    const detail = `${path}: ${shape}: ${input.length} chars, ${ms.toFixed(2)} ms CPU, reference ${reference.toFixed(2)} ms, bound ${bound.toFixed(2)} ms`
    t.diagnostic(detail)
    assert.ok(ms <= bound, detail)
  })
}
