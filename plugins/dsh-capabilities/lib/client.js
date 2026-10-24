window.__ModuleLoader__.load({
	id: "@rth/dsh-capabilities",
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
		
		// dsh-capabilities/client/index.jsx
		var index_exports = {};
		__export(index_exports, {
		  PANEL_ID: () => PANEL_ID,
		  apply: () => apply,
		  inject: () => inject
		});
		module.exports = __toCommonJS(index_exports);
		var import_react = require("react");
		var import_jsx_runtime = require("react/jsx-runtime");
		var PANEL_ID = "rth.capabilities";
		var STREAM = "api/rth-capabilities/stream";
		var inject = ["slots"];
		function apply(ctx) {
		  ctx.effect(() => ctx.slots.inject("main", () => ctx.slots.register(
		    { name: "main", key: PANEL_ID },
		    CapabilitiesPanel
		  )), "rth-capabilities: main panel");
		  ctx.effect(() => ctx.slots.inject("sidebar.panellist", () => ctx.slots.register(
		    { name: "sidebar.panellist", id: PANEL_ID, order: 50, label: () => "What I can do" },
		    PanelIcon
		  )), "rth-capabilities: sidebar entry");
		}
		function PanelIcon({ size = 16 }) {
		  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", { width: size, height: size, viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: "1.4", "aria-hidden": "true", children: [
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { x: "2", y: "2", width: "5", height: "5", rx: "1" }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { x: "9", y: "2", width: "5", height: "5", rx: "1" }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", { x: "2", y: "9", width: "5", height: "5", rx: "1" }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M9.5 11.5l1.5 1.5 3-3" })
		  ] });
		}
		function useMapStream() {
		  const [map, setMap] = (0, import_react.useState)(null);
		  const [stream, setStream] = (0, import_react.useState)("opening");
		  const [pushes, setPushes] = (0, import_react.useState)(0);
		  (0, import_react.useEffect)(() => {
		    const source = new EventSource(new URL(STREAM, document.baseURI));
		    source.addEventListener("map", (e) => {
		      setMap(JSON.parse(e.data));
		      setPushes((n) => n + 1);
		    });
		    source.onopen = () => setStream("open");
		    source.onerror = () => setStream("reconnecting");
		    return () => source.close();
		  }, []);
		  return { map, stream, pushes };
		}
		var GROUPS = [
		  ["capability", "What I can do"],
		  ["runtime", "How I run"],
		  ["moment", "TML moments"],
		  ["benchmark", "TML benchmark rows"]
		];
		var STATUS_TEXT = {
		  supported: "supported",
		  proxy: "stand-in only",
		  failing: "failing",
		  unverified: "unverified evidence",
		  unsupported: "not yet"
		};
		var STATUS_TONE = {
		  supported: "rgba(46,160,67,0.18)",
		  proxy: "rgba(210,153,34,0.18)",
		  failing: "rgba(218,54,51,0.18)",
		  unverified: "rgba(210,153,34,0.18)",
		  unsupported: "rgba(127,127,127,0.12)"
		};
		var muted = { opacity: 0.65 };
		function CapabilitiesPanel() {
		  const { map, stream, pushes } = useMapStream();
		  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { "data-rth": "capabilities", style: { height: "100%", overflow: "auto", padding: "24px 28px", fontSize: 14, lineHeight: 1.45 }, children: [
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", { style: { fontSize: 22, margin: "0 0 4px" }, children: "What I can do" }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: muted, children: "Computed only from Unfold-Bench scorecards. A cell without evidence says so; nothing is assumed." }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { "data-rth": "capabilities-stream", "data-state": stream, "data-pushes": pushes, style: { ...muted, fontSize: 12, marginTop: 4 }, children: [
		      "stream: ",
		      stream === "open" ? "live (pushed, no polling)" : stream,
		      " \xB7 maps received: ",
		      pushes
		    ] }),
		    map ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MapView, { map }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: { marginTop: 16 }, children: "Waiting for the first map\u2026" })
		  ] });
		}
		function MapView({ map }) {
		  const lmc = map.latest_measured_candidate;
		  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { "data-rth": "capabilities-runs", "data-runs": map.runs, style: { marginTop: 14, padding: "8px 10px", border: "1px solid rgba(127,127,127,0.3)", borderRadius: 6 }, children: [
		      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("b", { children: map.runs }),
		        " bench runs (",
		        map.unverified_runs.length,
		        " unverified) \xB7 registry v",
		        map.registry.version,
		        " ",
		        map.registry.sha256.slice(0, 12)
		      ] }),
		      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: muted, children: lmc ? `Latest measured candidate: ${lmc.backend}, agentd ${lmc.agentd_sha256.slice(0, 12)}, run ${lmc.run}. A measurement, not an activation receipt.` : "No verified run yet." })
		    ] }),
		    GROUPS.map(([group, title]) => {
		      const cells = map.cells.filter((c) => c.group === group);
		      if (!cells.length) return null;
		      const counts = map.summary[group] ?? {};
		      return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { "data-rth": "capabilities-group", "data-group": group, style: { marginTop: 20 }, children: [
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", { style: { fontSize: 16, margin: "0 0 2px" }, children: title }),
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: { ...muted, fontSize: 12, marginBottom: 6 }, "data-rth": "capabilities-summary", children: Object.entries(counts).filter(([, n]) => n).map(([s, n]) => `${n} ${STATUS_TEXT[s] ?? s}`).join(" \xB7 ") }),
		        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: { display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(240px, 1fr))", gap: 6 }, children: cells.map((c) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Cell, { cell: c }, c.id)) })
		      ] }, group);
		    })
		  ] });
		}
		function Cell({ cell }) {
		  const score = cell.score === null ? null : `score ${cell.score.toFixed(2)}${cell.best === null ? "" : `, best ${cell.best.toFixed(2)} over ${cell.trend.length} runs`}`;
		  const metrics = cell.metrics.map((m) => `${m.metric} ${m.stat} ${m.value} ${m.unit}`).join(" \xB7 ");
		  return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { "data-rth": "capability-cell", "data-id": cell.id, "data-status": cell.status, style: { background: STATUS_TONE[cell.status], borderRadius: 6, padding: "6px 8px" }, children: [
		    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: { fontWeight: 600 }, children: cell.title }),
		    /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { style: { fontSize: 12 }, children: [
		      STATUS_TEXT[cell.status] ?? cell.status,
		      cell.reason_code ? ` (${cell.reason_code.replaceAll("_", " ")})` : ""
		    ] }),
		    score ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: { ...muted, fontSize: 12 }, children: score }) : null,
		    metrics ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: { ...muted, fontSize: 12 }, children: metrics }) : null
		  ] });
		}
		return module.exports;
	}
});
