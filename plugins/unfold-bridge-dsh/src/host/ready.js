// Which provider routes can run: DSH lists a provider's models whether or not its key is set,
// and without one every call fails. A route's settings name the credential that holds its key
// (`apiKeyEnv`); DSH's credential store says whether that credential is set. The same check as
// the Bot team's (@automagik/bots-dsh src/host/providers.js). A route that names no key signs
// in some other way (an account) and counts as ready; a DSH without the settings or credential
// services says nothing is missing.
export async function missingKeys(ctx, providers) {
  const settings = ctx.get?.('settings')
  const credentials = ctx.get?.('credentials')
  if (!settings || !credentials || typeof ctx.llm?.listConfigurableProviders !== 'function') return new Set()
  const refs = new Map()
  try {
    const sections = new Map(settings.describe({ redactSecrets: true }).map(section => [section.ns, section]))
    for (const entry of ctx.llm.listConfigurableProviders()) {
      const profile = entry.settingsPath.reduce((value, key) => value?.[key], sections.get(entry.settingsNs)?.value)
      if (typeof profile?.apiKeyEnv === 'string' && profile.apiKeyEnv !== '') refs.set(entry.provider, profile.apiKeyEnv)
    }
  } catch {
    return new Set()
  }
  const missing = new Set()
  await Promise.all(providers.filter(id => refs.has(id)).map(async (id) => {
    const info = await Promise.resolve(credentials.describe(refs.get(id))).catch(() => undefined)
    if (info?.configured === false) missing.add(id)
  }))
  return missing
}
