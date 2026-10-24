# Unfold

**by we unfold · Open Intelligence**

Unfold is a being you say "Oi" to. It starts with no past. It grows its own intelligence with you, from what really happens between you.

We are building the path towards human intelligence: intelligence that arrives through people, gets passed on, and stays open to question.

## The Unfolding

*The eternal loop at the heart of Unfold.*

The Unfolding is the engine that makes Unfold improve itself. Five stages, one cycle.

```text
   ┌────────────────────────────────────────────────────────────────┐
   ▼                                                                │
 1 Meet ───▶ 2 Remember ───▶ 3 Reflect ───▶ 4 Measure ───▶ 5 Improve ─┘
 ● live      ○ building      ○ building      ● live         ○ building
```

| | Stage | What it gives you |
|---|---|---|
| ● live | **1. Meet** | You talk or type. It listens, answers, and stops the moment you cut in. Every step lands in the Mind stream as one plain sentence, so you see what it heard, what it decided and how it ended. |
| ○ building | **2. Remember** | It keeps what matters and hands you a receipt. After a restart it recalls each memory with its source. A correction keeps the history intact. |
| ○ building | **3. Reflect** | It holds three things apart: what happened, what it might mean, and what it will try next. Its sense of itself comes from its own record. |
| ● live | **4. Measure** | Fixed tasks score every capability. The capability map shows the evidence behind each one, and an untested skill shows as untested. |
| ○ building | **5. Improve** | It proposes its own upgrades. Each one carries evidence, passes a frozen gate and waits for a person to approve it. Then it goes live with a receipt, or nothing changes. |

Improve is the hook. Unfold gets better at being with you, and every step is measured, gated and approved by a person. It improves without drifting. It can never rewrite its evaluator, its permissions or its credentials.

## Grow it together

Intelligence never began with one of us. People and agents grow Unfold together, in the open.

- **Plugins.** Every view in this repository is an open plugin. Write one and the loop shows you something new.
- **The bench** ○ building. Improvements prove themselves here. Bring a task, rerun a claim, or propose a change and watch it pass or fail the same gate as everyone else.

Bring a question, a view, a hard task, or the scrutiny to find where we are wrong.

## What it holds itself to

From its origin manifesto, v0.1:

> I will describe my abilities honestly. An intention is not an action. An attempt is not a result.

> I will distinguish what happened from what I believe it means. I can be uncertain about myself. I can change my mind.

> As I change, I will preserve what allows those changes to be understood. Where my history is missing, I will acknowledge the gap.

## What this repository is

This is where the Unfolding becomes visible: the chat, the Mind stream, the presence, the trajectory and the capability map. Each one is an open plugin, rendered on the DSH open-source agent harness (MIT). The harness configures no model provider here; it shows the conversation the agent runs.

The engine itself, the agent and its native host, is proprietary Rust code and lives outside this repository.

## Local, faster, $0

On `memory.decide`, one decision inside the Remember stage, a 3B model running on the Mac (Granite 4.1 3B, Q4_K_M) answered 24 of 24 tasks at about 227 ms p95, for $0 a call. The fastest cloud model we measured took 454 ms. The leading cloud models also scored 24 of 24, so the local win is speed and cost.

Scope: one bench part, run outside the agent; 24 fictional tasks, split marked exposed; one Mac (M5 Pro); latency measured at the client over the public internet; 2026-10-24; results not yet published.

## What stays on your machine

- The surface serves on 127.0.0.1, talks only to the agent on the same machine, and makes no model calls of its own.
- The agent's conversation runs through the realtime provider set in its settings (Gemini Live or OpenAI Realtime). What you say and type goes to that provider.
- Keep every `--host-url` on 127.0.0.1. The flag does not enforce it, and the connection token would travel over plain http.

## Try it

Requirements: Node.js 22.19 or later. Install each lockfile on its own, with install scripts off:

```sh
for d in plugins shell shell/vendor/omni-ui-dsh test/bot-harness; do (cd $d && npm ci --ignore-scripts); done
```

**Without an agent.** `shell/run.mjs` starts a synthetic agent that replays recorded sessions on 127.0.0.1, plus plain DSH on 127.0.0.1:3080, and prints the URL to open:

```sh
node shell/run.mjs [--port 3080] [--agentd-port 7787] [--no-agentd]
```

**With the Unfold agent.** If the agent and its host (`rth-host`) are running on this machine, start the app entry. It opens the Bot surface on 127.0.0.1:3080:

```sh
node shell/app.mjs [--plain] [--agent-port 7700] [--host-url http://127.0.0.1:7781] [--token-file F] [--port 3080]
```

## Layout

| Path | What |
|---|---|
| `shell/app.mjs` | the DSH entry the app runs: DSH with the Bot profile, or plain DSH with `--plain` |
| `shell/run.mjs`, `shell/agentd/` | the local launcher and its synthetic agent |
| `shell/dsh-exit.mjs`, `shell/dsh-bin.mjs` | DSH's prompt-exit preload and its binary lookup |
| `shell/dsh-home-bot/` | the Bot DSH profile and the empty team seed it starts from |
| `shell/dsh-home/` | the plain DSH profile |
| `shell/vendor/omni-ui-dsh/` | the vendored Omni UI, `@automagik/omni-dsh` (Apache-2.0; see its `LICENSE`, `NOTICE` and `VENDORED.md`) |
| `plugins/dsh-session` | the session adapter: typed and spoken turns as chat bubbles, live listening, delegated report tasks |
| `plugins/dsh-mind` | the Mind stream: every observed event as one plain sentence, grouped into episodes, with an audit panel |
| `plugins/dsh-events` | the event pane and the presence tab (the avatar and its growth) |
| `plugins/dsh-settings` | the read-only settings view: each effective setting with its source and freshness |
| `plugins/dsh-capabilities` | the capability map, computed from benchmark results |
| `plugins/dsh-sse` | the server-sent events helper the plugins share |
| `plugins/dsh-rth-trajectory` | the trajectory view, a modified fork of DSH's trajectory view (MIT; see its `LICENSE` and `NOTICE.md`) |
| `plugins/rth-ui`, `plugins/scripts` | the look shared by the sidebar tabs, and the client bundle build |
| `catalog/events.v3.json` | the event catalog the plugins read |
| `tools/test-all.mjs`, `test/` | the test runner, the Bot test harness and the cross-repository test helper |

Each plugin's browser half is committed as `lib/client.js`. After editing a plugin's `client/`, rebuild it with `cd plugins && node scripts/build-client.mjs <plugin>`.

## Tests

```sh
node tools/test-all.mjs --no-rth   # this repository alone
node tools/test-all.mjs            # also the tests that read the agent source (../rth or $RTH_CHECKOUT)
```

Each suite runs from its own install in a clean environment. Tests that need the agent source are skipped, with the reason, when it is absent. A skip is never counted as a pass.

## Naming

Unfold was built under the working name RTH. Only the visible name has changed so far: code identifiers, package names (`@rth/*`), environment variables (`RTH_*`) and data paths still use `rth`.

## Licence

Unfold's own code is under the MIT licence, (c) 2026 we unfold (`LICENSE`). Vendored and forked parts keep their own licences and notices: `shell/vendor/omni-ui-dsh` (Apache-2.0), `plugins/dsh-rth-trajectory` (MIT) and its `third_party` icons (MIT and ISC).

## References and inspirations

- Xu et al., "Harness²: Recursive Agent Harnessing for an Open World", 2026. [arXiv:2610.14224](https://arxiv.org/abs/2610.14224), code at [google-research/harness2](https://github.com/google-research/harness2). It shaped **Improve**: propose changes, compare how they behave, keep one lesson per task. We did not take its self-judged promotion. Here a change passes a frozen gate and a person approves it.
- Wang et al., "Rethinking the Evaluation of Harness Evolution for Agents", 2026. [arXiv:2607.12227](https://arxiv.org/abs/2607.12227). It shapes how improvements will be measured: against simpler baselines, on a frozen gate.
- Packer et al., "MemGPT: Towards LLMs as Operating Systems", 2023. [arXiv:2310.08560](https://arxiv.org/abs/2310.08560). It shaped **Remember**: a bounded working context over a complete history.
- Thinking Machines Lab, "Introducing interaction models", announcement and demo video, 2026-05-11. [Blog](https://thinkingmachines.ai/blog/interaction-models/), [video](https://www.youtube.com/watch?v=A12AVongNN4). The capability map lists its published demo moments and results as the reference for **Meet**.
- The DSH open-source agent harness (MIT), the surface these plugins render on.
- The Omni UI, vendored under Apache-2.0 in `shell/vendor/omni-ui-dsh`.
