// Cross-repo contract tests: the files they compare against belong to rth (the Rust sources, the
// native app, the capability registry, the built rth-tasks binary). They are read from an rth
// checkout: $RTH_CHECKOUT, else the sibling ../rth of this repository. When a needed file is
// absent the test is SKIPPED with the reason, never passed.
import { existsSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const REPO = resolve(fileURLToPath(new URL('..', import.meta.url)))
export const RTH = process.env.RTH_CHECKOUT ? resolve(process.env.RTH_CHECKOUT) : resolve(REPO, '../rth')
/** An rth-owned file, by its path in the rth checkout. */
export const rthFile = (rel) => join(RTH, rel)
export const hasRth = (...rels) => rels.every((r) => existsSync(join(RTH, r)))
/** node:test options: `{}` when every file is there, else `{ skip: <why> }`. */
export const needRth = (...rels) => {
  const missing = rels.filter((r) => !existsSync(join(RTH, r)))
  return missing.length ? { skip: `needs an rth checkout with ${missing.join(', ')} (RTH_CHECKOUT, default ${RTH})` } : {}
}
/** The built rth-tasks ledger binary the delegate tests drive: $RTH_TASKS_STDIO, else the checkout's debug build. */
export const tasksStdio = () => process.env.RTH_TASKS_STDIO || join(RTH, 'target/debug/rth-tasks-stdio')
export const needFile = (p, what) => (existsSync(p) ? {} : { skip: `needs ${what}: ${p} (an rth build; RTH_TASKS_STDIO or RTH_CHECKOUT)` })
