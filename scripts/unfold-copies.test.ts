import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

// Files one Unfold plugin carries from another, byte for byte, so each installs on its own: the
// source is the one that is edited. A copy that drifts from it ships, or tests against, code
// nobody reviewed. The sse hub copies are held in sse-copies.test.ts.
const PLUGINS = join(import.meta.dir, '..', 'plugins');
// copy -> source
const COPIES: Record<string, string> = {
  'unfold-mind-dsh/catalog/events.v3.json': 'unfold-events-dsh/catalog/events.v3.json',
  'unfold-events-dsh/test/fixtures/redact-348.json': 'unfold-mind-dsh/test/fixtures/redact-348.json',
  'unfold-mind-dsh/src/client/ui.jsx': 'unfold-events-dsh/src/client/ui.jsx',
  'unfold-mind-dsh/test/support/window.js': 'unfold-events-dsh/src/window.js',
  'unfold-session-dsh/src/call-view.js': 'unfold-events-dsh/src/call-view.js',
  'unfold-settings-dsh/src/client/ui.jsx': 'unfold-events-dsh/src/client/ui.jsx',
  'unfold-settings-dsh/test/fixtures/redact-348.json': 'unfold-mind-dsh/test/fixtures/redact-348.json',
  'unfold-trajectory-dsh/catalog/events.v3.json': 'unfold-events-dsh/catalog/events.v3.json',
  'unfold-trajectory-dsh/src/mind/metrics.js': 'unfold-mind-dsh/src/metrics.js',
  'unfold-trajectory-dsh/src/mind/moments.js': 'unfold-mind-dsh/src/moments.js',
  'unfold-trajectory-dsh/test/support/catalog.js': 'unfold-mind-dsh/test/support/catalog.js',
  'unfold-trajectory-dsh/test/support/display.js': 'unfold-mind-dsh/src/display.js',
  'unfold-trajectory-dsh/test/support/mind.js': 'unfold-mind-dsh/src/mind.js',
  'unfold-trajectory-dsh/test/support/redact.js': 'unfold-mind-dsh/src/redact.js',
  'unfold-trajectory-dsh/test/support/sentences.js': 'unfold-mind-dsh/src/sentences.js',
  'unfold-trajectory-dsh/test/support/window.js': 'unfold-events-dsh/src/window.js',
};
// Mind's test catalog loader is unfold-events-dsh's src/catalog.js with the catalog one directory
// further up: adapted, not a copy.

describe('the Unfold plugins’ copied files', () => {
  test('each copy is byte for byte its source', () => {
    const drifted = Object.entries(COPIES)
      .filter(([copy, source]) => !readFileSync(join(PLUGINS, copy)).equals(readFileSync(join(PLUGINS, source))))
      .map(([copy, source]) => `${copy} (copy ${source})`);
    expect(drifted).toEqual([]);
  });
});

// The credential redaction is carried as code inside larger files: each copy of a block, by its
// declaration, is the block in its source, line for line. The #348 table (test/fixtures/redact-348.json,
// above) is one file every copy's tests read.
const REDACTION_COPIES = ['unfold-events-dsh/src/client/redact.js', 'unfold-events-dsh/src/window.js', 'unfold-settings-dsh/src/redact.js'];
const BLOCKS: [string, string, string[]][] = [
  // [the declaration, its source, its copies]
  ['const SECRET_KEY =', 'unfold-mind-dsh/src/redact.js', ['unfold-events-dsh/src/client/redact.js', 'unfold-events-dsh/src/window.js', 'unfold-settings-dsh/src/redact.js']],
  ['let OPEN_END = false', 'unfold-mind-dsh/src/redact.js', REDACTION_COPIES],
  ['function laterLine(m, nl, indent, before, name, sep, props, tail, run, br, indicator, body, prefix, q, close, offset, string) {', 'unfold-mind-dsh/src/redact.js', ['unfold-events-dsh/src/client/redact.js', 'unfold-events-dsh/src/window.js', 'unfold-settings-dsh/src/redact.js']],
  ['const SECRET_TEXT = [', 'unfold-mind-dsh/src/redact.js', ['unfold-events-dsh/src/client/redact.js', 'unfold-events-dsh/src/window.js', 'unfold-settings-dsh/src/redact.js']],
  ['const SECRET_VALUES = [', 'unfold-mind-dsh/src/redact.js', ['unfold-events-dsh/src/client/redact.js']],
  ...['const INVISIBLE_GATE =', 'const INVISIBLE =', 'const INVISIBLE_NAME =', 'const NAME_STEM =', 'const INVISIBLE_EXPLICIT =', 'export function nameText(s) {', 'export function redactText(s, open = false) {', "export function redact(value, key = '') {"].map(
    (declaration): [string, string, string[]] => [declaration, 'unfold-mind-dsh/src/redact.js', REDACTION_COPIES],
  ),
  ['export function secretsIn(s, open = false) {', 'unfold-mind-dsh/src/redact.js', ['unfold-events-dsh/src/client/redact.js']],
  ['const QUOTED_SECRET = [', 'unfold-events-dsh/src/window.js', ['unfold-mind-dsh/src/mind.js']],
  ['const YAML_KEY = [', 'unfold-mind-dsh/src/redact.js', ['unfold-events-dsh/src/client/redact.js', 'unfold-events-dsh/src/window.js', 'unfold-settings-dsh/src/redact.js', 'unfold-mind-dsh/src/mind.js']],
  ['function yamlBlockEnd(', 'unfold-mind-dsh/src/redact.js', ['unfold-events-dsh/src/client/redact.js', 'unfold-events-dsh/src/window.js', 'unfold-settings-dsh/src/redact.js', 'unfold-mind-dsh/src/mind.js']],
  ['function redactYamlBlocks(', 'unfold-mind-dsh/src/redact.js', ['unfold-events-dsh/src/client/redact.js', 'unfold-events-dsh/src/window.js', 'unfold-settings-dsh/src/redact.js']],
  ['function redactMalformedExplicitKeys(s) {', 'unfold-mind-dsh/src/redact.js', REDACTION_COPIES],
  // #507's explicit-key value scan: hidden on every copy, and remembered where secretsIn exists.
  ...['const KEY_NAME_LOWER =', 'const KEY_NAME_CAMEL =', 'const KEY_PROPS =', 'const KEY_BLOCK =', 'function explicitKeyName(text) {', 'function credentialName(name) {', 'function explicitLineValue(content) {', 'function explicitColonHits(', 'function explicitKeyHits(s) {', 'function escapedKeyHits(s) {', 'function questionLineHits(s) {', 'function redactQuestionLines(s) {', 'function redactExplicitKeyValues(s) {'].map(
    (declaration): [string, string, string[]] => [declaration, 'unfold-mind-dsh/src/redact.js', REDACTION_COPIES],
  ),
  ['const ESCAPED_SECRET = [', 'unfold-events-dsh/src/window.js', ['unfold-mind-dsh/src/mind.js']],
  ['const ESCAPED_END =', 'unfold-events-dsh/src/window.js', ['unfold-mind-dsh/src/mind.js']],
  ['const KEY_LINE_OPEN =', 'unfold-events-dsh/src/window.js', ['unfold-mind-dsh/src/mind.js']],
  ['const EX_COLON =', 'unfold-events-dsh/src/window.js', ['unfold-mind-dsh/src/mind.js']],
  ['const PROPERTY_LINE =', 'unfold-events-dsh/src/window.js', ['unfold-mind-dsh/src/mind.js']],
  ['const INDICATOR_LINE =', 'unfold-events-dsh/src/window.js', ['unfold-mind-dsh/src/mind.js']],
  ['const SKIPPED_LINE =', 'unfold-events-dsh/src/window.js', ['unfold-mind-dsh/src/mind.js']],
  ['function openKeyLine(raw, max) {', 'unfold-events-dsh/src/window.js', ['unfold-mind-dsh/src/mind.js']],
];

/**
 * The block `declaration` opens in `file`: one line, through the `]` that closes its list, or a
 * function through its closing `}`.
 */
function block(file: string, declaration: string): string {
  const lines = readFileSync(join(PLUGINS, file), 'utf8').split('\n');
  const start = lines.findIndex((line) => line.startsWith(declaration));
  if (start < 0) return `${declaration} is not in ${file}`;
  const end = declaration.endsWith('[') ? lines.indexOf(']', start) : declaration.endsWith('{') || declaration.startsWith('function ') ? lines.indexOf('}', start) : start;
  return lines.slice(start, end + 1).join('\n');
}

describe('the copied credential redaction', () => {
  test('each copy of a redaction block is its source block', () => {
    const drifted = BLOCKS.flatMap(([declaration, source, copies]) =>
      copies.filter((copy) => block(copy, declaration) !== block(source, declaration)).map((copy) => `${copy}: ${declaration} (copy ${source})`),
    );
    expect(drifted).toEqual([]);
  });
});
