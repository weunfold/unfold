const src = process.env.BRAIN_SRC
const brainId = process.env.BRAIN_ID
if (!src || !brainId) {
  process.stderr.write('memory-search: BRAIN_SRC and BRAIN_ID are required\n')
  process.exit(2)
}
const { getBrainSql } = await import(`${src}/lib/db.ts`)
const { vaultForBrain } = await import(`${src}/lib/brain-memory/store.ts`)
const { searchMemories } = await import(`${src}/lib/khortex-memory/postgres/khs-memory-store.ts`)
const sql = await getBrainSql(brainId)
const vault = vaultForBrain(brainId)
process.stdout.write(JSON.stringify({ ready: true, brain_id: brainId }) + '\n')
let buf = ''
for await (const chunk of process.stdin) {
  buf += chunk
  let i
  while ((i = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, i)
    buf = buf.slice(i + 1)
    if (!line.trim()) continue
    let req
    try { req = JSON.parse(line) } catch { continue }
    try {
      const rows = await searchMemories(sql, vault, String(req.query ?? ''), {
        limit: Math.min(Math.max(1, Number(req.limit) || 5), 50),
        brainMemory: { brainId, scope: 'global', includeSuperseded: false },
      })
      process.stdout.write(JSON.stringify({ id: req.id, ok: true, hits: rows.map((r, n) => ({ brain_ref: String(r.id), rank: n + 1 })) }) + '\n')
    } catch (e) {
      process.stdout.write(JSON.stringify({ id: req.id, ok: false, error: String(e?.message ?? e).slice(0, 300) }) + '\n')
    }
  }
}
process.exit(0)
