// What the inherited inspector and overview may say about an Unfold record: its validated
// outcome, and durations only on the clock that measured them.
// Pure; bundled into the runtime (UNFOLD.stateOf, UNFOLD.timelineDetail) and tested in node against the
// functions extracted from the built fork.
//
// Upstream's stateOf() falls through to "complete" for anything not marked as an error, which
// turned an open, cancelled or unattributed reply into "Completed". Unfold records get their own
// states instead: an outcome is claimed only when the adapter attributed one.
//   'complete'          attributed outcome "completed" (upstream label: Completed)
//   'error'             attributed failure, or a FAULT record (upstream label: Failed)
//   'unfold-pending'       no end observed yet
//   'unfold-cancelled'     attributed cancellation
//   'unfold-unavailable'   identity ambiguous or outcome not attributable
//   'unfold-other'         ended with a status this view does not classify
//   'unfold-observed'      an observation; it claims no outcome

// Exact known statuses only: the host/agent reply terminals (sentences.js doneStatus:
// completed, cancelled, failed) and the command terminals (CMD: completed, cancelled, failed,
// interrupted; rejected for admission). Anything else is unclassified, never success by substring.
const OUTCOME = Object.freeze({ completed: 'complete', cancelled: 'unfold-cancelled', failed: 'error', rejected: 'error', interrupted: 'unfold-other' })
const outcomeState = (s) => (s == null ? null : Object.hasOwn(OUTCOME, s) ? OUTCOME[s] : 'unfold-other')

/** The inspector state of one Unfold record (replaces upstream's fallback for Unfold cells only). */
export function stateOf(cell) {
  switch (cell.unfoldKind) {
    case 'unfold':
      if (cell.unfoldWhy || cell.unfoldOutcomeWhy) return 'unfold-unavailable'
      return outcomeState(cell.unfoldOutcome) ?? 'unfold-pending'
    case 'command':
      if (cell.unfoldOutcomeWhy) return 'unfold-unavailable'
      return outcomeState(cell.unfoldOutcome) ?? 'unfold-pending'
    case 'fault':
      return 'error'
    default:
      return 'unfold-observed'
  }
}

/** Locale keys for the Unfold-only states (unfold.status.*), in English and Chinese. */
export const STATUS_EN = {
  'unfold.status.pending': 'No end observed yet',
  'unfold.status.cancelled': 'Cancelled',
  'unfold.status.unavailable': 'Not attributed',
  'unfold.status.other': 'Ended (status not classified)',
  'unfold.status.observed': 'Observed (no outcome claimed)',
}
export const STATUS_ZH = {
  'unfold.status.pending': '尚未观察到结束',
  'unfold.status.cancelled': '已取消',
  'unfold.status.unavailable': '无法归属',
  'unfold.status.other': '已结束（状态未分类）',
  'unfold.status.observed': '已观察（不代表结果）',
}

/**
 * The overview tooltip detail for an Unfold record: a measured duration on its own producer clock,
 * and no start time. Upstream printed observed_at -> observed_at + duration, splicing the
 * projection's observation clock with a host or agent duration.
 */
export function timelineDetail(cell) {
  return cell.unfoldSpan === 'measured' && Number.isFinite(cell.unfoldDurationMs) ? { durationMs: Math.max(0, cell.unfoldDurationMs) } : {}
}
