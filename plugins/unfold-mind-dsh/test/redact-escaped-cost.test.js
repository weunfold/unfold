import assert from 'node:assert/strict'
import { test } from 'node:test'
import { redactText, secretsIn } from '../src/redact.js'
import { boundFor, fixedWork, measureReference } from '../../../scripts/redact-fuzz-guard.mjs'

const SHAPES = [
  ['minified calls with a backslash', 'f(a,!0),g(b,!1);'.repeat(9_000) + String.raw`"\n"`],
  ['ASCII-escaped JSON keys', '{' + Array.from({ length: 7_000 }, (_, i) => String.raw`"k\u00e9${i}":"v${i}"`).join(',') + '}'],
]
// Match the stream's 6x CPU reference and 400ms backstop; no wall-clock retry.
for (const [shape, text] of SHAPES) for (const [path, run] of [['redactText', redactText], ['secretsIn', secretsIn]]) {
  test(`${path}: ${shape} stays within the 6x CPU bound`, t => {
    assert.ok(text.length >= 100_000, 'the adversarial row is at least 100KB')
    run(text)
    const referenceMs = measureReference(redactText)
    const { bound } = boundFor(referenceMs, 6)
    const ms = fixedWork(() => run(text))
    const detail = `${path}: ${shape}: ${text.length} chars, ${ms.toFixed(2)} ms CPU, reference ${referenceMs.toFixed(2)} ms, bound ${bound.toFixed(2)} ms (6x, cap 400ms)`
    t.diagnostic(detail)
    assert.ok(ms <= bound, detail)
  })
}
