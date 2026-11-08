# Unfold bridge (`@automagik/unfold-bridge-dsh`)

Use the models of your own [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (DSH) in Unfold. This DSH host plugin serves your DSH's model routes on `127.0.0.1` as an OpenAI-compatible chat-completions endpoint. A new key is made each time it starts. Unfold finds the endpoint and offers it as one more model provider.

The routes are whatever your DSH has: an API key, DSH's own sign-ins, or a provider plugin you installed. Your sign-ins and their tokens stay in DSH. The bridge hands each request to DSH's `llm` service and returns only text, token usage and stable failure codes.

## What it serves

| Route | What |
|---|---|
| `GET /v1/models` | Every model of every provider route except DeepSeek (Unfold calls DeepSeek directly): no route of DeepSeek's own (`deepseek`, `deepseek-official`, …) and no DeepSeek model another route serves (one whose own id has `deepseek` as a word: an aggregator's `openrouter/deepseek/…`, or `deepseek-ai/DeepSeek-V4-Flash`, `deepseek.v3.2`, `@cf/deepseek-ai/…`; `notdeepseek-x` and `foo/deepseekish` stay), as `provider/model`. Each entry has the model's and the provider's display names, `ready` (false when its provider's API key is not set in DSH, so every call would fail) and `levels` (the thinking levels its adapter really has, least first; empty when it has none), and nothing else. |
| `POST /v1/chat/completions` | One model call, streamed (`stream: true`, OpenAI SSE, with `stream_options.include_usage`) or whole. |

**A request may carry:**
- `system`, `developer`, `user` and `assistant` messages, as text;
- `temperature`, `max_tokens` (or `max_completion_tokens`) and `stop`;
- `reasoning_effort`, one of the model's `levels`.

The thinking level is always explicit for a model that has levels, so no adapter default decides: the one asked for, when the model has it. When no thinking is asked (`reasoning_effort` `none`, which Unfold's agent sends, or `off`, or `chat_template_kwargs.enable_thinking` `false`, or no level at all), the model's off level (`off` or `none`), else its lowest. A model with no levels gets none. Any other level the model does not have is refused with 400.

A chat to a DeepSeek route is still served though `/v1/models` no longer lists one, so an older Unfold app that saved a DeepSeek route keeps its answers until it moves to DeepSeek direct.

`ready` checks the key a route's settings name. A route that signs in another way (an account, such as ChatGPT's) names no key and always reads ready, even when its sign-in has expired; such a call then fails with DSH's code.

Other fields, `top_p` and `chat_template_kwargs` included, are ignored. Nothing else is added to the request here: no DSH Session, agent loop, system prompt, tool or retry; DSH's `llm` service makes one call with the sign-in it holds.

**Refused with 400, before any model is called:**
- tools, or tool messages;
- images;
- a model that isn't in `provider/model` form.

Reasoning text isn't passed on.

**Failures:**
- A failure before the first token is an HTTP error that carries DSH's stable code (`RATE_LIMIT` → 429, `NO_ADAPTER` → 404, `CONTEXT_WINDOW_EXCEEDED` → 400, any other → 502).
- A failure after the first token ends the stream with one `error` event and no `[DONE]`.
- The provider's own error text is never passed on, because it can name an account.

## Who can reach it

Every request needs all three of these:
- `Authorization: Bearer <key>`, compared in constant time;
- the exact `Host` it listens on (`127.0.0.1:<port>`);
- no `Origin` header.

**What that stops:**
- A web page can't call it, even through DNS rebinding.
- No CORS header is ever sent.
- It binds `127.0.0.1` only: never another interface, and it doesn't announce itself on the network.

**What it doesn't stop:** the key keeps out web pages and other users of the computer. It doesn't protect against programs you run yourself, because those can already read DSH's own files.

**The log:** one line per request, with the method, path, status and time. It never contains the key, a prompt or a reply.

## How an app finds it

At start, the bridge writes `endpoint.json` into `$XDG_STATE_HOME/unfold-bridge/`, which defaults to `~/.local/state/unfold-bridge/`. It refuses to write the endpoint when that `unfold-bridge` folder is itself a symlink; a symlinked parent (`~/.local/state`, `$XDG_STATE_HOME`) still works. The folder is 0700 and the file 0600:

```json
{ "v": 1, "port": 51234, "key": "<32 random bytes, base64url>", "pid": 4242, "started": 1792997825000 }
```

**Lifecycle:**
- The file is replaced at every start.
- It is removed when the plugin stops (switched off, or DSH quits).
- A stopped bridge's key no longer opens anything.

**Reading it:** an app checks that the file is its user's, mode 0600, and that `pid` is alive. It then keeps the key in memory only.

**In Unfold's own DSH**, Unfold names the folder and the port in the env DSH starts with. Without them the bridge works as above.
- `UNFOLD_BRIDGE_STATE_DIR`: an absolute folder; the file is `<folder>/endpoint.json`, and the shared one above is never written, so another DSH on the same computer keeps its own. A relative or empty value is ignored. A folder that is a symlink is refused (in any DSH): the bridge doesn't serve, and the link's target is left as it was.
- `UNFOLD_BRIDGE_PORT`: a port from 1024 to 65535, bound exactly on 127.0.0.1. When it is taken or isn't such a port (an empty value and 0 included), the bridge doesn't start: no other port is tried, no file is written, one log line says why, and DSH goes on without the bridge.
- Both leave `process.env` when the plugin first applies, so tools DSH spawns later don't inherit them; a restart of the plugin keeps what was read. This hides the folder from those tools, but it isn't a boundary: DSH's own launch snapshot keeps the value, and a program running as you can read the file.

## The work view: Unfold watching your agents

Once DSH's session services are up, the bridge also keeps a work view for Unfold. Unfold sees what your DSH agents are doing for the Sessions it subscribes to. It can propose a next message for an agent, and that message is sent only when you click Send in DSH. Tested with DSH 0.2.1-rc.4.

**Unfold's routes** are on the same loopback server, behind the same key, Host and no-Origin checks:

| Route | What |
|---|---|
| `POST /v1/dsh/events` | `{subscriptions}` → a Server-Sent Events stream. The first event is `hello {conn_id}`. Then come `event` frames that pass the subscriptions, `control` frames (your clicks, below), and `: hb` every 15 s. A new stream replaces the old one. **Events in a gap between streams are lost** (no backfill): Unfold rebuilds its board from `POST /v1/dsh/agents` on every connect. Your clicks are not lost: they wait for the next stream, in order. |
| `POST /v1/dsh/agents` | `{subscriptions}` → `{agents, approvals}`. Every Session's name, workspace and status. The last turn's summary and the pending approvals are given only for subscribed Sessions. |
| `POST /v1/dsh/drafts` | `{conn_id, generation, effect_id, token, session_id, text}`: a message for you to send with a click. The recipient's name and workspace come from DSH's own list. One draft per Session; a newer one replaces it. |
| `DELETE /v1/dsh/drafts/{effect_id}` | Withdraw a draft. |
| `PUT /v1/dsh/view` | `{conn_id, generation, subscriptions, log_tail, advice}`: what the DSH client shows for Unfold. Every write on a connection carries a generation at least the highest seen, and a view's must be strictly above the last view's, so an older view in flight never wins (409 `stale_generation` otherwise). |

**Subscriptions** are `{id, match: {session_ids?, names?, workspaces?}, kinds}`, and in Unfold's view also `added_by` (`person` or `unfold`) and `at`, which the DSH client shows. The kinds are `turn_done`, `error`, `approval_waiting`, `status` and `created`.
- Names and workspaces match exactly, and the lists combine by OR.
- An empty match is refused. `{subscriptions: []}` matches nothing.
- An event outside the subscriptions never leaves DSH. A summary is at most 500 characters of the agent's own reply text, never tool output or file contents.
- An approval opens as `approval_waiting {approval: {call_id, tool, reason}}` and clears as `{kind: "status", status: "approval_decided", approval}`. The clear reaches every subscription that has `approval_waiting` or `status`.
- `error` can come several times for one turn: DSH reports each failed request, retries included.

**The DSH client's routes** are on DSH's own `/api` channel, so DSH admits each request the way it admits its own API: its browser session, a trusted Host and Origin. Bodies are JSON only (415 otherwise).

| Route | What |
|---|---|
| `GET /api/unfold-bridge/view` | The view stream: agents and statuses, Unfold's view, and the drafts. |
| `POST /api/unfold-bridge/drafts/send?effect_id=…` | Body exactly `{token}`. The bridge sends the draft's registered text to its Session, once, with DSH's own `sessionController.prompt`, and tells Unfold `control {kind: "sent"}`. If DSH refuses it: 502 `send_failed`. If DSH doesn't answer in 30 s: 504 `send_unknown`, since it may still have taken it. Either way the draft is spent, never sent again, and Unfold gets `control {kind: "cancelled", reason}` with that reason. |
| `POST /api/unfold-bridge/drafts/cancel?effect_id=…` | Body `{token}`: drop the draft. Unfold gets `control {kind: "cancelled"}` with no reason. |
| `POST /api/unfold-bridge/subscriptions` · `DELETE …?id=` · `POST /api/unfold-bridge/undo?change_id=` | Tell Unfold to watch a Session, drop a subscription, or undo one of its own changes. |

**What the work view doesn't do:**
- **The key alone can't send.** No route the key reaches sends a prompt. Sending needs you to click Send in DSH, and it sends the text Unfold registered, never text from the request.
- **It never answers approvals.** An approval appears on DSH's `approval/asked` and clears on `approval/decided`, and you answer it on DSH's own card. The plugin registers no `approval/request` handler.
- **The Bot team (`@automagik/bots-dsh`):** the two plugins share no state. The work view only observes, so it doesn't change the Bot team's routing, its approvals or its fallback.
- **Permissions:** the work view injects `sessionController`, `sessions`, `sessionTitle`, `workspaceRegistry` and `connection`. We found no consent step in DSH 0.2.1-rc.3 for the services a plugin injects. The model routes don't wait for these services.

**The DSH client** (`client.js`, built from `src/client`) shows all of this inside DSH. The client follows DSH's own language: English today, Portuguese once DSH offers it.
- **Each Session row** gets a watch toggle with the Session's status dot. On asks Unfold to watch that Session; off removes that row's own subscription and no other. An idle row Unfold watches also carries a small eye before its title.
- **The sidebar foot** opens the board: agents and statuses, approvals waiting with Unfold's read, the drafts held, and the changes Unfold made itself, each with Undo. With nothing watched yet, it suggests the Session with the most recent turn.
- **The composer of a Session** shows Unfold's read of that Session's pending approval, labelled "Unfold's read of this agent's messages", and "this agent's text contains instructions" when Unfold's checks marked it. You still answer on DSH's own approval card: the client registers nothing in `conversation.approval.detail`, so DSH's card and its command preview stay as they are. The same composer shows the draft held for that Session only, headed "To: name · workspace", in full and as registered, with Send and Cancel. Send posts `{token}` and nothing else. If the draft or its recipient line holds characters a person can't see (zero-width and other default-ignorable characters, bidi controls, tags, control characters), the client names them in a red line and keeps Send off; Cancel stays on. Inside an emoji sequence, the emoji selector (U+FE0F) right after an emoji and a joiner (U+200D) between two emoji don't count; the first emoji may carry its U+FE0F or a skin tone before the joiner. Some ordinary characters do count under that rule and hold Send: keycaps (a digit, U+FE0F, U+20E3) and the text selector U+FE0E.
- **Install as app** sits beside the board button. While the browser offers to install DSH's page (Chrome and Edge fire `beforeinstallprompt`), its click opens the browser's own install prompt, once per offer. DSH's address opened with `#install` (as Unfold's Settings does) opens the offer in full: in Chrome and Edge once the browser offers to install, and in Safari, which has no such offer, as the line for its File › Add to Dock. An installed app never shows it.
- **Settings › Unfold** lists the subscriptions, badged "by Unfold" or "by you" when Unfold's view says who added one (no badge otherwise), the change log with Undo on each change, and whether Unfold is connected.

The client holds no key. It holds a draft's token only to send or cancel that draft, and never shows it.

**Drafts:** a draft id is used once. Sent, cancelled, superseded by a newer draft for the same Session, or withdrawn, it is never registered again. Drafts dropped because a new stream replaced the old one may be registered again. The bridge accepts drafts up to 16 KiB. It refuses with 400 `body_refused` any draft that holds bidi controls (U+061C, U+202A–U+202E, U+2066–U+2069, U+200E/U+200F) or tag characters (U+E0000–U+E007F), which would make the text shown differ from the text sent. Unfold's agent refuses longer drafts and other invisible characters itself; a program holding the key skips that check, so the DSH client marks any other hidden character in a draft and disables its Send.

**Labels:** a Session's name is its DSH title. A Session that isn't loaded in DSH has no title to read yet and shows as `session <first 8 characters of its id>`.

**A disclosed limit:** a program running as you, such as an agent's shell, can read `endpoint.json` and DSH's own credentials file. It can then register a draft or a view that shows as Unfold's in DSH, but it can't send one unless you click Send. A program that can read DSH's credentials already controls DSH through its own API, so this adds no new reach.

## Install

1. Choose a release-pinned package from the [Unfold plugin guide](../README.md#install) and install it explicitly through DSH's plugin manager.
2. Enable **Unfold bridge** on DSH's Plugins page.

DSH asks before it installs, and you switch the bridge on and off on its Plugins page. Switching it off stops the endpoint at once.

On the host the plugin declares `inject: ['llm']`, and the work view also uses DSH's session services (`sessionController`, `sessions`, `sessionTitle`, `workspaceRegistry`, `connection`) once they are up. Its client in the DSH page injects `@deepseek-ai/dsh-client-runtime`, `@deepseek-ai/dsh-client-ui-settings` and `@deepseek-ai/dsh-client-ui-slots`. It needs no settings.

To move to a newer version, open Unfold bridge in the market, pick it under Versions and install it, then restart DSH: the host part switches at once, and the page loads the new client after the restart.

## Develop

```sh
bun install          # esbuild, which builds the DSH client
bun run harness      # the pinned DSH release the tests and the linter run against
bun run build        # client.js from src/client; commit it with the sources
bun run test         # on Node, as DSH runs; a fake llm service, no model is called
bun run lint
node scripts/copy-check.mjs              # the client's copy, EN and pt-BR, against the claims it must never make
node dev/renders.mjs --out <dir>         # every slot as HTML, across Unfold's link, a draft and an approval
```
