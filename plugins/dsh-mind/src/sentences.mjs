// One plain sentence per event, a category icon and a status chip. Every sentence says what was
// OBSERVED, in the event's own terms; nothing is promoted to a fact it does not carry. A speech
// candidate is a candidate (never "The owner spoke"), microphone audio is not speech, a transcript
// is the provider's (speaker and finality as the event states them), handed-to-device samples
// are not heard audio, an admission is not a result. A missing field reads "unknown".
//
// Two lanes, kept apart: `agentd` (catalog/events.v3.json wire kinds, from the rth-events
// observer feed or the agentd journal) and `host` (rth-host's --log JSONL notes, which are NOT
// in the events catalog; HOST_GLOSSARY below is this plugin's own reading of crates/rth/src).

export const CATEGORIES = {
  perceive: { icon: '👂', label: 'perceive', means: 'something arrived from a person, a sense or the provider' },
  decide: { icon: '⚖️', label: 'decide', means: 'the agent chose what to do' },
  act: { icon: '⚙️', label: 'act', means: 'the agent or host did something (started, stopped, sent)' },
  speak: { icon: '💬', label: 'speak', means: 'reply text or audio produced or handed to a device' },
  remember: { icon: '🗄️', label: 'remember', means: 'history, media or context stored, restored or failing' },
  system: { icon: '🔧', label: 'system', means: 'connections, health, mute, deployment, process life' },
}

/** Status chips. Pending, attempted, failed and unknown are first-class. */
export const STATUSES = ['observed', 'pending', 'attempted', 'completed', 'stopped', 'failed', 'unknown', 'gap']

const U = 'unknown'
const val = (x) => (x === undefined || x === null || x === '' ? U : Array.isArray(x) ? (x.length ? x.join(', ') : 'none') : typeof x === 'object' ? JSON.stringify(x) : String(x))
const quote = (s, n = 90) => {
  if (typeof s !== 'string') return U
  const t = s.replace(/\s+/g, ' ').trim()
  return `“${t.length > n ? `${t.slice(0, n - 1)}…` : t}”`
}
const ms = (x) => (Number.isFinite(x) ? `${Math.round(x)} ms` : U)
const db = (x) => (Number.isFinite(x) ? `${x.toFixed(1)} dBFS` : U)
const doneStatus = { completed: 'completed', cancelled: 'stopped', failed: 'failed' }

/**
 * @param {{lane:'agentd'|'host', kind:string, fields:object, count?:number, audio?:object, text?:string}} f
 * @returns {{category:string, sentence:string, status:string, known:boolean, duration_ms?:number|null, duration_basis?:string}}
 */
export function sentence(f) {
  const table = f.lane === 'host' ? HOST : AGENTD
  const rule = table[f.kind]
  if (!rule) {
    return {
      category: 'system',
      status: 'observed',
      known: false,
      sentence: f.lane === 'host'
        ? `Host noted “${f.kind}” (a kind Mind has no sentence for yet; see the raw record)`
        : `Agent sent “${f.kind}” (not in the events catalog; see the raw envelope)`,
    }
  }
  const out = rule(f.fields ?? {}, f)
  return { status: 'observed', known: true, ...out }
}

// ---- agentd wire kinds (catalog v3). The catalog `note` is the authority on what each means.
const AGENTD = {
  hello: (x) => ({
    category: 'system',
    sentence: `Agent ${val(x.agent)} ${val(x.version)} is available (boot ${val(x.boot_id)}, backend ${val(x.backend)}${x.deployment?.artifact_id ? `, deployment ${x.deployment.artifact_id}` : x.deployment ? '' : ', deployment unknown'})`,
  }),
  attach: (x) => ({ category: 'system', sentence: `${val(x.attachment_id)} asked to join as ${val(x.role)}`, status: 'pending' }),
  ready: (x) => ({ category: 'system', sentence: `${val(x.attachment_id)} joined as ${val(x.role)}`, status: 'completed' }),
  rejected: (x) => ({ category: 'system', sentence: `The agent refused a frame: ${val(x.reason)}`, status: 'failed' }),
  heartbeat: (x) => ({ category: 'system', sentence: `Agent is up (${ms(x.uptime_ms)}, ${val(x.attachments)} attached)` }),
  log: (x) => ({ category: 'system', sentence: `Agent note (${val(x.level)}): ${quote(x.msg, 140)}`, status: x.level === 'error' ? 'failed' : 'observed' }),
  journal_fault: (x) => ({ category: 'remember', sentence: `History could not be saved: ${val(x.detail)}`, status: 'failed' }),
  user_text: (x) => ({ category: 'perceive', sentence: `Received a typed message for request ${val(x.request_id)}: ${quote(x.text)}` }),
  cancel: (x) => ({ category: 'act', sentence: `Asked to stop reply ${val(x.response_id)}`, status: 'pending' }),
  decision: (x) => ({
    category: 'decide',
    sentence: `Decided to ${val(x.selected)} (of ${val(x.candidates)}) by ${val(x.policy)}${x.trigger_event ? `, triggered by ${x.trigger_event}` : ', trigger unknown'}`,
    status: 'completed',
    duration_ms: Number.isFinite(x.latency_ns) ? x.latency_ns / 1e6 : null,
    duration_basis: 'decision latency the agent reported',
  }),
  response_started: (x) => ({
    category: 'act',
    sentence: `Started reply ${val(x.response_id)} to request ${val(x.request_id)}${/^mic-/.test(x.request_id ?? '') ? ' (the microphone stream, not one utterance)' : ''}`,
    status: 'pending',
  }),
  agent_delta: (x, f) => ({ category: 'speak', sentence: `Wrote reply text for ${val(x.response_id)}: ${quote(f.text ?? x.text)}${f.count > 1 ? ` (${f.count} parts)` : ''}` }),
  agent_audio: (x, f) => ({
    category: 'speak',
    sentence: `Generated reply audio for ${val(x.response_id)}: ${f.audio ? `${f.audio.chunks} chunk(s), ${ms(f.audio.ms)}` : '1 chunk'} (generated, not heard)`,
    duration_ms: f.audio?.ms ?? null,
    duration_basis: 'audio length from chunk sizes',
  }),
  agent_done: (x) => ({
    category: 'act',
    sentence: `Reply ${val(x.response_id)} ended: ${doneStatus[x.status] ?? val(x.status)}`,
    status: doneStatus[x.status] ?? U,
  }),
  user_audio: (x, f) => ({
    category: 'perceive',
    sentence: `Microphone audio arrived on ${val(x.stream_id)}: ${f.audio ? `${f.audio.chunks} chunk(s), ${ms(f.audio.ms)}` : '1 chunk'} (silence and noise included; not evidence of speech)`,
    duration_ms: f.audio?.ms ?? null,
    duration_basis: 'audio length from chunk sizes',
  }),
  user_audio_end: (x) => ({ category: 'perceive', sentence: `Microphone stream ${val(x.stream_id)} ended or paused: ${val(x.chunks)} chunks sent, ${val(x.dropped_chunks)} dropped (not necessarily the end of an utterance)` }),
  audio_ingress: (x) => ({ category: 'perceive', sentence: `The agent received ${val(x.stream_id)}: ${val(x.chunks)} chunks, ${val(x.samples)} samples, ${val(x.gaps)} gaps (a receipt; not proof the model heard it)`, status: Number(x.gaps) > 0 ? 'unknown' : 'observed' }),
  input_transcript: (x) => ({
    category: 'perceive',
    sentence: `Provider transcribed ${quote(x.text)} (${x.final === true ? 'final' : 'not final'}; speaker not identified${x.association ? `; reply association ${x.association}` : ''})`,
  }),
  provider_signal: (x) => ({ category: 'perceive', sentence: `Provider said “${val(x.signal)}” for ${val(x.response_id)} (provider state, not local completion)` }),
  playback: (x) => ({
    category: 'speak',
    sentence: `Playback ${val(x.phase)} for ${val(x.response_id)}: ${val(x.played_samples)} samples handed to the device, ${val(x.dropped_samples)} dropped${x.reason ? ` (${x.reason})` : ''} (handed over, not proof it was heard)`,
    status: x.phase === 'started' ? 'pending' : x.phase === 'finished' ? 'completed' : x.phase === 'cleared' ? (x.reason === 'device_unavailable' ? 'failed' : 'stopped') : U,
  }),
  command_submitted: (x) => ({ category: 'perceive', sentence: `Typed command ${val(x.command_id)} submitted: ${quote(x.text)} (a submission, not yet admitted)`, status: 'attempted' }),
  command_admitted: (x) => ({
    category: 'act',
    sentence: `Command ${val(x.command_id)} ${CMD[x.status] ?? val(x.status)} at generation ${val(x.generation)}${x.status === 'admitted' ? ' (recorded durably; not yet a result)' : ''}`,
    status: CMD_STATUS[x.status] ?? U,
  }),
  media_stored: (x) => ({ category: 'remember', sentence: `Stored ${val(x.media)} samples ${val(x.sample_start)}..${val(x.sample_end)} of ${val(x.response_id ?? x.stream_id)} durably (${val(x.bytes)} bytes)`, status: 'completed' }),
  context_restore: (x) => ({
    category: 'remember',
    sentence: `Model context ${({ restored: 'restored', partial: 'partly restored', not_restored: 'not restored' })[x.status] ?? val(x.status)} at boot (${val(x.method)}: ${val(x.turns)} turns, ${val(x.tail_records)} tail records)`,
    status: x.status === 'restored' ? 'completed' : x.status === 'partial' ? 'unknown' : 'observed',
  }),
  // Delegation: ids and states only, never the task's words or the report.
  tasks_ready: (x) => ({ category: 'system', sentence: `Agent's task ledger is ready (${val(x.schema)})`, status: 'observed' }),
  task_state: (x) => ({ category: 'act', sentence: `Task ${val(x.task_id)}: worker ${val(x.record?.execution)}, posting ${val(x.record?.publication)} (${val(x.record?.outcome)}${x.record?.reason ? `, ${x.record.reason}` : ''})`, status: x.record?.outcome === 'refused' ? 'refused' : 'observed' }),
  task_update: (x) => ({ category: 'act', sentence: `The report worker reported ${val(x.update?.kind?.kind)} for task ${val(x.task_id)}`, status: 'observed' }),
  // Memory: ids, states and counts only, never what is remembered.
  memory_act: (x) => ({ category: 'remember', sentence: `Memory act ${val(x.act_id)} ${val(x.state)}`, status: x.state === 'committed' ? 'completed' : x.state === 'proposed' ? 'pending' : 'observed' }),
  // What the receipt's status says, never more. Selected locally is not delivered; nothing
  // here claims the model received or used a memory.
  memory_recall: (x) => {
    const id = val(x.call_id)
    const n = `${val(x.count)} of ${val(x.total)} saved items`
    const st = x.recall?.status
    switch (st) {
      case 'held': return { category: 'remember', sentence: `Recall ${id}: ${n} selected locally, held (not yet queued for the backend)`, status: 'pending' }
      case 'enqueued': return { category: 'remember', sentence: `Recall ${id}: ${n} selected locally and queued for the backend (not yet confirmed)`, status: 'attempted' }
      case 'backend_closed': return { category: 'remember', sentence: `Recall ${id}: not delivered, the backend was closed (${n} had been selected locally)`, status: 'failed' }
      case 'dropped_context_changed': case 'dropped_context_changed_at_backend': return { category: 'remember', sentence: `Recall ${id}: dropped, the conversation context changed${st.endsWith('_at_backend') ? ' at the backend' : ''} (${n} had been selected locally)`, status: 'stopped' }
      case 'selected_by_backend': return { category: 'remember', sentence: `Recall ${id}: the backend selected the recalled text`, status: 'observed' }
      default: return { category: 'remember', sentence: `Recall ${id}: ${n} selected locally, status ${val(st)}`, status: 'unknown' }
    }
  },
  executor_status: (x) => ({ category: 'system', sentence: x.status?.state === 'exited' ? `The report worker's DSH process exited (launch ${val(x.status?.launch_id)}; the launcher's receipt was verified)` : `The report worker is ${x.status?.available ? 'available' : 'unavailable'}`, status: 'observed' }),
  response_diagnostic: (x) => ({ category: 'act', sentence: `Reply ${val(x.response_id)} outcome detail: ${val(x.provider_status ?? 'no provider status')}${x.provider_reason ? ` (${x.provider_reason})` : ''}, ${x.outcome_source === 'local' ? `decided locally (${val(x.local_reason)})` : 'as the provider reported'}; not a terminal`, status: 'observed' }),
}
const CMD = { admitted: 'admitted', dispatched: 'dispatched', completed: 'completed', cancelled: 'stopped', failed: 'failed', interrupted: 'interrupted (agent restarted before a result was recorded)' }
const CMD_STATUS = { admitted: 'pending', dispatched: 'pending', completed: 'completed', cancelled: 'stopped', failed: 'failed', interrupted: 'unknown' }

// ---- rth-host --log notes (crates/rth/src/host.rs `note`, call.rs, vpio.rs). Not catalogued.
const HOST = {
  started: (x) => ({ category: 'system', sentence: `Host started (pid ${val(x.pid)}, voice ${val(x.voice)}, listening on ${val(x.listen)})` }),
  link_up: (x) => ({ category: 'system', sentence: `Host linked to the agent (boot ${val(x.boot_id)}, backend ${val(x.backend)}, command generation ${val(x.command_generation)})`, status: 'completed' }),
  link_down: (x) => ({ category: 'system', sentence: `Host lost the agent link: ${val(x.error)}`, status: 'failed' }),
  local_vad_onset: (x) => ({
    category: 'perceive',
    sentence: `Heard a possible start of talking on ${val(x.stream_id)} (speech candidate, energy onset at ${db(x.level_dbfs)}; ${x.audible === false ? 'reply not audible' : x.audible ? 'reply audible' : 'reply audibility unknown'}, action ${val(x.action)}). Not proof anyone spoke.`,
  }),
  echo_gate: (x) => ({ category: 'perceive', sentence: `Echo gate: frame ${db(x.frame_dbfs)} vs threshold ${db(x.threshold_dbfs)}, action ${val(x.action)}`, duration_ms: x.run_ms ?? null, duration_basis: 'gate run length the host reported' }),
  echo_probe: (x) => ({ category: 'perceive', sentence: `Echo probe for ${val(x.response_id)} (${val(x.phase)}): lag ${ms(x.probe?.lag_ms)}, coupling ${x.probe?.coupling_db != null ? `${x.probe.coupling_db.toFixed(1)} dB` : U}` }),
  input_transcript: (x) => ({ category: 'perceive', sentence: `Host saw a provider transcript: ${quote(x.text)} (speaker not identified)` }),
  response_started: (x) => ({ category: 'act', sentence: `Host saw reply ${val(x.response_id)} start for request ${val(x.request_id)}`, status: 'pending' }),
  reply_text: (x) => ({ category: 'speak', sentence: `Host recorded reply text for ${val(x.response_id)}: ${quote(x.text)}` }),
  response_done: (x) => ({ category: 'act', sentence: `Host saw reply ${val(x.response_id)} end: ${doneStatus[x.status] ?? val(x.status)}`, status: doneStatus[x.status] ?? U }),
  response_cancelled: (x) => ({ category: 'act', sentence: `Host marked reply ${val(x.response_id)} cancelled`, status: 'stopped' }),
  provider_signal: (x) => ({ category: 'perceive', sentence: `Host saw provider signal “${val(x.signal)}” for ${val(x.response_id)}` }),
  speaker_cleared: (x) => ({
    category: 'speak',
    sentence: `Speaker cleared (${val(x.reason)}): ${val(x.dropped_samples)} queued samples dropped${Array.isArray(x.cancelled) && x.cancelled.length ? `, cancelled ${x.cancelled.join(', ')}` : ''}`,
    status: 'stopped',
  }),
  playback: (x) => ({
    category: 'speak',
    sentence: `Playback ${val(x.phase)} for ${val(x.response_id)}: ${val(x.played_samples)} samples handed to the device, ${val(x.dropped_samples)} dropped (not proof it was heard)`,
    status: x.phase === 'started' ? 'pending' : x.phase === 'finished' ? 'completed' : 'stopped',
  }),
  cancel_written: (x) => ({ category: 'act', sentence: `Host sent a stop for reply ${val(x.response_id)} to the agent (sent, not yet confirmed)`, status: 'attempted' }),
  command_written: (x) => ({ category: 'act', sentence: `Host sent typed command ${val(x.command_id)} to the agent (envelope ${val(x.envelope)}; sent, not admitted)`, status: 'attempted' }),
  command_resent: (x) => ({ category: 'act', sentence: `Host resent command ${val(x.command_id)} (same frame, after a reconnect)`, status: 'attempted' }),
  command_admitted: (x) => ({ category: 'act', sentence: `Agent admitted command ${val(x.command_id)} (${val(x.status)}, generation ${val(x.generation)}${x.duplicate ? ', a duplicate' : ''})`, status: CMD_STATUS[x.status] ?? U }),
  command_finished: (x) => ({ category: 'act', sentence: `Command ${val(x.command_id)} finished ${val(x.status)}${x.response_id ? ` with reply ${x.response_id}` : ''}`, status: CMD_STATUS[x.status] ?? (x.status === 'completed' ? 'completed' : U) }),
  command_rejected: (x) => ({ category: 'act', sentence: `Command ${val(x.command_id)} was refused: ${val(x.reason ?? x.error)}`, status: 'failed' }),
  mute_request: (x) => ({ category: 'system', sentence: `Asked to ${x.muted ? 'mute' : 'unmute'} the microphone (via ${val(x.via)}; requested, not yet applied)`, status: 'pending' }),
  mute_applied: (x) => ({ category: 'system', sentence: `Microphone ${x.muted ? 'muted' : 'unmuted'} at the host (generation ${val(x.generation)}, ${val(x.partial_samples_discarded)} partial samples discarded)`, status: 'completed' }),
  talk: (x) => ({ category: 'system', sentence: `Push-to-talk ${x.down ? 'pressed' : 'released'} (applied: ${val(x.applied)})` }),
  call_request: (x) => ({ category: 'system', sentence: `Call ${val(x.call ?? x.action)} requested`, status: 'pending' }),
  call_devices_opened: () => ({ category: 'system', sentence: 'Call audio devices opened', status: 'completed' }),
  call_devices_released: () => ({ category: 'system', sentence: 'Call audio devices released', status: 'completed' }),
  call_input_closed: () => ({ category: 'system', sentence: 'Call microphone input closed', status: 'completed' }),
  mic_mode: (x) => ({ category: 'system', sentence: `Microphone mode: ${val(x.mode ?? x.mic_mode)}` }),
  mic_modes_ui: () => ({ category: 'system', sentence: 'Host showed the system microphone-modes panel' }),
  outbox_fault: (x) => ({ category: 'system', sentence: `Host could not queue a frame for the agent: ${val(x.error ?? x.detail)}`, status: 'failed' }),
  say_refused: (x) => ({ category: 'system', sentence: `Scripted microphone input refused: ${val(x.error ?? x.reason)}`, status: 'failed' }),
  control_without_link: () => ({ category: 'system', sentence: 'A control request arrived with no agent link (not sent)', status: 'failed' }),
  stop_without_link: () => ({ category: 'system', sentence: 'A stop arrived with no agent link (not sent)', status: 'failed' }),
  quit_requested: (x) => ({ category: 'system', sentence: `Host asked to quit (via ${val(x.via)})`, status: 'pending' }),
  quit: (x) => ({ category: 'system', sentence: `Host quit (devices stopped after ${ms(x.devices_stopped_after_ms)})`, status: 'completed' }),
}

/** What each host note kind is, for the audit panel (this plugin's reading; not a catalog). */
export const HOST_GLOSSARY = Object.fromEntries(Object.keys(HOST).map((k) => [k, `rth-host --log note “${k}” (crates/rth/src). Not in catalog/events.v3.json: Mind's own reading of the host source.`]))
export const HOST_KINDS = Object.keys(HOST)
export const AGENTD_KINDS = Object.keys(AGENTD)
