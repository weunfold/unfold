// RTH record taxonomy for the trajectory ledger.
// Pure data: every kind has a short human label, an icon name, a colour token pair (light, dark),
// an overview lane and a filter group. Colour is never the only signal: icon + label always show.

/** Overview lanes: five real-time lanes instead of upstream's Input / Model / Tools. */
export const LANES = Object.freeze(['Hear', 'Think', 'Speak', 'Act', 'Life'])
export const LANE = Object.freeze({ HEAR: 0, THINK: 1, SPEAK: 2, ACT: 3, LIFE: 4 })

/** Filter groups. */
export const GROUPS = Object.freeze([
  { id: 'you', label: 'You' },
  { id: 'rth', label: 'Unfold' },
  { id: 'voice', label: 'Voice' },
  { id: 'control', label: 'Control' },
  { id: 'signals', label: 'Signals' },
  { id: 'faults', label: 'Faults & gaps' },
])

/**
 * The kinds. `upstream` is the upstream cell kind the record rides on for the ledger machinery
 * (selection, inspector tabs, search); `lane` its overview lane; colours are [light, dark].
 */
export const KINDS = Object.freeze({
  voice: { label: 'YOU · VOICE', short: 'YOU', icon: 'mic', color: ['#2563eb', '#60a5fa'], lane: LANE.HEAR, group: 'you', upstream: 'user' },
  typed: { label: 'YOU · TYPED', short: 'YOU', icon: 'keyboard', color: ['#2563eb', '#60a5fa'], outlined: true, lane: LANE.HEAR, group: 'you', upstream: 'user' },
  vad: { label: 'VAD', short: 'VAD', icon: 'audio-waveform', color: ['#0891b2', '#22d3ee'], lane: LANE.HEAR, group: 'voice', upstream: 'context' },
  rth: { label: 'Unfold', short: 'Unfold', icon: 'sparkles', color: ['#7c3aed', '#a78bfa'], lane: LANE.THINK, group: 'rth', upstream: 'message' },
  speaking: { label: 'SPEAKING', short: 'SPEAKING', icon: 'volume-2', color: ['#c026d3', '#e879f9'], lane: LANE.SPEAK, group: 'voice', upstream: 'context' },
  // Audio the agent generated or received: not a host playback report.
  audio: { label: 'AUDIO OUT (generated)', short: 'AUDIO', icon: 'audio-lines', color: ['#a21caf', '#d8b4fe'], outlined: true, lane: LANE.SPEAK, group: 'voice', upstream: 'context' },
  interrupt: { label: 'INTERRUPT', short: 'INTERRUPT', icon: 'zap', color: ['#ea580c', '#fb923c'], lane: LANE.SPEAK, group: 'voice', upstream: 'context' },
  echo: { label: 'ECHO GUARD', short: 'ECHO', icon: 'shield', color: ['#0d9488', '#2dd4bf'], lane: LANE.HEAR, group: 'signals', upstream: 'context' },
  decision: { label: 'DECISION', short: 'DECISION', icon: 'git-branch', color: ['#b45309', '#fbbf24'], lane: LANE.ACT, group: 'rth', upstream: 'context' },
  context: { label: 'CONTEXT', short: 'CONTEXT', icon: 'book-open', color: ['#15803d', '#4ade80'], lane: LANE.ACT, group: 'rth', upstream: 'context' },
  command: { label: 'COMMAND', short: 'COMMAND', icon: 'terminal', color: ['#4f46e5', '#818cf8'], lane: LANE.ACT, group: 'control', upstream: 'context' },
  mic: { label: 'MIC / CALL', short: 'MIC', icon: 'mic-off', color: ['#475569', '#94a3b8'], lane: LANE.LIFE, group: 'control', upstream: 'context' },
  call: { label: 'MIC / CALL', short: 'CALL', icon: 'phone', color: ['#475569', '#94a3b8'], lane: LANE.LIFE, group: 'control', upstream: 'context' },
  link: { label: 'LINK', short: 'LINK', icon: 'plug', color: ['#6b7280', '#9ca3af'], lane: LANE.LIFE, group: 'control', upstream: 'context' },
  signal: { label: 'SIGNAL', short: 'SIGNAL', icon: 'radio', color: ['#64748b', '#94a3b8'], quiet: true, lane: LANE.LIFE, group: 'signals', upstream: 'context' },
  fault: { label: 'FAULT', short: 'FAULT', icon: 'triangle-alert', color: ['#dc2626', '#f87171'], lane: LANE.LIFE, group: 'faults', upstream: 'context' },
  gap: { label: 'GAP', short: 'GAP', icon: 'unlink', color: ['#ca8a04', '#facc15'], dashed: true, lane: LANE.LIFE, group: 'faults', upstream: 'context' },
  media: { label: 'MEDIA', short: 'MEDIA', icon: 'image', color: ['#6b7280', '#9ca3af'], lane: LANE.LIFE, group: 'control', upstream: 'context' },
})

// Rows whose job is to build the one RTH reply record (never shown as their own rows).
export const REPLY_PARTS = new Set(['response_started', 'agent_delta', 'agent_done', 'response_done', 'reply_text', 'command_finished'])
// Transcript rows fold into the one YOU · VOICE record of their episode.
export const TRANSCRIPT = new Set(['input_transcript'])

/**
 * Map one Mind public row (lane, kind, status, ref) to a record kind, or null when it is not a
 * standalone record (reply parts and transcripts are assembled elsewhere).
 */
export function kindOf(row) {
  const k = row.kind
  const lane = row.lane
  if (lane === 'mind') {
    if (/^journal_|seq_gap|link_lost|link_resumed|preload|panel_stream_lost/.test(k) || row.status === 'gap') return 'gap'
    return 'signal'
  }
  if (REPLY_PARTS.has(k) || TRANSCRIPT.has(k)) return null
  switch (k) {
    case 'user_text': case 'command_submitted': case 'message_submitted': return 'typed'
    case 'local_vad_onset': case 'user_audio': case 'user_audio_end': case 'audio_ingress': case 'input_audio_start': case 'input_audio_end': return 'vad'
    case 'playback': case 'message_playback_reported': return 'speaking'
    case 'agent_audio': case 'message_audio_added': return 'audio'
    case 'response_cancelled': case 'cancel': case 'cancel_written': case 'speaker_cleared': case 'message_stop_requested': case 'stop_requested': return 'interrupt'
    case 'echo_gate': case 'echo_probe': return 'echo'
    case 'decision': case 'provider_turn_signaled': return 'decision'
    case 'context_restore': case 'model_context_checked': case 'note_added': return 'context'
    case 'command_written': case 'command_admitted': return 'command'
    case 'mute_request': case 'mute_applied': return 'mic'
    case 'call_request': case 'call_devices_opened': case 'call_devices_released': case 'call': return 'call'
    case 'started': case 'link_up': case 'link_down': case 'hello': case 'attach': case 'ready': case 'quit_requested': case 'quit': return 'link'
    case 'provider_signal': case 'heartbeat': case 'agent_status_reported': return 'signal'
    case 'rejected': case 'journal_fault': case 'request_rejected': case 'history_save_failed': return 'fault'
    case 'media_stored': return 'media'
    case 'log': return row.ref?.level === 'error' || row.ref?.level === 'warn' ? 'fault' : 'signal'
    default: return row.status === 'failed' ? 'fault' : 'signal'
  }
}
