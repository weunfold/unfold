// The RTH settings projection: a pure function from what the plugin has READ to what the panel
// shows. Every value is a leaf `{ id, label, value, state, source, observed_at, note }`:
//
// - `live`: read from an authoritative snapshot inside its lease (host status), or observed in
//   the CURRENT uninterrupted observer attachment, from a positively known boot (agentd events).
//   The observer has no replay: a fact seen before a link gap is history, never live again until
//   it is re-observed (a new Hello re-establishes the boot; only a new ready line re-establishes
//   the provider session's settings).
// - `stale`: there was a value, but it is past its lease, from a previous boot, or its link is
//   down. The value is NOT shown as current: `value` is null and `last` keeps what was read.
// - `unknown`: nothing authoritative says. Never a default, never a guess.
// - `source`: a constant read from the working tree, not reported by the running agent.
//
// Four revisions are kept apart and never merged: the self-repo working-tree
// HEAD, the active binary deployment, the effective config revision (the provider setup_id)
// and the context revision (the context lineage and its watermark).
//
// Each leaf also carries its `basis` (host | events | bot | source) and, for host values,
// `expires_at`, so the browser can expire what it shows on its own clock (src/display.mjs).
import { verifyPrompt, parseReady } from './sources.mjs'

export const HOST_LEASE_MS = 15_000
/** Event-derived values shown in the browser expire this long after the server last confirmed them. */
export const STREAM_LEASE_MS = 15_000

/** The latest agentd facts seen in the rth-events window: Hello, the gemini ready line, context. */
export class Facts {
  constructor() {
    this.reset()
  }

  reset() {
    this.hello = null
    this.ready = null
    this.context = null
    this.order = 0
    this.epoch = 0
  }

  /** The observer link left `attached`: everything seen so far becomes history. */
  gap() {
    this.epoch++
  }

  /**
   * `seenAt`: when this plugin saw the row arrive, or null for a row already in the window.
   * agentd sends a Hello first on every attachment, so a Hello starts a new observation epoch:
   * rows after the newest Hello, while the link is attached, are this attachment's.
   */
  ingest(row, seenAt = null) {
    if (!row || row.source !== 'guest') return false
    const isHello = row.wire === 'hello'
    if (!isHello && row.wire !== 'context_restore' && !(row.wire === 'log' && parseReady(row.fields?.msg))) return false
    if (isHello) this.epoch++
    const f = { id: row.id, producer: row.producer ?? null, fields: row.fields ?? {}, seen_at: seenAt, order: ++this.order, epoch: this.epoch }
    if (isHello) this.hello = f
    else if (row.wire === 'context_restore') this.context = f
    else if (row.wire === 'log' && parseReady(row.fields?.msg)) this.ready = { ...f, parsed: parseReady(row.fields.msg) }
    else return false
    return true
  }
}

const bootOf = (producer) => (typeof producer === 'string' ? producer.replace(/^guest-/, '') : null)
const get = (obj, path) => path.split('.').reduce((o, k) => (o === null || o === undefined ? undefined : o[k]), obj)
const has = (obj, path) => {
  const parts = path.split('.')
  const parent = parts.length > 1 ? get(obj, parts.slice(0, -1).join('.')) : obj
  return parent !== null && typeof parent === 'object' && parts.at(-1) in parent
}

/**
 * @param {object} input
 * @param {{ body: object|null, read_at: number|null, ok: boolean, error: string|null } | null} input.host
 *   the host owner's /v1/status as read through rth-session (null: rth-session not loaded)
 * @param {Facts|null} input.facts  null: rth-events not loaded
 * @param {{ state: string } | null} input.eventsLink  the rth-events observer link
 * @param {{ memoryReview?: unknown } | null} input.bot  picked from the loaded Bot config
 * @param {object|null} input.policies  readPolicies()
 * @param {object|null} input.bounds  readContextBounds()
 */
export function project({ host = null, facts = null, eventsLink = null, bot = null, policies = null, bounds = null, now = Date.now(), leaseMs = HOST_LEASE_MS, streamLeaseMs = STREAM_LEASE_MS } = {}) {
  const H = hostReader(host, now, leaseMs)
  const A = agentReader(facts, eventsLink, H)
  const ready = A.ready
  const proof = ready.state === 'live' ? verifyPrompt(ready.value, policies?.policies) : { status: 'unverifiable', reason: ready.note }
  const ctx = A.context

  const runtime = [
    H.leaf('backend', 'Backend', 'link.backend'),
    H.leaf('backend_receipt', 'Backend in the activation receipt', 'deployment.active.receipt.backend', { also: 'deployment.active.receipt.backend_source' }),
    A.readyLeaf('model', 'Model', 'model'),
    A.readyLeaf('provider_generation', 'Provider session generation (gen)', 'gen', { note: 'counts provider sessions in this boot; a reconnect without resumption loses the provider context' }),
    A.readyLeaf('resumed', 'Last provider session resumed', 'resumed'),
    H.leaf('command_generation', 'Command generation', 'link.command_generation', { note: 'commands admitted to the durable command log; a submitter sends it as expected_generation' }),
    H.leaf('link_state', 'Host link to agentd', 'link.state', { also: 'link.since_utc_ms' }),
    H.leaf('agent', 'Agent address', 'link.agent'),
    H.leaf('boot_id', 'Boot', 'link.boot_id'),
    H.leaf('instance_id', 'Instance', 'link.instance_id'),
    H.leaf('session_id', 'Session', 'link.session_id'),
    H.leaf('deployment_hello', 'Deployment the agent reports (Hello)', 'link.deployment.deployment_id'),
    H.leaf('deployment_active', 'Deployment the controller activated', 'deployment.active.deployment_id'),
    agree(H),
    H.leaf('artifact_id', 'Active artifact (agent executable sha256)', 'deployment.active.artifact_id'),
    H.leaf('manifest_sha256', 'Active manifest sha256', 'deployment.active.manifest_sha256'),
    H.leaf('attestation', 'Attestation', 'deployment.active.attestation'),
    H.leaf('record_generation', 'Activation record generation', 'deployment.record_generation'),
    H.leaf('deployment_pending', 'Pending transition', 'deployment.pending'),
    H.leaf('self_repo_head', 'Self-repo HEAD (rth-core work tree)', 'self_repo.head'),
    H.leaf('self_repo_dirty', 'Self-repo dirty', 'self_repo.dirty', { also: 'self_repo.dirty_paths' }),
    eventsLinkLeaf(facts, eventsLink),
  ]

  const voice = [
    H.leaf('voice', 'Voice I/O', 'host.voice'),
    H.leaf('voice_io', 'Voice source', 'host.voice_policy.voice_io'),
    H.leaf('barge_in', 'Barge-in mode', 'host.voice_policy.barge_in'),
    H.leaf('talk', 'Talk mode', 'host.voice_policy.talk', { note: 'open or ptt; whether push-to-talk is held right now is not in any status' }),
    H.leaf('turn_taking', 'Turn-taking', 'host.voice_policy.turn_taking'),
    A.readyLeaf('provider_vad', 'Provider VAD knob', 'vad'),
    H.leaf('echo_cancellation', 'Echo cancellation', 'host.voice_policy.echo_cancellation'),
    H.leaf('local_cuts', 'Local VAD / gate may cut the speaker', 'host.voice_policy.local_vad_cuts', { also: 'host.voice_policy.local_gate_cuts' }),
    H.leaf('local_vad', 'Local VAD', 'host.voice_policy.vad'),
    H.leaf('chunk_ms', 'Microphone chunk (ms)', 'host.voice_policy.chunk_ms'),
    H.leaf('queue_seconds', 'Playout queue (s)', 'host.voice_policy.queue_seconds'),
    H.leaf('mic_state', 'Microphone', 'host.mic.state'),
    H.leaf('mic_requested', 'Mute requested', 'host.mic.requested'),
    H.leaf('mic_applied', 'Mute applied', 'host.mic.applied'),
    H.leaf('device', 'Voice device', 'host.voice_device.phase', { nullParent: 'host.voice_device', nullNote: 'the host reports no voice device (voice_device: null)' }),
    H.leaf('device_route', 'Default input / output at start', 'host.voice_device.default_devices_at_start', { nullParent: 'host.voice_device', nullNote: 'no voice device', also: 'host.voice_device.default_device_changes' }),
    H.leaf('mic_mode', 'macOS mic mode', 'host.voice_device.mic_mode', { nullParent: 'host.voice_device', nullNote: 'no voice device' }),
  ]

  const prompt = [
    A.readyLeaf('policy', 'Voice policy (versioned instruction)', 'policy'),
    A.readyLeaf('system_extra', 'Extra instruction appended', 'system_extra'),
    A.readyLeaf('proactive', 'Proactive audio', 'proactive'),
    A.readyLeaf('transcription_langs', 'Transcription language hints', 'transcription_langs'),
    A.readyLeaf('setup_id', 'Setup identity (config revision)', 'setup_id'),
    {
      id: 'prompt_proof',
      label: 'Prompt check',
      basis: 'events',
      value: proof.status,
      state: proof.status === 'verified' ? 'live' : 'unknown',
      source: `setup_id replica over ${policies?.path ?? 'gemini.rs (not read)'}`,
      observed_at: ready.observed_at,
      note: proof.status === 'verified'
        ? 'the working-tree text and the ready line\'s knobs reproduce the running setup_id exactly'
        : proof.reason,
    },
    {
      id: 'prompt_text',
      label: 'Effective system instruction',
      basis: 'events',
      value: proof.status === 'verified' ? (proof.text ?? '(none: the setup sends no system instruction)') : null,
      state: proof.status === 'verified' ? 'live' : 'unknown',
      source: proof.status === 'verified' ? `${policies.path} ${policies.policies[proof.policy]?.constant ?? ''}`.trim() : 'not reported by the agent',
      observed_at: ready.observed_at,
      note: proof.status === 'verified'
        ? `text sha256 ${policies.policies[proof.policy]?.sha256?.slice(0, 12) ?? '-'}; working-tree source, proven equal to what the agent sends`
        : 'shown only once the setup_id check passes',
      revision: proof.status === 'verified' ? (policies.policies[proof.policy]?.sha256 ?? null) : null,
    },
    botLeaf(bot),
  ]

  const context = [
    H.leaf('context_link', 'Model context at connect', 'link.context'),
    ctxLeaf(ctx, 'context_status', 'Restore status', 'status'),
    ctxLeaf(ctx, 'context_method', 'Restore method', 'method'),
    ctxLeaf(ctx, 'context_turns', 'Turns restored', 'turns'),
    ctxLeaf(ctx, 'context_tail', 'Journal tail records folded', 'tail_records'),
    ctxLeaf(ctx, 'context_truncated', 'Oldest turns dropped by the bound', 'truncated_turns'),
    ctxLeaf(ctx, 'context_lineage', 'Context lineage since (journal byte)', 'lineage_since'),
    ctxLeaf(ctx, 'context_watermark', 'Snapshot watermark (journal bytes)', 'watermark'),
    ctxLeaf(ctx, 'context_snapshot', 'Snapshot format', 'snapshot'),
    ctxLeaf(ctx, 'context_detail', 'Detail', 'detail'),
    proofLeaf('resumption', 'Gemini session resumption', proof, 'resumption', ready),
    proofLeaf('compression', 'Gemini context window compression (slidingWindow)', proof, 'compression', ready),
    H.leaf('truncation', 'Client-side context truncation', 'host.backend_capabilities.context_truncation'),
    boundLeaf(bounds, 'max_turns', 'Fold bound: turns', 'MAX_TURNS'),
    boundLeaf(bounds, 'max_context_bytes', 'Fold bound: context bytes', 'MAX_CONTEXT_BYTES'),
    boundLeaf(bounds, 'max_turn_bytes', 'Fold bound: one turn bytes', 'MAX_TURN_BYTES'),
  ]

  const byId = Object.fromEntries([...runtime, ...voice, ...prompt, ...context].map((l) => [l.id, l]))
  const rev = (id, label) => ({ label, leaf: id, value: byId[id].value, state: byId[id].state, source: byId[id].source, basis: byId[id].basis, expires_at: byId[id].expires_at ?? null })
  return {
    format: 'rth.settings-view/v1',
    at: now,
    stream_lease_ms: streamLeaseMs,
    revisions: {
      working_tree_head: rev('self_repo_head', 'Self-repo HEAD'),
      active_deployment: rev('artifact_id', 'Active binary'),
      config_revision: rev('setup_id', 'Config revision (setup_id)'),
      context_revision: { ...rev('context_lineage', 'Context revision (lineage since)'), watermark: byId.context_watermark.value },
    },
    sources: {
      host: host ? { ok: host.ok, read_at: host.read_at, error: host.error, lease_ms: leaseMs } : { ok: false, error: 'rth-session is not loaded: no host status' },
      agentd: facts ? { link: eventsLink?.state ?? 'unknown', boot: A.boot, epoch: facts.epoch, hello_this_attachment: A.helloCurrent, ready_this_attachment: Boolean(facts.ready && facts.ready.epoch === facts.epoch) } : { link: 'unknown', error: 'rth-events is not loaded: no agentd events' },
      policies: policies ? { path: policies.path, file_sha256: policies.file_sha256 ?? null, error: policies.error ?? null } : null,
      bounds: bounds ? { path: bounds.path, file_sha256: bounds.file_sha256 ?? null, error: bounds.error ?? null } : null,
    },
    sections: [
      { id: 'runtime', title: 'Runtime', leaves: runtime },
      { id: 'voice', title: 'Voice', leaves: voice },
      { id: 'prompt', title: 'Prompt and instructions', leaves: prompt },
      { id: 'context', title: 'Context', leaves: context },
    ],
    edits: { pending: [], applied: [], rejected: [], note: 'read-only: editing is not available yet' },
    gaps: GAPS,
  }
}

/** Values the panel cannot show from any existing status or event. */
export const GAPS = [
  { id: 'model_in_status', what: 'model', detail: 'the Gemini model appears only in the `gemini ready` log line; the observer has no replay, so a panel that attached after the provider connected sees none until the next provider session. Proposal: agentd reports {model, policy, setup_id, instruction sha256} in Hello or a `config_reported` event.' },
  { id: 'instruction_text', what: 'effective system instruction', detail: 'never on a wire; shown only when the setup_id replica proves the working-tree text. With system_extra=true (RTH_GEMINI_SYSTEM or a baked RTH_BUILD_GEMINI_SYSTEM) it cannot be shown at all.' },
  { id: 'resumption_compression', what: 'Gemini resumption / compression flags', detail: 'not in the ready line; known only through the setup_id match.' },
  { id: 'context_now', what: 'context fold size now', detail: 'the context report is per boot (Hello, context_restore); the live fold (turns and bytes held now) is not reported.' },
  { id: 'context_bounds', what: 'context bounds (128 turns / 32 KiB / 4 KiB)', detail: 'compile-time constants of rth-agentd; read here from working-tree source, not from the running artifact.' },
  { id: 'context_restore_in_host', what: 'context report in host status', detail: 'host /v1/status carries link.context (the string) only, not the context_restore detail.' },
  { id: 'mic_on_running_host', what: 'mute requested / applied', detail: 'host.mic exists in the current source (host.rs mic_status) but a host built before it omits the field; the panel then says unknown.' },
  { id: 'ptt_held', what: 'push-to-talk held now', detail: 'talk down/up is logged as a host note, not exposed in /v1/status.' },
  { id: 'agentd_status', what: 'agentd status endpoint', detail: 'rth-agentd has no status request; everything about it comes from Hello and the event stream.' },
  { id: 'bot_settings', what: 'Bot settings beyond memoryReview', detail: 'only memoryReview is picked from the loaded Bot config; nothing else of it is read.' },
]

function hostReader(host, now, leaseMs) {
  const body = host?.body ?? null
  const fresh = Boolean(host?.ok && host.read_at !== null && now - host.read_at <= leaseMs)
  const reason = !host ? 'rth-session is not loaded: no host status' : !body ? `host status not read: ${host.error ?? 'no read yet'}` : null
  const staleWhy = body && !fresh ? (host.ok ? `last read ${Math.round((now - host.read_at) / 1000)} s ago, past the ${leaseMs / 1000} s lease` : `the last read failed (${host.error}); showing nothing as current`) : null
  const H = {
    body,
    fresh,
    leaf(id, label, path, opts = {}) {
      const source = `host /v1/status ${path}`
      const base = { id, label, source, basis: 'host', observed_at: host?.read_at ?? null, expires_at: host?.read_at != null ? host.read_at + leaseMs : null }
      if (!body) return { ...base, value: null, state: 'unknown', note: reason }
      if (opts.nullParent && get(body, opts.nullParent) === null) {
        return { ...base, value: null, state: fresh ? 'live' : 'stale', note: opts.nullNote, reported_null: true }
      }
      if (!has(body, path)) {
        return { ...base, value: null, state: 'unknown', note: `${path} is absent from this host build's status` }
      }
      let value = get(body, path)
      if (opts.also) value = { [path.split('.').at(-1)]: value, [opts.also.split('.').at(-1)]: get(body, opts.also) ?? null }
      if (!fresh) return { ...base, value: null, last: value, state: 'stale', note: staleWhy }
      // A null the host wrote (e.g. deployment.pending) is a reported value: "none reported".
      return { ...base, value, state: 'live', note: opts.note ?? null, ...(value === null ? { reported_null: true } : {}) }
    },
  }
  return H
}

function agentReader(facts, eventsLink, H) {
  const linkUp = eventsLink?.state === 'attached'
  const hello = facts?.hello ?? null
  // The boot is positively known only from a Hello observed in this attachment.
  const helloCurrent = Boolean(hello && hello.epoch === facts.epoch && linkUp)
  const helloBoot = helloCurrent ? (hello.fields?.boot_id ?? bootOf(hello.producer)) : null
  const hostBoot = H.fresh ? get(H.body, 'link.boot_id') ?? null : null
  const conflict = Boolean(helloBoot && hostBoot && helloBoot !== hostBoot)
  const boot = conflict ? null : helloBoot
  const judge = (f, what) => {
    if (!facts) return { state: 'unknown', note: 'rth-events is not loaded: no agentd events' }
    if (!f) return { state: 'unknown', note: `no ${what} observed since DSH attached (the observer stream has no replay)` }
    if (f.epoch !== facts.epoch) return { state: 'stale', note: `observed before the observer link was interrupted and not re-observed since (no replay): a newer ${what} may have been missed` }
    if (!linkUp) return { state: 'stale', note: `the rth-events observer link is ${eventsLink?.state ?? 'unknown'}: a newer value may have been missed` }
    if (!helloCurrent) return { state: 'stale', note: 'no Hello observed in this observer attachment: the boot is not positively known' }
    if (conflict) return { state: 'stale', note: `the host reports boot ${hostBoot}, the observer's Hello says ${helloBoot}` }
    if (bootOf(f.producer) !== boot) return { state: 'stale', note: `from boot ${bootOf(f.producer)}; the current boot is ${boot}` }
    return { state: 'live', note: f.seen_at === null ? 'already in the event window when this panel loaded' : null }
  }
  const r = facts?.ready ?? null
  const rj = judge(r, 'gemini ready line')
  const ready = { ...rj, value: rj.state === 'live' ? r.parsed : null, observed_at: r?.seen_at ?? null }
  // The newest context report of the current boot: a context_restore event, else the Hello's.
  const helloCtx = facts?.hello ? { ...facts.hello, fields: facts.hello.fields?.instance?.context_restore ?? null, wire: 'hello instance.context_restore' } : null
  const evCtx = facts?.context ? { ...facts.context, wire: 'context_restore' } : null
  const pick = [evCtx, helloCtx].filter((c) => c && c.fields).sort((a, b) => b.order - a.order)[0] ?? null
  const cj = judge(pick, 'context report (Hello or context_restore)')
  if (facts?.hello && !helloCtx?.fields && !evCtx) cj.note = 'the Hello carries no context_restore (an agent that predates it)'
  const context = { ...cj, fact: pick }
  return {
    boot,
    helloCurrent,
    ready,
    context,
    readyLeaf(id, label, key, opts = {}) {
      const source = `agentd log "gemini ready" ${key}=`
      const base = { id, label, source, basis: 'events', observed_at: ready.observed_at }
      if (ready.state !== 'live') return { ...base, value: null, state: ready.state, last: r?.parsed?.[key] ?? undefined, note: ready.note }
      if (!(key in ready.value)) return { ...base, value: null, state: 'unknown', note: `the ready line has no ${key}=` }
      return { ...base, value: ready.value[key], state: 'live', note: opts.note ?? ready.note }
    },
  }
}

function ctxLeaf(c, id, label, key) {
  const f = c.fact
  const base = { id, label, source: f ? `agentd ${f.wire} ${key}` : 'agentd Hello / context_restore', basis: 'events', observed_at: f?.seen_at ?? null }
  if (c.state !== 'live') return { ...base, value: null, state: c.state, last: f?.fields?.[key], note: c.note }
  if (!(key in f.fields)) return { ...base, value: null, state: 'unknown', note: `the report has no ${key}` }
  return { ...base, value: f.fields[key], state: 'live', note: c.note }
}

function proofLeaf(id, label, proof, key, ready) {
  const base = { id, label, source: 'setup_id replica (the flag is part of the hashed setup)', basis: 'events', observed_at: ready.observed_at }
  if (proof.status !== 'verified') return { ...base, value: null, state: 'unknown', note: proof.reason }
  return { ...base, value: proof[key] ? 'on' : 'off', state: 'live', note: 'proven by the setup_id match' }
}

function boundLeaf(bounds, id, label, name) {
  const base = { id, label, source: `${bounds?.path ?? 'context.rs'} ${name}`, basis: 'source', observed_at: bounds?.read_at ?? null }
  if (!bounds || bounds.error || !(name in bounds.bounds)) return { ...base, value: null, state: 'unknown', note: bounds?.error ?? `${name} not found in the source` }
  return { ...base, value: bounds.bounds[name], state: 'source', note: 'working-tree source constant; the running agent does not report it' }
}

function agree(H) {
  const a = H.leaf('x', '', 'link.deployment')
  const b = H.leaf('y', '', 'deployment.active')
  const base = { id: 'deployment_agree', label: 'Hello and activation record agree', source: 'host /v1/status link.deployment vs deployment.active', basis: 'host', observed_at: a.observed_at, expires_at: a.expires_at }
  if (a.state !== 'live' || b.state !== 'live' || !a.value || !b.value) return { ...base, value: null, state: a.state === 'stale' || b.state === 'stale' ? 'stale' : 'unknown', note: 'needs both, live' }
  const same = ['deployment_id', 'artifact_id', 'manifest_sha256'].every((k) => a.value[k] === b.value[k])
  return { ...base, value: same ? 'yes' : 'no', state: 'live', note: same ? 'deployment_id, artifact_id and manifest_sha256 match' : 'the agent reports a different deployment than the controller activated' }
}

function eventsLinkLeaf(facts, link) {
  const base = { id: 'events_link', label: 'Event observer (rth-events)', source: 'rth-events snapshot().link', basis: 'events', observed_at: null }
  if (!facts) return { ...base, value: null, state: 'unknown', note: 'rth-events is not loaded' }
  return { ...base, value: link?.state ?? null, state: link ? 'live' : 'unknown', note: link?.detail ? String(link.detail) : null }
}

function botLeaf(bot) {
  const base = { id: 'bot_memory_review', label: 'Bot memory review', source: 'omniTeam.config.memoryReview (the loaded Bot plugin config)', basis: 'bot', observed_at: null }
  if (!bot) return { ...base, value: null, state: 'unknown', note: 'the Bot plugin (omniTeam) is not loaded' }
  // memory.js: `config.memoryReview !== false`, so absent means on.
  return { ...base, value: bot.memoryReview === false ? 'off' : 'on', state: 'live', note: bot.memoryReview === undefined ? 'not set: the Bot default is on' : null }
}
