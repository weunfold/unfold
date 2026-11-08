// @automagik/unfold-session-dsh: Unfold's single rolling session projected as ONE DSH root
// Session. DSH is a view and a composer here, never a second brain:
//
// - The composer's `session/prompt` still opens a DSH turn, but our `agent/pre-step` listener
//   (prepended, so nothing else's pre-step runs first) holds that turn: it sends the person's
//   text once, through Unfold's session-control endpoint (`POST /submit`, src/host-client.js),
//   and Unfold's app sends it to agentd on the Unfold Session as a typed command; it streams the
//   reply into the conversation as it arrives (read from agentd as an observer); it records one
//   user and one assistant bubble; and it returns `{kind:'reject'}`, so the DSH loop ends the
//   turn (`blocked`) without a model step: no `agent/request`, no `llm/stream`, no provider.
// - Only the person's own composer message is ever submitted. Nothing an event says (a
//   transcript, a reply, a note, another composer's turn) reaches the endpoint.
// - Every other root Session is refused at pre-step (no DSH root loop anywhere), and the
//   first-prompt title model is disabled in the profile.
// - The endpoint has one verb. A composer message is submitted at most once: no answer, a
//   refusal or a lost connection is shown as what it is, never retried. The stop button ends the
//   turn here; there is no verb to cancel a reply in Unfold.
// - Spoken turns are projected, never submitted: a reply the provider opened from the microphone
//   is queued into the root Session as a DSH turn whose one user message carries the rpcId
//   `unfold-voice-r-<response id>`; our pre-step recognizes that id BEFORE the typed path,
//   follows the reply by its ids (bridge.runVoice), writes one user bubble with what was heard
//   for it, then one reply bubble, and rejects the step. No command, no model call. What was
//   heard and taken by no reply is its own user bubble (`unfold-voice-h-…`), no reply.
// - Clean bubbles: chat bubbles carry the words only, no labels. The user bubble is written when
//   its reply STARTS. For a backend that cannot pair a transcript with its reply by id, the words
//   are the reply's by adjacency in time (src/voice.js). An interrupted or doubtful spoken reply
//   carries one short italic mark at its end, not a paragraph.
import { join } from 'node:path'
// Peers: the profile's own copies (the loop checks `instanceof` and scope dispatch uses symbol
// keys, so a second copy would silently not match).
import * as agentPkg from '@deepseek-ai/dsh-agent'
import * as llm from '@deepseek-ai/dsh-llm'
import Schema from '@deepseek-ai/schemastery'
import { fetchStream, sseHub } from './src/sse.js'
import { SessionBridge } from './src/bridge.js'
import { LiveView } from './src/live.js'
import { HostClient } from './src/host-client.js'
import { AnswerWith } from './src/answer-with.js'

export const name = 'unfold-session'

export const Config = Schema.object({
  sessionId: Schema.string().default('unfold-root').description('the one DSH root Session that shows Unfold'),
  title: Schema.string().default('Unfold'),
  workspaceDir: Schema.string().description('workspace directory for the root Session; defaults to <DSH_HOME>/unfold-workspace'),
  blockOtherSessions: Schema.boolean().default(true).description('refuse turns in every other root Session (DSH never runs a root loop)'),
  route: Schema.string().default('/api/unfold-session'),
})

export const inject = ['agents', 'sessions']

/** Match the Bot team and welcome's own-instance rule; channel secrets stay in the preload. */
export const inUnfoldInstance = (env = process.env) => [env.AUTOMAGIK_HOST, env.AUTOMAGIK_PRELOAD].some(value => String(value ?? '').trim().toLowerCase() === 'unfold')

const configRoute = (base, sessionId, enabled) => ({
  path: '/api/unfold-session/config', methods: ['GET'], requestBody: 'buffered',
  fetch: async () => Response.json({ enabled, sessionId, route: base }),
})

function textOf(messages) {
  return messages
    .flatMap((m) => (Array.isArray(m.content) ? m.content : []))
    .filter((b) => b?.type === 'text' && typeof b.text === 'string')
    .map((b) => b.text)
    .join('\n')
    .trim()
}

/** The short end mark of a spoken reply that did not simply complete: a mark, not a paragraph. */
// `stop_unconfirmed`: a stop was requested and no backend outcome was seen; it is never shown
// as a stop that happened. `cancelled` is the confirmed terminal.
export const VOICE_MARKS = { cancelled: 'interrupted', incomplete: 'may be incomplete', unknown: 'outcome unknown', stop_unconfirmed: 'stop unconfirmed', interrupted: 'interrupted' }

/** A spoken reply's bubble: the words, and for anything but a completed reply one italic mark. */
export function voiceFinalText(final) {
  const said = (final.text ?? '').trim()
  if (final.status === 'completed') return said
  const mark = VOICE_MARKS[final.status] ?? 'failed'
  return said ? `${said} *— ${mark}*` : `*${mark}*`
}

/** The bubble text for a final state; an interruption is stated, never dressed up as a reply. */
export function finalText(final) {
  if (final.voice) return voiceFinalText(final)
  const said = (final.text ?? '').trim()
  switch (final.status) {
    case 'completed':
      return said || (final.voice ? '[spoken reply; no text was received for it]' : said)
    case 'cancelled':
      // A spoken reply cut by barge-in or a stop: the text is what was GENERATED, which can be
      // more than was played.
      if (final.voice) {
        const provider = final.voice.signals?.includes('interrupted') ? '; the provider reported it interrupted' : ''
        return `${said}${said ? '\n\n' : ''}[cut off before it finished (barge-in or stop)${provider}. This is the text that was generated, which can be more than was played.]`
      }
      return `${said}${said ? '\n\n' : ''}[stopped]`
    case 'interrupted':
      return `${said}${said ? '\n\n' : ''}[interrupted: ${final.detail ?? 'the agent restarted before a result was recorded'}]`
    case 'rejected':
      return `[Unfold refused this message: ${final.detail}]`
    case 'stop_unconfirmed':
      return `${said}${said ? '\n\n' : ''}[stopped locally; the backend outcome is unknown]`
    case 'incomplete':
      return `${said}${said ? '\n\n' : ''}[possibly incomplete: ${final.detail}]`
    case 'unknown':
      return `${said}${said ? '\n\n' : ''}[outcome unknown: ${final.detail}]`
    case 'not_sent':
      return `[not sent: ${final.detail}]`
    case 'not_seen':
      return `[reply not seen: ${final.detail}]`
    default:
      return `${said}${said ? '\n\n' : ''}[failed: ${final.detail ?? 'no detail'}]`
  }
}

export const VOICE_KEY = 'unfold-voice-'

// Why a typed unit is shown as not sent without reaching the endpoint.
const NOT_COMPOSER = "this message did not come from the composer, so it was not sent to Unfold; only what the person sends is"
const NOT_SENT_STOPPED = 'the turn was stopped before this message was sent; it was not sent to Unfold'
const MAX_VOICE_ITEMS = 64

/** True for a message the person sent from DSH's composer (the Session API's `session/prompt`). */
export const fromComposer = (message) => message?.source?.kind === 'user' && typeof message.source.rpcId === 'string' && message.source.rpcId !== ''

/** The rpcId of a projected spoken turn or heard note, or null for anything else. */
export function voiceKeyOf(message) {
  const src = message?.source
  return src?.kind === 'user' && typeof src.rpcId === 'string' && src.rpcId.startsWith(VOICE_KEY) ? src.rpcId : null
}

/** The user bubble of a spoken reply: the words heard for it, or null (no bubble) when none. */
export function voiceUserText(heard) {
  if (!heard || heard.noted) return null
  const said = (heard.text ?? '').trim()
  if (!said) return null
  return `${said}${heard.truncated && !said.endsWith('[…]') ? ' […]' : ''}`
}

/** Words shown on their own (heard, taken by no reply): the words only, or null when empty. */
export function heardNoteText(n) {
  const said = (n.text ?? '').trim()
  if (!said) return null
  return `${said}${n.truncated && !said.endsWith('[…]') ? ' […]' : ''}`
}

export function apply(ctx, config, deps = {}) {
  config = Config(config ?? {})
  const ROOT = config.sessionId
  // Installing the Unfold card beside ordinary Bots must not take over this DSH.
  if (!inUnfoldInstance(deps.env ?? process.env)) {
    ctx.inject(['connection'], (sctx) => {
      sctx.effect(() => sctx.connection.fetch.register(configRoute(config.route, ROOT, false)), 'unfold-session: disabled discovery')
    })
    return
  }
  // `deps` is for tests only (a scripted host, bridge or observer); DSH calls apply(ctx, config).
  // The control endpoint and agentd's address come from the launcher preload's registry only.
  const host = deps.host ?? new HostClient()
  const bridge = deps.bridge ?? new SessionBridge({
    host,
    observer: deps.observer,
    log: (m) => ctx.logger?.info?.(m),
  })
  // What Unfold answers with (the Unfold Bot's model menu): read from the app only.
  const answerWith = deps.answerWith ?? new AnswerWith({ host, log: (m) => ctx.logger?.info?.(m) })
  const rereadAnswer = () => { void answerWith.read().catch(() => {}) }
  const PROMPT_MS = deps.promptMs ?? 5_000
  const counters = { not_composer: 0, llm_stream: 0, agent_request: 0, root_turns: 0, blocked_turns: 0, bubbles: 0, interrupted_bubbles: 0, voice_turns: 0, voice_notes: 0, voice_dropped: 0, voice_waiting: 0, drains: 0, not_sent: 0 }
  const revisions = new WeakMap()
  const turns = []

  // ── spoken turns: queued into the root Session, deduped by key (here, and in the durable log) ──
  const voice = { items: new Map(), chain: Promise.resolve(), controller: null }
  const log = (m) => ctx.logger?.info?.(m)
  const inSession = (agent, key) =>
    agent.inbox?.nextTurn?.some((m) => voiceKeyOf(m) === key) ||
    agent.inbox?.nextStep?.some((m) => voiceKeyOf(m) === key) ||
    agent.session?.snapshotEvents?.().some((e) => e.type === 'user/message' && voiceKeyOf(e.data) === key)
  async function deliver(item) {
    if (item.state !== 'pending' && item.state !== 'waiting') return
    const content = [{ type: 'text', text: item.text }]
    // The Session API resumes a cold root Session and dedupes on the rpcId against the durable log.
    if (voice.controller?.prompt) {
      // Bounded by our own timer: the API checks its signal only on entry, and a stalled resume
      // must not wedge the delivery chain (every later item would wait behind it).
      let timer
      try {
        await Promise.race([
          voice.controller.prompt({ requestId: item.key, sessionId: ROOT, mode: 'queue', content }, AbortSignal.timeout(PROMPT_MS)),
          new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`no answer in ${PROMPT_MS} ms`)), PROMPT_MS) }),
        ])
        item.state = 'queued'
        afterTurn(ctx.agents?.get?.(ROOT))
        return
      } catch (e) {
        log(`unfold-session: voice item ${item.key} not queued through the Session API (${e?.message ?? e}); trying the live agent`)
      } finally {
        clearTimeout(timer)
      }
    }
    const agent = ctx.agents?.get?.(ROOT)
    if (agent) {
      if (!inSession(agent, item.key)) agent.followup(llm.createUserMessage({ content, source: { kind: 'user', rpcId: item.key } }))
      item.state = 'queued'
      afterTurn(agent)
      return
    }
    // No live root Session yet: kept (bounded) until it appears.
    if (item.state !== 'waiting') counters.voice_waiting++
    item.state = 'waiting'
  }
  // Liveness. DSH's driver drains its inbox only after a turn that ENDS
  // (`turn()` returns `inbox.hasPending`); our intentional `reject` returns false before that
  // check, so the driver goes idle with user work still queued (a prompt sent while a turn ran:
  // typed or spoken). After every root turn, once the agent is idle (`whenIdle`: an event,
  // never a poll), the adapter gives that work its next wake through DSH's own queue surface:
  // the LAST pending message is removed and re-sent to the same list with `wakeup`. It is the
  // same message (same id, so the same `dsh-<message id>` command), back in the same place, so
  // order is unchanged and nothing is duplicated; the driver then claims as usual (all
  // next-step messages and the first next-turn one), and each claimed message is its own unit.
  function drain(agent) {
    const inbox = agent?.inbox
    if (!inbox || typeof agent.send !== 'function' || typeof inbox.remove !== 'function') return
    // Running: that turn passes through our pre-step and schedules the next drain itself.
    if (agent.status !== undefined && agent.status !== 'idle') return
    const target = inbox.nextTurn?.length ? 'next-turn' : inbox.nextStep?.length ? 'next-step' : null
    if (!target) return
    const last = (target === 'next-turn' ? inbox.nextTurn : inbox.nextStep).at(-1)
    if (!inbox.remove(last.id)) return
    counters.drains++
    agent.send(last, target, true)
  }
  const afterTurn = (agent) => {
    if (typeof agent?.whenIdle !== 'function') return
    void agent.whenIdle().then(() => setTimeout(() => drain(agent), 0))
  }
  const flushVoice = () => {
    for (const item of voice.items.values()) if (item.state === 'waiting') voice.chain = voice.chain.then(() => deliver(item)).catch((e) => log(`unfold-session: voice delivery failed: ${e?.message ?? e}`))
  }
  function offerVoice(item) {
    if (voice.items.has(item.key)) return
    item.state = 'pending'
    voice.items.set(item.key, item)
    while (voice.items.size > MAX_VOICE_ITEMS) {
      const [k, old] = voice.items.entries().next().value
      if (old.state === 'pending' || old.state === 'waiting') counters.voice_dropped++
      voice.items.delete(k)
    }
    voice.chain = voice.chain.then(() => deliver(item)).catch((e) => log(`unfold-session: voice delivery failed: ${e?.message ?? e}`))
  }
  // ── the live "listening…" view (src/live.mjs), pushed on change over <route>/live ──
  const liveHub = sseHub()
  const live = new LiveView({ onChange: (v, rev) => liveHub.send('live', v, rev) })
  // The connection's own heartbeat, apart from speech: a reader that hears nothing for its lease
  // drops what it shows. Sent only while someone reads.
  const LIVE_BEAT_MS = 5000
  const beat = setInterval(() => { if (liveHub.clients.size) liveHub.send('beat', { at: Date.now() }, live.rev) }, LIVE_BEAT_MS)
  beat.unref?.()
  // Unload ends every live stream with the goodbye frame, and any opened after it at once.
  ctx.effect(() => () => { clearInterval(beat); live.stop(); liveHub.close('unloaded') }, 'unfold-session: live view')
  // Listening starts on the provider's voice_activity_start. The native host's earlier local
  // onset is not shown: its admission (call input open, mic unmuted) is a fact only the native
  // host holds, and the control endpoint has no verb that reads it.
  bridge.onChange((kind, data) => {
    if (kind === 'voice-live') live.input(data)
    else if (kind === 'voice-heard') live.input({ type: 'note' })
  })
  bridge.onChange((kind, data) => {
    if (kind === 'voice-opened') {
      // The queued message's text is what DSH's queue shows while it waits: the words, never an id.
      offerVoice({ key: `${VOICE_KEY}r-${data.responseId}`, kind: 'reply', responseId: data.responseId, decisionId: data.decisionId, text: data.heard || '…' })
    } else if (kind === 'voice-heard') {
      const text = heardNoteText(data)
      if (text) offerVoice({ key: `${VOICE_KEY}h-${data.key}`.slice(0, 200), kind: 'note', text, note: data })
    }
  })
  ctx.on('agent/created', ({ agent }) => {
    if (agent?.id === ROOT) flushVoice()
  })

  ctx.effect(() => {
    // Every model change restarts the agent: a link that attaches again reads the pick again.
    const off = bridge.onChange?.((kind, link) => { if (kind === 'link' && link?.state === 'attached') rereadAnswer() })
    bridge.start()
    rereadAnswer()
    return () => {
      off?.()
      bridge.stop()
    }
  }, 'unfold-session: bridge')

  // Counted, never answered: any DSH model traffic at all is a failure of this plugin's promise.
  ctx.on('llm/stream', (options, next) => {
    counters.llm_stream++
    return next()
  }, { prepend: true })
  ctx.on('agent/request', (payload, next) => {
    counters.agent_request++
    return next()
  }, { prepend: true })

  ctx.on('agent/pre-step', async (payload, next) => {
    const { agent } = payload
    if (agent.session?.id !== ROOT && agent.id !== ROOT) {
      const subagent = agent.session?.header?.origin === 'subagent'
      if (config.blockOtherSessions && !subagent) {
        counters.blocked_turns++
        return { kind: 'reject' }
      }
      return next()
    }
    if (payload.messages.length === 0) return { kind: 'reject' }
    counters.root_turns++
    // DSH claims every pending next-step message plus one next-turn message per turn, so one
    // turn can carry several user messages. Each is its own unit, in claim order: a typed one is
    // its own submit (key `dsh-<its id>`), never folded into another; a spoken turn or heard note is
    // checked FIRST, so it never reaches the typed path, even when the adapter has forgotten it.
    for (const [i, m] of payload.messages.entries()) {
      const key = voiceKeyOf(m)
      const unit = { ...payload, messages: [m] }
      // Only the person's own composer message is ever submitted: source `user` with the rpcId
      // of the Session API's prompt, which is how DSH's composer sends. A message DSH put in the
      // Session any other way (a schedule reminder, a subagent, a job, an SDK or ACP client, a
      // command's follow-up) is shown as not sent; it never reaches the endpoint and never opens
      // a model step.
      const notComposer = !key && !fromComposer(m)
      if (notComposer) counters.not_composer++
      // A later typed unit whose turn was already stopped is shown as not sent, never submitted
      // (the first unit keeps the typed path's own stop handling).
      const stopped = i > 0 && payload.signal?.aborted === true
      const notSent = key ? null : notComposer ? NOT_COMPOSER : stopped ? NOT_SENT_STOPPED : null
      await runTurn(agent, unit, key ? voiceItemFor(key) : null, i + 1, notSent)
    }
    // `reject`, not `enter []`: listeners that wrap ours (time context, agent instructions,
    // session references) add context messages to an `enter` decision, and any message makes
    // the loop enter a model step. Every wrapper passes `reject` through unchanged; the loop
    // then closes the turn as `blocked` (rendered without a failure row), with no step.
    return { kind: 'reject' }
  }, { prepend: true })

  function voiceItemFor(key) {
    const known = voice.items.get(key)
    if (known) return known
    // Not ours any more (DSH or the adapter restarted with the turn still queued): still a
    // projection, shown as unknown; never a command.
    if (key.startsWith(`${VOICE_KEY}r-`)) return { key, kind: 'reply', responseId: key.slice(VOICE_KEY.length + 2), decisionId: null, forgotten: true }
    return { key, kind: 'note', forgotten: true }
  }

  async function runTurn(agent, { messages, turn, signal }, spoken = null, step = 1, notSent = null) {
    const s = agent.session
    // A voice-keyed message is never part of a command's text.
    const text = spoken ? '' : textOf(messages.filter((m) => !voiceKeyOf(m)))
    // One key per composer message: the bridge sends a key at most once.
    const key = spoken ? null : `dsh-${messages[0].id}`
    // Shown before (the adapter restarted with the message still queued): it was handled then,
    // and is never sent again.
    const shownBefore = !spoken && messages.some((m) => s.snapshotEvents?.().some((e) => e.type === 'user/message' && e.data?.id === m.id))
    if (spoken) {
      counters[spoken.kind === 'reply' ? 'voice_turns' : 'voice_notes']++
      spoken.state = 'shown'
    }
    const record = spoken
      ? { voice: spoken.kind, key: spoken.key, responseId: spoken.responseId ?? null, decisionId: spoken.decisionId ?? null, turn, started: Date.now(), status: 'following', deltas: 0 }
      : { key, commandId: null, turn, chars: text.length, started: Date.now(), status: 'submitting', deltas: 0 }
    turns.push(record)
    if (turns.length > 200) turns.shift()
    const acc = new llm.AssistantStreamAccumulator()
    const asm = new llm.BlockAssembler()
    const attemptId = llm.LlmAttemptId(`${s.id}:unfold:${turn}:${key ?? spoken.key}`)
    let index = 0
    let started = false
    const frame = (f) => {
      const r = (revisions.get(agent) ?? 0) + 1
      revisions.set(agent, r)
      agentPkg.emitAgentEvent(ctx, agent, 'agent/assistant-stream', { frame: { ...f, revision: r } })
    }
    const push = (chunk) => {
      const t = acc.push({ time: Date.now(), chunk })
      asm.push(t.chunk)
      frame({ type: 'chunk', attemptId, index: index++, time: t.time, chunk: t.chunk })
    }
    s.append('step/start', { turn, step })
    try {
      // Surface node 0 is the system message, as the loop's own first step reserves it.
      if (turn === 1 && step === 1) s.append('system/message', { turn, step, message: llm.createSystemMessage('') }, { surfaceOp: 'append' })
      if (!spoken && !shownBefore) {
        for (const m of messages) s.append('user/message', m, { surfaceOp: 'append' })
        // On disk before the submit: a restart mid-reply finds it shown, so it is never sent again.
        await ctx.sessions.flush?.(s)
      }
      if (spoken?.kind === 'note') {
        // A heard note: one user bubble, no reply, no stream. The queued message already holds
        // the note's text (also after a restart).
        s.append('user/message', messages[0], { surfaceOp: 'append' })
        record.status = 'shown'
        record.finished = Date.now()
        return
      }
      if (spoken) {
        // The user bubble is written now, as the reply starts: the words heard for it (by id, or
        // by adjacency for a backend that cannot pair); a later part is shown on its own after it.
        const heard = bridge.voiceCommit({ responseId: spoken.responseId })
        record.heard = heard ? { utteranceId: heard.utteranceId, final: heard.final, chars: heard.text.length, adjacent: !!heard.adjacent } : null
        const said = voiceUserText(heard)
        if (said) s.append('user/message', llm.freezeMessage({ ...messages[0], content: [{ type: 'text', text: said }] }), { surfaceOp: 'append' })
      }
      frame({ type: 'start', attemptId, turn, step })
      started = true
      push({ type: 'block-start', index: 0, blockType: 'text' })
      let final = null
      if (notSent) counters.not_sent++
      const events = spoken
        ? bridge.runVoice(spoken.responseId, signal)
        : notSent
          ? (async function* () { yield { type: 'final', status: 'not_sent', text: '', detail: notSent } })()
          : shownBefore
            ? (async function* () { yield { type: 'final', status: 'unknown', text: '', detail: 'this message was handled before the session adapter restarted; it is never sent again' } })()
            : bridge.run(key, text, signal)
      for await (const ev of events) {
        if (ev.type === 'admitted') {
          record.status = ev.command?.status
          record.commandId = ev.command?.command_id ?? null
        } else if (ev.type === 'delta') {
          record.deltas++
          push({ type: 'text-delta', index: 0, text: ev.text })
        } else if (ev.type === 'final') {
          final = ev
        }
      }
      record.status = final.status
      record.detail = final.detail ?? null
      record.responseId = final.responseId ?? null
      const body = spoken ? voiceFinalText(final) : finalText(final)
      const live = asm.interruptedBlocks().map((b) => b.text ?? '').join('')
      // The recorded reply (or the interruption notice) is what the bubble says: a gap in the
      // live stream must not shorten it, and a notice must not be lost.
      if (body.startsWith(live) && body.length > live.length) push({ type: 'text-delta', index: 0, text: body.slice(live.length) })
      const ok = final.status === 'completed'
      const content = body === '' ? [] : [{ type: 'text', text: body }]
      const model = spoken
        ? `agentd voice (${spoken.responseId}${final.voice?.decisionId ? `, decision ${final.voice.decisionId}` : ''})`
        : `agentd (${record.commandId ?? 'not sent'})`
      if (ok && body !== '') {
        push({ type: 'block-end', index: 0, block: { type: 'text', text: body } })
        push({ type: 'finish', reason: { kind: 'stop' } })
      }
      const ev = content.length
        ? s.append(
            'assistant/message',
            {
              turn,
              step,
              stream: [...acc.snapshot()],
              message: llm.createAssistantMessage({ content, source: { provider: 'unfold', model } }),
              ...(ok ? {} : { interrupted: true }),
            },
            { surfaceOp: 'append' },
          )
        : s.append('assistant/attempt', { turn, step, stream: [...acc.snapshot()] })
      counters.bubbles += content.length ? 1 : 0
      if (!ok) counters.interrupted_bubbles++
      frame({ type: 'end', attemptId, index, outcome: { kind: 'committed', eventType: ev.type, seq: ev.seq } })
      record.finished = Date.now()
      // A pick made in Unfold's own settings shows at the next turn's end at the latest.
      rereadAnswer()
    } catch (e) {
      record.status = 'error'
      record.detail = String(e?.message ?? e)
      if (started) frame({ type: 'end', attemptId, index, outcome: { kind: 'abandoned' } })
      throw e
    } finally {
      afterTurn(agent)
      s.append('step/end', { turn, step })
      await ctx.sessions.flush?.(s)
    }
  }

  // The one root Session: created once in an Unfold workspace, then left alone (DSH persists it;
  // a cold Session is resumed by the next prompt).
  ctx.inject(['sessionController'], (sctx) => {
    sctx.effect(() => {
      voice.controller = sctx.sessionController
      flushVoice()
      return () => { voice.controller = null }
    }, 'unfold-session: spoken turns through the Session API')
  })

  let resolveRoot, rejectRoot
  const ready = new Promise((resolve, reject) => { resolveRoot = resolve; rejectRoot = reject })
  // The Bot plugin observes this promise; a failed preparation is also logged below.
  void ready.catch(() => {})
  let rootStopped = false
  ctx.effect(() => () => {
    rootStopped = true
    rejectRoot(new Error('unfold-session: unloaded before root preparation completed'))
  }, 'unfold-session: root readiness lifecycle')
  ctx.inject(['sessionController', 'workspaceRegistry'], (sctx) => {
    sctx.effect(() => {
      let active = true
      const stopped = () => rootStopped || !active
      void (async () => {
        try {
          const known = await sctx.get('sessionPersistence')?.stat?.(ROOT)
          if (stopped()) return
          if (known) { resolveRoot(); return }
          const dir = config.workspaceDir || join(process.env.DSH_HOME || process.cwd(), 'unfold-workspace')
          const { mkdirSync } = await import('node:fs')
          if (stopped()) return
          mkdirSync(dir, { recursive: true })
          const ws = await sctx.workspaceRegistry.create(dir, config.title)
          if (stopped()) return
          await sctx.sessionController.create({ sessionId: ROOT, workspaceId: ws.id })
          if (stopped()) return
          resolveRoot()
        } catch (e) {
          if (stopped()) return
          ctx.logger?.warn?.(`unfold-session: could not prepare the root Session: ${e?.message ?? e}`)
          rejectRoot(e)
        }
      })()
      return () => {
        active = false
        rejectRoot(new Error('unfold-session: root preparation dependencies unloaded'))
      }
    }, 'unfold-session: root session')
  })

  const service = {
    sessionId: ROOT,
    ready,
    bridge,
    answerWith,
    counters,
    turns,
    live,
    liveHub,
    async status() {
      const caps = ctx.get('unfoldCapabilities')?.map?.()
      return {
        session: { dsh_session_id: ROOT, adapter_link: { ...bridge.link }, stats: { ...bridge.stats } },
        // The host's own status (its call, microphone and voice) is not readable here: always
        // `{unavailable}`, which the events and settings plugins show as unknown.
        owner: { unavailable: OWNER_UNAVAILABLE },
        // The last MEASURED candidate (the capability map), never shown as what is active.
        candidate: caps ? (caps.latest_measured_candidate ?? null) : { unavailable: 'capability map not loaded' },
        dsh: { model_calls: { llm_stream: counters.llm_stream, agent_request: counters.agent_request }, ...counters },
        voice: { ...bridge.voiceStats, transcripts: { ...bridge.ledger.stats }, open: bridge.voiceTracks.size, items: voice.items.size },
        live: { view: live.published(), rev: live.rev, clients: liveHub.clients.size },
        turns: turns.slice(-20),
      }
    },
  }
  ctx.effect(() => ctx.provide('unfoldSession', service), 'unfold-session: service')

  ctx.inject(['connection'], (sctx) => {
    sctx.effect(() => {
      const disposers = fetchRoutes(service, config.route, ROOT).map((route) => sctx.connection.fetch.register(route))
      return () => Promise.all(disposers.map((dispose) => dispose()))
    }, 'unfold-session: routes')
  })
}

/** Why `status().owner` is `{unavailable}`: the control endpoint has no status verb. */
export const OWNER_UNAVAILABLE = "the host's call, microphone and voice state belong to Unfold's own app; the control endpoint has no verb that reads them"

/** Why the call and mute routes answer `unavailable`: the endpoint's one verb is `submit`. */
export const CALL_UNAVAILABLE = "calls and the microphone belong to Unfold's own app; this instance has no verb for them"

/**
 * The call button's read. The native call state is not readable through Unfold's control
 * endpoint, so it is always `unavailable`, which the call view shows as such.
 */
export function callStatus() {
  return { unavailable: CALL_UNAVAILABLE, at: Date.now() }
}

/** Behind DSH's /api fence (Host/Origin check, browser auth): status, the live view, call and mute. */
export function fetchRoutes(service, base, sessionId = 'unfold-root') {
  const unavailable = () => Response.json({ error: CALL_UNAVAILABLE, unavailable: CALL_UNAVAILABLE }, { status: 503 })
  return [
    // Stable discovery path, even when the configured API route changes.
    configRoute(base, sessionId, true),
    // The live listening view: a snapshot, then each change (src/live.js).
    ...(service.liveHub ? [{ path: `${base}/live`, methods: ['GET'], requestBody: 'buffered', fetch: async (request) => fetchStream(service.liveHub, request, 'snapshot', service.live.published(), service.live.rev) }] : []),
    { path: `${base}/status`, methods: ['GET'], requestBody: 'buffered', fetch: async () => Response.json(await service.status()) },
    // The presence avatar's call and mute: read as unavailable, and a request is refused (503).
    { path: `${base}/call`, methods: ['GET', 'POST'], requestBody: 'buffered', fetch: async (request) => (request.method === 'GET' ? Response.json(callStatus()) : unavailable()) },
    { path: `${base}/mute`, methods: ['POST'], requestBody: 'buffered', fetch: async () => unavailable() },
  ]
}
