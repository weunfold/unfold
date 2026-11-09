// The market lists this plugin for everyone; its description says it needs the Unfold app.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

test('the market description says the plugin is part of Unfold', () => {
  const manifest = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'))
  const description = manifest.automagik.market.description.en
  assert.match(description, /Part of Unfold; works with the Unfold app\./)
  // Opt-in and unfinished: this repository's releases do not attach it or preinstall it
  // (Refs marketplace/unfold).
  assert.match(description, /Not mature yet, and not preinstalled/)
})
