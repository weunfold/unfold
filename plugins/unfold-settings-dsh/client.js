// Generated from src/client by `npm run build` (scripts/build-client.mjs). Do not edit by hand.
window.__ModuleLoader__.load({
  id: '@automagik/unfold-settings-dsh',
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
  PANEL_ID: () => PANEL_ID,
  UNFOLD_SETTINGS_TAB_ID: () => UNFOLD_SETTINGS_TAB_ID,
  UNFOLD_SETTINGS_TAB_KIND: () => UNFOLD_SETTINGS_TAB_KIND,
  apply: () => apply,
  inject: () => inject
});
module.exports = __toCommonJS(index_exports);
var import_jsx_runtime2 = require("react/jsx-runtime");

// src/client/ui.jsx
var import_jsx_runtime = require("react/jsx-runtime");
var import_react = require("react");
var BASE_CSS = `
.unfold-ui{--unfold-fg:light-dark(#0c2630,#e4f4f2);--unfold-muted:light-dark(#4f636c,#9db1b8);--unfold-faint:light-dark(#7d8d94,#6c8088);
 --unfold-line:light-dark(rgba(12,38,48,.11),rgba(228,244,242,.09));--unfold-soft:light-dark(rgba(12,38,48,.035),rgba(228,244,242,.035));
 --unfold-hover:light-dark(rgba(12,38,48,.06),rgba(228,244,242,.06));--unfold-accent:light-dark(#08808f,#62e6f2);
 --unfold-ok:light-dark(#1b7a4a,#74e3a9);--unfold-warn:light-dark(#995600,#ffc56e);--unfold-bad:light-dark(#b0281f,#ff9584);
 --unfold-mono:"SF Mono",ui-monospace,Menlo,monospace;--unfold-ease:cubic-bezier(.2,.7,.2,1);
 color:var(--unfold-fg);font-size:13px;line-height:1.5;-webkit-font-smoothing:antialiased;box-sizing:border-box;height:100%;overflow:auto}
.unfold-ui *,.unfold-ui *::before,.unfold-ui *::after{box-sizing:border-box}
.unfold-ui :focus-visible{outline:2px solid var(--unfold-accent);outline-offset:2px;border-radius:6px}
.unfold-ui button{font:inherit;color:inherit}
.unfold-muted{color:var(--unfold-muted)}
.unfold-faint{color:var(--unfold-faint)}
.unfold-num{font-variant-numeric:tabular-nums}
.unfold-mono{font-family:var(--unfold-mono);font-size:11px;line-height:1.45;overflow-wrap:anywhere}
.unfold-dot{display:inline-block;width:7px;height:7px;border-radius:50%;background:var(--unfold-faint);flex:none;vertical-align:middle}
.unfold-dot[data-tone=ok]{background:var(--unfold-ok)}
.unfold-dot[data-tone=warn]{background:var(--unfold-warn)}
.unfold-dot[data-tone=bad]{background:var(--unfold-bad)}
.unfold-dot[data-tone=accent]{background:var(--unfold-accent)}
.unfold-dot[data-pulse=true]{animation:unfold-pulse 2.4s var(--unfold-ease) infinite}
@keyframes unfold-pulse{0%,100%{box-shadow:0 0 0 0 color-mix(in oklab,currentColor 0%,transparent)}50%{box-shadow:0 0 0 4px color-mix(in oklab,var(--unfold-ok) 18%,transparent)}}
.unfold-fold{border-top:1px solid var(--unfold-line)}
.unfold-fold-head{all:unset;box-sizing:border-box;display:flex;align-items:center;gap:8px;width:100%;padding:9px 2px;cursor:pointer;color:var(--unfold-muted);font-size:12.5px;border-radius:6px}
.unfold-fold-head:hover{color:var(--unfold-fg)}
.unfold-fold-head:focus-visible{outline:2px solid var(--unfold-accent);outline-offset:1px}
.unfold-chev{width:12px;height:12px;flex:none;transition:transform .2s var(--unfold-ease)}
.unfold-fold-head[aria-expanded=true] .unfold-chev{transform:rotate(90deg)}
.unfold-fold-body{display:grid;grid-template-rows:0fr;transition:grid-template-rows .22s var(--unfold-ease)}
.unfold-fold-body[data-open=true]{grid-template-rows:1fr}
.unfold-fold-body>div{overflow:hidden;min-height:0}
.unfold-fold-inner{padding:0 2px 12px}
.unfold-facts{display:grid;grid-template-columns:auto 1fr;gap:5px 14px;margin:0}
.unfold-facts dt{color:var(--unfold-muted)}
.unfold-facts dd{margin:0}
.unfold-crash{padding:16px;color:var(--unfold-muted)}
@media (prefers-reduced-motion: reduce){.unfold-ui *,.unfold-ui *::before,.unfold-ui *::after{transition:none!important;animation:none!important}}
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
      return globalThis.localStorage?.getItem(`unfold-ui:${key}`) ?? null;
    } catch {
      return null;
    }
  },
  set(key, value) {
    try {
      globalThis.localStorage?.setItem(`unfold-ui:${key}`, value);
    } catch {
    }
  }
};
function useRemembered(key, initial) {
  const [value, setValue] = (0, import_react.useState)(() => {
    const raw = store.get(key);
    if (raw === null) return initial;
    try {
      return JSON.parse(raw);
    } catch {
      return initial;
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
var Chevron = () => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", { className: "unfold-chev", viewBox: "0 0 12 12", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M4.5 2.5 8 6l-3.5 3.5", fill: "none", stroke: "currentColor", strokeWidth: "1.5", strokeLinecap: "round", strokeLinejoin: "round" }) });
function Fold({ id, summary, children, defaultOpen = false, className = "", ...rest }) {
  const [open, setOpen] = useRemembered(`fold:${id}`, defaultOpen);
  const bodyId = (0, import_react.useId)();
  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { className: `unfold-fold ${className}`, "data-open": open, ...rest, children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", { type: "button", className: "unfold-fold-head", "aria-expanded": open, "aria-controls": bodyId, onClick: () => setOpen((o) => !o), children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Chevron, {}),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: summary })
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "unfold-fold-body", id: bodyId, "data-open": open, children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "unfold-fold-inner", children: open ? children : null }) }) })
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
    console.error(`[${this.props.name ?? "unfold"}] the tab failed to draw`, error);
  }
  render() {
    if (!this.state.error) return this.props.children;
    return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "unfold-ui", children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "unfold-crash", role: "alert", children: "This tab hit a problem and stopped drawing. Reopen it to try again." }) });
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
var clockS = (at) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
var Dot = ({ tone = "neutral", pulse = false, label }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "unfold-dot", "data-tone": tone, "data-pulse": pulse, role: label ? "img" : void 0, "aria-label": label, "aria-hidden": label ? void 0 : true });

// src/display.js
function shown(leaf, state = leaf.state) {
  if (state === "stale") return "unknown (stale)";
  if (state === "unknown") return "unknown";
  const v = leaf.value;
  if (v === null || v === void 0) return leaf.reported_null ? "none reported" : "unknown";
  return typeof v === "string" ? v : JSON.stringify(v);
}
var PanelModel = class {
  constructor() {
    this.view = null;
    this.offset = null;
    this.needsSnapshot = false;
    this.confirmed = null;
    this.stream = "opening";
    this.lostAt = null;
    this.refreshError = null;
  }
  /** Is `view` older than the view held? (Older serving, lower revision, or the same revision earlier.) */
  superseded(view) {
    const cur = this.view;
    if (!cur) return false;
    if ((view.serving_started_at ?? 0) !== (cur.serving_started_at ?? 0)) return (view.serving_started_at ?? 0) < (cur.serving_started_at ?? 0);
    if (view.serving !== cur.serving) return false;
    if ((view.rev ?? 0) !== (cur.rev ?? 0)) return (view.rev ?? 0) < (cur.rev ?? 0);
    return view.at < cur.at;
  }
  observe(serverAt, clientNow) {
    const d = clientNow - serverAt;
    this.offset = this.offset === null ? d : Math.min(this.offset, d);
  }
  /** A full view, from the stream or from a successful Refresh. Returns false when refused as superseded. */
  setView(view, clientNow) {
    if (this.superseded(view)) return false;
    this.observe(view.at, clientNow);
    this.view = view;
    this.confirmed = view.at;
    if (this.lostAt !== null && view.at > this.lostAt) this.lostAt = null;
    if (this.needsSnapshot && (view.serving !== this.wanted?.serving || (view.rev ?? 0) >= this.wanted.rev)) {
      this.needsSnapshot = false;
      this.wanted = null;
    }
    this.refreshError = null;
    return true;
  }
  /** Confirms the held view only when the beat names its serving and revision. */
  heartbeat(beat, clientNow) {
    if (!this.view) return false;
    if (beat.serving !== this.view.serving || beat.serving_started_at !== this.view.serving_started_at) return false;
    this.observe(beat.at, clientNow);
    if (beat.rev !== this.view.rev) {
      if (beat.rev > this.view.rev) {
        this.needsSnapshot = true;
        this.wanted = { serving: beat.serving, rev: beat.rev };
      }
      return false;
    }
    if (this.lostAt !== null || beat.at < this.view.at) return false;
    this.confirmed = Math.max(this.confirmed ?? beat.at, beat.at);
    return true;
  }
  streamOpen() {
    this.stream = "open";
  }
  streamLost(clientNow) {
    this.stream = "reconnecting";
    if (this.lostAt === null) this.lostAt = clientNow - (this.offset ?? 0);
  }
  refreshFailed(error) {
    this.refreshError = String(error);
  }
  /** The state a leaf may claim now, and why it changed, if it did. */
  effective(leaf, clientNow) {
    const base = leaf.state;
    if (base !== "live") return { state: base, why: null };
    if (this.refreshError) return { state: "stale", why: `the last refresh failed (${this.refreshError}); nothing is confirmed until the next view` };
    const now = clientNow - (this.offset ?? 0);
    if (leaf.basis === "host") {
      if (leaf.expires_at == null || now > leaf.expires_at) return { state: "stale", why: "the host read is older than its lease (expired in this panel)" };
      return { state: "live", why: null };
    }
    if (leaf.basis === "events" || leaf.basis === "bot") {
      if (this.lostAt !== null) return { state: "stale", why: "the settings stream was lost: agent facts are not being confirmed" };
      if (this.needsSnapshot) return { state: "stale", why: "the server has newer settings than this view: waiting for the new view" };
      const lease = this.view?.stream_lease_ms ?? 0;
      if (this.confirmed === null || now > this.confirmed + lease) {
        return { state: "stale", why: `no word from the settings stream for over ${Math.round(lease / 1e3)} s: agent facts are not being confirmed` };
      }
    }
    return { state: "live", why: null };
  }
  /**
   * Client time of the next state change with no input, or null: when to draw again. Only
   * expiries still ahead count; one already past changes nothing more (no redraw loop).
   */
  nextChange(clientNow) {
    if (!this.view || this.refreshError) return null;
    let at = Infinity;
    const ahead = (t) => {
      if (t > clientNow) at = Math.min(at, t);
    };
    const consider = (leaf) => {
      if (leaf.state !== "live") return;
      if (leaf.basis === "host" && leaf.expires_at != null) ahead(leaf.expires_at + 1 + (this.offset ?? 0));
      if ((leaf.basis === "events" || leaf.basis === "bot") && this.lostAt === null && this.confirmed !== null) {
        ahead(this.confirmed + (this.view.stream_lease_ms ?? 0) + 1 + (this.offset ?? 0));
      }
    };
    for (const s of this.view.sections) s.leaves.forEach(consider);
    Object.values(this.view.revisions).forEach(consider);
    return at === Infinity ? null : at;
  }
  /** Everything the panel draws, with effective states applied. */
  render(clientNow) {
    if (!this.view) return null;
    const draw = (leaf) => {
      const e = this.effective(leaf, clientNow);
      return { leaf, state: e.state, text: shown(leaf, e.state), note: e.why ?? leaf.note ?? null };
    };
    return {
      stream: this.stream,
      refreshError: this.refreshError,
      revisions: Object.entries(this.view.revisions).map(([key, r]) => ({ key, ...draw(r) })),
      sections: this.view.sections.map((s) => ({ id: s.id, title: s.title, leaves: s.leaves.map(draw) })),
      sources: this.view.sources,
      edits: this.view.edits,
      gaps: this.view.gaps
    };
  }
};
function lines(drawn) {
  const out = [];
  for (const r of drawn.revisions) out.push(`revision ${r.key}: ${r.text} [${r.state}]`);
  for (const s of drawn.sections) {
    out.push(`# ${s.title}`);
    for (const l of s.leaves) out.push(`${l.leaf.label}: ${l.text} [${l.state}] (${l.leaf.source})${l.note ? ` ${l.note}` : ""}`);
  }
  for (const g of drawn.gaps) out.push(`gap ${g.what}: ${g.detail}`);
  return out;
}
function viewLines(view) {
  const m = new PanelModel();
  m.setView(view, view.at);
  m.streamOpen();
  return lines(m.render(view.at));
}

// src/client/index.jsx
var import_react2 = require("react");
var PANEL_ID = "unfold.settings";
var UNFOLD_SETTINGS_TAB_ID = "@automagik/unfold-settings-dsh";
var UNFOLD_SETTINGS_TAB_KIND = "unfold.settings";
var ROUTE = "api/unfold-settings";
var inject = ["slots"];
async function apply(ctx) {
  const response = await fetch("api/unfold-settings/config", { cache: "no-store" });
  if (!response.ok) throw new Error(`Unfold configuration ${response.status}`);
  const config = await response.json();
  ROUTE = config.route.replace(/^\//, "").replace(/\/$/, "");
  ctx.effect(() => ctx.slots.inject("main", () => ctx.slots.register(
    { name: "main", key: PANEL_ID },
    guarded("unfold-settings", SettingsPanel)
  )), "unfold-settings: main panel");
  ctx.effect(() => ctx.slots.inject("sidebar.panellist", () => ctx.slots.register(
    { name: "sidebar.panellist", id: PANEL_ID, order: 55, label: () => "Unfold settings" },
    PanelIcon
  )), "unfold-settings: sidebar entry");
  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab", () => ctx.slots.register(
    { name: "sidebar.right.pane.tab", key: UNFOLD_SETTINGS_TAB_ID },
    guarded("unfold-settings", SettingsTabBody)
  )), "unfold-settings: right tab body");
  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab.title", () => ctx.slots.register(
    { name: "sidebar.right.pane.tab.title", key: UNFOLD_SETTINGS_TAB_ID },
    SettingsTabTitle
  )), "unfold-settings: right tab title");
  ctx.inject(["sidebarRightTabs", "sidebarRight"], (scope) => {
    scope.effect(() => scope.sidebarRightTabs.register({
      id: UNFOLD_SETTINGS_TAB_ID,
      kind: UNFOLD_SETTINGS_TAB_KIND,
      priority: "extension",
      title: () => "Setup",
      guide: [{
        id: "unfold-settings",
        order: 92,
        title: () => "Unfold settings",
        description: () => "How Unfold is set up right now (read-only)"
      }]
    }), "unfold-settings: right tab type");
    scope.effect(() => () => {
      for (const tab of scope.sidebarRight.openTabs.getSnapshot()) {
        if (tab.kind !== UNFOLD_SETTINGS_TAB_KIND) continue;
        try {
          scope.sidebarRight.closeIn(tab.sessionId, tab.tabId);
        } catch {
        }
      }
    }, "unfold-settings: close open tabs on unload");
  });
}
function SettingsTabTitle({ useTabInfo }) {
  const { tab } = useTabInfo();
  return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { "data-unfold": "settings-title", children: tab.title });
}
function SettingsTabBody() {
  return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(SettingsPanel, { compact: true });
}
function PanelIcon({ size = 16 }) {
  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("svg", { width: size, height: size, viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: "1.4", "aria-hidden": "true", children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M2 4h7M12 4h2M2 8h2M7 8h7M2 12h9M14 12h0" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: "10.5", cy: "4", r: "1.5" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: "5.5", cy: "8", r: "1.5" }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("circle", { cx: "12.5", cy: "12", r: "1.5" })
  ] });
}
function useSettings() {
  const model = (0, import_react2.useRef)(null);
  model.current ??= new PanelModel();
  const [, redraw] = (0, import_react2.useState)(0);
  const bump = (0, import_react2.useCallback)(() => redraw((n) => n + 1), []);
  const fetching = (0, import_react2.useRef)(false);
  const refresh = (0, import_react2.useCallback)(async () => {
    const m = model.current;
    if (fetching.current) return;
    fetching.current = true;
    try {
      const res = await fetch(new URL(ROUTE, document.baseURI), { cache: "no-store" });
      if (!res.ok) throw new Error(`status ${res.status}`);
      m.setView(await res.json(), Date.now());
    } catch (e) {
      m.refreshFailed(e?.message ?? e);
    } finally {
      fetching.current = false;
    }
    bump();
  }, [bump]);
  (0, import_react2.useEffect)(() => {
    const m = model.current;
    const source = new EventSource(new URL(`${ROUTE}/stream`, document.baseURI));
    source.addEventListener("settings", (e) => {
      m.setView(JSON.parse(e.data), Date.now());
      bump();
    });
    source.addEventListener("heartbeat", (e) => {
      m.heartbeat(JSON.parse(e.data), Date.now());
      if (m.needsSnapshot) void refresh();
      bump();
    });
    source.addEventListener("resnapshot", () => {
      m.streamLost(Date.now());
      bump();
    });
    source.onopen = () => {
      m.streamOpen();
      bump();
    };
    source.onerror = () => {
      m.streamLost(Date.now());
      bump();
    };
    return () => source.close();
  }, [bump, refresh]);
  const now = Date.now();
  const drawn = model.current.render(now);
  const next = model.current.nextChange(now);
  (0, import_react2.useEffect)(() => {
    if (next === null) return;
    const t = setTimeout(bump, Math.max(0, next - Date.now()));
    return () => clearTimeout(t);
  });
  return { drawn, refresh };
}
var SETTINGS_CSS = `
.unfold-set{padding:16px 16px 24px;display:flex;flex-direction:column;gap:14px}
.unfold-set[data-placement=main]{padding:28px 32px;max-width:760px}
.unfold-set-head{display:flex;align-items:baseline;gap:10px}
.unfold-set-head h1{margin:0;font-size:15px;font-weight:620;letter-spacing:-.005em}
.unfold-set[data-placement=main] .unfold-set-head h1{font-size:20px}
.unfold-set-fresh{display:flex;align-items:center;gap:7px;font-size:12px;color:var(--unfold-muted)}
.unfold-set-fresh button{all:unset;cursor:pointer;color:var(--unfold-accent);font-size:12px;margin-left:4px}
.unfold-set-fresh button:focus-visible{outline:2px solid var(--unfold-accent)}
.unfold-sum{display:flex;flex-direction:column;border:1px solid var(--unfold-line);border-radius:12px;overflow:hidden}
.unfold-sum-row{display:grid;grid-template-columns:minmax(0,9fr) minmax(0,11fr);gap:12px;padding:9px 12px;align-items:baseline}
.unfold-sum-row+.unfold-sum-row{border-top:1px solid var(--unfold-line)}
.unfold-sum-k{color:var(--unfold-muted)}
.unfold-sum-v{overflow-wrap:anywhere}
.unfold-sum-v[data-state=stale],.unfold-sum-v[data-state=unknown]{color:var(--unfold-faint);font-style:italic}
.unfold-sec{margin:4px 0 10px}
.unfold-sec h3{margin:10px 0 4px;font-size:12px;font-weight:600;color:var(--unfold-muted);letter-spacing:.02em}
.unfold-leaf{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1.2fr);gap:10px;padding:4px 0;font-size:12px;border-top:1px solid var(--unfold-line)}
.unfold-leaf-v{overflow-wrap:anywhere;font-family:var(--unfold-mono);font-size:11px}
.unfold-leaf-v[data-state=stale],.unfold-leaf-v[data-state=unknown]{color:var(--unfold-faint)}
.unfold-leaf-m{display:block;font-family:inherit;font-size:10.5px;color:var(--unfold-faint)}
.unfold-leaf pre{white-space:pre-wrap;margin:0;font-size:11px}
`;
var STATE_TEXT = { live: "current", source: "from the source code", stale: "out of date", unknown: "not reported" };
var SUMMARY = [
  ["Voice model", "model", (v) => v],
  ["Instructions", "policy", (v) => v],
  ["Who speaks when", "turn_taking", (v) => /^provider VAD/.test(v) ? "The voice provider decides" : v],
  ["Echo control", "echo_cancellation", (v) => /^apple_voice_processing/.test(v) ? "Apple's voice processing" : v],
  ["Microphone mode", "mic_mode", (v) => ({ voice_isolation: "Voice isolation", standard: "Standard", wide_spectrum: "Wide spectrum" })[v?.active] ?? v?.active ?? null],
  ["Memory when reconnecting", "context_link", (v) => ({ restored: "Picks up where it left off", partial: "Remembers part of it", not_restored: "Starts fresh" })[v] ?? v],
  ["Running build", "deployment_agree", (v) => v === "yes" ? "The activated build" : v === "no" ? "Not the activated build" : v],
  ["Changes from here", null, () => "Read-only for now"]
];
function plain(d, map) {
  if (!d) return { text: "Not reported", state: "unknown" };
  if (d.state === "stale") return { text: "Not sure right now", state: "stale" };
  if (d.state === "unknown" || d.leaf.value == null) return { text: "Not reported", state: "unknown" };
  const v = map(d.leaf.value);
  return { text: v == null ? "Not reported" : String(v), state: d.state };
}
function SettingsPanel({ compact = false }) {
  useStyle("unfold-ui-base", BASE_CSS);
  useStyle("unfold-ui-settings", SETTINGS_CSS);
  const { drawn, refresh } = useSettings();
  const now = useNow(5e3);
  const leaves = new Map((drawn?.sections ?? []).flatMap((s) => s.leaves.map((d) => [d.leaf.id, d])));
  const host = drawn?.sources?.host;
  const live = drawn?.stream === "open" && host?.ok;
  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "unfold-ui unfold-set", "data-unfold": "settings", "data-placement": compact ? "right-tab" : "main", children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "unfold-set-head", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h1", { children: "How Unfold is set up" }) }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "unfold-set-fresh", "data-unfold": "settings-stream", "data-state": drawn?.stream ?? "opening", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Dot, { tone: live ? "ok" : "neutral" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { children: !drawn ? "Reading…" : live ? `Up to date · checked ${ago(host.read_at, now)}` : drawn.stream !== "open" ? "Out of touch, reconnecting" : `Not sure right now · last checked ${host?.read_at ? ago(host.read_at, now) : "never"}` }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("button", { type: "button", "data-unfold": "settings-refresh", onClick: refresh, children: "Check now" }),
      drawn?.refreshError ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { "data-unfold": "settings-error", className: "unfold-faint", children: [
        "(couldn't check: ",
        drawn.refreshError,
        ")"
      ] }) : null
    ] }),
    drawn ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_jsx_runtime2.Fragment, { children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "unfold-sum", "data-unfold": "settings-summary", children: SUMMARY.map(([k, id, map]) => {
        const p = id ? plain(leaves.get(id), map) : { text: map(), state: "live" };
        return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "unfold-sum-row", children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "unfold-sum-k", children: k }),
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "unfold-sum-v", "data-state": p.state, "data-leaf": id ?? "", children: p.text })
        ] }, k);
      }) }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Fold, { id: "settings:all", summary: "All settings", children: drawn.sections.map((s) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("section", { className: "unfold-sec", "data-unfold": "settings-section", "data-section": s.id, children: [
          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("h3", { children: s.title }),
          s.leaves.map((d) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Leaf, { d, now }, d.leaf.id))
        ] }, s.id)) }),
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Fold, { id: "settings:technical", summary: "Technical details", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Technical, { drawn, now }) })
      ] })
    ] }) : null
  ] });
}
function Technical({ drawn, now }) {
  const h = drawn.sources.host;
  const a = drawn.sources.agentd;
  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "unfold-sec", style: { display: "flex", flexDirection: "column", gap: 8 }, children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { "data-unfold": "settings-revisions", style: { display: "flex", flexDirection: "column", gap: 4 }, children: drawn.revisions.map((r) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-rev": r.key, "data-state": r.state, className: "unfold-leaf", style: { borderTop: 0 }, children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "unfold-muted", children: r.leaf.label }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "unfold-leaf-v", "data-state": r.state, children: [
        r.text,
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "unfold-leaf-m", children: STATE_TEXT[r.state] ?? r.state })
      ] })
    ] }, r.key)) }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-unfold": "settings-sources", className: "unfold-mono unfold-faint", children: [
      "host status: ",
      h.ok ? `read ${ago(h.read_at, now)}` : `not current (${h.error})`,
      " · agent events: ",
      a.error ?? `observer ${a.link}, boot ${a.boot ?? "not positively known"}`,
      " · stream: ",
      drawn.stream
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-unfold": "settings-edits", className: "unfold-faint", children: [
      "Edits: ",
      drawn.edits.pending.length,
      " pending, ",
      drawn.edits.applied.length,
      " applied, ",
      drawn.edits.rejected.length,
      " rejected. ",
      drawn.edits.note
    ] }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { "data-unfold": "settings-gaps", children: [
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "unfold-muted", style: { marginBottom: 2 }, children: "Not reported anywhere yet" }),
      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { style: { margin: 0, paddingLeft: 16, fontSize: 11.5 }, className: "unfold-faint", children: drawn.gaps.map((g) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { children: [
        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("b", { children: g.what }),
        ": ",
        g.detail
      ] }, g.id)) })
    ] })
  ] });
}
function Leaf({ d, now }) {
  const { leaf, state, text, note } = d;
  const long = typeof leaf.value === "string" && leaf.value.length > 80 && state === "live";
  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "unfold-leaf", children: [
    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { "data-unfold": "settings-label", className: "unfold-muted", children: leaf.label }),
    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "unfold-leaf-v", "data-unfold": "settings-leaf", "data-id": leaf.id, "data-state": state, children: [
      long ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("pre", { children: text }) : text,
      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "unfold-leaf-m", children: [
        STATE_TEXT[state] ?? state,
        " · ",
        leaf.source,
        leaf.observed_at ? ` · ${ago(leaf.observed_at, now)}` : "",
        note ? ` · ${note}` : ""
      ] })
    ] })
  ] });
}
return Object.defineProperty({ ...module.exports }, Symbol.toStringTag, { value: 'Module' })
  },
})
