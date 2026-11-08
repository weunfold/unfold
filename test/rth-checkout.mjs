// Contract tests prefer the source in this repository. RTH_CHECKOUT selects
// another checkout; missing inputs are reported as skipped.
import { existsSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const REPO = resolve(fileURLToPath(new URL('..', import.meta.url)))
export const RTH = process.env.RTH_CHECKOUT ? resolve(process.env.RTH_CHECKOUT) : existsSync(join(REPO, 'agentd/Cargo.toml')) ? REPO : resolve(REPO, '../rth')
/** An rth-owned file, by its path in the rth checkout. */
export const rthFile = (rel) => existsSync(join(RTH, 'agentd/Cargo.toml')) && existsSync(join(RTH, 'agentd', rel)) ? join(RTH, 'agentd', rel) : join(RTH, rel)
export const hasRth = (...rels) => rels.every((r) => existsSync(rthFile(r)))
/** node:test options: `{}` when every file is there, else `{ skip: <why> }`. */
export const needRth = (...rels) => {
  const missing = rels.filter((r) => !existsSync(rthFile(r)))
  return missing.length ? { skip: `needs an rth checkout with ${missing.join(', ')} (RTH_CHECKOUT, default ${RTH})` } : {}
}
/** The built rth-tasks ledger binary the delegate tests drive: $RTH_TASKS_STDIO, else the checkout's debug build. */
export const tasksStdio = () => process.env.RTH_TASKS_STDIO || join(RTH, existsSync(join(RTH, 'agentd/Cargo.toml')) ? 'agentd' : '', 'target/debug/rth-tasks-stdio')
export const needFile = (p, what) => (existsSync(p) ? {} : { skip: `needs ${what}: ${p} (an rth build; RTH_TASKS_STDIO or RTH_CHECKOUT)` })
