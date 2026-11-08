// Generated from src/client by `npm run build` (scripts/build-client.mjs). Do not edit by hand.
window.__ModuleLoader__.load({
  id: '@automagik/unfold-session-dsh',
  factory: (require) => {
const module = { exports: {} }
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

// src/client/index.jsx
var index_exports = {};
__export(index_exports, {
  BOT_OVERRIDES_CSS: () => BOT_OVERRIDES_CSS,
  LIVE_CSS: () => LIVE_CSS,
  ListeningDock: () => ListeningDock,
  ROOT_SESSION: () => ROOT_SESSION,
  STATUS_ID: () => STATUS_ID,
  STATUS_KIND: () => STATUS_KIND,
  UNFOLD_TABS: () => UNFOLD_TABS,
  apply: () => apply,
  barsOf: () => barsOf,
  inject: () => inject,
  makeTabButtons: () => makeTabButtons
});
module.exports = __toCommonJS(index_exports);
var import_react = require("react");

// src/client/strings.js
var ANSWER_WITH_ZH = {
  "Unfold is not reachable from here right now": "现在无法从这里连接 Unfold",
  "Unfold's model list could not be read right now": "现在无法读取 Unfold 的模型列表",
  "Unfold does not offer that model or level now": "Unfold 现在不提供这个模型或级别",
  "Unfold needs your confirmation before it sends your messages there": "需要你确认后，Unfold 才会把你的消息发送到那里",
  "The pick may not have reached Unfold; open the menu again to see": "这次选择可能没有送达 Unfold，请重新打开菜单查看"
};

// src/client/index.jsx
var import_jsx_runtime = require("react/jsx-runtime");
var STATUS_ID = "@automagik/unfold-session-dsh";
var STATUS_KIND = "unfold.status";
var ROOT_SESSION = "unfold-root";
var STATUS = "api/unfold-session/status";
var LIVE = "api/unfold-session/live";
var inject = ["slots", "sidebarRightTabs", "sidebarRight"];
var UNFOLD_TABS = [
  { kind: "unfold.presence", label: "Presence", title: "Unfold presence", icon: "M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16M9 11h.01M15 11h.01" },
  { kind: "unfold.mind", label: "Mind", title: "Mind: what Unfold did and why", icon: "M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01" },
  { kind: "unfold.events", label: "Events", title: "Unfold events", icon: "M3 12h4l3-7 4 14 3-7h4" },
  { kind: "unfold.settings", label: "Setup", title: "Unfold setup (settings)", icon: "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M5 19l2-2M17 7l2-2" },
  { kind: "unfold.status", label: "Status", title: "Unfold status", icon: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 11v5M12 8h.01" }
];
var TabIcon = ({ d }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", { width: "14", height: "14", viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: "1.8", strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d }) });
var BOT_OVERRIDES_CSS = [
  ".bt-mic,.bt-voice,.bt-mic-note{display:none!important}",
  ".bt-model-dock{display:none!important}"
].join("\n");
var LIVE_CSS = [
  ".unfold-live{padding:2px 12px 6px;transition:opacity .2s ease}",
  ".unfold-live[data-leaving]{opacity:0}",
  ".unfold-live .unfold-live-body{display:inline-flex;align-items:center;gap:8px}",
  ".unfold-live:not(.bt-umsg){display:flex;justify-content:flex-end}",
  ".unfold-live:not(.bt-umsg) .unfold-live-body{padding:6px 12px;border-radius:14px;background:var(--fill-secondary,#8882)}",
  ".unfold-live-word{opacity:.75;font-style:italic}",
  ".unfold-live-dots i{display:inline-block;width:4px;height:4px;margin:0 1px;border-radius:2px;background:currentColor;opacity:.4;animation:unfold-live-dot 1.2s infinite ease-in-out}",
  ".unfold-live-dots i:nth-child(2){animation-delay:.2s}.unfold-live-dots i:nth-child(3){animation-delay:.4s}",
  "@keyframes unfold-live-dot{0%,80%,100%{opacity:.25}40%{opacity:.9}}",
  ".unfold-live-meter{display:inline-flex;align-items:flex-end;gap:2px;height:12px}",
  ".unfold-live-meter b{display:block;width:3px;border-radius:1px;background:currentColor;opacity:.2}",
  ".unfold-live-meter b[data-on]{opacity:.75}",
  "@media (prefers-reduced-motion:reduce){.unfold-live-dots i{animation:none;opacity:.6}.unfold-live{transition:none}}",
  // DSH's session header is pointer-events:none (the transcript scrolls under it) and the Bot turns
  // them back on only for its own pill, so a header button added through omni.headerButtons was
  // drawn but unclickable: the click fell through to the chat column below (found by
  // hit-testing the header). This includes the Trajectory button, matched by its data-unfold.
  ".bt-head .unfold-tabs,.bt-head .unfold-tab-btn,.bt-head [data-unfold=trajectory-toggle]{pointer-events:auto}",
  ".unfold-tabs{display:inline-flex;align-items:center;gap:2px}",
  ".unfold-tabs .unfold-tab-btn{display:inline-flex;align-items:center;gap:4px;width:auto;padding:0 7px;font-size:12px}",
  // Compact (the head is too narrow for words, e.g. with the right sidebar open): icons only, and
  // the Trajectory button's word hidden too, so nothing runs under the header's side button.
  ".unfold-tabs[data-compact] .unfold-tab-word{display:none}",
  ".unfold-tabs[data-compact] .unfold-tab-btn{padding:0 5px}",
  ".bt-head.unfold-head-compact [data-unfold=trajectory-toggle] span{display:none}"
].join("\n");
function barsOf(dbfs) {
  if (!Number.isFinite(dbfs)) return 0;
  return Math.max(0, Math.min(5, Math.round((dbfs + 50) / 8)));
}
function makeTabButtons(open) {
  return function UnfoldHeaderTabs({ bot }) {
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
        head.classList.toggle("unfold-head-compact", small);
      };
      const ro = new ResizeObserver(fit);
      ro.observe(head);
      fit();
      return () => {
        ro.disconnect();
        head.classList.remove("unfold-head-compact");
      };
    }, [session]);
    if (session !== ROOT_SESSION) return null;
    return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { ref, className: "unfold-tabs", "data-unfold": "header-tabs", "data-compact": compact || void 0, role: "group", "aria-label": "Unfold tabs", children: UNFOLD_TABS.map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", { type: "button", className: "bt-icon-btn unfold-tab-btn", "data-unfold": `tab-${t.kind}`, title: t.title, "aria-label": `Open ${t.title} in the right sidebar`, onClick: () => open(t.kind), children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TabIcon, { d: t.icon }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "unfold-tab-word", children: t.label })
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
  return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "bt-umsg unfold-live", "data-unfold": "listening", "data-state": shown.state, "data-leaving": leaving || void 0, "aria-live": "polite", children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "bt-ustack", children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { className: "bt-ubody unfold-live-body", children: [
    shown.text ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { "data-unfold": "listening-text", children: shown.text }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "unfold-live-word", "data-unfold": "listening-word", children: "listening" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { className: "unfold-live-dots", "aria-hidden": "true", children: [
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("i", {}),
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("i", {}),
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("i", {})
      ] })
    ] }),
    shown.level && !shown.text ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "unfold-live-meter", "data-unfold": "listening-meter", "data-bars": bars, title: meterTitle, "aria-label": meterTitle, children: [1, 2, 3, 4, 5].map((n) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("b", { "data-on": n <= bars || void 0, style: { height: 3 + n * 2 } }, n)) }) : null
  ] }) }) });
}
async function apply(ctx) {
  const response = await fetch("api/unfold-session/config", { cache: "no-store" });
  if (!response.ok) throw new Error(`Unfold configuration ${response.status}`);
  const config = await response.json();
  if (config.enabled === false) return;
  ROOT_SESSION = config.sessionId;
  const route = config.route.replace(/^\//, "").replace(/\/$/, "");
  STATUS = `${route}/status`;
  LIVE = `${route}/live`;
  ctx.effect(() => {
    const el = document.createElement("style");
    el.id = "unfold-bot-overrides";
    el.dataset.unfold = "bot-overrides";
    el.textContent = `${BOT_OVERRIDES_CSS}
${LIVE_CSS}`;
    document.head.append(el);
    return () => el.remove();
  }, "unfold-session: Bot overrides (the app owns speech, no model onboarding)");
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
    }, "unfold-session: open the root Session");
  });
  ctx.effect(() => ctx.sidebarRightTabs.register({
    id: STATUS_ID,
    kind: STATUS_KIND,
    priority: "extension",
    title: () => "Unfold status",
    guide: [{ id: "unfold-status", order: 91, title: () => "Unfold status", description: () => "The link to the agent and the latest measured candidate" }]
  }), "unfold-session: tab type");
  ctx.effect(() => () => {
    for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
      if (tab.kind !== STATUS_KIND) continue;
      try {
        ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId);
      } catch {
      }
    }
  }, "unfold-session: close open tabs on unload");
  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab", () => ctx.slots.register({ name: "sidebar.right.pane.tab", key: STATUS_ID }, StatusBody)), "unfold-session: tab body");
  ctx.effect(() => ctx.slots.inject("conversation.input.dock", () => ctx.slots.register({ name: "conversation.input.dock", id: "unfold-listening", order: -200 }, ListeningDock)), "unfold-session: listening bubble");
  let omni = null;
  try {
    const id = ["@automagik", "bots-dsh", "client"].join("/");
    omni = (typeof require === "function" ? require(id) : null)?.omni ?? null;
  } catch {
  }
  if (typeof omni?.addStrings === "function") ctx.effect(() => omni.addStrings(ANSWER_WITH_ZH), "unfold-session: model menu copy");
  if (omni?.headerButtons !== void 0) ctx.effect(() => omni.headerButtons.register({
    id: "unfold-tabs",
    order: 40,
    Component: makeTabButtons((kind) => {
      ctx.sidebarRight.openTabIn(ROOT_SESSION, kind);
      if (ctx.sidebarRight.isExpanded?.() === false) ctx.sidebarRight.toggleExpanded?.();
    })
  }), "unfold-session: Bot header tab buttons");
  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab.title", () => ctx.slots.register({ name: "sidebar.right.pane.tab.title", key: STATUS_ID }, StatusTitle)), "unfold-session: tab title");
}
function StatusTitle({ useTabInfo }) {
  const { tab } = useTabInfo();
  return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { "data-unfold": "status-title", children: tab.title });
}
var short = (s, n = 12) => typeof s === "string" && s.length > n ? `${s.slice(0, n)}…` : s ?? "—";
var card = { border: "1px solid var(--border, #8884)", borderRadius: 8, padding: "8px 10px", margin: "8px 0" };
var label = { fontSize: 11, opacity: 0.7, textTransform: "uppercase", letterSpacing: 0.4 };
function Row({ k, v, id }) {
  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: { display: "flex", gap: 8, fontSize: 12 }, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { style: { opacity: 0.7, minWidth: 96 }, children: k }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { "data-unfold": id, style: { fontFamily: "var(--font-mono, monospace)", wordBreak: "break-all" }, children: v === null || v === void 0 ? "—" : String(v) })
  ] });
}
function StatusBody() {
  const [s, setS] = (0, import_react.useState)(null);
  const [err, setErr] = (0, import_react.useState)(null);
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
  const link = s?.session?.adapter_link;
  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { "data-unfold": "status", style: { padding: 10, fontSize: 13 }, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: { display: "flex", gap: 8 }, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", { "data-unfold": "status-refresh", onClick: () => void load(), children: "Refresh" }) }),
    err && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { "data-unfold": "status-error", style: { color: "var(--danger, #c33)" }, children: [
      "status unavailable: ",
      err
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: card, "data-unfold": "card-link", children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: label, children: "Agent link (read only)" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "link", v: link?.state, id: "link-state" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "boot", v: link?.boot_id, id: "boot-id" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "session", v: link?.session_id, id: "session-id" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "generation", v: link?.command_generation, id: "generation" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "DSH model calls", v: s ? `${s.dsh.model_calls.llm_stream} streams, ${s.dsh.model_calls.agent_request} requests` : null, id: "model-calls" })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: card, "data-unfold": "card-candidate", children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: label, children: "Latest measured candidate (capability map, not an activation receipt)" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "run", v: s?.candidate?.run ?? s?.candidate?.unavailable, id: "candidate-run" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "backend", v: s?.candidate?.backend, id: "candidate-backend" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Row, { k: "agentd", v: short(s?.candidate?.agentd_sha256, 16), id: "candidate-agentd" })
    ] })
  ] });
}
return Object.defineProperty({ ...module.exports }, Symbol.toStringTag, { value: 'Module' })
  },
})
