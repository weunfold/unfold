window.__ModuleLoader__.load({
	id: "@rth/dsh-mind",
	factory: (require) => {
		var module = { exports: {} };
		var exports = module.exports;
		Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
		var __defProp = Object.defineProperty;
		var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
		var __getOwnPropNames = Object.getOwnPropertyNames;
		var __hasOwnProp = Object.prototype.hasOwnProperty;
		var __export = (target, all) => {
		  for (var name in all)
		    __defProp(target, name, { get: all[name], enumerable: true });
		};
		var __copyProps = (to, from, except, desc) => {
		  if (from && typeof from === "object" || typeof from === "function") {
		    for (let key of __getOwnPropNames(from))
		      if (!__hasOwnProp.call(to, key) && key !== except)
		        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
		  }
		  return to;
		};
		var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);
		
		// dsh-mind/client/index.jsx
		var index_exports = {};
		__export(index_exports, {
		  RTH_MIND_ID: () => RTH_MIND_ID,
		  RTH_MIND_KIND: () => RTH_MIND_KIND,
		  apply: () => apply,
		  inject: () => inject
		});
		module.exports = __toCommonJS(index_exports);
		var import_react2 = require("react");
		
		// rth-ui/ui.jsx
		var import_react = require("react");
		var import_jsx_runtime = require("react/jsx-runtime");
		var BASE_CSS = `
		.rth-ui{--rth-fg:light-dark(#0c2630,#e4f4f2);--rth-muted:light-dark(#4f636c,#9db1b8);--rth-faint:light-dark(#7d8d94,#6c8088);
		 --rth-line:light-dark(rgba(12,38,48,.11),rgba(228,244,242,.09));--rth-soft:light-dark(rgba(12,38,48,.035),rgba(228,244,242,.035));
		 --rth-hover:light-dark(rgba(12,38,48,.06),rgba(228,244,242,.06));--rth-accent:light-dark(#08808f,#62e6f2);
		 --rth-ok:light-dark(#1b7a4a,#74e3a9);--rth-warn:light-dark(#995600,#ffc56e);--rth-bad:light-dark(#b0281f,#ff9584);
		 --rth-mono:"SF Mono",ui-monospace,Menlo,monospace;--rth-ease:cubic-bezier(.2,.7,.2,1);
		 color:var(--rth-fg);font-size:13px;line-height:1.5;-webkit-font-smoothing:antialiased;box-sizing:border-box;height:100%;overflow:auto}
		.rth-ui *,.rth-ui *::before,.rth-ui *::after{box-sizing:border-box}
		.rth-ui :focus-visible{outline:2px solid var(--rth-accent);outline-offset:2px;border-radius:6px}
		.rth-ui button{font:inherit;color:inherit}
		.rth-muted{color:var(--rth-muted)}
		.rth-faint{color:var(--rth-faint)}
		.rth-num{font-variant-numeric:tabular-nums}
		.rth-mono{font-family:var(--rth-mono);font-size:11px;line-height:1.45;overflow-wrap:anywhere}
		.rth-dot{display:inline-block;width:7px;height:7px;border-radius:50%;background:var(--rth-faint);flex:none;vertical-align:middle}
		.rth-dot[data-tone=ok]{background:var(--rth-ok)}
		.rth-dot[data-tone=warn]{background:var(--rth-warn)}
		.rth-dot[data-tone=bad]{background:var(--rth-bad)}
		.rth-dot[data-tone=accent]{background:var(--rth-accent)}
		.rth-dot[data-pulse=true]{animation:rth-pulse 2.4s var(--rth-ease) infinite}
		@keyframes rth-pulse{0%,100%{box-shadow:0 0 0 0 color-mix(in oklab,currentColor 0%,transparent)}50%{box-shadow:0 0 0 4px color-mix(in oklab,var(--rth-ok) 18%,transparent)}}
		.rth-fold{border-top:1px solid var(--rth-line)}
		.rth-fold-head{all:unset;box-sizing:border-box;display:flex;align-items:center;gap:8px;width:100%;padding:9px 2px;cursor:pointer;color:var(--rth-muted);font-size:12.5px;border-radius:6px}
		.rth-fold-head:hover{color:var(--rth-fg)}
		.rth-fold-head:focus-visible{outline:2px solid var(--rth-accent);outline-offset:1px}
		.rth-chev{width:12px;height:12px;flex:none;transition:transform .2s var(--rth-ease)}
		.rth-fold-head[aria-expanded=true] .rth-chev{transform:rotate(90deg)}
		.rth-fold-body{display:grid;grid-template-rows:0fr;transition:grid-template-rows .22s var(--rth-ease)}
		.rth-fold-body[data-open=true]{grid-template-rows:1fr}
		.rth-fold-body>div{overflow:hidden;min-height:0}
		.rth-fold-inner{padding:0 2px 12px}
		.rth-facts{display:grid;grid-template-columns:auto 1fr;gap:5px 14px;margin:0}
		.rth-facts dt{color:var(--rth-muted)}
		.rth-facts dd{margin:0}
		.rth-crash{padding:16px;color:var(--rth-muted)}
		@media (prefers-reduced-motion: reduce){.rth-ui *,.rth-ui *::before,.rth-ui *::after{transition:none!important;animation:none!important}}
		`;
		function useStyle(id, css) {
		  (0, import_react.useEffect)(() => {
		    if (typeof document === "undefined" || !document.head) return;
		    let el = document.getElementById(id);
		    if (!el) {
		      el = document.createElement("style");
		      el.id = id;
		      document.head.append(el);
		    }
		    if (el.textContent !== css) el.textContent = css;
		  }, [id, css]);
		}
		var store = {
		  get(key) {
		    try {
		      return globalThis.localStorage?.getItem(`rth-ui:${key}`) ?? null;
		    } catch {
		      return null;
		    }
		  },
		  set(key, value) {
		    try {
		      globalThis.localStorage?.setItem(`rth-ui:${key}`, value);
		    } catch {
		    }
		  }
		};
		function useRemembered(key, initial2) {
		  const [value, setValue] = (0, import_react.useState)(() => {
		    const raw = store.get(key);
		    if (raw === null) return initial2;
		    try {
		      return JSON.parse(raw);
		    } catch {
		      return initial2;
		    }
		  });
		  const set = (next) => {
		    setValue((prev) => {
		      const v = typeof next === "function" ? next(prev) : next;
		      store.set(key, JSON.stringify(v));
		      return v;
		    });
		  };
		  return [value, set];
		}
		var Chevron = () => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", { className: "rth-chev", viewBox: "0 0 12 12", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M4.5 2.5 8 6l-3.5 3.5", fill: "none", stroke: "currentColor", strokeWidth: "1.5", strokeLinecap: "round", strokeLinejoin: "round" }) });
		function Fold({ id, summary, children, defaultOpen = false, className = "", ...rest }) {
		  const [open, setOpen] = useRemembered(`fold:${id}`, defaultOpen);
		  const bodyId = (0, import_react.useId)();
		  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { className: `rth-fold ${className}`, "data-open": open, ...rest, children: [
		    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", { type: "button", className: "rth-fold-head", "aria-expanded": open, "aria-controls": bodyId, onClick: () => setOpen((o) => !o), children: [
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Chevron, {}),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: summary })
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "rth-fold-body", id: bodyId, "data-open": open, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "rth-fold-inner", children: open ? children : null }) }) })
		  ] });
		}
		var Boundary = class extends import_react.Component {
		  constructor(props) {
		    super(props);
		    this.state = { error: null };
		  }
		  static getDerivedStateFromError(error) {
		    return { error };
		  }
		  componentDidCatch(error) {
		    console.error(`[${this.props.name ?? "rth"}] the tab failed to draw`, error);
		  }
		  render() {
		    if (!this.state.error) return this.props.children;
		    return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "rth-ui", children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "rth-crash", role: "alert", children: "This tab hit a problem and stopped drawing. Reopen it to try again." }) });
		  }
		};
		function guarded(name, Body) {
		  const Guarded = (props) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Boundary, { name, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Body, { ...props }) });
		  Guarded.displayName = `Guarded(${name})`;
		  return Guarded;
		}
		function useNow(ms = 1e3) {
		  const [now, setNow] = (0, import_react.useState)(() => Date.now());
		  (0, import_react.useEffect)(() => {
		    let t = null;
		    const run = () => {
		      clearInterval(t);
		      if (document.visibilityState === "hidden") return;
		      setNow(Date.now());
		      t = setInterval(() => setNow(Date.now()), ms);
		    };
		    run();
		    document.addEventListener("visibilitychange", run);
		    return () => {
		      clearInterval(t);
		      document.removeEventListener("visibilitychange", run);
		    };
		  }, [ms]);
		  return now;
		}
		function ago(at, now) {
		  if (!Number.isFinite(at)) return "unknown";
		  const s = Math.max(0, Math.round((now - at) / 1e3));
		  if (s < 5) return "just now";
		  if (s < 60) return `${s} s ago`;
		  const m = Math.round(s / 60);
		  if (m < 60) return `${m} min ago`;
		  return `at ${clock(at)}`;
		}
		var clock = (at) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
		var Dot = ({ tone = "neutral", pulse = false, label }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "rth-dot", "data-tone": tone, "data-pulse": pulse, role: label ? "img" : void 0, "aria-label": label, "aria-hidden": label ? void 0 : true });
		
		// dsh-mind/src/display.mjs
		var STREAM_BEATS = 3;
		function freshness(s, now) {
		  const leaseMs = s.heartbeatMs * STREAM_BEATS;
		  const out = { stream: null, agent: null, nextChange: null };
		  const next = [];
		  if (s.stream !== "open" || s.lastStreamAt == null) {
		    out.stream = { state: "lost", label: s.stream === "opening" ? "Mind stream: connecting" : "Mind stream: lost, reconnecting (nothing below is live)" };
		  } else if (now - s.lastStreamAt > leaseMs) {
		    out.stream = { state: "stale", label: `Mind stream: no word for ${Math.round((now - s.lastStreamAt) / 1e3)} s (stale; nothing below is live)` };
		  } else {
		    out.stream = { state: "live", label: "Mind stream: live (pushed)" };
		    next.push(s.lastStreamAt + leaseMs + 1);
		  }
		  const link = s.link;
		  if (out.stream.state !== "live") out.agent = { state: "unknown", label: "agent: unknown (this panel is not receiving)" };
		  else if (!link || link.state !== "attached") out.agent = { state: "down", label: `agent feed: ${link?.state ?? "unknown"}${link?.detail != null ? ` (${link.detail})` : ""}: nothing new is observed` };
		  else {
		    const last = link.heartbeat?.at ?? link.since;
		    const serverNow = now + (s.offsetMs ?? 0);
		    if (last == null) out.agent = { state: "unknown", label: "agent feed: attached, no heartbeat yet" };
		    else if (serverNow - last > link.lease_ms) out.agent = { state: "stale", label: `agent feed: attached but no heartbeat for ${Math.round((serverNow - last) / 1e3)} s (stale)` };
		    else {
		      out.agent = { state: "live", label: `agent feed: live (attach ${link.attaches}, heartbeat fresh)` };
		      next.push(last + link.lease_ms + 1 - (s.offsetMs ?? 0));
		    }
		  }
		  out.nextChange = next.length ? Math.min(...next) : null;
		  return out;
		}
		function auditLoader(fetchDetail) {
		  let token = 0;
		  let ctrl = null;
		  return {
		    select(id, on) {
		      const mine = ++token;
		      ctrl?.abort();
		      const c = new AbortController();
		      ctrl = c;
		      on({ state: "loading", id });
		      let read;
		      try {
		        read = Promise.resolve(fetchDetail(id, c.signal));
		      } catch (e) {
		        read = Promise.reject(e);
		      }
		      read.then((r) => {
		        if (mine !== token) return;
		        if (!r?.ok) return on({ state: "error", id, error: r?.error ?? "not available" });
		        const got = r.body?.row?.id;
		        if (got !== id) return on({ state: "error", id, error: `the server answered for ${got ?? "another row"}, not ${id} (Mind restarted, or the row is gone)` });
		        on({ state: "ready", id, data: r.body });
		      }, (e) => {
		        if (mine !== token || e?.name === "AbortError") return;
		        on({ state: "error", id, error: e?.message ?? String(e) });
		      });
		    },
		    cancel() {
		      token++;
		      ctrl?.abort();
		      ctrl = null;
		    }
		  };
		}
		
		// dsh-mind/src/moments.mjs
		function quoted(sentence) {
		  if (typeof sentence !== "string") return null;
		  const a = sentence.indexOf("\u201C");
		  const b = sentence.lastIndexOf("\u201D");
		  return a >= 0 && b > a ? sentence.slice(a + 1, b) : null;
		}
		function replyIdOf(row) {
		  const re = { response_started: /^Started reply (\S+)/, agent_done: /^Reply (\S+) ended/, response_done: /^Host saw reply (\S+) end:/, response_cancelled: /^Host marked reply (\S+) cancelled/ }[row.kind];
		  const m = re ? re.exec(row.sentence ?? "") : null;
		  return m && m[1] !== "unknown" ? m[1] : null;
		}
		function usageOf(row) {
		  if (row?.kind !== "log") return null;
		  const m = /^gemini usage gen=(\d+) total_tokens=(\d+)\b/.exec(quoted(row.sentence) ?? "");
		  return m ? { gen: Number(m[1]), total: Number(m[2]) } : null;
		}
		var gapsIn = (rows) => rows.filter((r) => r.status === "gap" && r.kind === "seq_gap").length;
		var DECIDED = { look_up: "look something up first", wait: "wait", respond: "reply" };
		function decidedTo(sentence) {
		  const m = /^Decided to (\S+)/.exec(sentence ?? "");
		  return m ? m[1] : null;
		}
		function quietLine(row) {
		  const s = row.sentence ?? "";
		  if (row.kind === "link_lost") return { text: "Lost the connection to the agent. Anything open is now unsure.", tone: "warn" };
		  if (row.kind === "link_resumed") return { text: "Connected again. What happened meanwhile isn't shown.", tone: "gap" };
		  if (row.kind === "heartbeat_stale") return { text: "The agent went quiet (no heartbeat)", tone: "warn" };
		  if (row.kind === "heartbeat_resumed") return { text: "The agent is answering again", tone: "faint" };
		  if (/^journal_(replaced|shrank|rewritten|missing)$/.test(row.kind) && /^Host journal/.test(s)) return { text: "Unfold's host log changed files: its timings before and after aren't compared", tone: "gap" };
		  if (row.status === "gap") return null;
		  switch (row.kind) {
		    case "log": {
		      const note = quoted(s);
		      if (!note || usageOf(row)) return null;
		      if (/^gemini ready\b/.test(note)) return { text: "Voice model is ready", tone: "faint" };
		      const level = /^Agent note \((\w+)\)/.exec(s)?.[1];
		      if (level === "error") return { text: "Unfold reported an error", tone: "bad" };
		      if (level === "warn") return { text: "Unfold reported a warning", tone: "warn" };
		      return null;
		    }
		    case "mute_applied":
		      return { text: /muted at the host/.test(s) && !/unmuted/.test(s) ? "Microphone muted" : "Microphone unmuted", tone: "faint" };
		    case "call_devices_opened":
		      return { text: "Voice woke up", tone: "faint" };
		    case "call_devices_released":
		      return { text: "Voice went to sleep", tone: "faint" };
		    case "link_up":
		      return row.lane === "host" ? { text: "Connected to the agent", tone: "faint" } : null;
		    case "link_down":
		      return { text: "Lost the connection to the agent", tone: "warn" };
		    case "journal_fault":
		      return { text: "Couldn't save history", tone: "bad" };
		    case "command_rejected":
		      return { text: "A message was refused", tone: "bad" };
		    case "outbox_fault":
		      return { text: "Couldn't pass something on to the agent", tone: "bad" };
		    case "rejected":
		      return { text: "The agent refused something", tone: "bad" };
		    case "speaker_cleared":
		      return { text: "Stopped speaking", tone: "faint" };
		    default:
		      return null;
		  }
		}
		var OPEN = /* @__PURE__ */ new Set(["pending", "attempted"]);
		var TERMINAL = /* @__PURE__ */ new Set(["completed", "stopped", "failed"]);
		function moments(rows, episodes, { live = true } = {}) {
		  const sorted = [...rows].sort((a, b) => a.observed_at - b.observed_at);
		  const byEp = /* @__PURE__ */ new Map();
		  for (const r of sorted) {
		    if (!r.episode || r.status === "gap") continue;
		    if (!byEp.has(r.episode)) byEp.set(r.episode, []);
		    byEp.get(r.episode).push(r);
		  }
		  const out = [];
		  const seenEp = /* @__PURE__ */ new Set();
		  for (const r of sorted) {
		    if (r.episode && r.status !== "gap") {
		      if (seenEp.has(r.episode)) continue;
		      seenEp.add(r.episode);
		      out.push(...episodeMoments(episodes[r.episode] ?? null, byEp.get(r.episode), live));
		      continue;
		    }
		    if (r.kind === "input_transcript" && r.lane === "agentd") {
		      const text = quoted(r.sentence);
		      if (text) out.push({ key: `h:${r.id}`, type: "heard", at: r.observed_at, last_at: r.observed_at, text, final: /\(final;/.test(r.sentence), episode: null, rows: [r.id], status: r.status });
		      continue;
		    }
		    const q = quietLine(r);
		    if (!q) continue;
		    const prev = out.at(-1);
		    if (prev?.type === "line" && prev.text === q.text) {
		      prev.count += 1;
		      prev.at = r.observed_at;
		      prev.rows.push(r.id);
		      continue;
		    }
		    out.push({ key: `l:${r.id}`, type: "line", at: r.observed_at, text: q.text, tone: q.tone, note: Boolean(q.note), count: 1, rows: [r.id], row: r });
		  }
		  return out.sort((a, b) => a.at - b.at);
		}
		function episodeMoments(ep, rows, live) {
		  if (!rows?.length) return [];
		  const out = [];
		  const transcripts = rows.filter((r) => r.kind === "input_transcript" && r.lane === "agentd");
		  const typed = rows.find((r) => ["command_written", "command_admitted", "command_submitted", "user_text"].includes(r.kind));
		  const delta = rows.filter((r) => r.kind === "agent_delta").at(-1);
		  const started = rows.find((r) => r.kind === "response_started");
		  const decision = rows.find((r) => r.kind === "decision");
		  const startedIds = rows.filter((r) => r.kind === "response_started" && r.lane === "agentd").map(replyIdOf);
		  const endedIds = new Set(rows.filter((r) => ["agent_done", "response_done", "response_cancelled"].includes(r.kind) && TERMINAL.has(r.status)).map(replyIdOf).filter(Boolean));
		  const unended = startedIds.some((rid) => !rid || !endedIds.has(rid));
		  const spoken = rows.some((r) => r.kind === "playback" && r.lane === "host" && r.status === "completed");
		  const status = !live && ep && OPEN.has(ep.status) ? "unknown" : ep?.status ?? "unknown";
		  const ids = rows.map((r) => r.id);
		  const id = ep?.id ?? rows[0].episode;
		  if (transcripts.length) {
		    const last = transcripts.at(-1);
		    const text = quoted(last.sentence);
		    if (text) out.push({ key: `h:${id}`, type: "heard", at: transcripts[0].observed_at, last_at: last.observed_at, text, final: /\(final;/.test(last.sentence), episode: id, rows: ids, status });
		  }
		  const typedText = typed ? quoted(typed.sentence) : null;
		  if (typed && typedText && typed.kind === "user_text") out.push({ key: `t:${id}`, type: "heard", typed: true, at: typed.observed_at, last_at: typed.observed_at, text: typedText, final: true, episode: id, rows: ids, status });
		  if (delta || started) {
		    const to = decision ? decidedTo(decision.sentence) : null;
		    if (to && to !== "respond") out.push({ key: `d:${id}`, type: "line", at: decision.observed_at, text: `Chose to ${DECIDED[to] ?? to.replace(/_/g, " ")}`, tone: "faint", count: 1, episode: id, rows: ids, decided: to, open: false });
		    const anchor = started ?? delta;
		    out.push({
		      key: `r:${id}`,
		      type: "reply",
		      at: anchor.observed_at,
		      last_at: rows.at(-1).observed_at,
		      text: delta ? quoted(delta.sentence) : null,
		      status,
		      open: OPEN.has(status),
		      // seen in progress, and the panel is live: may animate
		      // No outcome seen: never quiet, whatever the time or the link. Also uncertain while any reply
		      // started in this episode has not seen its own end (an episode joined by a shared id can hold
		      // several; the episode's outcome may be another one's). This only adds doubt, never removes it.
		      uncertain: !TERMINAL.has(ep?.status) || unended,
		      spoken,
		      toTyped: Boolean(typed),
		      decided: decision ? decidedTo(decision.sentence) : null,
		      duration_ms: ep?.duration_ms ?? null,
		      episode: id,
		      rows: ids
		    });
		  } else if (decision) {
		    const ended = rows.some((r) => ["agent_done", "response_done", "response_cancelled", "command_finished", "command_rejected"].includes(r.kind) && TERMINAL.has(r.status));
		    const to = decidedTo(decision.sentence);
		    if (to) out.push({ key: `d:${id}`, type: "line", at: decision.observed_at, text: `Chose to ${DECIDED[to] ?? to.replace(/_/g, " ")}`, tone: "faint", count: 1, episode: id, rows: ids, decided: to, open: live && to !== "wait" && !ended, uncertain: to !== "wait" && !ended });
		  } else if (typed) {
		    out.push({ key: `c:${id}`, type: "line", at: typed.observed_at, text: TERMINAL.has(ep?.status) ? "A typed message" : "Got a typed message, no reply yet", tone: "faint", count: 1, episode: id, rows: ids, open: OPEN.has(status), uncertain: !TERMINAL.has(ep?.status) });
		  } else if (!transcripts.length) {
		    for (const r of rows) {
		      const q = quietLine(r);
		      if (q) {
		        out.push({ key: `e:${id}`, type: "line", at: r.observed_at, text: q.text, tone: q.tone, count: 1, episode: id, rows: ids, row: r });
		        break;
		      }
		    }
		  }
		  return out;
		}
		var REPLY_FRESH_MS = 9e4;
		var DECISION_FRESH_MS = 6e4;
		var uncertain = (m) => Boolean(m.uncertain) && (m.type === "reply" || m.type === "line");
		function nowLine(ms, fresh, now) {
		  if (fresh.stream.state !== "live") return { text: /: connecting$/.test(fresh.stream.label) ? "Connecting\u2026" : "Out of touch, reconnecting", tone: "faint", state: "unknown" };
		  if (fresh.agent.state === "down") return { text: "The agent isn't connected", tone: "warn", state: "unknown" };
		  if (fresh.agent.state !== "live") return { text: "Not sure what Unfold is doing (the agent's heartbeat is late)", tone: "faint", state: "unknown" };
		  const pending = ms.filter(uncertain);
		  const reply = pending.filter((m) => m.type === "reply").at(-1);
		  if (reply?.open && now - (reply.last_at ?? reply.at) < REPLY_FRESH_MS) return { text: "Replying\u2026", tone: "accent", state: "busy", pulse: true };
		  const hearing = ms.filter((m) => m.type === "heard" && !m.final && !m.typed && now - m.last_at < 4e3).at(-1);
		  if (hearing) return { text: "Transcribing\u2026", tone: "accent", state: "busy", pulse: true };
		  const line = pending.filter((m) => m.type === "line").at(-1);
		  if (line?.open && now - line.at < DECISION_FRESH_MS && (!reply || line.at > reply.at)) return { text: `${what(line)}, no reply yet`, tone: "accent", state: "busy", pulse: true };
		  const last = pending.at(-1);
		  if (last?.type === "reply") return { text: "Not sure how the last reply ended (no end seen)", tone: "warn", state: "unknown", stale: true, since: last.last_at ?? last.at };
		  if (last) return { text: `${what(last)}, no reply seen since`, tone: "warn", state: "unknown", stale: true, since: last.at };
		  const lastReply = ms.filter((m) => m.type === "reply").at(-1);
		  return { text: "Quiet", tone: "muted", state: "idle", since: lastReply ? lastReply.last_at : null };
		}
		var what = (m) => m.key.startsWith("c:") ? "Got your message" : `Chose to ${DECIDED[m.decided] ?? String(m.decided).replace(/_/g, " ")}`;
		function captionOf(m, now = null) {
		  if (m.type === "heard") return m.typed ? "typed" : m.final ? "heard" : "heard (not final)";
		  if (m.type !== "reply") return "";
		  const bits = [];
		  if (m.open && now != null && now - (m.last_at ?? m.at) >= REPLY_FRESH_MS) bits.push("no end seen yet");
		  else if (m.open) bits.push(m.text ? "replying" : "starting a reply");
		  else if (m.status === "stopped") bits.push("stopped");
		  else if (m.status === "failed") bits.push("didn't finish");
		  else if (m.uncertain || m.status === "unknown") bits.push("not sure how it ended");
		  if (m.toTyped) bits.push("to a typed message");
		  return bits.join(" \xB7 ");
		}
		
		// dsh-mind/src/metrics.mjs
		var PRICES = Object.freeze({
		  id: "gemini-api-pricing@2026-10-19",
		  source: "https://ai.google.dev/gemini-api/docs/pricing",
		  page_updated: "2026-10-19 UTC",
		  verified: "2026-10-22",
		  currency: "USD",
		  unit: "per 1M tokens",
		  tier: "paid, standard",
		  models: Object.freeze({
		    "gemini-3.8-live": Object.freeze({ text_in: 0.75, audio_in: 3, text_out: 4.5, audio_out: 12 }),
		    "gemini-3.8-live-extended-thinking": Object.freeze({ text_in: 0.75, audio_in: 3, text_out: 4.5, audio_out: 12 }),
		    "gemini-3.1-flash-live-preview": Object.freeze({ text_in: 0.75, audio_in: 3, text_out: 4.5, audio_out: 12 }),
		    "gemini-2.5-flash-native-audio-preview-12-2025": Object.freeze({ text_in: 0.5, audio_in: 3, text_out: 2, audio_out: 12 })
		  })
		});
		var ref = (r) => r?.ref ?? {};
		var isHost = (r, kind) => r.lane === "host" && r.kind === kind;
		var isAgent = (r, kind) => r.lane === "agentd" && r.kind === kind;
		var time = (v) => typeof v === "number" && Number.isFinite(v) ? v : null;
		var hostT = (r) => time(r?.clock?.t_ms);
		var agentT = (r) => time(r?.clock?.t_ns);
		function span(name, start, end, clock2, why = null) {
		  const base = { name, start: start ? { kind: start.kind, row: start.id } : null, end: end ? { kind: end.kind, row: end.id } : null, clock: clock2 };
		  if (why) return { ...base, ms: null, why };
		  if (!start || !end) return { ...base, ms: null, why: !start ? "its start was not observed" : "its end was not observed" };
		  if (clock2 === "host") {
		    if (ref(start).host_life == null || ref(end).host_life == null) return { ...base, ms: null, why: "host process not established (no started record observed for it)" };
		    if (ref(start).host_life !== ref(end).host_life) return { ...base, ms: null, why: "start and end come from different rth-host processes" };
		    const a2 = hostT(start);
		    const b2 = hostT(end);
		    if (a2 == null || b2 == null) return { ...base, ms: null, why: `the ${a2 == null && b2 == null ? "start and end times were" : a2 == null ? "start time was" : "end time was"} not reported` };
		    const ms = b2 - a2;
		    return ms >= 0 ? { ...base, ms } : { ...base, ms: null, why: "clock went backwards" };
		  }
		  if (!ref(start).producer || ref(start).producer !== ref(end).producer) return { ...base, ms: null, why: "start and end come from different agent boots" };
		  if (ref(end).continuation) return { ...base, ms: null, why: "the first event was evicted; its first time is unknown" };
		  const a = agentT(start);
		  const b = agentT(end);
		  if (a == null || b == null) return { ...base, ms: null, why: `the ${a == null && b == null ? "start and end times were" : a == null ? "start time was" : "end time was"} not reported` };
		  const ns = b - a;
		  return ns >= 0 ? { ...base, ms: ns / 1e6 } : { ...base, ms: null, why: "clock went backwards" };
		}
		function replyMetrics(m, byId) {
		  const own = m.rows.map((id) => byId.get(id)).filter(Boolean);
		  const replies = [...new Set(own.filter((r) => r.kind === "response_started").map((r) => ref(r).response_id).filter(Boolean))];
		  const out = { reply: null, typed: false, status: null, spans: [], playback: "not reported", tokens: { value: null, why: "the agent's usage report names no reply" }, cost: { value: null, why: "needs per-reply usage with billable categories" } };
		  if (replies.length !== 1) return { ...out, why: replies.length ? `${replies.length} replies share this episode: not attributed` : "no reply start observed" };
		  const R = replies[0];
		  out.reply = R;
		  const hStarts = own.filter((r) => isHost(r, "response_started") && ref(r).response_id === R);
		  const aStarts = own.filter((r) => isAgent(r, "response_started") && ref(r).response_id === R);
		  const ambiguous = hStarts.length > 1 ? `reply ${R} was started ${hStarts.length} times on rth-host (${new Set(hStarts.map((r) => ref(r).host_life)).size > 1 ? "in different host processes" : "in one process"})` : aStarts.length > 1 ? `reply ${R} was started ${aStarts.length} times by the agent (${new Set(aStarts.map((r) => ref(r).producer)).size > 1 ? "in different agent boots" : "in one boot"})` : hStarts[0] && aStarts[0] && ref(hStarts[0]).request_id != null && ref(aStarts[0]).request_id != null && ref(hStarts[0]).request_id !== ref(aStarts[0]).request_id ? `rth-host and the agent bind reply ${R} to different requests` : null;
		  if (ambiguous) return { ...out, why: `${ambiguous}: the id is ambiguous, not attributed` };
		  const hStart = hStarts[0] ?? null;
		  const aStart = aStarts[0] ?? null;
		  const life = hStart ? ref(hStart).host_life : null;
		  const boot = aStart ? ref(aStart).producer : null;
		  const inHost = (r) => life != null && ref(r).host_life === life;
		  const inBoot = (r) => boot != null && ref(r).producer === boot;
		  const pick = (rows, inId) => ({ row: rows.find(inId) ?? null, other: rows.find((r) => !inId(r)) ?? null });
		  const hostOf = (kind, f = () => true) => own.filter((r) => isHost(r, kind) && f(r));
		  const agentOf = (kind, f = () => true) => own.filter((r) => isAgent(r, kind) && f(r));
		  const ofR = (r) => ref(r).response_id === R;
		  const hDone = pick(hostOf("response_done", ofR), inHost);
		  const hPlay = pick(hostOf("playback", (r) => ofR(r) && ref(r).phase === "started"), inHost);
		  const aDelta = pick(agentOf("agent_delta", ofR), inBoot);
		  const aDone = pick(agentOf("agent_done", ofR), inBoot);
		  out.status = ref(hDone.row).status ?? ref(aDone.row).status ?? null;
		  const Qh = hStart ? ref(hStart).request_id ?? null : null;
		  const Qa = aStart ? ref(aStart).request_id ?? null : null;
		  const hCmds = Qh ? hostOf("command_written", (r) => ref(r).command_id === Qh) : [];
		  const aCmds = Qa ? agentOf("command_admitted", (r) => ref(r).command_id === Qa && ref(r).status === "admitted") : [];
		  if (hCmds.length > 1 || aCmds.length > 1) return { ...out, why: `request ${hCmds.length > 1 ? Qh : Qa} was typed more than once (${hCmds.length > 1 ? "rth-host" : "agent"}): the id is ambiguous, not attributed` };
		  const hCmd = pick(hCmds, inHost);
		  const aCmd = pick(aCmds, inBoot);
		  const hostChain = !hStart ? "the reply's start on rth-host was not observed (its request binding is unknown)" : life == null ? "host process not established (no started record observed for it)" : null;
		  const agentChain = !aStart ? "the agent's own start for this reply (its request binding) was not observed" : boot == null ? "the agent boot is unknown" : null;
		  const chain = (name, start, end, clock2, whyChain, idName) => {
		    if (whyChain) return span(name, start.row ?? start.other, end.row ?? end.other, clock2, whyChain);
		    if (!start.row) return span(name, null, end.row, clock2, start.other ? `its start comes from another ${idName}` : "its start was not observed");
		    if (!end.row) return span(name, start.row, null, clock2, end.other ? `its end comes from another ${idName} than its start` : "its end was not observed");
		    return span(name, start.row, end.row, clock2);
		  };
		  const one = (row) => ({ row, other: null });
		  out.typed = Boolean(hCmds.length || aCmds.length);
		  if (out.typed) out.spans.push(chain("reply finished", hCmd, hDone, "host", hostChain, "rth-host process"));
		  out.spans.push(chain("reply duration", one(hStart), hDone, "host", hostChain, "rth-host process"));
		  if (hPlay.row) {
		    out.playback = "reported";
		    out.spans.push(out.typed ? chain("playback started", hCmd, hPlay, "host", hostChain, "rth-host process") : chain("playback started after the reply began", one(hStart), hPlay, "host", hostChain, "rth-host process"));
		  }
		  if (out.typed) out.spans.push(chain("first text received", aCmd, aDelta, "agent", agentChain, "agent boot"));
		  out.spans.push({ name: "provider time to first token", ms: null, why: "not instrumented (the provider adapter emits no first-token time)", clock: null, start: null, end: null });
		  return out;
		}
		function usageReports(rows) {
		  const seen = /* @__PURE__ */ new Set();
		  const reports = [];
		  for (const r of rows) {
		    const x = ref(r);
		    if (x.tag !== "provider_usage") continue;
		    const key = r.event_id ?? r.id;
		    if (seen.has(key)) continue;
		    seen.add(key);
		    reports.push({ row: r.id, gen: x.gen, total_tokens: x.total_tokens, producer: x.producer });
		  }
		  return reports;
		}
		function modelFor(m, rows, byId) {
		  const own = m.rows.map((id) => byId.get(id)).filter(Boolean);
		  const aStart = own.find((r) => isAgent(r, "response_started"));
		  if (!aStart) return null;
		  const i = rows.indexOf(aStart);
		  for (let k = i - 1; k >= 0; k--) {
		    const x = ref(rows[k]);
		    if (x.tag === "provider_ready" && x.producer === ref(aStart).producer) return x.model;
		  }
		  return null;
		}
		var measured = (met, name) => met?.spans?.find((s) => s.name === name && s.ms != null)?.ms ?? null;
		function sessionSummary(items, rows) {
		  const replies = items.filter((x) => x.type === "reply" && x.metrics);
		  const done = replies.filter((x) => x.metrics.status === "completed");
		  const durations = done.map((x) => measured(x.metrics, "reply duration")).filter((v) => v != null);
		  return {
		    replies: replies.length,
		    completed: done.length,
		    timed: durations.length,
		    avgDurationMs: durations.length ? durations.reduce((a, b) => a + b, 0) / durations.length : null,
		    usageReports: usageReports(rows).length,
		    tokensLinked: 0
		  };
		}
		var fmtSec = (v) => v == null ? null : v < 1e3 ? `${(v / 1e3).toFixed(2)} s` : `${(v / 1e3).toFixed(1)} s`;
		function replyCaption(m, now = null) {
		  return [captionOf(m, now), metricLine(m.metrics)].filter(Boolean).join(" \xB7 ");
		}
		function metricLine(met) {
		  if (!met || met.why) return "";
		  const parts = [];
		  const fin = measured(met, "reply finished");
		  const dur2 = measured(met, "reply duration");
		  const first = measured(met, "first text received");
		  if (fin != null) parts.push(`finished in ${fmtSec(fin)}`);
		  else if (dur2 != null) parts.push(`reply ${fmtSec(dur2)}`);
		  if (first != null) parts.push(`first text ${fmtSec(first)}`);
		  if (met.playback === "reported") parts.push("playback reported");
		  return parts.join(" \xB7 ");
		}
		
		// dsh-mind/client/index.jsx
		var import_jsx_runtime2 = require("react/jsx-runtime");
		var RTH_MIND_ID = "@rth/dsh-mind";
		var RTH_MIND_KIND = "rth.mind";
		var BASE = "api/rth-mind";
		var SHOWN = 300;
		var CATS = {
		  perceive: { icon: "\u{1F442}", label: "perceive" },
		  decide: { icon: "\u2696\uFE0F", label: "decide" },
		  act: { icon: "\u2699\uFE0F", label: "act" },
		  speak: { icon: "\u{1F4AC}", label: "speak" },
		  remember: { icon: "\u{1F5C4}\uFE0F", label: "remember" },
		  system: { icon: "\u{1F527}", label: "system" }
		};
		var inject = ["slots", "sidebarRightTabs", "sidebarRight"];
		function apply(ctx) {
		  ctx.effect(() => ctx.sidebarRightTabs.register({
		    id: RTH_MIND_ID,
		    kind: RTH_MIND_KIND,
		    priority: "extension",
		    title: () => "Mind",
		    guide: [{ id: "rth-mind", order: 92, title: () => "Mind", description: () => "What Unfold hears, says and does, in plain words" }]
		  }), "rth-mind: tab type");
		  ctx.effect(() => () => {
		    for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
		      if (tab.kind !== RTH_MIND_KIND) continue;
		      try {
		        ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId);
		      } catch {
		      }
		    }
		  }, "rth-mind: close open tabs on unload");
		  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab", () => ctx.slots.register({ name: "sidebar.right.pane.tab", key: RTH_MIND_ID }, guarded("rth-mind", MindBody))), "rth-mind: tab body");
		  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab.title", () => ctx.slots.register({ name: "sidebar.right.pane.tab.title", key: RTH_MIND_ID }, MindTitle)), "rth-mind: tab title");
		}
		function MindTitle({ useTabInfo }) {
		  const { tab } = useTabInfo();
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { "data-rth": "mind-title", children: tab.title });
		}
		var initial = { rows: [], episodes: {}, link: null, sources: null, heartbeatMs: 5e3, offsetMs: 0, lastStreamAt: null, stream: "opening", reconnects: 0, marks: [], suggestions: [], dropped: 0 };
		function reduce(s, { event, data, at }) {
		  switch (event) {
		    case "open":
		      return { ...s, stream: "open", lastStreamAt: at };
		    case "lost": {
		      const mark = { id: `local-${at}`, lane: "mind", source: "UI", kind: "panel_stream_lost", category: "system", status: "gap", observed_at: at + s.offsetMs, sentence: `This panel's stream ${data}. It reconnects and reloads the server's snapshot; rows the server evicted meanwhile are not shown.`, clock: { basis: "browser clock" }, local: true };
		      return s.stream === "open" ? { ...s, stream: "lost", marks: [...s.marks, mark].slice(-20) } : { ...s, stream: "lost" };
		    }
		    case "snapshot":
		      return {
		        ...s,
		        rows: data.rows,
		        episodes: Object.fromEntries(data.episodes.map((e) => [e.id, e])),
		        link: data.link,
		        sources: data.sources,
		        heartbeatMs: data.heartbeat_ms ?? s.heartbeatMs,
		        offsetMs: data.at - at,
		        lastStreamAt: at,
		        stream: "open",
		        reconnects: s.lastStreamAt == null ? s.reconnects : s.reconnects + 1,
		        suggestionKeys: data.suggestions?.keys ?? [],
		        dropped: data.dropped
		      };
		    case "row":
		      return { ...s, rows: [...s.rows, data], lastStreamAt: at };
		    case "row-updated":
		      return { ...s, rows: s.rows.map((r) => r.id === data.id ? data : r), lastStreamAt: at };
		    case "evicted":
		      return { ...s, rows: s.rows.filter((r) => r.id !== data.id), dropped: s.dropped + 1, lastStreamAt: at };
		    case "episode":
		      return { ...s, episodes: { ...s.episodes, [data.id]: data }, lastStreamAt: at };
		    case "episode-removed": {
		      const { [data.id]: _, ...rest } = s.episodes;
		      return { ...s, episodes: rest, lastStreamAt: at };
		    }
		    case "merged": {
		      const { [data.from]: _, ...rest } = s.episodes;
		      return { ...s, episodes: rest, rows: s.rows.map((r) => r.episode === data.from ? { ...r, episode: data.into } : r), lastStreamAt: at };
		    }
		    case "link":
		      return { ...s, link: data, lastStreamAt: at };
		    case "heartbeat":
		      return { ...s, link: s.link ? { ...s.link, heartbeat: { ...s.link.heartbeat, at: data.at, stale: false } } : s.link, lastStreamAt: at };
		    case "heartbeat-mind":
		      return { ...s, link: data.link ?? s.link, offsetMs: data.at - at, lastStreamAt: at };
		    default:
		      return s;
		  }
		}
		function useMindStream() {
		  const [state, dispatch] = (0, import_react2.useReducer)(reduce, initial);
		  const beatMs = (0, import_react2.useRef)(5e3);
		  beatMs.current = state.heartbeatMs;
		  (0, import_react2.useEffect)(() => {
		    let source = null;
		    let lease = null;
		    let retry = null;
		    let backoff = 1e3;
		    let closed = false;
		    const alive = () => {
		      clearTimeout(lease);
		      lease = setTimeout(() => {
		        dispatch({ event: "lost", data: "went silent past its lease", at: Date.now() });
		        source?.close();
		        retry = setTimeout(open, backoff);
		        backoff = Math.min(backoff * 2, 8e3);
		      }, beatMs.current * 3 + 500);
		    };
		    const names = ["snapshot", "row", "row-updated", "evicted", "episode", "episode-removed", "merged", "link", "heartbeat", "heartbeat-mind"];
		    function open() {
		      if (closed) return;
		      source = new EventSource(new URL(`${BASE}/stream`, document.baseURI));
		      for (const name of names) {
		        source.addEventListener(name, (e) => {
		          alive();
		          if (name === "snapshot") backoff = 1e3;
		          dispatch({ event: name, data: JSON.parse(e.data), at: Date.now() });
		        });
		      }
		      source.addEventListener("resnapshot", () => dispatch({ event: "lost", data: "was dropped by the server (slow reader)", at: Date.now() }));
		      source.onopen = () => alive();
		      source.onerror = () => dispatch({ event: "lost", data: "errored", at: Date.now() });
		    }
		    open();
		    return () => {
		      closed = true;
		      clearTimeout(lease);
		      clearTimeout(retry);
		      source?.close();
		    };
		  }, []);
		  return state;
		}
		function useFreshness(state) {
		  const [now, setNow] = (0, import_react2.useState)(Date.now());
		  const f = freshness({ stream: state.stream, lastStreamAt: state.lastStreamAt, heartbeatMs: state.heartbeatMs, offsetMs: state.offsetMs, link: state.link }, now);
		  (0, import_react2.useEffect)(() => {
		    if (f.nextChange == null) return;
		    const t = setTimeout(() => setNow(Date.now()), Math.max(0, f.nextChange - Date.now()));
		    return () => clearTimeout(t);
		  }, [f.nextChange, state.lastStreamAt]);
		  (0, import_react2.useEffect)(() => setNow(Date.now()), [state.lastStreamAt, state.stream]);
		  return f;
		}
		var muted = { color: "var(--rth-muted)" };
		var box = { border: "1px solid var(--rth-line)", borderRadius: 10, padding: "8px 10px", background: "var(--rth-soft)" };
		var chipBase = { fontSize: 10.5, borderRadius: 9, padding: "0 7px", whiteSpace: "nowrap", lineHeight: "17px", color: "var(--rth-muted)" };
		var button = { font: "inherit", fontSize: 12, padding: "3px 10px", borderRadius: 7, border: "1px solid var(--rth-line)", background: "transparent", color: "inherit", cursor: "pointer" };
		var pad2 = (n, w = 2) => String(n).padStart(w, "0");
		var hms = (ms) => {
		  if (!Number.isFinite(ms)) return "unknown";
		  const d = new Date(ms);
		  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}.${pad2(d.getMilliseconds(), 3)}`;
		};
		var dur = (ms) => ms == null ? null : ms < 1 ? `${ms.toFixed(2)} ms` : ms < 1e3 ? `${Math.round(ms)} ms` : `${(ms / 1e3).toFixed(1)} s`;
		var STATUS_WORD = { observed: "seen", pending: "open", attempted: "sent", completed: "done", stopped: "stopped", failed: "failed", unknown: "not sure", gap: "gap" };
		function Chip({ status, children }) {
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { "data-rth": "mind-status", "data-status": status, className: "rth-chip", style: chipBase, children: children ?? STATUS_WORD[status] ?? status });
		}
		function producerTime(row) {
		  const c = row.clock ?? {};
		  if (Number.isFinite(c.wall_ms_estimate)) return { text: `\u2248${hms(c.wall_ms_estimate)} agent`, title: c.wall_basis };
		  if (Number.isFinite(c.t_ms)) return { text: `host +${(c.t_ms / 1e3).toFixed(3)} s`, title: c.basis };
		  if (Number.isFinite(c.t_ns)) return { text: `agent +${(c.t_ns / 1e9).toFixed(3)} s`, title: `${c.basis}; ${c.wall_basis}` };
		  return null;
		}
		var MIND_CSS = `
		.rth-mind{display:flex;flex-direction:column;height:100%;overflow:hidden;position:relative}
		.rth-now{display:flex;align-items:center;gap:9px;padding:14px 16px 12px;border-bottom:1px solid var(--rth-line)}
		.rth-now-text{font-size:14px;font-weight:600;letter-spacing:-.005em}
		.rth-now[data-state=unknown] .rth-now-text{color:var(--rth-muted);font-weight:500}
		.rth-now-meta{margin-left:auto;font-size:11.5px;color:var(--rth-faint)}
		/* Plain block flow (no flex column): nothing can be squeezed into its neighbour (WebKit can
		   shrink flex items until the captions overlap). */
		.rth-feed{flex:1 1 auto;overflow:auto;padding:14px 14px 18px;display:block;transition:opacity .4s}
		.rth-feed[data-live=false]{opacity:.6}
		.rth-sum-line{padding:6px 16px 7px;font-size:11.5px;color:var(--rth-faint);border-bottom:1px solid var(--rth-line)}
		.rth-break{display:grid;grid-template-columns:auto 1fr;gap:3px 12px;margin:0 0 8px;font-size:11.5px}
		.rth-break dt{color:var(--rth-faint)}
		.rth-break dd{margin:0}
		.rth-empty{margin:auto;max-width:240px;text-align:center;color:var(--rth-faint);font-size:12.5px}
		.rth-m{box-sizing:border-box;display:block;cursor:pointer;border-radius:12px;max-width:100%;margin:6px 0;padding:0;background:none;border:0;color:inherit;font:inherit;text-align:start}
		.rth-m:focus-visible{outline:2px solid var(--rth-accent);outline-offset:2px}
		.rth-m-heard{display:flex;flex-direction:column;align-items:flex-end;width:fit-content;max-width:86%;margin:12px 0 6px auto}
		.rth-m-heard .rth-bubble{padding:7px 12px;border-radius:16px 16px 5px 16px;background:color-mix(in oklab,var(--rth-accent) 13%,transparent);color:var(--rth-fg)}
		.rth-m-heard[data-final=false] .rth-bubble{background:color-mix(in oklab,var(--rth-accent) 7%,transparent);color:var(--rth-muted)}
		.rth-mind .rth-m-heard .rth-cap{text-align:right}
		.rth-m-reply{width:fit-content;max-width:92%;margin:6px auto 10px 0;padding:2px}
		.rth-m-reply .rth-said{font-size:13.5px;line-height:1.5}
		.rth-m-reply[data-open=true] .rth-said{color:var(--rth-muted)}
		.rth-mind .rth-cap{margin-top:3px;font-size:11px;color:var(--rth-faint)}
		.rth-mind .rth-m:hover .rth-cap{color:var(--rth-muted)}
		.rth-m-line{width:fit-content;max-width:94%;margin:6px auto;padding:1px 8px;text-align:center;font-size:11.5px;color:var(--rth-faint)}
		.rth-m-line[data-tone=warn]{color:var(--rth-warn)}
		.rth-m-line[data-tone=gap]{color:color-mix(in oklab,var(--rth-warn) 55%,var(--rth-faint))}
		.rth-m-line[data-tone=bad]{color:var(--rth-bad)}
		.rth-working{display:inline-flex;gap:3px;margin-left:2px;vertical-align:middle}
		.rth-working i{width:4px;height:4px;border-radius:50%;background:currentColor;opacity:.35;animation:rth-blink 1.2s infinite}
		.rth-working i:nth-child(2){animation-delay:.2s}.rth-working i:nth-child(3){animation-delay:.4s}
		@keyframes rth-blink{0%,100%{opacity:.25}40%{opacity:.9}}
		.rth-steps{margin:6px 0 4px;padding:8px 10px;border-radius:10px;border:1px solid var(--rth-line);background:var(--rth-soft);font-size:12px;cursor:default;text-align:left;align-self:stretch}
		.rth-steps ol{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:2px}
		.rth-step{all:unset;box-sizing:border-box;display:flex;gap:8px;width:100%;padding:3px 4px;border-radius:6px;cursor:pointer;align-items:baseline}
		.rth-step:hover{background:var(--rth-hover)}
		.rth-step:focus-visible{outline:2px solid var(--rth-accent)}
		.rth-step-t{flex:none;font-variant-numeric:tabular-nums;color:var(--rth-faint);font-size:11px;min-width:58px}
		.rth-step-s{flex:1;min-width:0;overflow-wrap:anywhere}
		.rth-steps-foot{display:flex;gap:10px;margin-top:6px;flex-wrap:wrap}
		.rth-link{all:unset;cursor:pointer;color:var(--rth-accent);font-size:11.5px}
		.rth-link:focus-visible{outline:2px solid var(--rth-accent)}
		.rth-jump{position:absolute;left:50%;bottom:58px;transform:translateX(-50%);border:1px solid var(--rth-line);border-radius:14px;padding:4px 12px;font-size:12px;background:light-dark(#fff,#0f2129);color:var(--rth-fg);cursor:pointer;box-shadow:0 6px 18px -8px rgba(0,0,0,.4)}
		.rth-mind-foot{padding:0 14px;border-top:1px solid var(--rth-line);max-height:55%;overflow:auto}
		.rth-mind-foot>.rth-fold{border-top:0}
		.rth-src{list-style:none;margin:0 0 8px;padding:0;display:flex;flex-direction:column;gap:3px}
		.rth-src li{display:flex;gap:8px;align-items:center}
		.rth-input{font:inherit;font-size:12px;padding:5px 9px;border-radius:7px;border:1px solid var(--rth-line);background:transparent;color:inherit;width:100%}
		.rth-all{list-style:none;margin:8px 0 0;padding:0;display:flex;flex-direction:column;gap:3px}
		.rth-chip{border:1px solid var(--rth-line)}
		.rth-chip[data-status=failed]{color:var(--rth-bad);border-color:color-mix(in oklab,var(--rth-bad) 40%,transparent)}
		.rth-chip[data-status=pending],.rth-chip[data-status=attempted]{color:var(--rth-accent)}
		.rth-chip[data-status=unknown],.rth-chip[data-status=gap]{color:var(--rth-warn);border-style:dashed}
		`;
		var Working = () => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-working", "aria-hidden": "true", children: [
		  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("i", {}),
		  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("i", {}),
		  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("i", {})
		] });
		function MindBody() {
		  useStyle("rth-ui-base", BASE_CSS);
		  useStyle("rth-ui-mind", MIND_CSS);
		  const state = useMindStream();
		  const fresh = useFreshness(state);
		  const now = useNow(5e3);
		  const live = fresh.stream.state === "live" && fresh.agent.state === "live";
		  const [open, setOpen] = (0, import_react2.useState)(() => /* @__PURE__ */ new Set());
		  const [audit, setAudit] = (0, import_react2.useState)(null);
		  const list = (0, import_react2.useRef)(null);
		  const stick = (0, import_react2.useRef)(true);
		  const [unseen, setUnseen] = (0, import_react2.useState)(0);
		  const all = (0, import_react2.useMemo)(() => [...state.rows, ...state.marks].sort((a, b) => a.observed_at - b.observed_at), [state.rows, state.marks]);
		  const byIdAll = (0, import_react2.useMemo)(() => new Map(all.map((r) => [r.id, r])), [all]);
		  const items = (0, import_react2.useMemo)(() => moments(all, state.episodes, { live }).map((m) => m.type === "reply" && !m.open ? { ...m, metrics: replyMetrics(m, byIdAll), model: modelFor(m, all, byIdAll) } : m), [all, state.episodes, live, byIdAll]);
		  const summary = (0, import_react2.useMemo)(() => sessionSummary(items, all), [items, all]);
		  const serverNow = Date.now() + state.offsetMs;
		  const head = nowLine(items, fresh, serverNow);
		  const byId = (0, import_react2.useMemo)(() => new Map(all.map((r) => [r.id, r])), [all]);
		  const count = items.length;
		  (0, import_react2.useEffect)(() => {
		    const el = list.current;
		    if (!el) return;
		    if (stick.current) {
		      el.scrollTop = el.scrollHeight;
		      setUnseen(0);
		    } else setUnseen((n) => n + 1);
		  }, [count, items.at(-1)?.text]);
		  const onScroll = () => {
		    const el = list.current;
		    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
		    if (stick.current) setUnseen(0);
		  };
		  const jump = () => {
		    stick.current = true;
		    list.current.scrollTop = list.current.scrollHeight;
		    setUnseen(0);
		  };
		  const toggle = (key) => setOpen((s) => {
		    const n = new Set(s);
		    n.has(key) ? n.delete(key) : n.add(key);
		    return n;
		  });
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-ui rth-mind", "data-rth": "mind", children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("header", { className: "rth-now", "data-state": head.state, "data-rth": "mind-now", children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Dot, { tone: head.state === "unknown" ? "neutral" : head.state === "busy" ? "accent" : "ok", pulse: Boolean(head.pulse) }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-now-text", "data-rth": "mind-agent", "data-state": fresh.agent.state, role: "status", children: [
		        head.text,
		        head.pulse ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Working, {}) : null
		      ] }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-now-meta rth-num", "data-rth": "mind-stream", "data-state": fresh.stream.state, children: head.stale && head.since ? `since ${ago(head.since, serverNow)}` : head.state === "unknown" ? state.lastStreamAt ? `last update ${ago(state.lastStreamAt, now)}` : "" : head.since ? `last reply ${ago(head.since, serverNow)}` : "" })
		    ] }),
		    summary.replies ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-sum-line rth-num", "data-rth": "mind-summary", title: "In what this panel still holds (not a durable session total). Tokens and cost need per-reply usage from the agent.", children: [`${summary.replies} repl${summary.replies > 1 ? "ies" : "y"}`, summary.avgDurationMs != null ? `avg reply ${fmtSec(summary.avgDurationMs)} (${summary.timed} of ${summary.completed} completed timed)` : null, "tokens and cost: not linked yet"].filter(Boolean).join(" \xB7 ") }) : null,
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-feed", ref: list, onScroll, "data-live": live, "data-rth": "mind-rows", children: items.length ? items.map((m) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Moment, { m, open: open.has(m.key), onToggle: () => toggle(m.key), rows: m.rows.map((id) => byId.get(id)).filter(Boolean), episode: state.episodes[m.episode], audit, onAudit: setAudit, live, now: serverNow }, m.key)) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-empty", children: "Nothing yet. When Unfold hears something, replies or notes something, it shows up here." }) }),
		    unseen > 0 ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { type: "button", className: "rth-jump", onClick: jump, children: [
		      "\u2193 ",
		      unseen,
		      " new"
		    ] }) : null,
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("footer", { className: "rth-mind-foot", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Fold, { id: "mind:more", summary: "More", children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Sources, { sources: state.sources, fresh, reconnects: state.reconnects, gaps: gapsIn(all), usage: usageReports(all).length }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Fold, { id: "mind:every-event", summary: "Every event (technical)", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(EveryEvent, { rows: all, episodes: state.episodes, dropped: state.dropped, audit, onAudit: setAudit }) })
		    ] }) })
		  ] });
		}
		function Moment({ m, open, onToggle, rows, episode, audit, onAudit, live, now }) {
		  const going = m.open && now - (m.last_at ?? m.at) < REPLY_FRESH_MS;
		  const cls = m.type === "heard" ? "rth-m rth-m-heard" : m.type === "reply" ? "rth-m rth-m-reply" : "rth-m rth-m-line";
		  const body = m.type === "heard" ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-bubble", children: m.text }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-cap", children: captionOf(m, now) })
		  ] }) : m.type === "reply" ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-said", children: [
		      m.text ?? (m.open || m.uncertain ? "A reply started" : "A reply (no text seen)"),
		      going ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Working, {}) : null
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-cap", "data-rth": "mind-metrics", children: replyCaption(m, now) })
		  ] }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
		    m.text,
		    m.count > 1 ? ` \xD7${m.count}` : ""
		  ] });
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(
		      "div",
		      {
		        role: "button",
		        tabIndex: 0,
		        className: cls,
		        "aria-expanded": open,
		        "data-rth": "mind-moment",
		        "data-type": m.type,
		        "data-final": m.final === void 0 ? void 0 : String(m.final),
		        "data-open": String(Boolean(m.open)),
		        "data-stale": m.uncertain && !going ? "true" : void 0,
		        "data-tone": m.tone,
		        "data-note": m.note ? "true" : void 0,
		        "data-episode": m.episode ?? "",
		        onClick: onToggle,
		        onKeyDown: (e) => {
		          if (e.key === "Enter" || e.key === " ") {
		            e.preventDefault();
		            onToggle();
		          }
		        },
		        children: body
		      }
		    ),
		    open ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Steps, { m, rows, episode, audit, onAudit, live }) : null
		  ] });
		}
		function Breakdown({ met, model }) {
		  const clockName = { host: "rth-host clock", agent: "agent clock" };
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dl", { className: "rth-break", "data-rth": "mind-breakdown", children: [
		    met.why ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Timing" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dd", { children: [
		        "not available: ",
		        met.why
		      ] })
		    ] }) : null,
		    met.spans.map((sp) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_react2.Fragment, { children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: sp.name[0].toUpperCase() + sp.name.slice(1) }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dd", { children: sp.ms != null ? `${fmtSec(sp.ms)} \xB7 ${sp.start.kind} \u2192 ${sp.end.kind} (${clockName[sp.clock]})` : `not available: ${sp.why}` })
		    ] }, sp.name)),
		    !met.why ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Playback" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dd", { children: met.playback === "reported" ? "reported by rth-host (handed to the device; not proof it was heard)" : "not reported" })
		    ] }) : null,
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Status" }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dd", { children: met.status ?? "not reported" }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Model configured before it" }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dd", { children: model ? `${model} (observed context, by order; not a per-reply binding)` : "not known" }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Tokens" }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dd", { children: [
		      "not available: ",
		      met.tokens.why
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Cost" }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dd", { children: [
		      "not available: ",
		      met.cost.why
		    ] })
		  ] });
		}
		function Steps({ m, rows, episode, audit, onAudit, live }) {
		  const [suggesting, setSuggesting] = (0, import_react2.useState)(false);
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-steps", "data-rth": "mind-steps", children: [
		    m.type === "heard" && !m.typed ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-faint", style: { marginBottom: 4 }, children: [
		      "What the provider transcribed. It does not say who spoke",
		      m.final ? "" : ", and it may still change",
		      "."
		    ] }) : null,
		    m.metrics ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Breakdown, { met: m.metrics, model: m.model }) : null,
		    episode ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-faint", style: { marginBottom: 4 }, children: [
		      episode.title,
		      " \xB7 ",
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Chip, { status: !live && ["pending", "attempted"].includes(episode.status) ? "unknown" : episode.status })
		    ] }) : null,
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ol", { children: rows.map((r) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { type: "button", className: "rth-step", "data-rth": "mind-row", "data-id": r.id, "data-kind": r.kind, "data-category": r.category, "data-status": r.status, "data-episode": r.episode ?? "", "data-lane": r.lane, "aria-expanded": audit === r.id, onClick: () => !r.local && onAudit(audit === r.id ? null : r.id), children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-step-t", children: hms(r.observed_at).slice(0, 8) }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-step-s", "data-rth": "mind-sentence", children: r.sentence }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Chip, { status: r.status })
		      ] }),
		      audit === r.id ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Audit, { id: r.id, onClose: () => onAudit(null), onSelect: onAudit }) : null
		    ] }, r.id)) }),
		    episode ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-steps-foot", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", className: "rth-link", "data-rth": "mind-suggest", onClick: () => setSuggesting(!suggesting), children: suggesting ? "Close the suggestion" : "Suggest a better way" }) }) : null,
		    suggesting && episode ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Suggest, { ep: episode }) : null
		  ] });
		}
		var SOURCE_NAMES = { agentd_feed: "The agent's live feed", host_journal: "The host's log", agentd_journal: "The agent's journal", brain: "Brain (memory)", jev: "Jev" };
		var SOURCE_STATE = { bound: ["Connected", "ok"], following: ["Following", "ok"], off: ["Not set up", "neutral"], "no feed": ["No feed yet", "neutral"] };
		function Sources({ sources, fresh, reconnects, gaps = 0, usage = 0 }) {
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-rth": "mind-sources", children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-faint", style: { marginBottom: 6 }, children: "What Mind can see" }),
		    sources ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "rth-src", children: Object.entries(sources).map(([k, v]) => {
		      const [word, tone] = SOURCE_STATE[v.state] ?? [v.state, "warn"];
		      return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { "data-source": k, "data-state": v.state, title: v.detail, children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Dot, { tone }),
		        " ",
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { children: SOURCE_NAMES[k] ?? k }),
		        " ",
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-faint", children: word })
		      ] }, k);
		    }) }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-faint", children: "Not known yet." }),
		    gaps ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-faint", style: { marginBottom: 6 }, "data-rth": "mind-gaps", children: [
		      gaps,
		      " gap",
		      gaps > 1 ? "s" : "",
		      " in this feed (sequence numbers not seen here: sent to another connection, or skipped while catching up; how many events is not known)."
		    ] }) : null,
		    usage ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-faint", style: { marginBottom: 6 }, "data-rth": "mind-usage", children: [
		      usage,
		      " provider usage report",
		      usage > 1 ? "s" : "",
		      " seen, not linked to any reply (they name a connection, not a reply), so not counted per reply."
		    ] }) : null,
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-mono rth-faint", style: { marginBottom: 8 }, children: [
		      fresh.stream.label,
		      reconnects ? ` \xB7 reloaded ${reconnects}\xD7` : "",
		      " \xB7 ",
		      fresh.agent.label
		    ] })
		  ] });
		}
		function EveryEvent({ rows, episodes, dropped, audit, onAudit }) {
		  const [query, setQuery] = useRemembered("mind:query", "");
		  const q = query.trim().toLowerCase();
		  const shown = rows.filter((r) => !q || `${r.sentence} ${r.kind} ${r.event_id ?? ""} ${r.episode ?? ""}`.toLowerCase().includes(q)).slice(-SHOWN).reverse();
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-rth": "mind-every", children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("input", { className: "rth-input", "data-rth": "mind-search", value: query, onChange: (e) => setQuery(e.target.value), placeholder: "Search sentences and ids", "aria-label": "Search every event" }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-faint", style: { margin: "6px 0 0", fontSize: 11.5 }, "data-rth": "mind-count", children: [
		      shown.length,
		      " of ",
		      rows.length,
		      " rows",
		      dropped ? `, ${dropped} evicted` : ""
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "rth-all", children: shown.map((r) => {
		      const pt = producerTime(r);
		      return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { type: "button", className: "rth-step", "data-rth": "mind-row", "data-id": r.id, "data-kind": r.kind, "data-category": r.category, "data-status": r.status, "data-episode": r.episode ?? "", "data-lane": r.lane, "aria-expanded": audit === r.id, onClick: () => !r.local && onAudit(audit === r.id ? null : r.id), children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-step-t", children: hms(r.observed_at).slice(0, 8) }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-step-s", children: [
		            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { "data-rth": "mind-sentence", children: r.sentence }),
		            /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-faint", style: { display: "block", fontSize: 10.5 }, children: [
		              CATS[r.category]?.label ?? r.category,
		              " \xB7 ",
		              r.source,
		              "\xB7",
		              r.lane,
		              pt ? ` \xB7 ${pt.text}` : "",
		              r.duration_ms != null ? ` \xB7 ${dur(r.duration_ms)}` : "",
		              r.count > 1 ? ` \xB7 \xD7${r.count}` : "",
		              r.episode && episodes[r.episode] ? ` \xB7 ${r.episode}` : ""
		            ] })
		          ] }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Chip, { status: r.status })
		        ] }),
		        audit === r.id ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Audit, { id: r.id, onClose: () => onAudit(null), onSelect: onAudit }) : null
		      ] }, r.id);
		    }) })
		  ] });
		}
		function newKey() {
		  try {
		    return `mind-${crypto.randomUUID()}`;
		  } catch {
		    return `mind-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
		  }
		}
		function Suggest({ ep }) {
		  const [key, setKey] = (0, import_react2.useState)(newKey);
		  const [actor, setActor] = (0, import_react2.useState)(() => {
		    try {
		      return localStorage.getItem("rth-mind-actor") ?? "";
		    } catch {
		      return "";
		    }
		  });
		  const [text, setText] = (0, import_react2.useState)("");
		  const [result, setResult] = (0, import_react2.useState)(null);
		  const [history, setHistory] = (0, import_react2.useState)(null);
		  (0, import_react2.useEffect)(() => {
		    fetch(new URL(`${BASE}/suggestions?episode=${encodeURIComponent(ep.id)}`, document.baseURI)).then((r) => r.json()).then((b) => setHistory(b.suggestions)).catch(() => setHistory(null));
		  }, [ep.id, result]);
		  const send = async () => {
		    try {
		      localStorage.setItem("rth-mind-actor", actor);
		    } catch {
		    }
		    setResult({ state: "sending" });
		    try {
		      const r = await fetch(new URL(`${BASE}/suggestions`, document.baseURI), { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ key, actor, scope: { episode: ep.id, title: ep.title }, text }) });
		      const b = await r.json();
		      if (!r.ok) return setResult({ state: "failed", detail: b.error ?? `status ${r.status}` });
		      setResult({ state: b.duplicate ? "duplicate" : "saved", record: b.record });
		      setText("");
		      setKey(newKey());
		    } catch (e) {
		      setResult({ state: "failed", detail: `${e.message} (retrying sends the same key)` });
		    }
		  };
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-rth": "mind-suggest-form", style: { marginTop: 6, display: "flex", flexDirection: "column", gap: 4 }, children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: { fontSize: 12, ...muted }, children: "How would a mind handle this differently? A proposal for people to read: it is appended to this plugin's local log, sent nowhere, and changes nothing in Unfold." }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("input", { "data-rth": "mind-suggest-actor", value: actor, onChange: (e) => setActor(e.target.value), placeholder: "your name (not verified)", maxLength: 80, style: { ...button, cursor: "text" } }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("textarea", { "data-rth": "mind-suggest-text", value: text, onChange: (e) => setText(e.target.value), rows: 3, maxLength: 4e3, placeholder: "e.g. a person would wait for the end of my sentence before answering", style: { ...button, cursor: "text", resize: "vertical" } }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: { display: "flex", gap: 6, alignItems: "center" }, children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", "data-rth": "mind-suggest-send", style: button, disabled: !text.trim() || result?.state === "sending", onClick: send, children: "Append suggestion" }),
		      result ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { "data-rth": "mind-suggest-result", "data-state": result.state, style: { fontSize: 12 }, children: result.state === "saved" ? `saved (${result.record.key})` : result.state === "duplicate" ? "already saved under this key; nothing added" : result.state === "failed" ? `not saved: ${result.detail}` : "sending\u2026" }) : null
		    ] }),
		    history?.length ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { "data-rth": "mind-suggest-history", style: { margin: 0, paddingLeft: 16, fontSize: 12, ...muted }, children: history.map((h) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { children: [
		      h.at.slice(0, 19).replace("T", " "),
		      " ",
		      h.actor,
		      ": ",
		      h.text
		    ] }, h.key)) }) : null
		  ] });
		}
		function Audit({ id, onClose, onSelect }) {
		  const [d, setD] = (0, import_react2.useState)(null);
		  const [error, setError] = (0, import_react2.useState)(null);
		  const [copied, setCopied] = (0, import_react2.useState)(null);
		  const loader = (0, import_react2.useRef)(null);
		  if (!loader.current) {
		    loader.current = auditLoader(async (rowId, signal) => {
		      const r = await fetch(new URL(`${BASE}/event?id=${encodeURIComponent(rowId)}`, document.baseURI), { signal });
		      const body = await r.json().catch(() => ({}));
		      return r.ok ? { ok: true, body } : { ok: false, error: body.error ?? `status ${r.status}` };
		    });
		  }
		  (0, import_react2.useEffect)(() => {
		    setD(null);
		    setError(null);
		    setCopied(null);
		    loader.current.select(id, (s) => {
		      if (s.state === "ready") setD(s.data);
		      else if (s.state === "error") setError(s.error);
		    });
		  }, [id]);
		  (0, import_react2.useEffect)(() => () => loader.current?.cancel(), []);
		  const copy = async () => {
		    if (d?.row?.id !== id) return;
		    const text = JSON.stringify({ evidence: d.evidence, evidence_sha256: d.evidence_sha256, evidence_sha256_basis: d.evidence_sha256_basis }, null, 2);
		    try {
		      await navigator.clipboard.writeText(text);
		      setCopied({ state: "copied" });
		    } catch {
		      setCopied({ state: "manual", text });
		    }
		  };
		  const kv = (o) => Object.entries(o ?? {}).filter(([, v]) => v !== null && v !== void 0 && !(typeof v === "object" && !Object.values(v).some((x) => x != null)));
		  const section = { margin: "6px 0 2px", fontWeight: 600, fontSize: 12 };
		  const pre = { margin: 0, padding: 6, fontSize: 11, maxHeight: 180, overflow: "auto", background: "rgba(127,127,127,0.1)", borderRadius: 4, whiteSpace: "pre-wrap", overflowWrap: "anywhere" };
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-rth": "mind-audit", "data-id": id, style: { ...box, flex: "0 1 52%", overflow: "auto", borderColor: "rgba(59,130,246,0.7)" }, children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: { display: "flex", gap: 6, alignItems: "center" }, children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("b", { style: { flex: 1 }, children: "Audit record" }),
		      d ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", "data-rth": "mind-copy", "data-state": copied?.state ?? "idle", style: button, onClick: copy, children: copied?.state === "copied" ? "Copied \u2713" : "Copy as evidence" }) : null,
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", style: button, onClick: onClose, "aria-label": "close audit record", children: "\u2715" })
		    ] }),
		    error ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: muted, children: [
		      "Not available: ",
		      error
		    ] }) : null,
		    !d && !error ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: muted, children: "loading\u2026" }) : null,
		    copied?.state === "manual" ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("textarea", { readOnly: true, value: copied.text, rows: 6, style: { width: "100%", fontSize: 11 }, onFocus: (e) => e.target.select() }) : null,
		    d ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: { fontSize: 12 }, children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-rth": "mind-audit-sentence", style: { marginTop: 4 }, children: [
		        CATS[d.row.category]?.icon,
		        " ",
		        d.row.sentence,
		        " ",
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Chip, { status: d.row.status })
		      ] }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: { ...muted, fontSize: 11 }, children: d.evidence.row.sentence_basis }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: section, children: "Ids" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("table", { "data-rth": "mind-audit-ids", style: { borderCollapse: "collapse", fontSize: 11 }, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tbody", { children: [
		        [...kv({ event: d.ids.event, cause: d.ids.cause, producer: d.ids.producer, seq: d.ids.seq, session: d.ids.session_id, boot: d.ids.boot_id, epoch: d.ids.epoch, generation: d.ids.generation, attachment: d.ids.attachment_id }), ...kv(d.ids.correlation)].map(([k, v]) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { style: { paddingRight: 8, ...muted }, children: k }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { style: { overflowWrap: "anywhere" }, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("code", { children: String(v) }) })
		        ] }, k)),
		        !d.ids.event ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { style: { paddingRight: 8, ...muted }, children: "event" }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { children: "unknown (this source carries no event id)" })
		        ] }) : null
		      ] }) }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: section, children: "Cause chain (explicit cause ids only)" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-rth": "mind-audit-chain", children: [
		        d.chain.parents.length ? d.chain.parents.map((p, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
		          "\u2191".repeat(i + 1),
		          " ",
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("a", { href: "#", "data-rth": "mind-audit-parent", onClick: (e) => (e.preventDefault(), onSelect(p.id)), children: p.sentence })
		        ] }, p.id)) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: muted, children: d.ids.cause ? "" : "no cause id on this event" }),
		        d.chain.unresolved ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: muted, children: [
		          "\u2191 ",
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("code", { children: d.chain.unresolved.event_id }),
		          ": ",
		          d.chain.unresolved.reason
		        ] }) : null,
		        d.chain.children.map((c) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
		          "\u2193 ",
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("a", { href: "#", "data-rth": "mind-audit-child", onClick: (e) => (e.preventDefault(), onSelect(c.id)), children: c.sentence })
		        ] }, c.id))
		      ] }),
		      d.episode ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: section, children: [
		          "Episode ",
		          d.episode.id,
		          ": ",
		          d.episode.title,
		          " ",
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Chip, { status: d.episode.status })
		        ] }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: { ...muted, fontSize: 11 }, children: [
		          "grouped through shared ids (response, command, decision, utterance) and explicit causes only \xB7 ",
		          d.episode.because
		        ] })
		      ] }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: { ...section, fontWeight: 400, ...muted }, children: "Not part of an episode (stream, connection or system row, or no shared id)." }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: section, children: "Source" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("table", { style: { borderCollapse: "collapse", fontSize: 11 }, children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tbody", { children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { style: { paddingRight: 8, ...muted }, children: "source" }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("td", { children: [
		            d.row.source,
		            " \xB7 lane ",
		            d.row.lane,
		            " \xB7 training eligible: ",
		            d.row.training_eligible === null ? "unknown" : String(d.row.training_eligible)
		          ] })
		        ] }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { style: { paddingRight: 8, ...muted }, children: "observed" }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("td", { children: [
		            d.evidence.observed_at,
		            " (DSH host clock)"
		          ] })
		        ] }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("td", { style: { paddingRight: 8, ...muted }, children: "clock" }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("td", { children: [
		            d.clock.basis,
		            d.clock.t_ns != null ? `: t_ns ${d.clock.t_ns}` : "",
		            d.clock.t_ms != null ? `: t_ms ${d.clock.t_ms}` : "",
		            ". Wall time: ",
		            d.clock.wall_ms_estimate != null ? `${new Date(d.clock.wall_ms_estimate).toISOString()} (${d.clock.wall_basis})` : d.clock.wall_basis
		          ] })
		        ] }),
		        d.origins.map((o, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("tr", { children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("td", { style: { paddingRight: 8, ...muted }, children: [
		            "origin ",
		            i + 1
		          ] }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("td", { style: { overflowWrap: "anywhere" }, children: [
		            o.via,
		            "; process ",
		            o.process ?? "unknown",
		            "; file ",
		            o.file ?? "none",
		            o.offset != null ? ` @ byte ${o.offset} (${o.length} bytes)` : "; offset unknown",
		            o.agent_journal ? `; agent journal ${o.agent_journal}` : "",
		            o.note ? `; ${o.note}` : ""
		          ] })
		        ] }, i))
		      ] }) }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: section, children: "Integrity" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-rth": "mind-audit-integrity", style: { fontSize: 11 }, children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { children: d.integrity.journal_record_hash }),
		        d.integrity.observer_feed ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { children: d.integrity.observer_feed }) : null,
		        d.integrity.file_lines.map((l, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
		          "line sha256 ",
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("code", { children: l.line_sha256 }),
		          " at byte ",
		          l.offset,
		          " of ",
		          l.file,
		          " (",
		          l.basis,
		          ")"
		        ] }, i)),
		        d.integrity.payload_hashes.map((h) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
		          h.field,
		          " ",
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("code", { children: h.value }),
		          ": ",
		          h.means
		        ] }, h.field)),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: muted, children: [
		          "evidence sha256 ",
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("code", { children: d.evidence_sha256 }),
		          " (",
		          d.evidence_sha256_basis,
		          ")"
		        ] })
		      ] }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: section, children: "What this kind means" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Definition, { def: d.definition }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: section, children: "Raw envelope (redacted)" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { style: { ...muted, fontSize: 11 }, children: d.envelope_basis }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("pre", { "data-rth": "mind-audit-envelope", style: pre, children: d.envelope ? JSON.stringify(d.envelope, null, 2) : "none" })
		    ] }) : null
		  ] });
		}
		function Definition({ def }) {
		  if (!def) return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { children: "unknown" });
		  if (def.glossary) return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { "data-rth": "mind-audit-definition", style: { fontSize: 11 }, children: def.glossary });
		  const c = def.catalog;
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-rth": "mind-audit-definition", style: { fontSize: 11 }, children: [
		    c?.name ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("b", { children: c.name }),
		      " (wire ",
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("code", { children: def.kind }),
		      ", phase ",
		      c.phase,
		      ": ",
		      c.phase_means,
		      "; ",
		      c.file,
		      " v",
		      c.version,
		      ").",
		      c.note ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
		        " ",
		        c.note,
		        "."
		      ] }) : null,
		      c.field_notes?.map((n, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: { marginTop: 2 }, children: [
		        "Fields ",
		        n.added.join(", ") || "(none added)",
		        " (",
		        n.source,
		        "): ",
		        n.note
		      ] }, i))
		    ] }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
		      "catalog: ",
		      c?.missing ?? "unknown"
		    ] }),
		    def.protocol?.variant ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { style: { marginTop: 2 }, children: [
		      "rth-protocol v2 ",
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("code", { children: [
		        "Event::",
		        def.protocol.variant
		      ] }),
		      " (",
		      def.protocol.direction,
		      "): ",
		      def.protocol.comment
		    ] }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
		      "protocol: ",
		      def.protocol?.missing ?? "unknown"
		    ] })
		  ] });
		}
		return module.exports;
	}
});
