// Where DSH's command is: its package.json "bin" field, resolved from the shell's package, not
// node_modules/.bin (packaging does not keep .bin). In a dev install this is exactly the file the
// .bin/dsh link points at.
import { readFileSync, realpathSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'

export function resolveDshBin(shellDir) {
  let pkgJson
  try {
    pkgJson = createRequire(join(shellDir, 'package.json')).resolve('@deepseek-ai/dsh/package.json')
  } catch (e) {
    throw new Error(`@deepseek-ai/dsh is not installed for ${shellDir} (${e.code ?? e.message})`)
  }
  const pkg = JSON.parse(readFileSync(pkgJson, 'utf8'))
  const bin = typeof pkg.bin === 'string' ? pkg.bin : pkg.bin?.dsh
  if (typeof bin !== 'string' || !bin) throw new Error(`@deepseek-ai/dsh ${pkg.version ?? ''} names no "dsh" bin in ${pkgJson}`)
  return realpathSync(join(dirname(pkgJson), bin))
}
