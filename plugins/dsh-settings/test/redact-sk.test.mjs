// The settings copy of the credential patterns: what /api/rth-settings serves passes through
// `redact`, so a standalone sk- credential in any served string (a status note, a link detail,
// nested at any depth) is masked; hashes and ids stay. Synthetic values only.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { REDACTED, redact, redactText } from '../src/redact.mjs'

const CANARY = 'sk-proofCANARY0123456789abcdefABCDEF'

test('the bare canary is masked in served strings at any depth; existing families unchanged', () => {
  const r = redact({
    host: { backend_capabilities: { context_truncation: { note: `slidingWindow only, key ${CANARY}` } } },
    link: { agent: `127.0.0.1:0 ${CANARY}` },
    list: [CANARY, 'plain'],
    msg: 'Authorization: Bearer eyJhbGciOi.x.y and GEMINI_API_KEY=AIzaSyD-abcdefghijklmnopqrstuvwxyz012345',
    manifest_sha256: '8bc340e750b82c176990fe60d623d024f0e59da7f21d6cb8dc33de7e1472110d',
    artifact_id: 'art-3f2a9c1e-7b4d-4e2a-9c1f-0a1b2c3d4e5f',
  })
  assert.ok(!JSON.stringify(r).includes('proofCANARY'), JSON.stringify(r))
  assert.equal(r.host.backend_capabilities.context_truncation.note, `slidingWindow only, key ${REDACTED}`)
  assert.deepEqual(r.list, [REDACTED, 'plain'])
  assert.match(r.msg, /Bearer \[redacted\].*GEMINI_API_KEY=\[redacted\]/)
  assert.equal(r.manifest_sha256, '8bc340e750b82c176990fe60d623d024f0e59da7f21d6cb8dc33de7e1472110d')
  assert.equal(r.artifact_id, 'art-3f2a9c1e-7b4d-4e2a-9c1f-0a1b2c3d4e5f')
  assert.equal(redactText('we use sk-learn-pipeline-v2 here'), 'we use sk-learn-pipeline-v2 here')
  assert.equal(redactText(`api_key: "${CANARY}"`), `api_key: "${REDACTED}"`)
})
