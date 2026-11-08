// Which provider routes can run (src/host/ready.js): a route whose settings name a key that is
// not set is missing; a route that names none, or a DSH without the services, is not.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { missingKeys } from '../src/host/ready.js'

function ctxWith({ configured = {}, services = true } = {}) {
  const settings = { describe: () => [{ ns: 'llm-pi-ai', value: { providers: { codex: { apiKeyEnv: 'CODEX_API_KEY' }, kimi: { apiKeyEnv: 'KIMI_API_KEY' }, account: {} } } }] }
  const credentials = { describe: async ref => ({ configured: configured[ref] === true }) }
  return {
    get: name => (services ? { settings, credentials }[name] : undefined),
    llm: {
      listConfigurableProviders: () => ['codex', 'kimi', 'account'].map(provider => ({ provider, settingsNs: 'llm-pi-ai', settingsPath: ['providers', provider] })),
    },
  }
}

test('a route whose key is not set is missing; a set key or no key named is ready', async () => {
  const missing = await missingKeys(ctxWith({ configured: { KIMI_API_KEY: true } }), ['codex', 'kimi', 'account', 'other'])
  assert.deepEqual([...missing], ['codex'])
})

test('without the settings and credential services nothing is said to be missing', async () => {
  assert.equal((await missingKeys(ctxWith({ services: false }), ['codex'])).size, 0)
  assert.equal((await missingKeys({ get: () => ({ describe() { throw new Error('locked') } }), llm: { listConfigurableProviders: () => [] } }, ['codex'])).size, 0)
})
