// @rth/dsh-session: RTH's single rolling session projected as ONE DSH root Session. DSH is a view
// and a composer here, never a second brain:
//
// - The composer's `session/prompt` still opens a DSH turn, but our `agent/pre-step` listener
//   (prepended, so nothing else's pre-step runs first) holds that turn: it sends the text as a
//   typed command through the host control owner, which alone holds rth-agentd's controller
//   lease; it streams RTH's reply into the conversation as it arrives (read from rth-agentd as
//   an observer); it records one user and one assistant bubble; and it returns
//   `{kind:'reject'}`, so the DSH loop ends the turn (`blocked`) without a model step: no
//   `agent/request`, no `llm/stream`, no provider.
// - Every other root Session is refused at pre-step (no DSH root loop anywhere), and the
//   first-prompt title model is disabled in the profile.
// - The stop button aborts the turn's signal; that is mapped to the owner's cancel.
// - A retry after a lost answer, a dropped link or an agent restart reuses the same command id,
//   which rth-agentd answers from its durable command log: one decision, one bubble, and an
//   honest `interrupted` bubble when the agent restarted before a result was recorded.
// - Spoken turns are projected, never submitted: a reply
//   the provider opened from the microphone is queued into rth-root as a DSH turn whose one
//   user message carries the rpcId `rth-voice-r-<response id>`; our pre-step recognizes that
//   id BEFORE the typed path, follows the reply by its ids (bridge.runVoice), writes one user
//   bubble with what was heard for it, then one reply bubble, and rejects the step. No command,
//   no command_submitted, no model call. What was heard and taken by no reply is its own user
//   bubble (`rth-voice-h-…`), no reply.
// - Clean bubbles: chat bubbles carry the words only, no labels. The user
//   bubble is written when its reply STARTS. For a backend that cannot pair a transcript with its
//   reply by id (Gemini), the words are the reply's by adjacency in time (src/voice.mjs); how
//   they were associated is shown in Trajectory and Mind's audit, not in chat. An interrupted or
//   doubtful spoken reply carries one short italic mark at its end, not a paragraph.
import { createRequire } from 'node:module'
import { readFileSync, realpathSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import Schema from '@deepseek-ai/schemastery'
import { fetchStream, sseHub } from '@rth/dsh-sse'
import { RthBridge } from './src/bridge.mjs'
import { LiveView } from './src/live.mjs'
import { HostClient } from './src/host-client.mjs'
// The credential redactor the Events surface draws through (key-aware, quoted and escaped values):
// every panic text this plugin exports passes it first.
import { redact } from '../dsh-mind/src/redact.mjs'
import { TASK_KEY } from './src/delegate/executor.mjs'
import { makeLiveExecutor } from './src/delegate/dsh-adapters.mjs'
import { TranscriptJournal } from './src/delegate/journal.mjs'
import { TASK_PROVIDER } from './src/delegate/snapshot.mjs'
import { sha256 } from './src/delegate/canon.mjs'

export const name = 'rth-dsh-session'

const HERE = dirname(fileURLToPath(import.meta.url))

/**
 * The host's own copies of the DSH runtime packages: the loop checks `instanceof` and scope
 * dispatch uses symbol keys, so a second copy would silently not match. Resolved from the
 * running DSH executable, then from the repo's shell (tests).
 */
async function hostModule(id) {
  const bases = []
  if (process.env.RTH_DSH_MODULES) bases.push(join(process.env.RTH_DSH_MODULES, 'noop.js'))
  try {
    if (process.argv[1]) bases.push(realpathSync(process.argv[1]))
  } catch {}
  bases.push(join(HERE, '../../shell/package.json'))
  for (const base of bases) {
    try {
      return await import(pathToFileURL(createRequire(base).resolve(id)).href)
    } catch {}
  }
  throw new Error(`rth-dsh-session: cannot resolve the host's ${id}`)
}

const llm = await hostModule('@deepseek-ai/dsh-llm')
const agentPkg = await hostModule('@deepseek-ai/dsh-agent')

export const Config = Schema.object({
  sessionId: Schema.string().default('rth-root').description('the one DSH root Session that shows Unfold'),
  title: Schema.string().default('Unfold'),
  workspaceDir: Schema.string().description('workspace directory for the root Session; defaults to <DSH_HOME>/rth-workspace'),
  blockOtherSessions: Schema.boolean().default(true).description('refuse turns in every other root Session (DSH never runs a root loop)'),
  host: Schema.object({
    url: Schema.string().default('http://127.0.0.1:7781'),
    tokenFile: Schema.string().description("the host owner's token file (0600)"),
  }),
  agentd: Schema.object({
    host: Schema.string().default('127.0.0.1'),
    port: Schema.natural().default(7700),
  }),
  route: Schema.string().default('/api/rth-session'),
  // Delegation: off unless enabled with a pinned price table.
  delegate: Schema.object({
    enabled: Schema.boolean().default(false).description('run the report worker (needs prices pinned and the host task routes)'),
    provider: Schema.string().default('openai'),
    model: Schema.string().description('the low-cost OpenAI text model id, pinned at enable time'),
    pendingDir: Schema.string().description('defaults to <DSH_HOME>/rth/pending'),
    spendLedger: Schema.string().description('defaults to <DSH_HOME>/rth/spend.jsonl'),
    perTaskUsd: Schema.number().default(0.05),
    perDayUsd: Schema.number().default(1),
    // A run's own report limits; unset keeps the everyday defaults (budget.mjs DEFAULTS).
    maxSteps: Schema.natural().description('steps per report (default 3)'),
    attemptsPerStep: Schema.natural().description('provider attempts per step: the route\'s maxRetries + 1 (default 2)'),
    maxOutputTokens: Schema.natural().description('output tokens per worker request, reasoning included (default 4000)'),
    reasoningEffort: Schema.string().description('the worker\'s reasoning effort (DSH thinking level); unset sends none'),
    // Acceptance runs only: the acceptance's control record, pinned.
    accept: Schema.object({
      control: Schema.string(),
      controlSha256: Schema.string(),
    }).description('acceptance runs: the spend control record; when set, every worker step needs a claimed attempt'),
    prices: Schema.object({
      model: Schema.string(),
      inputPerMTok: Schema.number(),
      outputPerMTok: Schema.number(),
      reasoningPerMTok: Schema.number(),
      source: Schema.string().description('where the prices were read, with its date'),
      pinnedAt: Schema.string(),
    }).description('unpinned → delegate stays off'),
  }),
  faults: Schema.object({
    dropAcks: Schema.natural().default(0).description('proof only: pretend the first N owner answers were lost'),
    route: Schema.boolean().default(false).description('proof only: POST <route>/faults {dropAcks} arms lost acks at run time'),
  }),
})

export const inject = ['agents', 'sessions']

const textOfContent = (message) => (message?.content ?? []).filter((b) => b?.type === 'text').map((b) => b.text).join('')

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
      return `[Unfold refused this command: ${final.detail}]`
    case 'stop_unconfirmed':
      return `${said}${said ? '\n\n' : ''}[stopped locally; the backend outcome is unknown]`
    case 'incomplete':
      return `${said}${said ? '\n\n' : ''}[possibly incomplete: ${final.detail}]`
    case 'unknown':
      return `${said}${said ? '\n\n' : ''}[outcome unknown: ${final.detail}]`
    case 'not_sent':
      return `[not sent: ${final.detail}]`
    default:
      return `${said}${said ? '\n\n' : ''}[failed: ${final.detail ?? 'no detail'}]`
  }
}

export const VOICE_KEY = 'rth-voice-'
export { TASK_KEY }
/** A delegation report item: its own held turn, never a command or a voice turn. */
export function taskKeyOf(message) {
  const src = message?.source
  return src?.kind === 'user' && typeof src.rpcId === 'string' && src.rpcId.startsWith(TASK_KEY) ? src.rpcId : null
}
const queuedKeyOf = (m) => voiceKeyOf(m) ?? taskKeyOf(m)
const MAX_VOICE_ITEMS = 64

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

/**
 * The root Session's workspace: config.workspaceDir, else $DSH_HOME/rth-workspace. With neither it
 * is refused (no fallback to the working directory, which a Finder launch makes `/`).
 */
export function workspaceDirFor(config, env) {
  if (config.workspaceDir) return config.workspaceDir
  if (env.DSH_HOME) return join(env.DSH_HOME, 'rth-workspace')
  throw new Error('rth-session: DSH_HOME is not set and no workspaceDir is configured; refusing to create the root workspace in the working directory')
}

export function apply(ctx, config, deps = {}) {
  config = Config(config ?? {})
  const ROOT = config.sessionId
  // `deps` is for tests only (a scripted owner and bridge); DSH calls apply(ctx, config).
  const host = deps.host ?? new HostClient({ url: config.host.url, tokenFile: config.host.tokenFile })
  const bridge = deps.bridge ?? new RthBridge({
    host,
    agentd: config.agentd,
    faults: config.faults,
    log: (m) => ctx.logger?.info?.(m),
  })
  const PROMPT_MS = deps.promptMs ?? 5_000
  const counters = { llm_stream: 0, agent_request: 0, root_model_requests: 0, other_model_requests: 0, task_model_requests: {}, task_turns: 0, task_bubbles: 0, refused_subagents: 0, root_turns: 0, blocked_turns: 0, bubbles: 0, interrupted_bubbles: 0, voice_turns: 0, voice_notes: 0, voice_dropped: 0, voice_waiting: 0, drains: 0, not_sent: 0 }
  const revisions = new WeakMap()
  const turns = []

  // ── spoken turns: queued into rth-root, deduped by key (here, and in the durable log) ──
  const voice = { items: new Map(), chain: Promise.resolve(), controller: null }
  const log = (m) => ctx.logger?.info?.(m)
  const inSession = (agent, key) =>
    agent.inbox?.nextTurn?.some((m) => queuedKeyOf(m) === key) ||
    agent.inbox?.nextStep?.some((m) => queuedKeyOf(m) === key) ||
    agent.session?.snapshotEvents?.().some((e) => e.type === 'user/message' && queuedKeyOf(e.data) === key)
  async function deliver(item) {
    if (item.state !== 'pending' && item.state !== 'waiting') return
    const content = [{ type: 'text', text: item.text }]
    // The Session API resumes a cold rth-root and dedupes on the rpcId against the durable log.
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
        log(`rth-session: voice item ${item.key} not queued through the Session API (${e?.message ?? e}); trying the live agent`)
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
    // No live rth-root yet: kept (bounded) until it appears.
    if (item.state !== 'waiting') counters.voice_waiting++
    item.state = 'waiting'
  }
  // Liveness. DSH's driver drains its inbox only after a turn that ENDS
  // (`turn()` returns `inbox.hasPending`); our intentional `reject` returns false before that
  // check, so the driver goes idle with user work still queued (a prompt sent while a turn ran:
  // typed or spoken). After every rth-root turn, once the agent is idle (`whenIdle`: an event,
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
    for (const item of voice.items.values()) if (item.state === 'waiting') voice.chain = voice.chain.then(() => deliver(item)).catch((e) => log(`rth-session: voice delivery failed: ${e?.message ?? e}`))
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
    voice.chain = voice.chain.then(() => deliver(item)).catch((e) => log(`rth-session: voice delivery failed: ${e?.message ?? e}`))
  }
  // ── the live "listening…" view (src/live.mjs), pushed on change over <route>/live ──
  const liveHub = sseHub()
  const live = new LiveView({ onChange: (v, rev) => liveHub.send('live', v, rev) })
  const liveStats = { onsets: 0, onsets_ignored: 0, onsets_not_admitted: 0, onsets_superseded: 0, input_closed: 0, mind: 'not loaded' }
  // The onset fence: every owner read started for an onset carries the epoch it began in; any newer
  // evidence that makes that onset obsolete (input closed, a reply, a note, unload) advances the
  // epoch, and the read's answer is dropped unless the epoch is still the one it began in, checked
  // immediately before publishing. Not `live.rev`: clearing an
  // already-idle view does not advance it, and that is exactly the race.
  let onsetEpoch = 0
  let unloaded = false
  const fenceOnsets = () => { onsetEpoch++ }
  // The connection's own heartbeat, apart from speech: a reader that hears nothing for its
  // lease drops what it shows. Sent only while someone reads.
  const LIVE_BEAT_MS = 5000
  const beat = setInterval(() => { if (liveHub.clients.size) liveHub.send('beat', { at: Date.now() }, live.rev) }, LIVE_BEAT_MS)
  beat.unref?.()
  ctx.effect(() => () => { unloaded = true; fenceOnsets(); clearInterval(beat); live.stop() }, 'rth-session: live view')
  // rth-host's VAD runs before its admission gate, so a local onset is logged even while the
  // mic is muted or the call input is closed. The onset shortcut is taken only on applied facts read
  // fresh from the owner at that moment: the call active with its input open, the mic applied
  // unmuted. Anything else (muted, muting, unmuting, closed, off, unknown, unreadable) is not
  // listening: the provider's own voice_activity_start still shows it, a moment later.
  const ONSET_FACTS_MS = 800
  const admitted = (body) => {
    const call = body?.host?.call
    const mic = body?.host?.mic
    return call?.state === 'active' && call?.input?.call_input === 'open' && call?.input?.applied_closed === false && mic?.state === 'unmuted' && mic?.applied?.muted === false
  }
  async function onsetFacts() {
    let timer
    try {
      const r = await Promise.race([host.status(), new Promise((resolve) => { timer = setTimeout(() => resolve(null), ONSET_FACTS_MS) })])
      return r?.status === 200 && admitted(r.body)
    } catch {
      return false
    } finally {
      clearTimeout(timer)
    }
  }
  // rth-host's onsets reach DSH only through Mind's host-journal binding: read its public service
  // (the redacted audit envelope of each local_vad_onset row). Without Mind, listening starts on
  // the provider's voice_activity_start, a fraction of a second later, with no level.
  ctx.inject(['rthMind'], (mctx) => {
    mctx.effect(() => {
      const m = mctx.rthMind
      liveStats.mind = 'bound'
      // This attachment's own lifetime: a read it started belongs to it. When it is disposed (Mind
      // detached or replaced, the plugin still loaded), its pending reads are void: `binding.live`
      // goes false and the shared epoch advances. A later attachment gets a binding of its own, so
      // its fresh onsets are admitted as usual.
      const binding = { live: true }
      const off = m?.mind?.onChange?.((change, row) => {
        if (change !== 'row' || row?.lane !== 'host' || row?.history) return
        if (row.kind === 'mute_applied' || row.kind === 'call_devices_released' || row.kind === 'call') {
          // The host closed the input (an applied mute, a call that is not active, devices released):
          // whatever showed listening ends now.
          const env = m.detail?.(row.id)?.envelope
          const closed = row.kind === 'call_devices_released' || (row.kind === 'mute_applied' && env?.muted === true) || (row.kind === 'call' && env?.state && env.state !== 'active')
          if (closed) { liveStats.input_closed++; fenceOnsets(); live.input({ type: 'input-closed' }) }
          return
        }
        if (row.kind !== 'local_vad_onset') return
        const env = m.detail?.(row.id)?.envelope
        if (!env) return
        liveStats.onsets++
        if (env.audible !== false) { liveStats.onsets_ignored++; return }
        const epoch = onsetEpoch
        void onsetFacts().then((ok) => {
          if (unloaded || !binding.live || epoch !== onsetEpoch) { liveStats.onsets_superseded++; return }
          if (!ok) { liveStats.onsets_not_admitted++; return }
          live.input({ type: 'onset', audible: env.audible, level_dbfs: env.level_dbfs })
        })
      })
      return () => { binding.live = false; fenceOnsets(); off?.(); liveStats.mind = 'not loaded' }
    }, 'rth-session: host onsets for the live view')
  })
  // ── delegation: the report executor, text delivery only ──
  // Off unless `delegate.enabled` (the default is off; enabling also needs pinned prices and the
  // host owner's task routes). Tests inject `deps.executor`, built over scripted fakes.
  // The report's source: agentd's journal ids from the observer stream, never chat adjacency.
  const journal = deps.journal ?? new TranscriptJournal()
  let delegationSnap = null
  bridge.onChange((kind, data) => { if (kind === 'journal') journal.ingest(data) })
  const executor = deps.executor ?? (config.delegate.enabled ? makeExecutor(ctx, config, host, { ROOT, log: (m) => ctx.logger?.warn?.(m), offerTask: (i) => offerTask(i), journal }) : null)
  function offerTask({ key, taskId, again }) {
    // `again`: the executor re-runs its own unresolved chain; the earlier item has been delivered.
    if (again && voice.items.get(key)?.state === 'queued') voice.items.delete(key)
    offerVoice({ key, kind: 'task', taskId, text: '(relatório)' })
  }
  if (executor) {
    executor.offerTaskItem ??= offerTask
    // Pending publications are reconciled at bootstrap and on every observer attach: a
    // successor never waits for an operator, and a failed read is retried at the next attach.
    const reconcile = (why) => void executor.reconcile?.().catch((e) => log(`rth-session: delegate reconcile (${why}): ${e?.message ?? e}`))
    bridge.onChange((kind, data) => {
      // The lifetime is captured here, synchronously at ingest, on the link that carried it.
      if (kind === 'task-state') void executor.onTaskState(data, executor.lifetimeOf?.() ?? null).catch((e) => log(`rth-session: task ${data?.task_id}: ${e?.message ?? e}`))
      else if (kind === 'link' && data?.state === 'attached') reconcile('attach')
    })
    ctx.effect(() => {
      reconcile('bootstrap')
      // Unload or reload: fence every task and dispose this instance's activations.
      return () => executor.dispose?.()
    }, 'rth-session: delegate reconcile')
    // The Tasks service (the Bot's `rthDelegation`): a read-only snapshot of
    // the ledger records this executor holds, under one producer lifetime: agentd's boot and this
    // plugin instance. A new agentd boot or a reload is a new lifetime; rows keep the lifetime they
    // were observed under, so the Bot retires them until agentd's inventory re-observes them.
    executor.lifetimeOf ??= () => `${bridge.link.boot_id ?? 'unattached'}|${executor.incarnation?.launch_id ?? ''}|${executor.incarnation?.plugin_instance ?? ''}`
    // A delivered report's text after a restart (rth-root cold): read once from the persisted session
    // through a fresh read handle at the ledger's chat_ref, digest-checked, cached; a failed read
    // backs off (2 s, doubling to 30 s). Never shown unless its digest is the committed one.
    const reportTexts = new Map()
    const persistedReport = (r) => {
      const c = reportTexts.get(r.task_id)
      if (c && (c.text != null || c.pending || Date.now() < c.retryAt)) return c
      const sp = ctx.get?.('sessionPersistence')
      const seq = r.chat_ref?.seq
      if (!sp?.open || !Number.isInteger(seq)) return { text: null, why: sp?.open ? 'no chat_ref seq' : 'no sessionPersistence service' }
      const backoff = Math.min(30_000, (c?.backoff ?? 1_000) * 2)
      reportTexts.set(r.task_id, { text: null, pending: true, backoff, why: 'reading the persisted rth-root' })
      void (async () => {
        let h
        let text = null
        let why = 'the persisted row is not the committed report'
        try {
          h = await sp.open(ROOT, 'read')
          const e = ((await h.read(seq)).events ?? []).find((x) => x.seq === seq && x.type === 'assistant/message' && x.data?.message?.source?.model === `${TASK_KEY}${r.task_id}`)
          const got = e ? (e.data.message.content ?? []).filter((b) => b?.type === 'text').map((b) => b.text).join('') : null
          if (got != null && sha256(got) === r.result_sha256) text = got
          else if (!e) why = `no report row at seq ${seq}`
        } catch (err) {
          why = `read failed: ${String(err?.message ?? err).slice(0, 200)}`
        }
        await h?.close?.().catch?.(() => {})
        reportTexts.set(r.task_id, text != null ? { text } : { text: null, why, backoff, retryAt: Date.now() + backoff })
      })()
      return reportTexts.get(r.task_id)
    }
    delegationSnap = () => delegationSnapshot(executor, ctx.agents?.get?.(ROOT), persistedReport)
    ctx.effect(() => ctx.provide('rthDelegation', { snapshot: delegationSnap }), 'rth-session: delegation snapshot')
  }

  bridge.onChange((kind, data) => {
    if (kind === 'voice-live') {
      // A reply opened: its user bubble is in the chat, so an onset read still pending is obsolete.
      if (data.type === 'reply') fenceOnsets()
      live.input(data)
    } else if (kind === 'voice-heard') {
      fenceOnsets()
      live.input({ type: 'note' })
    }
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
    bridge.start()
    return () => bridge.stop()
  }, 'rth-session: bridge')

  // Counted, never answered: any DSH model traffic at all is a failure of this plugin's promise.
  ctx.on('llm/stream', (options, next) => {
    counters.llm_stream++
    return next()
  }, { prepend: true })
  ctx.on('agent/request', (payload, next) => {
    counters.agent_request++
    // Split: rth-root never runs a model; only an admitted child may.
    const sid = payload?.agent?.session?.id ?? payload?.agent?.id
    const task = executor?.admittedChild(sid)
    if (task) counters.task_model_requests[task] = (counters.task_model_requests[task] ?? 0) + 1
    else if (sid === ROOT) counters.root_model_requests++
    else counters.other_model_requests++
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
      // Only the admitted report child passes, not every subagent session;
      // its per-step budget check runs here, before the step.
      if (config.blockOtherSessions && subagent) {
        const task = executor?.admittedChild(agent.session?.id)
        if (!task) {
          counters.refused_subagents++
          return { kind: 'reject' }
        }
        const gate = executor.beforeChildStep?.(task)
        if (gate && !gate.ok) return { kind: 'reject' }
      }
      return next()
    }
    if (payload.messages.length === 0) return { kind: 'reject' }
    counters.root_turns++
    // DSH claims every pending next-step message plus one next-turn message per turn, so one
    // turn can carry several user messages. Each is its own unit, in claim order: a typed one is
    // its own command (`dsh-<its id>`), never folded into another; a spoken turn or heard note is
    // checked FIRST, so it never reaches the typed path, even when the adapter has forgotten it.
    for (const [i, m] of payload.messages.entries()) {
      // A report item first: never a command, never a voice turn.
      const task = taskKeyOf(m)
      if (task) {
        await runTaskTurn(agent, { ...payload, messages: [m] }, task, i + 1)
        continue
      }
      const key = voiceKeyOf(m)
      const unit = { ...payload, messages: [m] }
      // A later typed unit whose turn was already stopped is shown as not sent, never submitted
      // (the first unit keeps the typed path's own stop handling).
      const notSent = i > 0 && payload.signal?.aborted === true
      await runTurn(agent, unit, key ? voiceItemFor(key) : null, i + 1, !key && notSent)
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

  // The report bubble: the owner's chain inside one held rth-root turn. The
  // floor is persisted before the append; delivery needs the awaited flush AND a fresh read-back
  // through the persistence service; no user bubble is written for the queued item.
  async function runTaskTurn(agent, { turn }, key, step) {
    const s = agent.session
    const taskId = key.slice(TASK_KEY.length)
    counters.task_turns++
    let appendedSeq = null
    let appendedNow = false
    let flushed = null
    let flushError = null
    let prep = null
    s.append('step/start', { turn, step })
    try {
      if (turn === 1 && step === 1) s.append('system/message', { turn, step, message: llm.createSystemMessage('') }, { surfaceOp: 'append' })
      prep = executor?.prepareAppend(taskId, s.seq ?? s.snapshotEvents?.().length) ?? null
      if (prep) {
        const existing = (s.snapshotEvents?.() ?? []).filter((e) => e.type === 'assistant/message' && e.data?.message?.source?.model === key)
        const same = existing.find((e) => sha256(textOfContent(e.data.message)) === prep.result_sha256)
        if (same) appendedSeq = same.seq
        else if (!existing.length) {
          const ev = s.append('assistant/message', { turn, step, stream: [], message: llm.createAssistantMessage({ content: [{ type: 'text', text: prep.text }], source: { provider: TASK_PROVIDER, model: key } }) }, { surfaceOp: 'append' })
          appendedSeq = ev.seq
          appendedNow = true
          counters.task_bubbles++
        }
      }
    } finally {
      afterTurn(agent)
      s.append('step/end', { turn, step })
      try {
        // `true` only when a durability listener took part (DSH sessions.flush); the executor
        // decides what that proves together with whether this turn appended.
        flushed = await ctx.sessions.flush(s)
      } catch (e) {
        flushError = e
      }
    }
    if (prep) await executor.afterFlush(taskId, { appendedSeq, appendedNow, flushed, flushError })
  }

  async function runTurn(agent, { messages, turn, signal }, spoken = null, step = 1, notSent = false) {
    const s = agent.session
    // A voice-keyed message is never part of a command's text.
    const text = spoken ? '' : textOf(messages.filter((m) => !voiceKeyOf(m)))
    // Stable per message: the same composer message is the same RTH command on every retry.
    const commandId = spoken ? null : `dsh-${messages[0].id}`
    if (spoken) {
      counters[spoken.kind === 'reply' ? 'voice_turns' : 'voice_notes']++
      spoken.state = 'shown'
    }
    const record = spoken
      ? { voice: spoken.kind, key: spoken.key, responseId: spoken.responseId ?? null, decisionId: spoken.decisionId ?? null, turn, started: Date.now(), status: 'following', deltas: 0 }
      : { commandId, turn, text, started: Date.now(), status: 'submitting', deltas: 0 }
    turns.push(record)
    if (turns.length > 200) turns.shift()
    if (!spoken) journal.typed(commandId, text)
    const acc = new llm.AssistantStreamAccumulator()
    const asm = new llm.BlockAssembler()
    const attemptId = llm.LlmAttemptId(`${s.id}:rth:${turn}:${commandId ?? spoken.key}`)
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
      if (!spoken) for (const m of messages) s.append('user/message', m, { surfaceOp: 'append' })
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
          ? (async function* () { yield { type: 'final', status: 'not_sent', text: '', detail: 'the turn was stopped before this message was sent; it was not sent to Unfold' } })()
          : bridge.run(commandId, text, signal)
      for await (const ev of events) {
        if (ev.type === 'admitted') {
          record.status = ev.command?.status
          record.generation = ev.command?.generation
          record.duplicate = ev.duplicate
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
        ? `rth-agentd voice (${spoken.responseId}${final.voice?.decisionId ? `, decision ${final.voice.decisionId}` : ''})`
        : `rth-agentd (${commandId})`
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
              message: llm.createAssistantMessage({ content, source: { provider: 'rth', model } }),
              ...(ok ? {} : { interrupted: true }),
            },
            { surfaceOp: 'append' },
          )
        : s.append('assistant/attempt', { turn, step, stream: [...acc.snapshot()] })
      counters.bubbles += content.length ? 1 : 0
      if (!ok) counters.interrupted_bubbles++
      frame({ type: 'end', attemptId, index, outcome: { kind: 'committed', eventType: ev.type, seq: ev.seq } })
      record.finished = Date.now()
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

  // The one root Session: created once in an RTH workspace, then left alone (DSH persists it;
  // a cold Session is resumed by the next prompt).
  ctx.inject(['sessionController'], (sctx) => {
    sctx.effect(() => {
      voice.controller = sctx.sessionController
      flushVoice()
      return () => { voice.controller = null }
    }, 'rth-session: spoken turns through the Session API')
  })

  ctx.inject(['sessionController', 'workspaceRegistry'], (sctx) => {
    sctx.effect(() => {
      void (async () => {
        try {
          const known = await sctx.get('sessionPersistence')?.stat?.(ROOT)
          if (known) return
          const dir = workspaceDirFor(config, process.env)
          const { mkdirSync } = await import('node:fs')
          mkdirSync(dir, { recursive: true })
          const ws = await sctx.workspaceRegistry.create(dir, config.title)
          await sctx.sessionController.create({ sessionId: ROOT, workspaceId: ws.id })
        } catch (e) {
          ctx.logger?.warn?.(`rth-session: could not prepare the root Session: ${e?.message ?? e}`)
        }
      })()
    }, 'rth-session: root session')
  })

  const service = {
    bridge,
    counters,
    faultsRoute: config.faults.route,
    turns,
    live,
    liveHub,
    async status() {
      const owner = await host.status()
      const caps = ctx.get('rthCapabilities')?.map?.()
      return {
        session: { dsh_session_id: ROOT, adapter_link: { ...bridge.link }, stats: { ...bridge.stats } },
        owner: owner.status === 200 ? owner.body : { unavailable: owner.body?.error ?? `status ${owner.status}` },
        // Kept apart, never merged: what is ACTIVE (controller record + receipt), what was
        // last MEASURED (the capability map's candidate), and the SOURCE (self-repo).
        active: owner.status === 200 ? owner.body.deployment : null,
        candidate: caps ? (caps.latest_measured_candidate ?? null) : { unavailable: 'capability map not loaded' },
        source: owner.status === 200 ? owner.body.self_repo : null,
        dsh: { model_calls: { llm_stream: counters.llm_stream, agent_request: counters.agent_request, root_model_requests: counters.root_model_requests, task_model_requests: { ...counters.task_model_requests } }, ...counters },
        // The Tasks rows as served (their report text left out of this status).
        delegate: executor ? { enabled: true, stats: { ...executor.stats }, recent: [...(executor.recent ?? [])], tasks: (delegationSnap?.().tasks ?? []).map(({ report_text, ...row }) => ({ ...row, has_report_text: report_text != null })) } : { enabled: false },
        voice: { ...bridge.voiceStats, transcripts: { ...bridge.ledger.stats }, open: bridge.voiceTracks.size, items: voice.items.size },
        live: { view: live.published(), rev: live.rev, clients: liveHub.clients.size, ...liveStats },
        turns: turns.slice(-20),
      }
    },
  }
  ctx.effect(() => ctx.provide('rthSession', service), 'rth-session: service')

  ctx.inject(['connection'], (sctx) => {
    sctx.effect(() => {
      const disposers = fetchRoutes(service, config.route, host).map((route) => sctx.connection.fetch.register(route))
      return () => Promise.all(disposers.map((dispose) => dispose()))
    }, 'rth-session: routes')
  })
}

/** When this DSH started: an exit record from before it describes an older rth-host. */
const STARTED_MS = Date.now()

/**
 * How the RTH app that started this DSH reports its rth-host's own exit, from the environment it
 * gave DSH: `RTH_HOST_EXIT_FILE` (its `<data>/host/last-exit.json`) and
 * `RTH_HOST_RESTART=dsh-stdout` (it takes a restart request as one line on DSH's stdout). An app
 * that sets neither is unaffected.
 */
export function hostStopConfig(env = process.env) {
  return { exitFile: env.RTH_HOST_EXIT_FILE || null, ownerFile: env.RTH_HOST_OWNER_FILE || null, restart: env.RTH_HOST_RESTART === 'dsh-stdout', since: STARTED_MS }
}

/** The pid of the owner the app last started (`<data>/host/current-owner.json`), or undefined
 * when the app gives no such record, or null when it gives one that cannot be read. */
function currentOwner(stop) {
  if (!stop?.ownerFile) return undefined
  try {
    const o = JSON.parse(readFileSync(stop.ownerFile, 'utf8'))
    return o?.kind === 'host_owner' && Number.isInteger(o.pid) ? o.pid : null
  } catch {
    return null
  }
}

/** The one line the RTH app reads on DSH's stdout as "Restart voice host" (main.rs `dsh_control`). */
export const RESTART_LINE = 'rth-app: restart-host'

/**
 * The app's record of an rth-host that stopped on its own, as current evidence, or null. It counts
 * only if it was written while this DSH ran AND it names the owner the app last started
 * (`current-owner.json`, rewritten at every spawn: a restart retires the old death at once, even
 * if this DSH never reads the successor). Without an owner record (an app that writes none), it
 * must name the last owner this DSH saw live. An owner record that cannot be read proves nothing:
 * unknown, never "stopped".
 * The death itself stays as history (the next launch reports it). The whole record is redacted
 * BEFORE any field is trimmed.
 */
export function hostStopped(stop) {
  if (!stop?.exitFile) return null
  let r
  try {
    r = JSON.parse(readFileSync(stop.exitFile, 'utf8'))
  } catch {
    return null
  }
  if (r?.kind !== 'host_exit' || !(Number(r.utc_ms) >= stop.since)) return null
  // The app's owner record is the authority when it gives one; the last owner this DSH saw live
  // is the fallback for an app that does not.
  const owner = currentOwner(stop)
  if (owner !== undefined ? owner !== r.pid : stop.live?.pid != null && r.pid !== stop.live.pid) return null
  const safe = redact({ exit: r.exit, panic: r.panic })
  const cut = (v, n) => (typeof v === 'string' ? (v.length > n ? `${v.slice(0, n)}…` : v) : null)
  return { pid: r.pid ?? null, exit: cut(safe.exit, 120), panic: cut(safe.panic, 400), at: r.utc_ms }
}

/**
 * The call button's read: the owner's `host.call` and `host.mic` receipts and the link state,
 * nothing else (no controller or repo read). `unavailable` when the owner cannot be read; then,
 * if the app recorded that its rth-host stopped, `hostStopped` (and `restart` when it can take
 * a restart request from here).
 */
export async function callStatus(host, stop = hostStopConfig()) {
  const r = await host.status()
  // The last owner seen live (its pid): what a later death record must name to count.
  if (r.status === 200 && Number.isInteger(r.body?.host?.pid)) stop.live = { pid: r.body.host.pid, at: Date.now() }
  if (r.status !== 200) {
    const out = { unavailable: r.body?.error ?? `owner status ${r.status}` }
    const stopped = hostStopped(stop)
    if (stopped) Object.assign(out, { hostStopped: stopped, restart: Boolean(stop.restart) })
    return out
  }
  return { call: r.body?.host?.call ?? null, mic: r.body?.host?.mic ?? null, link: r.body?.link?.state ?? null, at: Date.now() }
}

/** Behind DSH's /api fence (Host/Origin check, browser auth): status and stop, the call and mute requests. */
export function fetchRoutes(service, base, host, { stop = hostStopConfig(), emit = (line) => process.stdout.write(`${line}\n`) } = {}) {
  return [
    // The live listening view: a snapshot, then each change (src/live.mjs).
    ...(service.liveHub ? [{ path: `${base}/live`, methods: ['GET'], requestBody: 'buffered', fetch: async (request) => fetchStream(service.liveHub, request, 'snapshot', service.live.published(), service.live.rev) }] : []),
    { path: `${base}/status`, methods: ['GET'], requestBody: 'buffered', fetch: async () => Response.json(await service.status()) },
    ...(service.faultsRoute
      ? [{
          path: `${base}/faults`,
          methods: ['POST'],
          requestBody: 'buffered',
          fetch: async (request) => {
            const body = await request.json().catch(() => ({}))
            service.bridge.faults.dropAcks = Number(body.dropAcks ?? 0)
            return Response.json({ faults: service.bridge.faults })
          },
        }]
      : []),
    {
      path: `${base}/stop`,
      methods: ['POST'],
      requestBody: 'buffered',
      fetch: async () => {
        const r = await host.cancel()
        return Response.json(r.body, { status: r.status === 200 ? 200 : 502 })
      },
    },
    // The composer's call button: GET reads the receipts, POST {call: 'start'|'end'} is a
    // request to the native owner (the browser never opens a microphone).
    {
      path: `${base}/call`,
      methods: ['GET', 'POST'],
      requestBody: 'buffered',
      fetch: async (request) => {
        if (request.method === 'GET') return Response.json(await callStatus(host, stop))
        const body = await request.json().catch(() => ({}))
        if (body.call !== 'start' && body.call !== 'end') return Response.json({ error: "body must be {call: 'start' | 'end'}" }, { status: 400 })
        const r = await host.call(body.call)
        return Response.json(r.body, { status: r.status === 0 ? 502 : r.status })
      },
    },
    // "Restart voice host" from the avatar: a request to the RTH app that owns rth-host (one line
    // on DSH's stdout, which the app reads), honoured only when that app said it takes one and its
    // rth-host is not answering. The app itself refuses unless its own owner slot is empty.
    {
      path: `${base}/restart-host`,
      methods: ['POST'],
      requestBody: 'buffered',
      fetch: async () => {
        if (!stop.restart) return Response.json({ error: 'this Unfold app cannot restart rth-host from here: use the Unfold menu or the tray (Restart voice host)' }, { status: 409 })
        const r = await host.status()
        if (r.status === 200) return Response.json({ error: 'rth-host is running: nothing to restart' }, { status: 409 })
        emit(RESTART_LINE)
        return Response.json({ requested: true }, { status: 202 })
      },
    },
    {
      path: `${base}/mute`,
      methods: ['POST'],
      requestBody: 'buffered',
      fetch: async (request) => {
        const body = await request.json().catch(() => ({}))
        if (typeof body.muted !== 'boolean') return Response.json({ error: 'body must be {muted: true | false}' }, { status: 400 })
        const r = await host.mute(body.muted)
        return Response.json(r.body, { status: r.status === 0 ? 502 : r.status })
      },
    },
  ]
}

/**
 * `rthDelegation.snapshot()`: `{lifetime, tasks}` for the Bot's read-only Tasks view.
 * Each row is a ledger record as this executor last accepted it, with the fields the Bot reads:
 * `rev` is agentd's; `live_seq` is the executor's explicit liveness (it still holds that task's live
 * activation); there are no timestamps (the ledger has none, so none are invented). The report text
 * is given only for a delivered report: this process's validated result, or the rth-root row the
 * ledger's `chat_ref` names when its digest matches (the live session, else one read of the persisted
 * one); otherwise null with `report_text_reason`.
 */
export function delegationSnapshot(executor, root, persistedReport = () => ({ text: null, why: 'no persisted reader' })) {
  const lifetime = executor.lifetimeOf?.() ?? null
  const tasks = []
  for (const t of executor.tasks?.values?.() ?? []) {
    // The display record and the lifetime that observed it, never the operational one.
    const r = t.view?.rec
    if (!r?.task_id || !Number.isFinite(r.rev)) continue
    let report_text = null
    let report_text_reason = null
    if (r.publication === 'delivered') {
      if (t.result?.report && t.result.result_sha256 === r.result_sha256) report_text = t.result.report
      else {
        const row = root?.session?.snapshotEvents?.().find((e) => e.seq === r.chat_ref?.seq && e.type === 'assistant/message' && e.data?.message?.source?.model === `${TASK_KEY}${r.task_id}`)
        const text = row ? (row.data.message.content ?? []).filter((b) => b?.type === 'text').map((b) => b.text).join('') : null
        if (text != null && sha256(text) === r.result_sha256) report_text = text
        else {
          const got = persistedReport(r)
          report_text = got?.text ?? null
          if (report_text == null) report_text_reason = got?.why ?? 'the delivered row is not readable'
        }
      }
    }
    tasks.push({
      task_id: r.task_id, rev: r.rev, live_seq: t.view.lifetime === lifetime ? t.liveSeq ?? 0 : 0, lifetime: t.view.lifetime ?? null, kind: 'report',
      execution: r.execution, publication: r.publication, title: String(r.args?.task ?? 'Report').slice(0, 120),
      child_session_id: r.child_id ?? null, report_text, report_text_reason, accepted_at: null, delivered_at: null, updated_at: null,
    })
  }
  return { lifetime, tasks }
}

/**
 * The live executor over DSH's real services (src/delegate/dsh-adapters.mjs). Disabled by default.
 */
function makeExecutor(ctx, config, host, opts) {
  return makeLiveExecutor(ctx, config, host, opts)
}
