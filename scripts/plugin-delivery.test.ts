import { expect, test } from 'bun:test';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = join(import.meta.dir, '..');
const plugins = readdirSync(join(ROOT, 'plugins')).filter((name) => /^unfold-.+-dsh$/.test(name)).sort();

test('CI builds and tests every independently installed Unfold plugin', () => {
  const workflow = readFileSync(join(ROOT, '.github/workflows/plugins.yml'), 'utf8');
  const rows = [...workflow.matchAll(/^          - (unfold-.+-dsh)$/gm)].map((m) => m[1]).sort();
  expect(rows).toEqual(plugins);
  expect(plugins).toHaveLength(9);
  expect(workflow).toContain('bun install --frozen-lockfile');
  expect(workflow).toContain('bun run harness');
  expect(workflow).toContain('node scripts/build-client.mjs');
  expect(workflow).toContain('--test --test-concurrency=4');
  for (const action of workflow.matchAll(/uses: (\S+)/g)) expect(action[1]).toMatch(/@[a-f0-9]{40}$/);
});

test('plugin sources keep their published identities and point to their home repository', () => {
  for (const plugin of plugins) {
    const pkg = JSON.parse(readFileSync(join(ROOT, 'plugins', plugin, 'package.json'), 'utf8'));
    expect(pkg.name).toBe(`@automagik/${plugin}`);
    expect(pkg.repository).toEqual({ type: 'git', url: 'git+https://github.com/weunfold/unfold.git', directory: `plugins/${plugin}` });
    expect(pkg.scripts.test).not.toContain('test-guard.mjs');
  }
});
