window.__ModuleLoader__.load({
	id: "@rth/dsh-session",
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
		
		// dsh-session/client/index.jsx
		var index_exports = {};
		__export(index_exports, {
		  BOT_OVERRIDES_CSS: () => BOT_OVERRIDES_CSS,
		  LIVE_CSS: () => LIVE_CSS,
		  ListeningDock: () => ListeningDock,
		  ROOT_SESSION: () => ROOT_SESSION,
		  RTH_STATUS_ID: () => RTH_STATUS_ID,
		  RTH_STATUS_KIND: () => RTH_STATUS_KIND,
		  RTH_TABS: () => RTH_TABS,
		  apply: () => apply,
		  barsOf: () => barsOf,
		  inject: () => inject,
		  makeTabButtons: () => makeTabButtons
		});
		module.exports = __toCommonJS(index_exports);
		var import_react = require("react");
		var import_jsx_runtime = require("react/jsx-runtime");
		var RTH_STATUS_ID = "@rth/dsh-session";
		var RTH_STATUS_KIND = "rth.status";
		var ROOT_SESSION = "rth-root";
		var STATUS = "api/rth-session/status";
		var STOP = "api/rth-session/stop";
		var LIVE = "api/rth-session/live";
		var inject = ["slots", "sidebarRightTabs", "sidebarRight"];
		var RTH_TABS = [
		  { kind: "rth.presence", label: "Presence", title: "Unfold presence", icon: "M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16M9 11h.01M15 11h.01" },
		  { kind: "rth.mind", label: "Mind", title: "Mind: what Unfold did and why", icon: "M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01" },
		  { kind: "rth.events", label: "Events", title: "Unfold events", icon: "M3 12h4l3-7 4 14 3-7h4" },
		  { kind: "rth.settings", label: "Setup", title: "Unfold setup (settings)", icon: "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M5 19l2-2M17 7l2-2" },
		  { kind: "rth.status", label: "Status", title: "Unfold status", icon: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 11v5M12 8h.01" }
		];
		var TabIcon = ({ d }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", { width: "14", height: "14", viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: "1.8", strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d }) });
		var BOT_OVERRIDES_CSS = [
		  ".bt-mic,.bt-voice,.bt-mic-note{display:none!important}",
		  ".bt-model-dock{display:none!important}"
		].join("\n");
		var LIVE_CSS = [
		  ".rth-live{padding:2px 12px 6px;transition:opacity .2s ease}",
		  ".rth-live[data-leaving]{opacity:0}",
		  ".rth-live .rth-live-body{display:inline-flex;align-items:center;gap:8px}",
		  ".rth-live:not(.bt-umsg){display:flex;justify-content:flex-end}",
		  ".rth-live:not(.bt-umsg) .rth-live-body{padding:6px 12px;border-radius:14px;background:var(--fill-secondary,#8882)}",
		  ".rth-live-word{opacity:.75;font-style:italic}",
		  ".rth-live-dots i{display:inline-block;width:4px;height:4px;margin:0 1px;border-radius:2px;background:currentColor;opacity:.4;animation:rth-live-dot 1.2s infinite ease-in-out}",
		  ".rth-live-dots i:nth-child(2){animation-delay:.2s}.rth-live-dots i:nth-child(3){animation-delay:.4s}",
		  "@keyframes rth-live-dot{0%,80%,100%{opacity:.25}40%{opacity:.9}}",
		  ".rth-live-meter{display:inline-flex;align-items:flex-end;gap:2px;height:12px}",
		  ".rth-live-meter b{display:block;width:3px;border-radius:1px;background:currentColor;opacity:.2}",
		  ".rth-live-meter b[data-on]{opacity:.75}",
		  "@media (prefers-reduced-motion:reduce){.rth-live-dots i{animation:none;opacity:.6}.rth-live{transition:none}}",
		  // DSH's session header is pointer-events:none (the transcript scrolls under it) and the Bot turns
		  // them back on only for its own pill, so a header button added through omni.headerButtons would
		  // be drawn but unclickable: the click falls through to the chat column below. This includes the
		  // Trajectory button, matched by its data-rth.
		  ".bt-head .rth-tabs,.bt-head .rth-tab-btn,.bt-head [data-rth=trajectory-toggle]{pointer-events:auto}",
		  ".rth-tabs{display:inline-flex;align-items:center;gap:2px}",
		  ".rth-tabs .rth-tab-btn{display:inline-flex;align-items:center;gap:4px;width:auto;padding:0 7px;font-size:12px}",
		  // Compact (the head is too narrow for words, e.g. with the right sidebar open): icons only, and
		  // the Trajectory button's word hidden too, so nothing runs under the header's side button.
		  ".rth-tabs[data-compact] .rth-tab-word{display:none}",
		  ".rth-tabs[data-compact] .rth-tab-btn{padding:0 5px}",
		  ".bt-head.rth-head-compact [data-rth=trajectory-toggle] span{display:none}"
		].join("\n");
		function barsOf(dbfs) {
		  if (!Number.isFinite(dbfs)) return 0;
		  return Math.max(0, Math.min(5, Math.round((dbfs + 50) / 8)));
		}
		function makeTabButtons(open) {
		  return function RthHeaderTabs({ bot }) {
		    const session = bot?.sessionId ?? bot?.id;
		    const ref = (0, import_react.useRef)(null);
		    const full = (0, import_react.useRef)(0);
		    const [compact, setCompact] = (0, import_react.useState)(false);
		    (0, import_react.useLayoutEffect)(() => {
		      const el = ref.current;
		      const head = el?.closest(".bt-head");
		      if (!el || !head || typeof ResizeObserver === "undefined") return void 0;
		      const fit = () => {
		        if (!el.hasAttribute("data-compact")) full.current = el.getBoundingClientRect().width;
		        const others = [...head.children].filter((c) => c !== el && getComputedStyle(c).position !== "absolute").reduce((w, c) => w + c.getBoundingClientRect().width, 0);
		        const side = [...head.children].filter((c) => getComputedStyle(c).position === "absolute").reduce((w, c) => Math.max(w, c.getBoundingClientRect().width), 0);
		        const small = head.clientWidth < others + full.current + 2 * (side + 16);
		        setCompact(small);
		        head.classList.toggle("rth-head-compact", small);
		      };
		      const ro = new ResizeObserver(fit);
		      ro.observe(head);
		      fit();
		      return () => {
		        ro.disconnect();
		        head.classList.remove("rth-head-compact");
		      };
		    }, [session]);
		    if (session !== ROOT_SESSION) return null;
		    return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { ref, className: "rth-tabs", "data-rth": "header-tabs", "data-compact": compact || void 0, role: "group", "aria-label": "Unfold tabs", children: RTH_TABS.map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", { type: "button", className: "bt-icon-btn rth-tab-btn", "data-rth": `tab-${t.kind}`, title: t.title, "aria-label": `Open ${t.title} in the right sidebar`, onClick: () => open(t.kind), children: [
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TabIcon, { d: t.icon }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "rth-tab-word", children: t.label })
		    ] }, t.kind)) });
		  };
		}
		function useLive(on) {
		  const leaseMs = 12e3;
		  const capMs = 3e4;
		  const [v, setV] = (0, import_react.useState)(null);
		  (0, import_react.useEffect)(() => {
		    if (!on || typeof EventSource === "undefined") {
		      setV(null);
		      return void 0;
		    }
		    const later = typeof setTimeout === "function" ? setTimeout : null;
		    const cancel = typeof clearTimeout === "function" ? clearTimeout : () => {
		    };
		    let speech = null;
		    let lease = null;
		    let live = true;
		    const drop = () => {
		      if (live) setV(null);
		    };
		    const renew = () => {
		      cancel(lease);
		      lease = later ? later(drop, leaseMs) : null;
		    };
		    const es = new EventSource(LIVE);
		    const take = (e) => {
		      let x;
		      try {
		        x = JSON.parse(e.data);
		      } catch {
		        return;
		      }
		      renew();
		      cancel(speech);
		      speech = null;
		      if (x && x.state && x.state !== "idle") {
		        const ttl = Number.isFinite(x.ttl_ms) ? Math.max(0, Math.min(x.ttl_ms, capMs)) : 8e3;
		        if (ttl <= 0) return drop();
		        speech = later ? later(drop, ttl) : null;
		      }
		      setV(x);
		    };
		    es.addEventListener("snapshot", take);
		    es.addEventListener("live", take);
		    es.addEventListener("beat", renew);
		    es.addEventListener("error", () => {
		      cancel(speech);
		      speech = null;
		      drop();
		    });
		    renew();
		    return () => {
		      live = false;
		      cancel(speech);
		      cancel(lease);
		      es.close();
		      setV(null);
		    };
		  }, [on]);
		  return v;
		}
		function ListeningDock({ sessionId }) {
		  const on = sessionId === ROOT_SESSION;
		  const v = useLive(on);
		  const [shown, setShown] = (0, import_react.useState)(null);
		  const [leaving, setLeaving] = (0, import_react.useState)(false);
		  const timer = (0, import_react.useRef)(null);
		  (0, import_react.useEffect)(() => {
		    clearTimeout(timer.current);
		    if (v && v.state !== "idle") {
		      setShown(v);
		      setLeaving(false);
		    } else if (shown) {
		      setLeaving(true);
		      timer.current = setTimeout(() => {
		        setShown(null);
		        setLeaving(false);
		      }, 220);
		    }
		    return () => clearTimeout(timer.current);
		  }, [v]);
		  if (!on || !shown) return null;
		  const bars = shown.level ? barsOf(shown.level.dbfs) : 0;
		  const meterTitle = shown.level ? `${shown.level.basis}: ${shown.level.dbfs} dBFS (not a live level)` : null;
		  return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "bt-umsg rth-live", "data-rth": "listening", "data-state": shown.state, "data-leaving": leaving || void 0, "aria-live": "polite", children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "bt-ustack", children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "bt-ubody rth-live-body", children: [
		    shown.text ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { "data-rth": "listening-text", children: shown.text }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "rth-live-word", "data-rth": "listening-word", children: "listening" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { className: "rth-live-dots", "aria-hidden": "true", children: [
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("i", {}),
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("i", {}),
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("i", {})
		      ] })
		    ] }),
		    shown.level && !shown.text ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "rth-live-meter", "data-rth": "listening-meter", "data-bars": bars, title: meterTitle, "aria-label": meterTitle, children: [1, 2, 3, 4, 5].map((n) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("b", { "data-on": n <= bars || void 0, style: { height: 3 + n * 2 } }, n)) }) : null
		  ] }) }) });
		}
		function apply(ctx) {
		  ctx.effect(() => {
		    const el = document.createElement("style");
		    el.id = "rth-bot-overrides";
		    el.dataset.rth = "bot-overrides";
		    el.textContent = `${BOT_OVERRIDES_CSS}
		${LIVE_CSS}`;
		    document.head.append(el);
		    return () => el.remove();
		  }, "rth-session: Bot overrides (native speech owner, no model onboarding)");
		  ctx.inject(["uiWorkspace"], (sctx) => {
		    sctx.effect(() => {
		      let tries = 0;
		      let timer = null;
		      const attempt = () => {
		        try {
		          sctx.uiWorkspace.openSession(ROOT_SESSION);
		          return;
		        } catch {
		        }
		        if (++tries < 60) timer = setTimeout(attempt, 250);
		      };
		      timer = setTimeout(attempt, 0);
		      return () => clearTimeout(timer);
		    }, "rth-session: open the root Session");
		  });
		  ctx.effect(() => ctx.sidebarRightTabs.register({
		    id: RTH_STATUS_ID,
		    kind: RTH_STATUS_KIND,
		    priority: "extension",
		    title: () => "Unfold status",
		    guide: [{ id: "rth-status", order: 91, title: () => "Unfold status", description: () => "Active artifact, latest measured candidate, self-repo" }]
		  }), "rth-session: tab type");
		  ctx.effect(() => () => {
		    for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
		      if (tab.kind !== RTH_STATUS_KIND) continue;
		      try {
		        ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId);
		      } catch {
		      }
		    }
		  }, "rth-session: close open tabs on unload");
		  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab", () => ctx.slots.register({ name: "sidebar.right.pane.tab", key: RTH_STATUS_ID }, StatusBody)), "rth-session: tab body");
		  ctx.effect(() => ctx.slots.inject("conversation.input.dock", () => ctx.slots.register({ name: "conversation.input.dock", id: "rth-listening", order: -200 }, ListeningDock)), "rth-session: listening bubble");
		  let omni = null;
		  try {
		    const id = ["@automagik", "omni-dsh", "client"].join("/");
		    omni = (typeof require === "function" ? require(id) : null)?.omni ?? null;
		  } catch {
		  }
		  if (omni?.headerButtons !== void 0) ctx.effect(() => omni.headerButtons.register({
		    id: "rth-tabs",
		    order: 40,
		    Component: makeTabButtons((kind) => {
		      ctx.sidebarRight.openTabIn(ROOT_SESSION, kind);
		      if (ctx.sidebarRight.isExpanded?.() === false) ctx.sidebarRight.toggleExpanded?.();
		    })
		  }), "rth-session: Bot header tab buttons");
		  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab.title", () => ctx.slots.register({ name: "sidebar.right.pane.tab.title", key: RTH_STATUS_ID }, StatusTitle)), "rth-session: tab title");
		}
		function StatusTitle({ useTabInfo }) {
		  const { tab } = useTabInfo();
		  return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { "data-rth": "status-title", children: tab.title });
		}
		var short = (s, n = 12) => typeof s === "string" && s.length > n ? `${s.slice(0, n)}\u2026` : s ?? "\u2014";
		var card = { border: "1px solid var(--border, #8884)", borderRadius: 8, padding: "8px 10px", margin: "8px 0" };
		var label = { fontSize: 11, opacity: 0.7, textTransform: "uppercase", letterSpacing: 0.4 };
		function Row({ k, v, rth }) {
		  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: { display: "flex", gap: 8, fontSize: 12 }, children: [
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { style: { opacity: 0.7, minWidth: 96 }, children: k }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { "data-rth": rth, style: { fontFamily: "var(--font-mono, monospace)", wordBreak: "break-all" }, children: v === null || v === void 0 ? "\u2014" : String(v) })
		  ] });
		}
		function StatusBody() {
		  const [s, setS] = (0, import_react.useState)(null);
		  const [err, setErr] = (0, import_react.useState)(null);
		  const [stop, setStop] = (0, import_react.useState)(null);
		  const load = (0, import_react.useCallback)(async () => {
		    try {
		      const r = await fetch(STATUS, { cache: "no-store" });
		      if (!r.ok) throw new Error(`status ${r.status}`);
		      setS(await r.json());
		      setErr(null);
		    } catch (e) {
		      setErr(String(e.message ?? e));
		    }
		  }, []);
		  (0, import_react.useEffect)(() => {
		    void load();
		  }, [load]);
		  const onStop = async () => {
		    const r = await fetch(STOP, { method: "POST" });
		    setStop(r.ok ? "stop sent to the host owner" : `stop failed (${r.status})`);
		  };
		  const active = s?.active?.active;
		  const link = s?.owner?.link;
		  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { "data-rth": "status", style: { padding: 10, fontSize: 13 }, children: [
		    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: { display: "flex", gap: 8 }, children: [
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", { "data-rth": "status-refresh", onClick: () => void load(), children: "Refresh" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", { "data-rth": "status-stop", onClick: () => void onStop(), children: "Stop speaking" })
		    ] }),
		    stop && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { "data-rth": "stop-result", style: { fontSize: 12, marginTop: 4 }, children: stop }),
		    err && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { "data-rth": "status-error", style: { color: "var(--danger, #c33)" }, children: [
		      "status unavailable: ",
		      err
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: card, "data-rth": "card-link", children: [
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: label, children: "Host owner and agent link" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "link", v: link?.state, rth: "link-state" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "boot", v: link?.boot_id, rth: "boot-id" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "session", v: link?.session_id, rth: "session-id" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "generation", v: link?.command_generation, rth: "generation" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "backend", v: link?.backend, rth: "backend" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "voice", v: s?.owner?.host?.voice, rth: "voice" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "voice state", v: s?.owner?.host?.call ? `${s.owner.host.call.state} (desired ${s.owner.host.call.desired}): ${s.owner.host.call.reason}` : null, rth: "call-state" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "DSH model calls", v: s ? `${s.dsh.model_calls.llm_stream} streams, ${s.dsh.model_calls.agent_request} requests` : null, rth: "model-calls" })
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: card, "data-rth": "card-active", children: [
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: label, children: "Active artifact (controller record)" }),
		      active ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "deployment", v: active.deployment_id, rth: "deployment-id" }),
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "artifact", v: short(active.artifact_id, 16), rth: "artifact-id" }),
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "attestation", v: active.attestation, rth: "attestation" }),
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "receipt", v: short(active.receipt?.sha256, 16), rth: "receipt-sha" }),
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "receipt boot", v: active.receipt?.boot_id, rth: "receipt-boot" }),
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "backend", v: active.receipt?.backend, rth: "receipt-backend" })
		      ] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "active", v: s?.active?.note ?? s?.active?.error ?? (s ? "none" : null), rth: "deployment-id" })
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: card, "data-rth": "card-candidate", children: [
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: label, children: "Latest measured candidate (capability map, not an activation receipt)" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "run", v: s?.candidate?.run ?? s?.candidate?.unavailable, rth: "candidate-run" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "backend", v: s?.candidate?.backend, rth: "candidate-backend" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "agentd", v: short(s?.candidate?.agentd_sha256, 16), rth: "candidate-agentd" })
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: card, "data-rth": "card-source", children: [
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: label, children: "Self-repo (source)" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "HEAD", v: short(s?.source?.head, 12) ?? s?.source?.note, rth: "repo-head" }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "dirty", v: s?.source ? String(s.source.dirty ?? s.source.note ?? s.source.error) : null, rth: "repo-dirty" }),
		      s?.source?.dirty_paths?.length ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "paths", v: s.source.dirty_paths.join(", "), rth: "repo-paths" }) : null
		    ] })
		  ] });
		}
		return module.exports;
	}
});
