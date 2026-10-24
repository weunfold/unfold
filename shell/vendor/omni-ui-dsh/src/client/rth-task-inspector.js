import { createElement as h, useEffect, useState } from 'react'
import { createLeaseMemo, monoNow } from './rth-lease.js'
import { t } from './i18n.js'
import { TasksIcon } from './icons.js'

// Delegate tasks under Tasks: READ-ONLY rows and inspector. A projected row never opens a DSH
// session (no actions.openAgentSession), has no context menu, no rename/archive/adopt and no New
// session. The inspector shows the report and its state only: no composer, no retry, no cancel.

/** The browser enforces each row's lease on its own monotonic clock, without waiting for
 * another server message; an expired observation never revives. */
const memo = createLeaseMemo()
export const leased = (task, nowMono) => memo.leased(task, nowMono)

function useNow(ms = 1000) {
  const [now, setNow] = useState(monoNow())
  useEffect(() => {
    const timer = setInterval(() => setNow(monoNow()), ms)
    return () => clearInterval(timer)
  }, [ms])
  return now
}

export function RthTaskRows({ tasks: raw, wide, selected, onSelect }) {
  const now = useNow()
  const tasks = (raw ?? []).map(task => leased(task, now))
  if (!tasks.length) return null
  return h('div', { className: 'bt-rth-tasks', role: 'list', 'aria-label': t('Tasks') },
    tasks.map(task => h('button', {
      key: task.id, type: 'button', role: 'listitem', className: 'bt-row bt-rth-task', 'data-rth-task': task.task_id, 'data-card': task.card,
      'aria-current': selected === task.id ? 'page' : undefined,
      title: wide ? undefined : `${task.title}: ${task.label}`,
      onClick: () => onSelect(task.id),
      onContextMenu: (event) => event.preventDefault(),
    },
    h('span', { className: 'bt-agent-ava' }, h(TasksIcon, { size: 20 })),
    wide ? h('span', { className: 'bt-row-body' },
      h('span', { className: 'bt-row-name' }, task.title),
      h('span', { className: 'bt-row-preview' }, task.label)) : null)))
}

export function RthTaskInspector({ task: raw, onClose }) {
  const task = leased(raw, useNow())
  if (!task) return null
  return h('section', { className: 'bt-rth-inspector', 'aria-label': t('Task details'), 'data-card': task.card },
    h('header', null,
      h('strong', null, task.title),
      h('span', { className: 'bt-rth-state' }, task.label),
      h('button', { type: 'button', onClick: onClose, 'aria-label': t('Close') }, '×')),
    task.report_text ? h('pre', { className: 'bt-rth-report' }, task.report_text)
      : h('p', { className: 'bt-hint' }, task.card === 'unknown' ? t('The task status is unknown; no result is shown.') : t('No report yet.')),
    h('details', null, h('summary', null, t('Details')),
      h('code', null, JSON.stringify(task.detail))))
}
