# Unfold's trajectory for DeepSeek Harness (`@automagik/unfold-trajectory-dsh`)

Unfold's real-time trajectory: what Unfold heard, thought, said and did, as one ledger with a
timing overview and an inspector. It is a fork of DeepSeek's trajectory UI
(`@deepseek-ai/dsh-client-ui-trajectory` 0.2.1-rc.3, MIT) that keeps upstream's toolbar, overview,
ledger and inspector and reads Unfold's Mind stream instead of a DSH session's events. Tested
with DSH 0.2.1-rc.4, whose trajectory UI ships the same bundle bytes as rc.3.

- **Where it shows.** In Unfold's configured Session (`unfold-root` by default): as its own conversation view,
  "Unfold", next to Chat and DeepSeek's Trajectory, and as a "Trajectory" tab in the right
  sidebar. With the Bot team installed, the Bot's header gets a Trajectory button that opens the
  tab. Any other session shows a short note that the view is Unfold's.
- **Where its data comes from.** The configured session and Mind route are discovered through the read-only
  `api/unfold-session/config` and `api/unfold-mind/config` endpoints on the DSH page. One
  `EventSource` reads that Mind route's `/stream`, which `@automagik/unfold-mind-dsh` serves. The plugin opens no other connection, names no
  host or port and reads no environment. It only reads.
- **What it claims.** Only what the Mind rows show: a reply is cancelled only when its own host
  process says so, a duration is shown only when both ends were measured on one clock, and
  anything else is "not measured", with the reason.
- **DeepSeek's Trajectory stays.** This is a separate plugin with its own loader id, dictionary,
  storage names and CSS classes, so both can load in one page.

## Develop

```sh
bun install              # esbuild, for the client build
node scripts/build-client.mjs          # writes client.js; commit it
node scripts/build-client.mjs --check  # exits 1 when client.js is stale
bun run test
bun run harness && bun run lint
```

`upstream/` holds DeepSeek's `LICENSE`, `package.json` and `lib/client.js` exactly as published
(`upstream/SHA256SUMS` lists every file of the package). The build applies the patch set in
`scripts/build-client.mjs`, each one counted, and `NOTICE.md` lists every change.

## License

MIT (`LICENSE`, DeepSeek's). `NOTICE.md` has the upstream notice and the Lucide, Feather and
jsdiff notices that the built `client.js` repeats.
