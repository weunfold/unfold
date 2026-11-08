# Unfold (`@automagik/unfold-bot-dsh`)

Unfold for [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (DSH), on the Automagik Bot team (`@automagik/bots-dsh`). This host plugin adds the **Unfold Bot** (kind `unfold`): Unfold inside DSH. The Bot team draws it by its kind, with Unfold's own clay pet.

## What it does

- **In Unfold's own Automagik instance** (`AUTOMAGIK_HOST=unfold` or `AUTOMAGIK_PRELOAD=unfold`), the Unfold Bot is the team's Main Bot. The plugin hands the team a Main Bot maker (`mainBotMaker`), so the team's first Main Bot is Unfold and no default Bot is made beside it.
- **Anywhere else** (the Automagik app, or a plain DSH where the welcome's Unfold card installs this plugin), Unfold joins as one more Bot. A team with no Main Bot yet makes its default one first, so the companion the person made in the welcome stays their Main Bot.
- It makes the Bot once. An Unfold Bot that is there is kept, and one the person deleted isn't made again (`unfold.json` in the team's home).
- The Bot's Session is titled with its name, so Unfold can watch it by that name through the Unfold bridge before it exists. Its greeting waits for a model, so Unfold sees it from its first turn.

## Its defaults

The Unfold Bot type starts with the Unfold bridge and the six Unfold companion bundles (`UNFOLD_PLUGINS` in `src/host/unfold.js`). The bridge and all six companion bundles are in this repository and released together at its one version. The welcome installs their release tarballs, or their sibling paths when the welcome comes from a linked checkout or a pinned git source. The SSE library is shipped in the streaming bundles; it is released separately for reuse and is not installed as a DSH bundle.

| Plugin | What it brings |
|---|---|
| `@automagik/unfold-bridge-dsh` | Unfold watches this DSH |
| `@automagik/unfold-sse-dsh` | Unfold's events as they happen |
| `@automagik/unfold-events-dsh` | What Unfold did, as a timeline |
| `@automagik/unfold-capabilities-dsh` | What Unfold can do now |
| `@automagik/unfold-settings-dsh` | Unfold's settings, to read |
| `@automagik/unfold-mind-dsh` | What Unfold is thinking |
| `@automagik/unfold-trajectory-dsh` | Each run, step by step |
| `@automagik/unfold-session-dsh` | Unfold's own session, in DSH |

These companion packages are optional and are installed only by an explicit user choice. This repository does not preinstall them. The Bot team and its artwork remain in Automagik and must be supplied separately; see the [installation guide](../README.md#install).

## Develop

```sh
bun run harness      # the pinned DSH release the linter runs against
bun run test         # on Node, against a stand-in team runtime and the Unfold bridge's work view
bun run lint
```
