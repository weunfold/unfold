// The completion contract: plain text, validated outside the model. The child's
// text is Markdown ending in exactly one `SUMMARY: <one sentence>` line. This proves a bounded
// FORMAT only, not that the summary is one sentence, accurate or free of instructions.
import { sha256 } from './canon.mjs'

export const REPORT_MIN = 200
export const REPORT_MAX = 12_000
export const SUMMARY_MAX = 240

/** `{ok: true, report, summary, result_sha256}` or `{ok: false, reason}`. */
export function validateResult({ turnEnd, stopReason, interrupted, text, toolAttempts }) {
  if (turnEnd !== 'completed') return { ok: false, reason: `turn_${turnEnd ?? 'unknown'}` }
  if (interrupted) return { ok: false, reason: 'interrupted' }
  if (stopReason !== 'stop') return { ok: false, reason: `finish_${stopReason ?? 'unknown'}` }
  if (toolAttempts) return { ok: false, reason: 'tool_attempted' }
  if (typeof text !== 'string') return { ok: false, reason: 'no_text' }
  const lines = text.replace(/\s+$/, '').split('\n')
  const summaries = lines.filter((l) => /^SUMMARY:/.test(l.trim()))
  if (summaries.length !== 1) return { ok: false, reason: `summary_lines_${summaries.length}` }
  const last = lines.at(-1).trim()
  if (!/^SUMMARY:/.test(last)) return { ok: false, reason: 'summary_not_last' }
  const summary = last.slice('SUMMARY:'.length).trim()
  if (summary.length < 1 || summary.length > SUMMARY_MAX) return { ok: false, reason: 'summary_length' }
  const report = lines.slice(0, -1).join('\n').trim()
  if (report.length < REPORT_MIN || report.length > REPORT_MAX) return { ok: false, reason: 'report_length' }
  return { ok: true, report, summary, result_sha256: sha256(report) }
}
