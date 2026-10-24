// The Bot's state read, bounded and age-normalized at client ingress. One read at a time: a
// newer read aborts the older one, a missed deadline aborts the underlying fetch (not only the
// wait), disposal aborts the current read, and an answer is validated against the deadline when
// it is accepted, not only when a timer fires.
// Each active delegate task row gets its local monotonic deadline HERE, before it enters the roster:
// the server's remaining lease counted from when the request was SENT, which is conservative
// (the server measured it no earlier than that), so transit, holding or a late first render can
// never stretch it.
import { monoNow } from './rth-lease.js'

const deadlineError = ms => new Error(`state read missed its ${ms} ms deadline`)

export function normalizeTasks(value, requestedAt) {
  if (Array.isArray(value?.rthTasks)) {
    value.rthTasks = value.rthTasks.map(t => (t?.terminal ? t : { ...t, deadline_mono: requestedAt + Math.max(0, Number(t?.lease_remaining_ms) || 0) }))
  }
  return value
}

export function createStateIngress({ call, deadlineMs, now = monoNow }) {
  let next = 0
  let current = null
  let disposed = false
  const read = payload => {
    if (disposed) return Promise.reject(new Error('state ingress disposed'))
    const order = ++next
    current?.controller.abort(new Error('superseded state read'))
    const controller = new AbortController()
    const me = { controller }
    current = me
    const requestedAt = now()
    let timer
    const timedOut = new Promise((_, reject) => {
      timer = setTimeout(() => {
        const e = deadlineError(deadlineMs)
        controller.abort(e)
        reject(e)
      }, deadlineMs)
    })
    const aborted = new Promise((_, reject) => controller.signal.addEventListener('abort', () => reject(controller.signal.reason ?? new Error('aborted')), { once: true }))
    const answer = Promise.resolve()
      .then(() => call('state', payload, controller.signal))
      .then(value => {
        if (now() - requestedAt > deadlineMs) {
          const e = deadlineError(deadlineMs)
          controller.abort(e)
          throw e
        }
        if (disposed) throw new Error('state ingress disposed')
        if (order !== next) throw new Error('superseded state read')
        return normalizeTasks(value, requestedAt)
      })
    return Promise.race([answer, timedOut, aborted]).finally(() => {
      clearTimeout(timer)
      if (current === me) current = null
    })
  }
  return {
    read,
    dispose() {
      disposed = true
      current?.controller.abort(new Error('state ingress disposed'))
      current = null
    },
  }
}
