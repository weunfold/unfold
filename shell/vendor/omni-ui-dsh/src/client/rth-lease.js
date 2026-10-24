// The browser's own freshness for delegate task rows. Each observation (`obs`) keeps the
// local monotonic deadline it was given at ingress (`deadline_mono`, state-ingress.js) when first
// seen; repeats, renders and clock changes never extend it, and once expired it stays expired
// until a NEW observation arrives. Terminal rows never expire.
export const monoNow = () => (typeof performance !== 'undefined' ? performance.now() : Date.now())

export function createLeaseMemo() {
  const seen = new Map()
  return {
    leased(task, nowMono = monoNow()) {
      if (!task || task.terminal) return task
      let entry = seen.get(task.task_id)
      if (!entry || entry.obs !== task.obs) {
        // A row without an ingress deadline (none should reach here) gets the most conservative one.
        const deadline = Number.isFinite(task.deadline_mono) ? task.deadline_mono : nowMono
        entry = { obs: task.obs, deadline, expired: false }
        seen.set(task.task_id, entry)
      }
      if (entry.expired || nowMono > entry.deadline) {
        entry.expired = true
        return { ...task, card: 'unknown', label: 'Status unknown (no recent update)', report_text: null, ended_at: null, started_at: null }
      }
      return task
    },
  }
}
