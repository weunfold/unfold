// What the inherited inspector and overview may say about an RTH record.
// Pure; bundled into the runtime (RTH.stateOf, RTH.timelineDetail) and tested in node against the
// functions extracted from the built fork.
//
// Upstream's stateOf() falls through to "complete" for anything not marked as an error, which
// would show an open, cancelled or unattributed reply as "Completed". RTH records get their own
// states instead: an outcome is claimed only when the adapter attributed one.
//   'complete'          attributed outcome "completed" (upstream label: Completed)
//   'error'             attributed failure, or a FAULT record (upstream label: Failed)
//   'rth-pending'       no end observed yet
//   'rth-cancelled'     attributed cancellation
//   'rth-unavailable'   identity ambiguous or outcome not attributable
//   'rth-other'         ended with a status this view does not classify
//   'rth-observed'      an observation; it claims no outcome

// Exact known statuses only: the host/agent reply terminals (sentences.mjs doneStatus:
// completed, cancelled, failed) and the command terminals (CMD: completed, cancelled, failed,
// interrupted; rejected for admission). Anything else is unclassified, never success by substring.
const OUTCOME = Object.freeze({ completed: 'complete', cancelled: 'rth-cancelled', failed: 'error', rejected: 'error', interrupted: 'rth-other' })
const outcomeState = (s) => (s == null ? null : Object.hasOwn(OUTCOME, s) ? OUTCOME[s] : 'rth-other')

/** The inspector state of one RTH record (replaces upstream's fallback for RTH cells only). */
export function stateOf(cell) {
  switch (cell.rthKind) {
    case 'rth':
      if (cell.rthWhy || cell.rthOutcomeWhy) return 'rth-unavailable'
      return outcomeState(cell.rthOutcome) ?? 'rth-pending'
    case 'command':
      if (cell.rthOutcomeWhy) return 'rth-unavailable'
      return outcomeState(cell.rthOutcome) ?? 'rth-pending'
    case 'fault':
      return 'error'
    default:
      return 'rth-observed'
  }
}

/** Locale keys for the RTH-only states (rth.status.*), in English and Chinese. */
export const STATUS_EN = {
  'rth.status.pending': 'No end observed yet',
  'rth.status.cancelled': 'Cancelled',
  'rth.status.unavailable': 'Not attributed',
  'rth.status.other': 'Ended (status not classified)',
  'rth.status.observed': 'Observed (no outcome claimed)',
}
export const STATUS_ZH = {
  'rth.status.pending': '尚未观察到结束',
  'rth.status.cancelled': '已取消',
  'rth.status.unavailable': '无法归属',
  'rth.status.other': '已结束（状态未分类）',
  'rth.status.observed': '已观察（不代表结果）',
}

/**
 * The overview tooltip detail for an RTH record: a measured duration on its own producer clock,
 * and no start time. Upstream prints observed_at -> observed_at + duration, which would splice the
 * projection's observation clock with a host or agent duration.
 */
export function timelineDetail(cell) {
  return cell.rthSpan === 'measured' && Number.isFinite(cell.rthDurationMs) ? { durationMs: Math.max(0, cell.rthDurationMs) } : {}
}
