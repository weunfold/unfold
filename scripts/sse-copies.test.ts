import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';

// The Unfold plugins carry the server-sent events hub as a copy, src/sse.js, so each installs on
// its own; plugins/unfold-sse-dsh/index.js is the one that is edited. A copy that drifts from it
// would ship a hub nobody reviewed.
const PLUGINS = join(import.meta.dir, '..', 'plugins');
const HUB = join(PLUGINS, 'unfold-sse-dsh', 'index.js');
const COPIES = [
  'unfold-capabilities-dsh/src/sse.js',
  'unfold-events-dsh/src/sse.js',
  'unfold-mind-dsh/src/sse.js',
  'unfold-session-dsh/src/sse.js',
  'unfold-settings-dsh/src/sse.js',
];
// The bridge's own stream helper, adapted from an older hub before this one existed: not a copy.
const NOT_COPIES = ['unfold-bridge-dsh/src/host/sse.js'];

function sseFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    if (entry.name === 'node_modules') return [];
    const path = join(dir, entry.name);
    return entry.isDirectory() ? sseFiles(path) : entry.name === 'sse.js' ? [relative(PLUGINS, path)] : [];
  });
}

describe('the sse hub copies', () => {
  test('the Unfold plugins carry exactly the pinned copies', () => {
    const found = readdirSync(PLUGINS)
      .filter((app) => /^unfold-.+-dsh$/.test(app) && app !== 'unfold-sse-dsh')
      .flatMap((app) => sseFiles(join(PLUGINS, app)))
      .filter((file) => !NOT_COPIES.includes(file))
      .sort();
    expect(found).toEqual(COPIES);
  });

  test('each copy is byte for byte plugins/unfold-sse-dsh/index.js', () => {
    const hub = readFileSync(HUB);
    const drifted = COPIES.filter((file) => !readFileSync(join(PLUGINS, file)).equals(hub));
    expect(drifted).toEqual([]);
  });
});
