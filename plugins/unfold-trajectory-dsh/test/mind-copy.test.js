// The files this package shares with @automagik/unfold-mind-dsh, carried byte for byte: each plugin
// installs on its own, so it keeps its own copy, and the trajectory explains a reply in the same
// words as Mind. src/mind/ ships in client.js; test/support/ and catalog/ drive the real event
// window -> Mind path in the tests. Change a file there, copy it here, and move its pin.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { test } from 'node:test'

// Our path: [its path in unfold-mind-dsh, sha256].
const PINNED = {
  'src/mind/metrics.js': ['src/metrics.js', 'da9cd2482d337613821df000dc23d52a5420a84db2d66d25893b8b0421e6c9cb'],
  'src/mind/moments.js': ['src/moments.js', '8cf3966723733950012457448fd9b8ce68f489236a949ee98d9d55c449d4f327'],
  'test/support/mind.js': ['src/mind.js', 'a15acae48af959eb62a8be2fd1b95b681ee11280090d80efe8c35b2ed00026f0'],
  'test/support/redact.js': [
    'src/redact.js', 'ec69d03b5a8609b32e3b0ab22f640a02d89b2464d979f567801723f4790193ce'],
  'test/support/sentences.js': ['src/sentences.js', 'b327b65802d386f0f57104f59960173057628f60ea93edaf97d071767f810641'],
  'test/support/display.js': ['src/display.js', '41aa8e7d2def996f7a1ffa1e516c7ddf814379d83ee6138740cc1015abae5c13'],
  'test/support/catalog.js': ['test/support/catalog.js', 'c3e93d3df9136234fff3649243cd5157d5731e7663f1030ce9bc0c29cdd4912e'],
  'test/support/window.js': [
    'test/support/window.js', '68cb0d6af35d72b00e11ed52d6afdf8831f6b888ae79b9f91af04ed7260e9909'],
  'catalog/events.v3.json': ['catalog/events.v3.json', '74daa4ab1b2555f01006850318b9e5a0484c334392f5bda84b69701692d85621'],
}
const sha = (url) => createHash('sha256').update(readFileSync(url)).digest('hex')

for (const [ours, [theirs, pin]] of Object.entries(PINNED)) {
  test(`${ours} is unfold-mind-dsh's ${theirs}, byte for byte`, () => {
    assert.equal(sha(new URL(`../${ours}`, import.meta.url)), pin)
    const sibling = new URL(`../../unfold-mind-dsh/${theirs}`, import.meta.url)
    if (existsSync(sibling)) assert.equal(sha(sibling), pin, `${theirs} moved: copy plugins/unfold-mind-dsh/${theirs} to ${ours} and move the pin`)
  })
}
