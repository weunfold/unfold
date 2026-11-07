// A stand-in for DSH's llm service: the providers and models it lists, and one canned chunk
// stream per call, recorded with the options each call was given, and the thinking levels of the
// models given `levels`. Nothing here calls a model.

export const TEXT = [
  { type: 'block-start', index: 0, blockType: 'text' },
  { type: 'text-delta', index: 0, text: 'Olá' },
  { type: 'text-delta', index: 0, text: ', Ana.' },
  { type: 'block-end', index: 0, block: { type: 'text', text: 'Olá, Ana.' } },
  { type: 'usage', usage: { inputTokens: 12, outputTokens: 4, cacheReadTokens: 3 } },
  { type: 'finish', reason: { kind: 'stop' } },
]

export const failure = (code, message = `the provider said no (${code})`) => [
  { type: 'finish', reason: { kind: 'error', failure: { code, message } } },
]

// `streams` are used in order, one per call; the last one repeats. `hold` is a promise a
// stream waits on after its first text (a model still answering).
export function fakeLlm({ providers = [{ id: 'deepseek-official', name: 'DeepSeek' }, { id: 'codex', name: 'ChatGPT' }], models = {
  'deepseek-official': [{ provider: 'deepseek-official', id: 'deepseek-v4-flash', name: 'DeepSeek V4 Flash' }],
  codex: [{ provider: 'codex', id: 'gpt-5.5', name: 'GPT-5.5' }],
}, streams = [TEXT], hold, levels = {} } = {}) {
  const calls = []
  return {
    calls,
    // `levels`: per `provider/model`, the thinking level ids its adapter lists, least first.
    async resolveModelInfo(provider, model) {
      const ids = levels[`${provider}/${model}`]
      return ids ? { reasoning: { efforts: ids.map(id => ({ id, name: id })) } } : {}
    },
    listProviders: () => providers.map(provider => ({ ...provider })),
    async listModels(provider) {
      if (provider === 'broken') throw new Error('no catalog')
      return (models[provider] ?? []).map(model => ({ ...model }))
    },
    async *stream(options) {
      const chunks = streams[Math.min(calls.length, streams.length - 1)]
      calls.push(options)
      let held = false
      for (const chunk of chunks) {
        if (options.signal?.aborted) {
          yield { type: 'finish', reason: { kind: 'aborted', failure: { code: 'ABORTED', message: 'aborted' } } }
          return
        }
        yield chunk
        if (hold && !held && chunk.type === 'text-delta') {
          held = true
          await hold
        }
      }
    },
  }
}

// What the tests make assistant history messages with, in place of dsh-llm's
// createAssistantMessage (index.js passes the real one).
export const assistant = ({ content, source }) => ({ id: `m-${content[0]?.text}`, role: 'assistant', content, source: { kind: 'model', ...source } })
