window.__ModuleLoader__.load({
	id: "@rth/dsh-events",
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
		
		// dsh-events/client/index.jsx
		var index_exports = {};
		__export(index_exports, {
		  RTH_EVENTS_ID: () => RTH_EVENTS_ID,
		  RTH_EVENTS_KIND: () => RTH_EVENTS_KIND,
		  RTH_PRESENCE_ID: () => RTH_PRESENCE_ID,
		  RTH_PRESENCE_KIND: () => RTH_PRESENCE_KIND,
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
		var clock = (at) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
		var clockS = (at) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
		var Dot = ({ tone = "neutral", pulse = false, label }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "rth-dot", "data-tone": tone, "data-pulse": pulse, role: label ? "img" : void 0, "aria-label": label, "aria-hidden": label ? void 0 : true });
		
		// dsh-session/src/call-view.mjs
		var PENDING_MS = 6e3;
		var LEASE_MS = 1e4;
		var READ_TIMEOUT_MS = 4e3;
		var RESTART_PENDING_MS = 25e3;
		function createCallReader({ fetch: get, url = "api/rth-session/call", now = () => performance.now(), timeoutMs = READ_TIMEOUT_MS, onRead = () => {
		} } = {}) {
		  let issued = 0;
		  let applied = 0;
		  let latest = null;
		  return {
		    latest: () => latest,
		    async load() {
		      const seq = ++issued;
		      const ctl = new AbortController();
		      const timer = setTimeout(() => ctl.abort(), timeoutMs);
		      let body;
		      try {
		        const r = await get(url, { cache: "no-store", signal: ctl.signal });
		        body = r.ok ? await r.json() : { unavailable: `DSH route ${r.status}` };
		      } catch (e) {
		        body = { unavailable: ctl.signal.aborted ? `no answer within ${timeoutMs / 1e3} s` : String(e?.message ?? e) };
		      } finally {
		        clearTimeout(timer);
		      }
		      if (seq < applied) return { stale: true, seq };
		      applied = seq;
		      latest = { ...body, receivedAt: now(), seq };
		      onRead(latest);
		      return latest;
		    }
		  };
		}
		var want = (p) => p.want === "start" ? "on" : "off";
		function callView(read, pending, now) {
		  const base = { state: "unknown", label: "Call", action: null, busy: false, tone: "neutral", title: "", reason: null, mute: null, pending: null };
		  if (!read) return { ...base, busy: true, title: "reading the call state from the native owner\u2026" };
		  const fresh = typeof read.receivedAt !== "number" || now - read.receivedAt <= LEASE_MS;
		  if (read.unavailable && read.hostStopped && fresh) {
		    const h = read.hostStopped;
		    const why = `rth-host stopped (${h.exit ?? "how it ended is unknown"}${h.panic ? `; ${h.panic}` : ""})`;
		    const stopped = { ...base, state: "stopped", tone: "error", reason: why };
		    if (pending?.want === "restart" && now - pending.at < RESTART_PENDING_MS) {
		      return { ...stopped, label: "Restarting voice host\u2026", busy: true, tone: "pending", pending: "waiting", title: `${why}: restart requested; waiting for the new rth-host to answer` };
		    }
		    const late = pending?.want === "restart" ? "no_receipt" : null;
		    if (!read.restart) return { ...stopped, label: "Voice host stopped", pending: late, title: `${why}: voice is off. Restart it from the Unfold menu or the tray (Restart voice host)` };
		    return { ...stopped, label: "Restart voice host", action: "restart", pending: late, title: `${why}: voice is off. Restart starts it again, asleep: waking voice opens the mic and speaker` };
		  }
		  if (pending?.want === "restart") pending = null;
		  const age = typeof read.receivedAt === "number" ? now - read.receivedAt : 0;
		  const escape = (last) => last === "active" || last === "starting" ? { action: "end", label: "End call (state unknown)" } : {};
		  if (read.unavailable) {
		    return { ...base, label: "Call unavailable", title: `the native owner cannot be read: ${read.unavailable}`, reason: read.unavailable, ...escape(read.lastState) };
		  }
		  if (age > LEASE_MS) {
		    return { ...base, state: "unknown", label: "Call: state unknown", tone: "pending", stale: true, title: `no fresh receipt from the native owner for ${Math.round(age / 1e3)} s: the call and the mute state are unknown`, reason: "stale", ...escape(read.call?.state) };
		  }
		  const call = read.call;
		  if (!call || typeof call.state !== "string") {
		    return { ...base, label: "Call unavailable", title: "this rth-host predates in-app calls: relaunch Unfold with the new build", reason: "no call owner" };
		  }
		  const link = read.link && read.link !== "up" ? ` \xB7 agent link: ${read.link}` : "";
		  let v;
		  switch (call.state) {
		    case "off":
		      v = { ...base, state: "off", label: "Start call", action: "start", title: `start a voice call: rth-host opens the microphone and speaker${link}` };
		      break;
		    case "starting":
		      v = { ...base, state: "starting", label: "Starting call\u2026", action: "end", busy: true, tone: "pending", title: `${call.reason ?? "starting"}. Click to end it (the start completes, then the devices are released)${link}` };
		      break;
		    case "active":
		      v = { ...base, state: "active", label: "End call", action: "end", tone: "danger", title: `in a call: ${call.reason ?? "the microphone is admitted, the speaker plays"}. End call closes the microphone and the speaker; the conversation stays${link}` };
		      break;
		    case "ending":
		      v = { ...base, state: "ending", label: "Ending call\u2026", busy: true, tone: "pending", title: `${call.reason ?? "releasing the devices"}${link}` };
		      break;
		    case "error": {
		      const why = call.error ?? call.reason ?? "the call did not start";
		      v = { ...base, state: "error", label: "Start call", action: "start", tone: "error", reason: why, title: `the last call did not start: ${why}. Click to try again${link}` };
		      break;
		    }
		    default:
		      return { ...base, label: "Call unavailable", title: `unknown call state ${call.state}`, reason: `unknown state ${call.state}` };
		  }
		  v.desired = call.desired ?? null;
		  if (pending) {
		    const answered = call.desired === want(pending);
		    if (!answered && now - pending.at < PENDING_MS) {
		      v = { ...v, busy: true, action: null, pending: "waiting", title: `${pending.want === "start" ? "start" : "end"} requested; waiting for the native owner's receipt` };
		    } else if (!answered) {
		      v = { ...v, pending: "no_receipt", title: `no receipt for the ${pending.want} request within ${PENDING_MS / 1e3} s. ${v.title}` };
		    }
		  }
		  if (v.state === "active") v.mute = muteView(read.mic);
		  return v;
		}
		function muteView(mic) {
		  switch (mic?.state) {
		    case "muted":
		      return { label: "Unmute", muted: true, action: false, busy: false, title: "muted: no microphone audio is sent (input only; the speaker keeps playing)" };
		    case "unmuted":
		      return { label: "Mute", muted: false, action: true, busy: false, title: "mute the microphone (input only; the speaker keeps playing; not End call)" };
		    case "muting":
		      return { label: "Muting\u2026", muted: false, action: null, busy: true, title: "mute requested, not yet applied by rth-host" };
		    case "unmuting":
		      return { label: "Unmuting\u2026", muted: true, action: null, busy: true, title: "unmute requested, not yet applied by rth-host (still muted)" };
		    default:
		      return null;
		  }
		}
		function nextRead(prev, got) {
		  if (!got || got.stale) return prev;
		  if (got.unavailable) return { ...got, lastState: prev?.call?.state ?? prev?.lastState ?? null };
		  return got;
		}
		function pollMs(v) {
		  if (v.busy || v.pending === "waiting" || v.mute?.busy || v.stale || v.state === "unknown") return 600;
		  return v.state === "active" ? 3e3 : 5e3;
		}
		
		// dsh-events/src/avatar.mjs
		var CALL_ROUTE = "api/rth-session/call";
		var MUTE_ROUTE = "api/rth-session/mute";
		var RESTART_ROUTE = "api/rth-session/restart-host";
		var ACTUAL = { off: "Voice asleep", starting: "Waking up\u2026", active: "Voice awake", ending: "Going to sleep\u2026", error: "Voice didn't wake", stopped: "Voice stopped", unknown: "Out of touch" };
		var WORD = { off: "asleep", starting: "waking up", active: "awake", ending: "going to sleep", error: "didn't wake", stopped: "stopped (rth-host ended)", unknown: "unknown" };
		var ASKED = { start: "wake", end: "sleep", restart: "restart" };
		function voiceControl(read, pending, now) {
		  const v = callView(read, pending, now);
		  const actual = ["off", "starting", "active", "ending", "error", "stopped"].includes(v.state) ? v.state : "unknown";
		  const escape = actual === "unknown" && v.action === "end";
		  let label;
		  if (v.action === "start") label = actual === "error" ? "Try waking voice again" : "Wake voice";
		  else if (v.action === "end") label = escape ? "Put voice to sleep (not sure it is awake)" : actual === "starting" ? "Put voice back to sleep" : "Put voice to sleep";
		  else if (v.action === "restart") label = "Restart voice";
		  else if (v.pending === "waiting") label = `${pending.want === "start" ? "Waking voice" : pending.want === "restart" ? "Restarting voice" : "Putting voice to sleep"}: waiting for Unfold to confirm`;
		  else if (!read) label = "Checking voice\u2026";
		  else label = actual === "ending" ? "Going to sleep\u2026" : actual === "stopped" ? "Restart from the Unfold menu or the tray" : "Voice can't be changed right now";
		  let settling = null;
		  if (v.pending === "waiting") settling = `${pending.want === "start" ? "Waking" : pending.want === "restart" ? "Restarting" : "Going to sleep"}\u2026 waiting for Unfold to confirm`;
		  else if (v.pending === "no_receipt") settling = `Unfold didn't confirm the ${ASKED[pending.want] ?? "request"} within ${(pending.want === "restart" ? RESTART_PENDING_MS : PENDING_MS) / 1e3} s. Voice is ${WORD[actual]}.`;
		  else if (v.desired && actual !== "unknown" && actual !== "error" && v.desired !== (actual === "active" || actual === "starting" ? "on" : "off")) settling = `Asked for voice ${v.desired}; it is ${WORD[actual]} so far`;
		  const status = !read ? "Checking\u2026" : ACTUAL[actual];
		  const reason = actual === "error" || actual === "stopped" ? v.reason ?? null : null;
		  const hint = v.action === "start" ? actual === "error" ? "Tap to try again" : "Tap to wake voice" : v.action === "end" ? actual === "starting" ? "Tap to cancel" : "Tap to put voice to sleep" : v.action === "restart" ? "Tap to restart voice (it comes back asleep)" : null;
		  return {
		    actual,
		    desired: v.desired ?? null,
		    action: v.action,
		    label,
		    status,
		    word: WORD[actual],
		    reason,
		    settling,
		    hint,
		    title: v.title,
		    stale: Boolean(v.stale),
		    read: Boolean(read),
		    readAt: read?.receivedAt ?? null,
		    // local monotonic ms of the last receipt (technical fold only)
		    pending: v.pending,
		    call: v,
		    // for pollMs
		    mute: muteGlyph(read, now)
		  };
		}
		function muteGlyph(read, now) {
		  const age = read && typeof read.receivedAt === "number" ? now - read.receivedAt : 0;
		  const fresh = read && !read.unavailable && age <= LEASE_MS && read.call && typeof read.call.state === "string";
		  const m = fresh ? muteView(read.mic) : null;
		  if (!m) {
		    return { state: "unknown", muted: null, action: null, label: "Microphone: not sure if muted", title: fresh ? "this rth-host reports no mute state" : "no fresh receipt from rth-host: the mute state is unknown" };
		  }
		  const state = read.mic.state;
		  const label = m.action === true ? "Mute microphone" : m.action === false ? "Unmute microphone" : m.label;
		  return { state, muted: m.muted, action: m.action, label, title: m.title };
		}
		function toggleRequest(voice) {
		  if (voice.action === "restart") return { url: RESTART_ROUTE, body: { restart: true } };
		  return voice.action ? { url: CALL_ROUTE, body: { call: voice.action } } : null;
		}
		function muteRequest(voice) {
		  return voice.mute.action === null ? null : { url: MUTE_ROUTE, body: { muted: voice.mute.action } };
		}
		var readDelay = (voice) => pollMs(voice.call);
		var HIDDEN_READ_MS = LEASE_MS - 2e3;
		function shouldRead({ hidden, sinceLast, delay }) {
		  if (sinceLast === Infinity) return true;
		  return sinceLast >= (hidden ? Math.max(delay, HIDDEN_READ_MS) : delay);
		}
		var FPS = { active: 30, calm: 8, rest: 0, still: 0 };
		function sceneFor(view, voice) {
		  const pose = view?.pose ?? "unknown";
		  const speech = view?.layers?.speech?.state ?? "unknown";
		  const actual = voice?.actual ?? "unknown";
		  const s = (form, face, motion, dim, density, status, extra = {}) => ({ form, face, motion, dim, density, status, tint: null, pose, label: status, because: view?.because ?? "", ...extra });
		  let scene;
		  if (pose === "unknown") scene = s("scatter", null, "still", 0.5, 0.6, `Unknown: ${view?.because ?? "no presence"}`, { tint: "grey" });
		  else if (actual === "unknown") scene = s("scatter", null, "still", 0.5, 0.6, `Unknown: ${voice?.title || "no fresh voice receipt from rth-host"}`, { tint: "grey" });
		  else if (actual === "stopped") scene = s("fault", "wince", "still", 0.6, 0.5, `Voice stopped: ${voice?.reason ?? "rth-host ended"}`, { tint: "warm" });
		  else if (pose === "wince") scene = s("fault", "wince", "active", 1, actual === "active" ? 1 : 0.6, `Fault: ${view.because}`, { tint: "warm" });
		  else if (actual === "starting") scene = s("gather", null, "active", 0.8, 0.85, "Voice waking: the dust gathers");
		  else if (actual === "ending") scene = s("disperse", null, "active", 0.55, 0.5, "Voice going to sleep: the dust disperses");
		  else if (actual === "off" || actual === "error") {
		    const rest = pose === "asleep" ? "settle" : pose === "sleepy" ? "slow" : pose === "pending" ? "orbit" : "drift";
		    const what = pose === "pending" ? "a typed request is open (pending, not thinking)" : "Unfold stays on; typed chat works";
		    scene = s(rest, null, pose === "pending" ? "calm" : "rest", 0.42, 0.35, `Voice asleep \xB7 ${what}`);
		  } else if (pose === "speaking") scene = s("face", "speak", "active", 1, 1, "Voice awake \xB7 speaking (device-rendered audio reported by rth-host)");
		  else if (speech === "candidate") scene = s("face", "listen", "active", 1.05, 1, "Voice awake \xB7 speech candidate (the provider transcribed input; not proof anyone spoke to Unfold)");
		  else if (pose === "pending") scene = s("orbit", null, "calm", 0.9, 1, "Voice awake \xB7 pending: a request is open (not thinking)");
		  else if (pose === "sleepy") scene = s("slow", null, "rest", 0.6, 0.9, "Voice awake \xB7 resting (idle 2+ min); the mic glyph says what the mic is");
		  else if (pose === "asleep") scene = s("settle", null, "rest", 0.4, 0.9, "Voice awake \xB7 resting (idle 10+ min); the mic glyph says what the mic is");
		  else scene = s("drift", null, "rest", 0.85, 1, "Voice awake \xB7 idle, waiting for the next event");
		  scene.key = [scene.form, scene.face, scene.tint, scene.dim, scene.density, scene.motion].join("|");
		  return scene;
		}
		function lifeFor(view, voice) {
		  return (view?.pose ?? "unknown") !== "unknown" && voice?.actual === "active";
		}
		var GRAINS_PER_CP = 100;
		var levelOf = (cp) => Math.max(1, 1 + Math.floor((cp - 100) / 10));
		var coloursOf = (level) => Math.min(10, 1 + Math.floor((level - 1) / 10));
		var CP_PREVIEW = Object.freeze({ cp: 100, level: levelOf(100), stage: "Seed", colours: coloursOf(levelOf(100)), preview: true });
		function grainBudget(cssWidth, cssHeight, cp = CP_PREVIEW.cp) {
		  const cap = Math.max(2e4, Math.min(4e4, Math.round(Math.max(0, cssWidth) * Math.max(0, cssHeight) / 4)));
		  return { cap, grains: Math.min(cp * GRAINS_PER_CP, cap) };
		}
		function createVoiceController({ fetch: f, now = () => performance.now(), onChange = () => {
		} } = {}) {
		  let read = null;
		  let pending = null;
		  let note = null;
		  let alive = true;
		  const reader = createCallReader({ fetch: f, url: CALL_ROUTE, now, onRead: (got) => {
		    read = nextRead(read, got);
		    settle();
		    if (alive) onChange();
		  } });
		  const settle = () => {
		    if (pending?.want === "restart") {
		      if (read && !read.unavailable) pending = null;
		    } else if (pending && read?.call?.desired === (pending.want === "start" ? "on" : "off")) pending = null;
		  };
		  const post = async (req, label) => {
		    note = null;
		    let why = null;
		    try {
		      const r = await f(req.url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(req.body), signal: AbortSignal.timeout(READ_TIMEOUT_MS) });
		      if (!(r.ok || r.status === 202)) {
		        const b = await r.json().catch(() => ({}));
		        why = b.error ?? b.refused ?? `refused (${r.status})`;
		      }
		    } catch (e) {
		      why = String(e?.message ?? e);
		    }
		    if (why) note = `${label} not delivered: ${why}`;
		    if (alive) onChange();
		    void reader.load();
		    return why;
		  };
		  return {
		    view: () => ({ ...voiceControl(read, pending, now()), note }),
		    load: () => reader.load(),
		    async toggle() {
		      const req = toggleRequest(voiceControl(read, pending, now()));
		      if (!req) return null;
		      pending = { want: req.body.restart ? "restart" : req.body.call, at: now() };
		      onChange();
		      const why = await post(req, req.body.restart ? "Restart voice" : req.body.call === "start" ? "Wake voice" : "Sleep voice");
		      if (why) pending = null;
		      return req;
		    },
		    async mute() {
		      const req = muteRequest(voiceControl(read, pending, now()));
		      if (!req) return null;
		      await post(req, req.body.muted ? "Mute" : "Unmute");
		      return req;
		    },
		    get disposed() {
		      return !alive;
		    },
		    dispose() {
		      alive = false;
		    }
		  };
		}
		var voiceKey = (v) => [v.actual, v.desired, v.action, v.pending, v.stale, v.mute.state, v.mute.action, v.settling, v.status, v.reason, v.note].join("|");
		function presenceWords(view, voice) {
		  const pose = view?.pose ?? "unknown";
		  const request = view?.layers?.request;
		  if (pose === "unknown") {
		    if (view?.layers === null || view?.layers === void 0) return { line: "Reconnecting\u2026", tone: "faint" };
		    if (request?.state === "unknown" && view.layers.link?.state === "live") return { line: "Catching up\u2026", tone: "faint" };
		    return { line: "Can't see what Unfold is doing right now", tone: "faint" };
		  }
		  if (pose === "wince") return { line: "Something went wrong. See below.", tone: "warn" };
		  if (pose === "speaking") return { line: "Speaking", tone: "accent" };
		  if (pose === "pending") return { line: /\breply \S+ started/.test(request?.label ?? "") ? "Replying\u2026" : "Got a message, no reply yet", tone: "accent" };
		  if (voice?.mute?.state === "muted" && voice.actual === "active") return { line: "Your microphone is muted", tone: "warn" };
		  if (pose === "sleepy" || pose === "asleep") return { line: "Quiet for a while", tone: "faint" };
		  if (voice?.actual === "off" || voice?.actual === "error" || voice?.actual === "stopped") return { line: "You can still type to Unfold", tone: "faint" };
		  return null;
		}
		var MIC_WORDS = { muted: "Muted", unmuted: "Not muted", not_muted: "Not muted", muting: "Muting\u2026", unmuting: "Unmuting\u2026", off: "Off (voice asleep)", unknown: "Not sure" };
		function presenceFacts(view, voice) {
		  const link = view?.layers?.link?.state;
		  const audio = view?.layers?.audio?.state;
		  const request = view?.layers?.request?.state;
		  const faults = view?.faults ?? [];
		  const micState = voice?.mute?.state && voice.mute.state !== "unknown" ? voice.mute.state : view?.layers?.mic?.state ?? "unknown";
		  return [
		    ["Connection", link === "live" ? "Connected" : view?.layers ? "Not connected right now" : "Not sure (reconnecting)"],
		    ["Voice", voice?.read ? `${voice.word[0].toUpperCase()}${voice.word.slice(1)}${voice.reason ? ` (${voice.reason})` : ""}` : "Checking\u2026"],
		    ["Microphone", voice?.actual === "off" && micState !== "muted" ? "Closed (voice asleep)" : MIC_WORDS[micState] ?? "Not sure"],
		    ["Right now", request === "pending" ? "Handling a message" : request === "none" ? "Nothing open" : "Not sure"],
		    ["Sound out", audio === "rendering" ? "Playing a reply" : audio === "quiet" ? "Nothing playing" : "Not reported to this tab"],
		    ["Problems", faults.length ? `${faults.length} recent: ${faults.at(-1).summary}` : "None seen"]
		  ];
		}
		
		// dsh-events/client/dust-renderer.js
		var DEBUG = globalThis.__rthAvatar ??= { live: 0, created: 0, frames: 0, rafs: 0, timeouts: 0, observers: 0 };
		var HY = 40;
		var EY = -24;
		var EX = 46;
		var MY = 38;
		var FACE = 0.9;
		function line(pts, width = 2, taper = false) {
		  const L = [0];
		  for (let i = 1; i < pts.length; i++) L.push(L[i - 1] + Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]));
		  const total = L[L.length - 1] || 1;
		  return (u, rr) => {
		    const d = u * total;
		    let lo = 0, hi = L.length - 1;
		    while (hi - lo > 1) {
		      const m = lo + hi >> 1;
		      if (L[m] < d) lo = m;
		      else hi = m;
		    }
		    const a = pts[lo], b = pts[hi], f = (d - L[lo]) / (L[hi] - L[lo] || 1);
		    const dx = b[0] - a[0], dy = b[1] - a[1], len = Math.hypot(dx, dy) || 1;
		    const w = taper ? width * (0.25 + Math.sin(Math.PI * u) * 0.75) : width;
		    return [a[0] + dx * f - dy / len * rr * w, a[1] + dy * f + dx / len * rr * w];
		  };
		}
		var ring = (cx, cy, rx, ry, width = 2) => (u, rr) => {
		  const a = u * Math.PI * 2;
		  return [cx + Math.cos(a) * (rx + rr * width), cy + Math.sin(a) * (ry + rr * width)];
		};
		var fill = (cx, cy, rx, ry) => (u, rr) => {
		  const a = u * Math.PI * 2, r = Math.sqrt(Math.abs(rr));
		  return [cx + Math.cos(a) * rx * r, cy + Math.sin(a) * ry * r];
		};
		var both = (a, b) => (u, rr) => u < 0.5 ? a(u * 2, rr) : b(u * 2 - 1, rr);
		var arc = (cx, cy, w, bend, n = 14) => Array.from({ length: n + 1 }, (_, i) => {
		  const x = -w + 2 * w * i / n, q = x / w;
		  return [cx + x, cy + bend * (1 - q * q)];
		});
		var place = (fn, s, dx, dy) => (u, rr) => {
		  const p = fn(u, rr);
		  return [p[0] * s + dx, p[1] * s + dy];
		};
		var keyed = (fn, key) => (fn.key = key, fn);
		function eyes(kind) {
		  const L = -EX, R = EX;
		  const raw = {
		    open: () => both(fill(L, EY, 11, 15), fill(R, EY, 11, 15)),
		    happy: () => both(line(arc(L, EY + 5, 22, -12), 5.5, true), line(arc(R, EY + 5, 22, -12), 5.5, true)),
		    wince: () => both(line([[L - 15, EY - 12], [L + 12, EY], [L - 15, EY + 12]], 3), line([[R + 15, EY - 12], [R - 12, EY], [R + 15, EY + 12]], 3))
		  }[kind]();
		  return keyed(place(raw, FACE, 0, 24 - EY * FACE), `e:${kind}`);
		}
		function mouth(kind, amp = 0) {
		  const raw = {
		    smile: () => line(arc(0, MY, 42, 16), 5.5, true),
		    speak: () => ring(0, MY + 5, 20 + amp * 5, 4 + amp * 18, 2.6),
		    zigzag: () => line([[-30, MY + 8], [-15, MY], [0, MY + 10], [15, MY], [30, MY + 8]], 2.4)
		  }[kind]();
		  return keyed(place(raw, FACE, 0, 92 - MY * FACE), `m:${kind}:${kind === "speak" ? amp.toFixed(2) : ""}`);
		}
		var orbitRing = (t) => keyed(ring(0, HY, 118 + Math.sin(t * 2) * 4, 140 + Math.sin(t * 2) * 3, 4), `s:orbit:${t.toFixed(2)}`);
		function expressionOf(scene, t, amp) {
		  const X = { eyes: null, mouth: null, sym: null, cloud: "drift", dim: scene.dim, tint: scene.tint, jitter: 0 };
		  switch (scene.form) {
		    case "scatter":
		      X.cloud = "scatter";
		      break;
		    case "gather":
		      X.cloud = "gather";
		      break;
		    case "disperse":
		      X.cloud = "disperse";
		      break;
		    case "slow":
		      X.cloud = "slow";
		      break;
		    case "settle":
		      X.cloud = "settle";
		      break;
		    case "orbit":
		      X.cloud = "orbit";
		      X.sym = orbitRing(t);
		      break;
		    case "fault":
		      X.jitter = 2;
		      break;
		    default:
		      X.cloud = "drift";
		  }
		  if (scene.face === "listen") {
		    X.eyes = eyes("open");
		    X.mouth = mouth("smile");
		  } else if (scene.face === "speak") {
		    X.eyes = eyes("happy");
		    X.mouth = mouth("speak", amp);
		  } else if (scene.face === "wince") {
		    X.eyes = eyes("wince");
		    X.mouth = mouth("zigzag");
		  }
		  return X;
		}
		var CYAN = [95, 243, 255];
		var GREY = [150, 140, 175];
		var WARM = [255, 120, 80];
		var GROUP_NAMES = ["eyes", "mouth", "sym", "line", "orb", "cloud"];
		var SHARES = [0.1, 0.08, 0.12, 0.17, 0.1, 0.43];
		var NU = 512;
		var NR = 9;
		var STAT = 8;
		var clamp = (v, a, b) => v < a ? a : v > b ? b : v;
		var sstep = (a, b, v) => {
		  const t = clamp((v - a) / (b - a), 0, 1);
		  return t * t * (3 - 2 * t);
		};
		function hash2(a, b) {
		  let n = Math.imul(a, 374761393) + Math.imul(b, 668265263) | 0;
		  n = Math.imul(n ^ n >>> 13, 1274126177);
		  return ((n ^ n >>> 16) >>> 0) / 4294967296;
		}
		function vnoise(x, y) {
		  const xi = Math.floor(x), yi = Math.floor(y), u = x - xi, v = y - yi, su = u * u * (3 - 2 * u), sv = v * v * (3 - 2 * v);
		  const a = hash2(xi, yi), b = hash2(xi + 1, yi), c = hash2(xi, yi + 1), d = hash2(xi + 1, yi + 1);
		  return (a + (b - a) * su) * (1 - sv) + (c + (d - c) * su) * sv;
		}
		var fbm = (x, y) => (vnoise(x, y) * 0.5 + vnoise(x * 2 + 5.3, y * 2 + 1.7) * 0.25 + vnoise(x * 4 + 11.1, y * 4 + 7.9) * 0.125) / 0.875;
		var WAVES = [[180, 10, 0.21, 0.9], [120, 7, 1.9, -0.6], [260, 8, 3.7, 0.4], [90, 4, 5.1, 1.1]].map(([lam, disp, dir, w]) => {
		  const k = 6.283 / lam;
		  return { kx: Math.cos(dir) * k, ky: Math.sin(dir) * k, A: disp / k, w: w * 0.25, p: dir * 3 };
		});
		var GX = 33;
		var GY = 43;
		var MODES = { drift: 0, scatter: 1, settle: 2, gather: 3, slow: 4, orbit: 5, disperse: 6 };
		function makeDust(cap) {
		  const F = () => new Float32Array(cap);
		  const px = F(), py = F(), vx = F(), vy = F(), hx = F(), hy = F(), ha = F(), hr = F(), ph = F(), fu = F(), fr = F(), dz = F();
		  const iu = new Uint16Array(cap), jr = new Uint8Array(cap), grp = new Uint8Array(cap), alt = new Uint8Array(cap);
		  const pos = new Float32Array(cap * 2), stat = new Float32Array(cap * STAT);
		  const curlX = new Float32Array(GX * GY), curlY = new Float32Array(GX * GY);
		  const cum = [];
		  SHARES.reduce((a, v, k) => cum[k] = a + v, 0);
		  for (let i = 0; i < cap; i++) {
		    const q = (i * 0.6180339887498949 + 0.31) % 1;
		    let g = 0;
		    while (g < 5 && q >= cum[g]) g++;
		    grp[i] = g;
		    let a = 0, r = 0, x = 0, y = 0;
		    for (let t = 0; t < 8; t++) {
		      a = Math.random() * 6.283;
		      r = Math.pow(Math.random(), 0.6);
		      x = Math.cos(a) * r * 145;
		      y = Math.sin(a) * r * 195;
		      if (Math.random() < 0.18 + 0.82 * sstep(0.36, 0.68, fbm(x / 36 + 3.7, y / 36 + 9.2))) break;
		    }
		    ha[i] = a;
		    hr[i] = r;
		    hx[i] = x;
		    hy[i] = y;
		    const u = Math.random(), rr = (Math.random() * 2 - 1) * (Math.random() < 0.18 ? 1.5 : 1);
		    const tu = u * (NU - 1);
		    iu[i] = Math.min(NU - 2, Math.floor(tu));
		    fu[i] = tu - iu[i];
		    const tr = (rr + 1) * 0.5 * (NR - 1);
		    jr[i] = clamp(Math.floor(tr), 0, NR - 2);
		    fr[i] = tr - jr[i];
		    px[i] = pos[2 * i] = x;
		    py[i] = pos[2 * i + 1] = y;
		    ph[i] = Math.random() * 6.283;
		    alt[i] = Math.random() < 0.04 ? 1 : 0;
		    const lr = Math.random(), layer = lr < 0.62 ? 0 : lr < 0.94 ? 1 : 2;
		    dz[i] = [0.7, 1, 1.6][layer];
		    const k = i * STAT;
		    stat[k] = g;
		    stat[k + 1] = Math.random();
		    stat[k + 2] = ph[i];
		    stat[k + 3] = 0.5 + Math.pow(Math.random(), 2.2);
		    stat[k + 4] = 0.45 + Math.random() * 0.55;
		    stat[k + 5] = layer;
		    stat[k + 6] = 1 - 0.7 * clamp((r - 0.8) / 0.2, 0, 1);
		    stat[k + 7] = -1;
		  }
		  const luts = GROUP_NAMES.slice(0, 5).map(() => new Float32Array(NU * NR * 2)), seams = luts.map(() => new Uint8Array(NU));
		  const keys = new Array(5).fill(null), act = new Array(6).fill(null), actS = new Array(6).fill(null);
		  function buildLUT(fn, L, S) {
		    for (let i = 0; i < NU; i++) {
		      const u = i / (NU - 1);
		      for (let j = 0; j < NR; j++) {
		        const p = fn(u, -1 + 2 * j / (NR - 1)), o = (i * NR + j) * 2;
		        L[o] = p[0];
		        L[o + 1] = p[1];
		      }
		    }
		    const mid = NR - 1 >> 1;
		    for (let i = 0; i < NU - 1; i++) {
		      const o = (i * NR + mid) * 2, n2 = o + NR * 2, dx = L[n2] - L[o], dy = L[n2 + 1] - L[o + 1];
		      S[i] = dx * dx + dy * dy > 36 ? 1 : 0;
		    }
		  }
		  function buildCurl(sec) {
		    for (let j = 0; j < GY; j++) for (let i = 0; i < GX; i++) {
		      const x = -160 + i * 10, y = -210 + j * 10;
		      let dx = 0, dy = 0;
		      for (const v of WAVES) {
		        const c = v.A * Math.cos(v.kx * x + v.ky * y + v.w * sec + v.p);
		        dx += c * v.ky;
		        dy -= c * v.kx;
		      }
		      curlX[j * GX + i] = dx;
		      curlY[j * GX + i] = dy;
		    }
		  }
		  let rs = 625341585;
		  const RND = 23283064365386963e-26;
		  return {
		    cap,
		    pos,
		    stat,
		    /** Step the first n grains toward the expression's targets; `still` snaps them (reduced motion). */
		    step(X, n, sec, dt, still) {
		      for (let g = 0; g < 5; g++) {
		        const fn = X[GROUP_NAMES[g]];
		        if (!fn) {
		          act[g] = null;
		          continue;
		        }
		        if (!fn.key || fn.key !== keys[g]) {
		          buildLUT(fn, luts[g], seams[g]);
		          keys[g] = fn.key || null;
		        }
		        act[g] = luts[g];
		        actS[g] = seams[g];
		      }
		      const k = still ? 1 : 0.075, damp = still ? 0 : 0.8, f60 = dt * 60;
		      const breathe = still ? 0 : Math.sin(sec * 1.5) * 1.6;
		      const jit = X.jitter && !still ? X.jitter * 4 : 0;
		      const mode = MODES[X.cloud] || 0;
		      const spin = still ? 0 : sec * (mode === 4 ? 0.012 : mode === 5 ? 0.16 : 0.035);
		      const flow = !still && (mode === 0 || mode === 4 || mode === 5), flowAmt = mode === 4 ? 0.4 : 1;
		      if (flow) buildCurl(sec);
		      for (let i = 0; i < n; i++) {
		        const L = act[grp[i]];
		        let tx, ty;
		        if (L) {
		          const S = actS[grp[i]], a = iu[i], q = fr[i];
		          let f = fu[i];
		          if (S[a]) f = f < 0.5 ? 0 : 1;
		          const b0 = (a * NR + jr[i]) * 2, b1 = b0 + NR * 2;
		          const x0 = L[b0] + (L[b0 + 2] - L[b0]) * q, y0 = L[b0 + 1] + (L[b0 + 3] - L[b0 + 1]) * q;
		          const x1 = L[b1] + (L[b1 + 2] - L[b1]) * q, y1 = L[b1 + 1] + (L[b1 + 3] - L[b1 + 1]) * q;
		          tx = x0 + (x1 - x0) * f;
		          ty = y0 + (y1 - y0) * f + breathe;
		          if (alt[i] && !still) {
		            tx += Math.sin(sec * 2 + ph[i]) * 6;
		            ty += Math.cos(sec * 1.7 + ph[i]) * 6;
		          }
		        } else if (mode === 1) {
		          const j = still ? 0 : Math.sin(sec * 4 + ph[i] * 9) * 6;
		          tx = hx[i] * 1.03 + j;
		          ty = hy[i] * 1.02 - j;
		        } else if (mode === 2) {
		          tx = hx[i] * 1.02;
		          ty = 182 + Math.abs(hy[i]) * 0.06 - hr[i] * 14;
		        } else if (mode === 3) {
		          const a2 = (ha[i] + spin * (1.3 - hr[i])) * 2, r = hr[i] * 0.45;
		          tx = Math.cos(a2) * r * 150;
		          ty = HY * 0.8 + Math.sin(a2) * r * 170;
		        } else if (mode === 6) {
		          tx = hx[i] * 1.45 + Math.cos(ha[i]) * 30;
		          ty = hy[i] * 1.3 + Math.sin(ha[i]) * 30;
		        } else {
		          const a2 = ha[i] + spin * (1.3 - hr[i]) * dz[i];
		          tx = Math.cos(a2) * hr[i] * 145;
		          ty = Math.sin(a2) * hr[i] * 195 + (mode === 4 ? 10 : 0);
		          if (flow) {
		            const gx = clamp((tx + 160) / 10, 0, GX - 1.001), gy = clamp((ty + 210) / 10, 0, GY - 1.001), ci = gx | 0, cj = gy | 0, u = gx - ci, v = gy - cj;
		            const o = cj * GX + ci, s = dz[i] * flowAmt;
		            tx += ((curlX[o] * (1 - u) + curlX[o + 1] * u) * (1 - v) + (curlX[o + GX] * (1 - u) + curlX[o + GX + 1] * u) * v) * s;
		            ty += ((curlY[o] * (1 - u) + curlY[o + 1] * u) * (1 - v) + (curlY[o + GX] * (1 - u) + curlY[o + GX + 1] * u) * v) * s;
		          }
		        }
		        if (jit) {
		          rs = Math.imul(rs, 1664525) + 1013904223 | 0;
		          tx += rs * RND * jit;
		          rs = Math.imul(rs, 1664525) + 1013904223 | 0;
		          ty += rs * RND * jit;
		        }
		        let x, y;
		        if (still) {
		          x = tx;
		          y = ty;
		        } else {
		          rs = Math.imul(rs, 1664525) + 1013904223 | 0;
		          const nx = rs * RND * 0.3;
		          rs = Math.imul(rs, 1664525) + 1013904223 | 0;
		          const ny = rs * RND * 0.3;
		          const nvx = (vx[i] + (tx - px[i]) * k) * damp + nx, nvy = (vy[i] + (ty - py[i]) * k) * damp + ny;
		          vx[i] = nvx;
		          vy[i] = nvy;
		          x = px[i] + nvx * f60;
		          y = py[i] + nvy * f60;
		        }
		        x = x < -148 ? -148 : x > 148 ? 148 : x;
		        y = y < -198 ? -198 : y > 198 ? 198 : y;
		        px[i] = x;
		        py[i] = y;
		        pos[2 * i] = x;
		        pos[2 * i + 1] = y;
		      }
		    },
		    formed(g) {
		      return !!act[g];
		    }
		  };
		}
		var GAINS = [0.62, 0.62, 0.6, 0.6, 0, 0.42];
		var granOf = (n) => clamp(Math.pow(4e4 / Math.max(1, n), 0.35), 0.8, 2.2);
		var energyOf = (n) => clamp(Math.pow(6e4 / Math.max(1, n), 0.3), 0.7, 1.8);
		var VS_PTS = `
		  precision highp float;
		  attribute vec2 aPos; attribute vec4 aInfo; attribute vec4 aA;
		  uniform vec2 uScale; uniform float uFormed[6]; uniform float uGain[6];
		  uniform float uTint, uDim, uStill, uSec, uPx, uGran, uEnergy; uniform vec3 uCol;
		  varying vec3 vCol; varying float vA, vSize, vSig;
		  void main() {
		    int gi = int(aInfo.x + 0.5);
		    float hue = aInfo.y, ph = aInfo.z, layer = aA.y;
		    float formed = 0.0, gain = 0.0;
		    for (int k = 0; k < 6; k++) if (k == gi) { formed = uFormed[k]; gain = uGain[k]; }
		    vec3 col;
		    if (uTint > 0.5 && uTint < 1.5) col = uCol * (0.85 + 0.3 * hue);
		    else if (formed > 0.5 && uTint > 1.5) col = vec3(1.0, 0.471, 0.314);
		    else col = formed > 0.5 ? uCol : uCol * mix(0.55, 1.15, hue);
		    float tw = 1.0;
		    if (uStill < 0.5) tw = 0.6 + 0.4 * sin(uSec * (1.2 + fract(ph * 7.31) * 4.5) + ph * 3.0);
		    float a, s;
		    if (formed > 0.5) { a = aA.x * gain; s = aInfo.w * uGran; }
		    else {
		      a = aA.x * aA.z * uGain[5] * (layer < 0.5 ? 0.5 : (layer < 1.5 ? 1.0 : 0.16));
		      s = layer < 0.5 ? aInfo.w * 0.65 * uGran : (layer < 1.5 ? aInfo.w * uGran : 2.2 + aInfo.w * 1.4 * uGran);
		    }
		    a = min(1.0, a * uDim * tw) * uEnergy;
		    s *= uPx;
		    float sig = max(0.42, s * 0.5);
		    float e = s * 0.5 / sig; a *= min(1.0, e * e);
		    vSig = sig; vSize = max(2.0, ceil(sig * 6.0)); gl_PointSize = vSize;
		    vCol = col; vA = a;
		    gl_Position = a > 0.0 ? vec4(aPos * uScale, 0.0, 1.0) : vec4(2.0, 2.0, 2.0, 1.0);
		  }`;
		var FS_PTS = `
		  precision mediump float;
		  varying vec3 vCol; varying float vA, vSize, vSig;
		  void main() {
		    vec2 d = (gl_PointCoord - 0.5) * vSize;
		    float w = exp(-dot(d, d) / (2.0 * vSig * vSig));
		    gl_FragColor = vec4(vCol * (vA * w), 1.0);
		  }`;
		var VS_FULL = `
		  attribute vec2 aXY; varying vec2 vUv;
		  void main() { vUv = aXY * 0.5 + 0.5; gl_Position = vec4(aXY, 0.0, 1.0); }`;
		var FS_DOWN = `
		  precision mediump float;
		  uniform sampler2D uTex; uniform vec2 uTexel; varying vec2 vUv;
		  void main() {
		    gl_FragColor = 0.25 * (texture2D(uTex, vUv + uTexel * vec2(-1.0, -1.0)) + texture2D(uTex, vUv + uTexel * vec2(1.0, -1.0))
		                         + texture2D(uTex, vUv + uTexel * vec2(-1.0, 1.0)) + texture2D(uTex, vUv + uTexel * vec2(1.0, 1.0)));
		  }`;
		var FS_BLUR = `
		  precision mediump float;
		  uniform sampler2D uTex; uniform vec2 uDir; varying vec2 vUv;
		  void main() {
		    vec2 o1 = uDir * 1.3846154, o2 = uDir * 3.2307692;
		    gl_FragColor = texture2D(uTex, vUv) * 0.2270270
		      + (texture2D(uTex, vUv + o1) + texture2D(uTex, vUv - o1)) * 0.3162162
		      + (texture2D(uTex, vUv + o2) + texture2D(uTex, vUv - o2)) * 0.0702703;
		  }`;
		var FS_COMP = `
		  #ifdef GL_FRAGMENT_PRECISION_HIGH
		  precision highp float;
		  #else
		  precision mediump float;
		  #endif
		  uniform sampler2D uScene, uB1, uB2, uB3;
		  uniform vec2 uRes; uniform float uSec, uGain, uExpo, uNeb; uniform vec3 uNebA, uNebB;
		  varying vec2 vUv;
		  float hash(vec2 p) { p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
		  float noise(vec2 p) {
		    vec2 i = floor(p), f = fract(p); f = f * f * (3.0 - 2.0 * f);
		    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
		  }
		  float fbm(vec2 p) { float v = 0.0, a = 0.5; for (int i = 0; i < 5; i++) { v += a * noise(p); p = p * 2.03 + 17.1; a *= 0.5; } return v; }
		  void main() {
		    vec3 scene = texture2D(uScene, vUv).rgb;
		    vec3 bloom = texture2D(uB1, vUv).rgb * 0.8 + texture2D(uB2, vUv).rgb * 0.7 + texture2D(uB3, vUv).rgb * 0.6;
		    vec3 c = 1.0 - exp(-(scene + bloom) * uGain * uExpo);
		    vec2 q = (vUv - 0.5) * 2.0; float d = length(q) / 1.41421;
		    vec3 bg = mix(vec3(0.078, 0.059, 0.118), vec3(0.027, 0.024, 0.043), clamp(d / 0.75, 0.0, 1.0));
		    vec2 np = vec2(vUv.x * uRes.x / uRes.y, vUv.y) * 2.6;
		    float n1 = fbm(np + vec2(uSec * 0.010, -uSec * 0.007)), n2 = fbm(np * 1.4 + vec2(-uSec * 0.008, 4.0));
		    bg += (uNebA * smoothstep(0.42, 0.9, n1) + uNebB * smoothstep(0.48, 0.95, n2)) * uNeb * (1.0 - 0.7 * clamp(d, 0.0, 1.0));
		    vec3 o = 1.0 - (1.0 - bg) * (1.0 - c);
		    o += (hash(gl_FragCoord.xy) - 0.5) / 255.0;
		    gl_FragColor = vec4(o, 1.0);
		  }`;
		function makeWebGL(canvas, dust, size, base = CYAN) {
		  const attrs = { alpha: false, antialias: false, depth: false, stencil: false, premultipliedAlpha: false, preserveDrawingBuffer: false, powerPreference: "low-power" };
		  let gl = canvas.getContext("webgl2", attrs);
		  const gl2 = !!gl;
		  if (!gl) gl = canvas.getContext("webgl", attrs);
		  if (!gl) return null;
		  let progs, bufPos, bufStat, bufTri, fmt, targets = null, lost = false;
		  function compile(vsSrc, fsSrc) {
		    const sh = (type, src) => {
		      const s = gl.createShader(type);
		      gl.shaderSource(s, src);
		      gl.compileShader(s);
		      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS) && !gl.isContextLost()) throw new Error(gl.getShaderInfoLog(s));
		      return s;
		    };
		    const p = gl.createProgram();
		    gl.attachShader(p, sh(gl.VERTEX_SHADER, vsSrc));
		    gl.attachShader(p, sh(gl.FRAGMENT_SHADER, fsSrc));
		    ["aPos", "aInfo", "aA"].forEach((n2, i) => gl.bindAttribLocation(p, i, n2));
		    gl.bindAttribLocation(p, 0, "aXY");
		    gl.linkProgram(p);
		    if (!gl.getProgramParameter(p, gl.LINK_STATUS) && !gl.isContextLost()) throw new Error(gl.getProgramInfoLog(p));
		    const u = {};
		    const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS) || 0;
		    for (let i = 0; i < n; i++) {
		      const name = gl.getActiveUniform(p, i).name;
		      u[name.replace("[0]", "")] = gl.getUniformLocation(p, name);
		    }
		    return { p, u };
		  }
		  function pickFormat() {
		    if (gl2 && gl.getExtension("EXT_color_buffer_float")) return { internal: gl.RGBA16F, type: gl.HALF_FLOAT, hdr: true };
		    return { internal: gl.RGBA, type: gl.UNSIGNED_BYTE, hdr: false };
		  }
		  function texture(w, h) {
		    const t = gl.createTexture();
		    gl.bindTexture(gl.TEXTURE_2D, t);
		    gl.texImage2D(gl.TEXTURE_2D, 0, fmt.internal, w, h, 0, gl.RGBA, fmt.type, null);
		    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
		    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
		    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
		    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
		    return t;
		  }
		  function target(w, h) {
		    const tex = texture(w, h), fb = gl.createFramebuffer();
		    gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
		    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
		    const ok = gl.checkFramebufferStatus(gl.FRAMEBUFFER) === gl.FRAMEBUFFER_COMPLETE;
		    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
		    return { tex, fb, w, h, ok };
		  }
		  const allTargets = () => [targets.scene, ...targets.levels.flat()];
		  function freeTargets() {
		    if (!targets) return;
		    allTargets().forEach((t) => {
		      gl.deleteTexture(t.tex);
		      gl.deleteFramebuffer(t.fb);
		    });
		    targets = null;
		  }
		  function ensureTargets() {
		    const { W, H } = size;
		    if (targets && targets.w === W && targets.h === H) return true;
		    for (let attempt = 0; attempt < 2; attempt++) {
		      freeTargets();
		      const lv = (d) => [Math.max(1, Math.ceil(W / d)), Math.max(1, Math.ceil(H / d))];
		      targets = { w: W, h: H, scene: target(W, H), levels: [2, 4, 8].map((d) => [target(...lv(d)), target(...lv(d))]) };
		      if (allTargets().every((t) => t.ok)) return true;
		      if (!fmt.hdr) break;
		      fmt = { internal: gl.RGBA, type: gl.UNSIGNED_BYTE, hdr: false };
		    }
		    return false;
		  }
		  function setup() {
		    lost = false;
		    targets = null;
		    fmt = pickFormat();
		    progs = { pts: compile(VS_PTS, FS_PTS), down: compile(VS_FULL, FS_DOWN), blur: compile(VS_FULL, FS_BLUR), comp: compile(VS_FULL, FS_COMP) };
		    const buf = (data, usage) => {
		      const b = gl.createBuffer();
		      gl.bindBuffer(gl.ARRAY_BUFFER, b);
		      gl.bufferData(gl.ARRAY_BUFFER, data, usage);
		      return b;
		    };
		    bufPos = buf(dust.pos, gl.DYNAMIC_DRAW);
		    bufStat = buf(dust.stat, gl.STATIC_DRAW);
		    bufTri = buf(new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
		  }
		  try {
		    setup();
		  } catch (err) {
		    console.warn("rth avatar: WebGL setup failed", err);
		    return null;
		  }
		  const onLost = (e) => {
		    e.preventDefault();
		    lost = true;
		  };
		  const onRestored = () => {
		    try {
		      setup();
		    } catch (err) {
		      console.warn(err);
		    }
		  };
		  canvas.addEventListener("webglcontextlost", onLost);
		  canvas.addEventListener("webglcontextrestored", onRestored);
		  function pass(prog, dst, src, name, v) {
		    gl.bindFramebuffer(gl.FRAMEBUFFER, dst.fb);
		    gl.viewport(0, 0, dst.w, dst.h);
		    gl.useProgram(prog.p);
		    gl.activeTexture(gl.TEXTURE0);
		    gl.bindTexture(gl.TEXTURE_2D, src.tex);
		    gl.uniform1i(prog.u.uTex, 0);
		    gl.uniform2f(prog.u[name], v[0], v[1]);
		    gl.drawArrays(gl.TRIANGLES, 0, 3);
		  }
		  const formedArr = new Float32Array(6), gainArr = new Float32Array(6);
		  return {
		    name: gl2 ? "WebGL2" : "WebGL1",
		    draw(X, n, sec, still) {
		      if (lost || gl.isContextLost() || !ensureTargets()) return;
		      const { W, H, DPR } = size;
		      const scale = Math.min(W / 300, H / 400) * 0.98, sx = scale * 2 / W, sy = -scale * 2 / H;
		      const out = fmt.hdr ? 1 : 0.3;
		      const tint = X.tint === "grey" ? 1 : X.tint === "warm" ? 2 : 0;
		      gl.bindFramebuffer(gl.FRAMEBUFFER, targets.scene.fb);
		      gl.viewport(0, 0, W, H);
		      gl.clearColor(0, 0, 0, 0);
		      gl.clear(gl.COLOR_BUFFER_BIT);
		      gl.enable(gl.BLEND);
		      gl.blendFunc(gl.ONE, gl.ONE);
		      const P = progs.pts;
		      gl.useProgram(P.p);
		      for (let g = 0; g < 6; g++) {
		        formedArr[g] = dust.formed(g) ? 1 : 0;
		        gainArr[g] = GAINS[g] * out;
		      }
		      gl.uniform1fv(P.u.uFormed, formedArr);
		      gl.uniform1fv(P.u.uGain, gainArr);
		      gl.uniform2f(P.u.uScale, sx, sy);
		      const col = tint === 1 ? GREY : base;
		      gl.uniform3f(P.u.uCol, col[0] / 255, col[1] / 255, col[2] / 255);
		      gl.uniform1f(P.u.uTint, tint);
		      gl.uniform1f(P.u.uDim, X.dim);
		      gl.uniform1f(P.u.uStill, still ? 1 : 0);
		      gl.uniform1f(P.u.uSec, sec % 3600);
		      gl.uniform1f(P.u.uPx, DPR >= 1.5 ? 1.3 : 1);
		      gl.uniform1f(P.u.uGran, granOf(n));
		      gl.uniform1f(P.u.uEnergy, energyOf(n));
		      gl.bindBuffer(gl.ARRAY_BUFFER, bufPos);
		      gl.bufferSubData(gl.ARRAY_BUFFER, 0, dust.pos.subarray(0, n * 2));
		      gl.enableVertexAttribArray(0);
		      gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
		      gl.bindBuffer(gl.ARRAY_BUFFER, bufStat);
		      gl.enableVertexAttribArray(1);
		      gl.vertexAttribPointer(1, 4, gl.FLOAT, false, STAT * 4, 0);
		      gl.enableVertexAttribArray(2);
		      gl.vertexAttribPointer(2, 4, gl.FLOAT, false, STAT * 4, 16);
		      if (n > 0) gl.drawArrays(gl.POINTS, 0, n);
		      gl.disableVertexAttribArray(1);
		      gl.disableVertexAttribArray(2);
		      gl.disable(gl.BLEND);
		      gl.bindBuffer(gl.ARRAY_BUFFER, bufTri);
		      gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
		      let src = targets.scene;
		      for (const [a, b] of targets.levels) {
		        pass(progs.down, a, src, "uTexel", [1 / src.w, 1 / src.h]);
		        pass(progs.blur, b, a, "uDir", [1 / a.w, 0]);
		        pass(progs.blur, a, b, "uDir", [0, 1 / a.h]);
		        src = a;
		      }
		      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
		      gl.viewport(0, 0, W, H);
		      const C = progs.comp;
		      gl.useProgram(C.p);
		      [targets.scene, ...targets.levels.map((l) => l[0])].forEach((t, i) => {
		        gl.activeTexture(gl.TEXTURE0 + i);
		        gl.bindTexture(gl.TEXTURE_2D, t.tex);
		      });
		      gl.uniform1i(C.u.uScene, 0);
		      gl.uniform1i(C.u.uB1, 1);
		      gl.uniform1i(C.u.uB2, 2);
		      gl.uniform1i(C.u.uB3, 3);
		      gl.uniform2f(C.u.uRes, W, H);
		      gl.uniform1f(C.u.uSec, still ? 0 : sec % 3600);
		      gl.uniform1f(C.u.uGain, 1 / out);
		      gl.uniform1f(C.u.uExpo, 1.25);
		      const nA = tint === 1 ? GREY : tint === 2 ? WARM : base, nB = tint === 1 ? GREY : base.map((v) => v * 0.7);
		      gl.uniform3f(C.u.uNebA, nA[0] / 255, nA[1] / 255, nA[2] / 255);
		      gl.uniform3f(C.u.uNebB, nB[0] / 255, nB[1] / 255, nB[2] / 255);
		      gl.uniform1f(C.u.uNeb, 0.075 * Math.min(1, X.dim));
		      gl.drawArrays(gl.TRIANGLES, 0, 3);
		      gl.activeTexture(gl.TEXTURE0);
		    },
		    dispose() {
		      canvas.removeEventListener("webglcontextlost", onLost);
		      canvas.removeEventListener("webglcontextrestored", onRestored);
		      if (!gl.isContextLost()) {
		        freeTargets();
		        Object.values(progs).forEach((p) => gl.deleteProgram(p.p));
		        [bufPos, bufStat, bufTri].forEach((b) => gl.deleteBuffer(b));
		      }
		    }
		  };
		}
		var CAP_2D = 4e3;
		function makeCanvas2D(canvas, dust, size, base = CYAN) {
		  const ctx = canvas.getContext("2d");
		  if (!ctx) return null;
		  return {
		    name: "Canvas 2D",
		    draw(X, n0, sec, still) {
		      const { W, H } = size;
		      const n = Math.min(n0, CAP_2D);
		      const scale = Math.min(W / 300, H / 400) * 0.98, cx = W / 2, cy = H / 2;
		      ctx.globalCompositeOperation = "source-over";
		      ctx.fillStyle = "#0a0812";
		      ctx.fillRect(0, 0, W, H);
		      ctx.globalCompositeOperation = "lighter";
		      const st = dust.stat, pos = dust.pos;
		      for (let i = 0; i < n; i++) {
		        const k = i * STAT, g = st[k], formed = dust.formed(g), layer = st[k + 5];
		        const col = X.tint === "grey" ? GREY : formed && X.tint === "warm" ? WARM : base;
		        const tw = still ? 1 : 0.75 + Math.sin(sec * 3 + st[k + 2] * 3) * 0.25;
		        const a = Math.min(1, st[k + 4] * X.dim * tw * (formed ? GAINS[g] : GAINS[5] * st[k + 6] * (layer === 0 ? 0.5 : layer === 1 ? 1 : 0.16)) * 2);
		        if (a <= 4e-3) continue;
		        ctx.fillStyle = `rgba(${col[0]},${col[1]},${col[2]},${a.toFixed(3)})`;
		        const sz = Math.max(1, st[k + 3] * 1.6 * (formed ? 1.2 : layer === 2 ? 2 : 1));
		        ctx.fillRect(cx + pos[2 * i] * scale - sz / 2, cy + pos[2 * i + 1] * scale - sz / 2, sz, sz);
		      }
		      ctx.globalCompositeOperation = "source-over";
		    },
		    dispose() {
		    }
		  };
		}
		var TIER_RGB = { cyan: [95, 243, 255], magenta: [255, 95, 210], violet: [169, 139, 255], amber: [255, 197, 110], rose: [255, 143, 168], mint: [127, 240, 192], "ice-blue": [184, 230, 255], coral: [255, 143, 106], lime: [200, 240, 110], "white-gold": [255, 233, 184] };
		function dustRenderer(canvas, { reducedMotion = false, grains = 1e4, colour = "cyan" } = {}) {
		  const base = TIER_RGB[colour] ?? CYAN;
		  DEBUG.live += 1;
		  DEBUG.created += 1;
		  const dust = makeDust(grains);
		  const size = { W: 1, H: 1, DPR: 1 };
		  const measure = () => {
		    const r = canvas.getBoundingClientRect();
		    size.DPR = Math.min(2, globalThis.devicePixelRatio || 1);
		    const w = Math.max(1, Math.round(r.width * size.DPR)), h = Math.max(1, Math.round(r.height * size.DPR));
		    if (w === size.W && h === size.H) return false;
		    size.W = w;
		    size.H = h;
		    canvas.width = w;
		    canvas.height = h;
		    return true;
		  };
		  measure();
		  const gfx = makeWebGL(canvas, dust, size, base) ?? makeCanvas2D(canvas, dust, size, base);
		  let scene = null;
		  let reduced = reducedMotion;
		  let visible = true;
		  let disposed = false;
		  let raf = 0;
		  let timer = 0;
		  let last = 0;
		  let shown = 0;
		  let frames = 0;
		  let bye = null;
		  const resize = () => {
		    if (measure()) paintStill();
		  };
		  const ro = typeof ResizeObserver === "function" ? new ResizeObserver(resize) : null;
		  if (ro) {
		    ro.observe(canvas);
		    DEBUG.observers += 1;
		  }
		  const moving = () => (FPS[scene?.motion] ?? FPS.calm) > 0;
		  const want2 = () => !disposed && visible && !reduced && scene && gfx && moving();
		  const target = () => Math.round(grains * (scene?.density ?? 1));
		  function frame(ts, still) {
		    const sec = ts / 1e3;
		    const dt = last ? Math.min(0.05, (ts - last) / 1e3) : 0.016;
		    last = ts;
		    const goal = target();
		    shown = still ? goal : Math.round(shown + (goal - shown) * Math.min(1, dt * 3));
		    if (Math.abs(goal - shown) < 50) shown = goal;
		    const amp = scene.face === "speak" && !still ? 0.35 + 0.65 * Math.abs(Math.sin(sec * 7.3) * Math.sin(sec * 3.1 + 1)) : 0.5;
		    const X = expressionOf(scene, still ? 0 : sec, amp);
		    if (bye) {
		      const k = clamp((ts - bye.t0) / bye.ms, 0, 1);
		      X.eyes = X.mouth = X.sym = null;
		      X.cloud = "disperse";
		      X.dim *= (1 - k) * (1 - k);
		    }
		    dust.step(X, Math.max(shown, goal), still ? 0 : sec, dt, still);
		    gfx.draw(X, shown, sec, still);
		    frames += 1;
		    DEBUG.frames += 1;
		  }
		  function paintStill() {
		    if (!disposed && visible && scene && gfx) frame(performance.now(), true);
		  }
		  function cancel() {
		    if (raf) {
		      cancelAnimationFrame(raf);
		      raf = 0;
		      DEBUG.rafs -= 1;
		    }
		    if (timer) {
		      clearTimeout(timer);
		      timer = 0;
		      DEBUG.timeouts -= 1;
		    }
		  }
		  function tick(ts) {
		    raf = 0;
		    DEBUG.rafs -= 1;
		    if (!want2()) return;
		    const fps = FPS[scene.motion] ?? FPS.calm;
		    if (fps >= 30 && last && ts - last < 1e3 / fps - 2) return schedule();
		    frame(ts, false);
		    schedule();
		  }
		  function schedule() {
		    if (!want2() || raf || timer) return;
		    const fps = FPS[scene.motion] ?? FPS.calm;
		    if (fps >= 30) {
		      raf = requestAnimationFrame(tick);
		      DEBUG.rafs += 1;
		      return;
		    }
		    timer = setTimeout(() => {
		      timer = 0;
		      DEBUG.timeouts -= 1;
		      if (!want2()) return;
		      raf = requestAnimationFrame(tick);
		      DEBUG.rafs += 1;
		    }, Math.round(1e3 / fps));
		    DEBUG.timeouts += 1;
		  }
		  return {
		    name: gfx?.name ?? "none",
		    draw(next) {
		      const changed = !scene || next.key !== scene.key;
		      scene = next;
		      canvas.dataset.scene = next.key;
		      if (bye) return;
		      if (reduced || !moving()) {
		        cancel();
		        if (changed) paintStill();
		        return;
		      }
		      if (changed) {
		        cancel();
		        last = 0;
		      }
		      schedule();
		    },
		    setReducedMotion(on) {
		      reduced = Boolean(on);
		      if (bye) return;
		      cancel();
		      if (reduced || !moving()) paintStill();
		      else schedule();
		    },
		    /** The tab or window is hidden (or the canvas is off screen): stop rendering until it shows. */
		    setVisible(on) {
		      visible = Boolean(on);
		      if (bye) return;
		      if (!visible) {
		        cancel();
		        return;
		      }
		      last = 0;
		      if (reduced || !moving()) paintStill();
		      else schedule();
		    },
		    /** The app's quit: the face lets go and the dust disperses and fades out over `ms` (a fade under
		     *  reduced motion). True when it started. Not gated on `visible`: a stale flag would silently
		     *  skip the goodbye, and a page that is really hidden runs no animation frame anyway. */
		    goodbye(ms = 500) {
		      if (disposed || !gfx || !scene) return false;
		      if (bye) return true;
		      cancel();
		      bye = { t0: performance.now(), ms };
		      canvas.dataset.goodbye = "dissolving";
		      if (reduced) {
		        canvas.style.transition = `opacity ${ms}ms ease-in`;
		        canvas.style.opacity = "0";
		        return true;
		      }
		      last = 0;
		      const step = (ts) => {
		        raf = 0;
		        DEBUG.rafs -= 1;
		        if (disposed) return;
		        frame(ts, false);
		        if (ts - bye.t0 < ms) {
		          raf = requestAnimationFrame(step);
		          DEBUG.rafs += 1;
		        } else canvas.dataset.goodbye = "done";
		      };
		      raf = requestAnimationFrame(step);
		      DEBUG.rafs += 1;
		      return true;
		    },
		    stats: () => ({ renderer: gfx?.name ?? "none", grains, shown, frames, running: Boolean(raf || timer), fps: scene && want2() ? FPS[scene.motion] : 0 }),
		    dispose() {
		      if (disposed) return;
		      disposed = true;
		      cancel();
		      if (ro) {
		        ro.disconnect();
		        DEBUG.observers -= 1;
		      }
		      gfx?.dispose();
		      DEBUG.live -= 1;
		    }
		  };
		}
		
		// dsh-events/src/queue.mjs
		var WORDS = {
		  en: { unknown: "?", speaking: "Speaking", replying: "Replying", writing: "Writing", listening: "Listening", working: "Working", quiet: "Quiet", asleep: "Asleep" },
		  pt: { unknown: "?", speaking: "Falando", replying: "Respondendo", writing: "Escrevendo", listening: "Ouvindo", working: "Trabalhando", quiet: "Em sil\xEAncio", asleep: "Dormindo" }
		};
		var langOf = (tag) => /^pt\b/i.test(tag ?? "") ? "pt" : "en";
		function openItems(view) {
		  const request = view?.layers?.request;
		  if (request?.state !== "pending") return [];
		  const label = String(request.label ?? "");
		  const items = [];
		  for (const m of label.matchAll(/\bcommand (\S+) admitted\b/g)) items.push({ id: `c:${m[1]}`, kind: "request", ref: m[1] });
		  for (const m of label.matchAll(/\breply (\S+) started\b/g)) items.push({ id: `r:${m[1]}`, kind: "reply", ref: m[1] });
		  return items;
		}
		function stateOf(view, voice, queue = null) {
		  const pose = view?.pose ?? "unknown";
		  if (pose === "unknown" || !voice?.read || voice.actual === "unknown") return "unknown";
		  const lost = (queue?.slots ?? []).some((s) => s.phase === "lost");
		  if (pose === "speaking") return "speaking";
		  const items = openItems(view);
		  if (items.some((i) => i.kind === "reply")) return "replying";
		  if (items.length || view?.layers?.request?.state === "pending") return "working";
		  if (lost) return "unknown";
		  if (voice.actual === "off" || voice.actual === "error") return "asleep";
		  return "quiet";
		}
		var SETTLE_MS = 1400;
		var MAX_SLOTS = 5;
		function stepQueue(prev, view, now) {
		  const live = view?.pose !== "unknown" && view?.layers?.link?.state === "live";
		  const open = live ? openItems(view) : null;
		  const byId = new Map((prev?.slots ?? []).map((s) => [s.id, s]));
		  const slots = [];
		  if (open) {
		    for (const item of open) {
		      const old = byId.get(item.id);
		      const keep = old && (old.phase === "running" || old.phase === "lost");
		      slots.push({ ...item, since: keep ? old.since : now, phase: "running", unsure: false });
		      byId.delete(item.id);
		    }
		  }
		  for (const s of byId.values()) {
		    if (!live) slots.push(s.phase === "running" ? { ...s, unsure: true } : s);
		    else if (s.phase === "running") slots.push(s.unsure ? { ...s, phase: "lost", unsure: false } : { ...s, phase: "ended", endedAt: now });
		    else if (s.phase === "lost") slots.push(s);
		    else if (now - s.endedAt < SETTLE_MS) slots.push(s);
		  }
		  slots.sort((a, b) => a.since - b.since);
		  return { slots };
		}
		function nextQueueChange(q) {
		  const ends = (q?.slots ?? []).filter((s) => s.phase === "ended").map((s) => s.endedAt + SETTLE_MS);
		  return ends.length ? Math.min(...ends) : null;
		}
		var ago = (ms) => ms < 6e4 ? `${Math.max(0, Math.round(ms / 1e3))} s` : `${Math.round(ms / 6e4)} min`;
		function slotLine(slot, now) {
		  const age = ago(now - slot.since);
		  if (slot.phase === "ended") return slot.kind === "reply" ? "A reply ended" : "A request ended";
		  if (slot.phase === "lost") return `${slot.kind === "reply" ? "A reply" : "A request"} \xB7 not sure how it ended (the link dropped while it was open)`;
		  const unsure = slot.unsure ? " \xB7 not sure it is still running (out of touch)" : "";
		  if (slot.kind === "reply") return `Replying \xB7 ${age}${unsure}`;
		  return `A request is open \xB7 ${age}${unsure}`;
		}
		
		// dsh-mind/src/redact.mjs
		var REDACTED = "[redacted]";
		var SECRET_TEXT = [
		  // Whole-value priority: a credential key's value is secret as ONE unit, whatever it
		  // starts with ("Bearer ", "Basic ", even an already-redacted marker, which is untrusted input),
		  // so the key rules run first and no inner pattern can split a value they recognise.
		  // A quoted value runs to its UNESCAPED closing quote (JSON: `\"` and `\\` do not end it; a
		  // trailing lone `\` is an escape still to come), or to the end while it is still open (a
		  // streamed `KEY="…` is hidden until its quote arrives).
		  [/\b([A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*)=("(?:[^"\\]|\\[\s\S]|\\$)*(?:"|$)|'(?:[^'\\]|\\[\s\S]|\\$)*(?:'|$)|\S+)/g, (_, name) => `${name}=${REDACTED}`],
		  // `name: value` / `name=value`. A quoted value (any prefix, spaces and escapes included) runs to
		  // its unescaped closing quote, or to the end while open. A bare value that is an authorization
		  // scheme (`Authorization: Bearer x`) is left to the scheme rule below.
		  [/\b(x-goog-api-key|authorization|api[_-]?key|token)(["']?\s*[:=]\s*)(?:(["'])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(?!Bearer\b|Basic\b|\[redacted\])[^\s"',}]+)/gi, (_, name, sep, q, close) => q ? `${name}${sep}${q}${REDACTED}${close}` : `${name}${sep}${REDACTED}`],
		  [/\b(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+/g, (_, scheme) => `${scheme} ${REDACTED}`],
		  [/\bAIza[0-9A-Za-z_-]{20,}/g, () => REDACTED],
		  // Standalone sk- credentials (the contract above): a whole key, then a still-open run at the end.
		  [/(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))sk-(?=[A-Za-z0-9_-]*[A-Z])(?=[A-Za-z0-9_-]*[a-z])(?=[A-Za-z0-9_-]*[0-9])[A-Za-z0-9_-]{20,}/g, () => REDACTED],
		  [/(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))sk-[A-Za-z0-9_-]+$/, () => REDACTED]
		];
		function redactText(s) {
		  let out = s;
		  for (const [re, fn] of SECRET_TEXT) out = out.replace(re, fn);
		  return out;
		}
		
		// dsh-events/client/index.jsx
		var import_jsx_runtime2 = require("react/jsx-runtime");
		var RTH_EVENTS_ID = "@rth/dsh-events";
		var RTH_EVENTS_KIND = "rth.events";
		var RTH_PRESENCE_ID = "@rth/dsh-events/presence";
		var RTH_PRESENCE_KIND = "rth.presence";
		var STREAM = "api/rth-events/stream";
		var PRESENCE_STREAM = "api/rth-presence/stream";
		var SHOWN = 200;
		var createRenderer = dustRenderer;
		var inject = ["slots", "sidebarRightTabs", "sidebarRight"];
		function apply(ctx) {
		  ctx.effect(() => ctx.sidebarRightTabs.register({
		    id: RTH_EVENTS_ID,
		    kind: RTH_EVENTS_KIND,
		    priority: "extension",
		    title: () => "Events",
		    guide: [{
		      id: "rth-events",
		      order: 90,
		      title: () => "Unfold events",
		      description: () => "Every event from the agent, as it arrives"
		    }]
		  }), "rth-events: tab type");
		  ctx.effect(() => ctx.sidebarRightTabs.register({
		    id: RTH_PRESENCE_ID,
		    kind: RTH_PRESENCE_KIND,
		    priority: "extension",
		    title: () => "Unfold",
		    guide: [{
		      id: "rth-presence",
		      order: 91,
		      title: () => "Unfold",
		      description: () => "Unfold itself: tap to wake or sleep voice"
		    }]
		  }), "rth-presence: tab type");
		  ctx.effect(() => () => {
		    for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
		      if (tab.kind !== RTH_EVENTS_KIND && tab.kind !== RTH_PRESENCE_KIND) continue;
		      try {
		        ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId);
		      } catch {
		      }
		    }
		  }, "rth-events: close open tabs on unload");
		  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab", () => ctx.slots.register(
		    { name: "sidebar.right.pane.tab", key: RTH_EVENTS_ID },
		    guarded("rth-events", EventsBody)
		  )), "rth-events: tab body");
		  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab.title", () => ctx.slots.register(
		    { name: "sidebar.right.pane.tab.title", key: RTH_EVENTS_ID },
		    EventsTitle
		  )), "rth-events: tab title");
		  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab", () => ctx.slots.register(
		    { name: "sidebar.right.pane.tab", key: RTH_PRESENCE_ID },
		    guarded("rth-presence", PresenceBody)
		  )), "rth-presence: tab body");
		  ctx.effect(() => ctx.slots.inject("sidebar.right.pane.tab.title", () => ctx.slots.register(
		    { name: "sidebar.right.pane.tab.title", key: RTH_PRESENCE_ID },
		    PresenceTitle
		  )), "rth-presence: tab title");
		}
		function EventsTitle({ useTabInfo }) {
		  const { tab } = useTabInfo();
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { "data-rth": "events-title", children: tab.title });
		}
		var initial = { rows: [], link: null, source: null, stream: "opening", attaches: 0, history: [], status: null, rev: 0, dropped: 0 };
		function reduce(state, { event, data }) {
		  switch (event) {
		    case "open":
		      return { ...state, stream: "open" };
		    case "error":
		      return { ...state, stream: "reconnecting" };
		    case "snapshot": {
		      const attached = data.link?.state === "attached";
		      return { ...state, rows: data.rows, link: data.link, source: data.source, status: data.status, rev: data.rev, dropped: data.dropped, attaches: Math.max(state.attaches, attached ? 1 : 0) };
		    }
		    case "added":
		      return { ...state, rows: [...state.rows, data] };
		    case "updated":
		      return { ...state, rows: state.rows.map((r) => r.key === data.key ? { ...r, ...data } : r) };
		    case "evicted":
		      return { ...state, rows: state.rows.filter((r) => r.key !== data.key), dropped: state.dropped + 1 };
		    case "status":
		      return { ...state, status: data };
		    case "link": {
		      const attaches = data.state === "attached" ? state.attaches + 1 : state.attaches;
		      const history = [...state.history, { ...data, at: (/* @__PURE__ */ new Date()).toLocaleTimeString() }].slice(-6);
		      return { ...state, link: data, attaches, history };
		    }
		    default:
		      return state;
		  }
		}
		function useEventStream() {
		  const [state, dispatch] = (0, import_react2.useReducer)(reduce, initial);
		  (0, import_react2.useEffect)(() => {
		    const source = new EventSource(new URL(STREAM, document.baseURI));
		    const on = (event) => (e) => dispatch({ event, data: e.data === void 0 ? null : JSON.parse(e.data) });
		    const names = ["snapshot", "added", "updated", "evicted", "status", "link"];
		    const handlers = names.map((name) => [name, on(name)]);
		    for (const [name, fn] of handlers) source.addEventListener(name, fn);
		    source.onopen = () => dispatch({ event: "open" });
		    source.onerror = () => dispatch({ event: "error" });
		    return () => source.close();
		  }, []);
		  return state;
		}
		function linkText(link, attaches) {
		  if (!link) return "agentd: not linked yet";
		  switch (link.state) {
		    case "attached":
		      return attaches > 1 ? `agentd: reconnected (attach ${attaches}) as a read-only observer` : "agentd: attached as a read-only observer";
		    case "waiting":
		      return `agentd: link lost, reconnecting in ${link.detail} ms`;
		    case "connecting":
		      return "agentd: connecting";
		    case "error":
		      return `agentd: link error (${link.detail})`;
		    default:
		      return `agentd: ${link.state}`;
		  }
		}
		var EVENTS_CSS = `
		.rth-ev{display:flex;flex-direction:column;height:100%;overflow:hidden}
		.rth-ev-head{display:flex;align-items:center;gap:9px;padding:14px 16px 12px;border-bottom:1px solid var(--rth-line)}
		.rth-ev-head b{font-size:14px;font-weight:600}
		.rth-ev-head[data-state=down] b{color:var(--rth-muted);font-weight:500}
		.rth-ev-meta{margin-left:auto;font-size:11.5px;color:var(--rth-faint)}
		.rth-ev-note{padding:8px 16px;font-size:11.5px;color:var(--rth-warn);border-bottom:1px solid var(--rth-line)}
		.rth-ev-list{flex:1 1 auto;overflow:auto;list-style:none;margin:0;padding:6px 8px 16px}
		.rth-ev-row{all:unset;box-sizing:border-box;display:grid;grid-template-columns:10px 1fr auto;gap:2px 10px;align-items:baseline;width:100%;padding:6px 8px;border-radius:8px;cursor:pointer}
		.rth-ev-row:hover{background:var(--rth-hover)}
		.rth-ev-row:focus-visible{outline:2px solid var(--rth-accent)}
		.rth-ev-row .rth-dot{width:6px;height:6px;align-self:center}
		.rth-ev-name{font-size:12.5px;font-weight:550;min-width:0}
		.rth-ev-time{font-size:11px;color:var(--rth-faint);font-variant-numeric:tabular-nums}
		.rth-ev-sum{grid-column:2/4;font-size:12px;color:var(--rth-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
		.rth-ev-row[aria-expanded=true] .rth-ev-sum{white-space:normal;overflow-wrap:anywhere}
		.rth-ev-more{grid-column:2/4;margin-top:4px}
		.rth-ev-row[data-known=false] .rth-ev-name{color:var(--rth-warn)}
		.rth-ev-foot{padding:0 14px;border-top:1px solid var(--rth-line)}
		.rth-ev-foot>.rth-fold{border-top:0}
		.rth-ev-fresh{animation:rth-in .5s var(--rth-ease)}
		@keyframes rth-in{from{background:color-mix(in oklab,var(--rth-accent) 12%,transparent)}to{background:transparent}}
		`;
		var PHASE = { connection: ["Connection", "neutral"], agent: ["Agent", "neutral"], input: ["Input", "accent"], decision: ["Decision", "accent"], action: ["Action", "ok"], result: ["Result", "ok"] };
		var plainName = (n) => n ? `${n[0].toUpperCase()}${n.slice(1).replace(/_/g, " ")}` : "Unknown event";
		function linkWords(link, stream) {
		  if (stream !== "open") return ["Reconnecting\u2026", "down"];
		  if (!link) return ["Connecting\u2026", "down"];
		  switch (link.state) {
		    case "attached":
		      return ["Connected", "live"];
		    case "waiting":
		      return ["Lost the agent, trying again", "down"];
		    case "connecting":
		      return ["Connecting\u2026", "down"];
		    case "error":
		      return ["Can't reach the agent", "down"];
		    default:
		      return [plainName(link.state), "down"];
		  }
		}
		function EventsBody() {
		  useStyle("rth-ui-base", BASE_CSS);
		  useStyle("rth-ui-events", EVENTS_CSS);
		  const { rows, link, source, stream, attaches, history } = useEventStream();
		  const [open, setOpen] = (0, import_react2.useState)(null);
		  const seen = (0, import_react2.useRef)(/* @__PURE__ */ new Map());
		  const first = (0, import_react2.useRef)(true);
		  for (const r of rows) if (!seen.current.has(r.key)) seen.current.set(r.key, first.current ? null : Date.now());
		  if (rows.length) first.current = false;
		  const shown = rows.slice(-SHOWN).reverse();
		  const [words, state] = linkWords(link, stream);
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-ui rth-ev", "data-rth": "events", children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("header", { className: "rth-ev-head", "data-state": state, "data-rth": "events-link", "data-attaches": attaches, children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Dot, { tone: state === "live" ? "ok" : "neutral" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("b", { "data-state": link?.state ?? "none", children: words }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-ev-meta rth-num", "data-rth": "events-count", children: [
		        rows.length,
		        " events"
		      ] })
		    ] }),
		    attaches > 1 ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-ev-note", children: "Reconnected. Events from while it was down are not shown here." }) : null,
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("ol", { className: "rth-ev-list", "data-rth": "events-rows", children: [
		      shown.map((row) => {
		        const [phase, tone] = PHASE[row.phase] ?? ["Other", "neutral"];
		        const at = seen.current.get(row.key);
		        const isOpen = open === row.key;
		        return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { type: "button", className: `rth-ev-row${at && Date.now() - at < 1500 ? " rth-ev-fresh" : ""}`, "aria-expanded": isOpen, onClick: () => setOpen(isOpen ? null : row.key), "data-rth": "event-row", "data-key": row.key, "data-name": row.name, "data-count": row.count, "data-known": String(row.known), children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Dot, { tone }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-ev-name", children: [
		            row.known ? plainName(row.name) : "Unrecognised event",
		            row.count > 1 ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-faint", children: [
		              " \xD7",
		              row.count
		            ] }) : null
		          ] }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-ev-time", children: at ? clockS(at) : "" }),
		          row.summary ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-ev-sum", children: redactText(row.summary) }) : null,
		          isOpen ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-ev-more rth-mono rth-faint", children: [
		            phase,
		            " \xB7 ",
		            row.wire,
		            row.known ? "" : " (not in the catalog)",
		            " \xB7 ",
		            row.key,
		            row.cause ? ` \xB7 caused by ${row.cause}` : "",
		            at ? ` \xB7 seen ${clockS(at)}` : " \xB7 seen before this tab opened"
		          ] }) : null
		        ] }) }, row.key);
		      }),
		      !shown.length ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { className: "rth-faint", style: { padding: 16, textAlign: "center" }, children: "No events yet." }) : null
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("footer", { className: "rth-ev-foot", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Fold, { id: "events:about", summary: "About this list", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-faint", style: { display: "flex", flexDirection: "column", gap: 6, fontSize: 12 }, children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { children: "Everything the agent sends to Unfold's read-only observer, newest first, named from the event catalog. Mind tells the same story in plain sentences." }),
		      source && !source.host_frames ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { children: "This is the live feed only: what the host sends is not shown, and nothing is replayed, so some causes stay unknown." }) : null,
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-mono", "data-rth": "events-stream", "data-state": stream, children: [
		        linkText(link, attaches),
		        " \xB7 tab stream ",
		        stream
		      ] }),
		      history.length ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { "data-rth": "events-link-history", className: "rth-mono", style: { margin: 0, paddingLeft: 16 }, children: history.map((h, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { children: [
		        h.at,
		        " ",
		        h.state,
		        h.detail == null ? "" : ` (${h.detail})`
		      ] }, i)) }) : null
		    ] }) }) })
		  ] });
		}
		function PresenceTitle({ useTabInfo }) {
		  const { tab } = useTabInfo();
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { "data-rth": "presence-title", children: tab.title });
		}
		var PRESENCE_LEASE_MS = 25e3;
		function usePresence() {
		  const [state, setState] = (0, import_react2.useState)({ view: null, stream: "opening" });
		  (0, import_react2.useEffect)(() => {
		    let source = null;
		    let lease = null;
		    let retry = null;
		    let backoff = 1e3;
		    const stale = () => {
		      source?.close();
		      setState((s) => ({ ...s, stream: "stale" }));
		      retry = setTimeout(open, backoff);
		      backoff = Math.min(backoff * 2, 8e3);
		    };
		    const alive = () => {
		      clearTimeout(lease);
		      lease = setTimeout(stale, PRESENCE_LEASE_MS);
		    };
		    function open() {
		      source = new EventSource(new URL(PRESENCE_STREAM, document.baseURI));
		      alive();
		      source.addEventListener("presence", (e) => {
		        alive();
		        backoff = 1e3;
		        setState({ view: JSON.parse(e.data), stream: "open" });
		      });
		      source.addEventListener("heartbeat", alive);
		      source.onerror = () => setState((s) => ({ ...s, stream: "reconnecting" }));
		    }
		    open();
		    return () => {
		      clearTimeout(lease);
		      clearTimeout(retry);
		      source?.close();
		    };
		  }, []);
		  return state;
		}
		var STALE = { pose: "unknown", label: "Unknown", because: "the presence stream is not open", layers: null, faults: [] };
		function useReducedMotion() {
		  const query = typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : null;
		  const [reduced, setReduced] = (0, import_react2.useState)(Boolean(query?.matches));
		  (0, import_react2.useEffect)(() => {
		    if (!query) return;
		    const on = (e) => setReduced(e.matches);
		    query.addEventListener("change", on);
		    return () => query.removeEventListener("change", on);
		  }, []);
		  return reduced;
		}
		function useVoice() {
		  const [, force] = (0, import_react2.useReducer)((n) => n + 1, 0);
		  const ctl = (0, import_react2.useRef)(null);
		  const make = () => createVoiceController({ fetch: (...a) => fetch(...a), onChange: force });
		  if (!ctl.current) ctl.current = make();
		  (0, import_react2.useEffect)(() => {
		    if (ctl.current.disposed) ctl.current = make();
		    const c = ctl.current;
		    let last = -Infinity;
		    let key = voiceKey(c.view());
		    const due = () => {
		      if (!shouldRead({ hidden: document.visibilityState === "hidden", sinceLast: performance.now() - last, delay: readDelay(c.view()) })) return;
		      last = performance.now();
		      void c.load();
		    };
		    const tick = setInterval(() => {
		      const k = voiceKey(c.view());
		      if (k !== key) {
		        key = k;
		        force();
		      }
		      due();
		    }, 500);
		    const onVisible = () => {
		      if (document.visibilityState !== "hidden") {
		        last = -Infinity;
		        due();
		      }
		    };
		    document.addEventListener("visibilitychange", onVisible);
		    DEBUG2.listeners += 1;
		    due();
		    return () => {
		      clearInterval(tick);
		      document.removeEventListener("visibilitychange", onVisible);
		      DEBUG2.listeners -= 1;
		      c.dispose();
		    };
		  }, []);
		  return ctl.current;
		}
		var DEBUG2 = globalThis.__rthAvatar ??= { live: 0, created: 0, frames: 0, rafs: 0, timeouts: 0, observers: 0 };
		DEBUG2.listeners ??= 0;
		function useVisible(ref) {
		  const [visible, setVisible] = (0, import_react2.useState)(true);
		  (0, import_react2.useEffect)(() => {
		    let page = document.visibilityState !== "hidden";
		    let onScreen = true;
		    const apply2 = () => setVisible(page && onScreen);
		    const onPage = () => {
		      page = document.visibilityState !== "hidden";
		      apply2();
		    };
		    document.addEventListener("visibilitychange", onPage);
		    DEBUG2.listeners += 1;
		    const io = typeof IntersectionObserver === "function" && ref.current ? new IntersectionObserver((entries) => {
		      onScreen = entries.some((e) => e.isIntersecting);
		      apply2();
		    }) : null;
		    if (io) {
		      io.observe(ref.current);
		      DEBUG2.observers += 1;
		    }
		    return () => {
		      document.removeEventListener("visibilitychange", onPage);
		      DEBUG2.listeners -= 1;
		      if (io) {
		        io.disconnect();
		        DEBUG2.observers -= 1;
		      }
		    };
		  }, []);
		  return visible;
		}
		var MicGlyph = ({ state }) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("svg", { width: "16", height: "16", viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: "1.9", strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": "true", children: [
		  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("rect", { x: "9", y: "2.5", width: "6", height: "11.5", rx: "3" }),
		  /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M18.5 10.5v.5a6.5 6.5 0 0 1-13 0v-.5M12 17.5V21" }),
		  state === "muted" || state === "unmuting" ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M3.5 3.5l17 17" }) : null
		] });
		var MIC_WORD = { muted: "Muted", muting: "Muting\u2026", unmuting: "Unmuting\u2026" };
		var PRESENCE_CSS = `
		.rth-pres{display:flex;flex-direction:column;align-items:center;gap:14px;padding:14px 16px 22px}
		/* No device frame: the creature floats in a luminous capsule in the tab itself.
		   The capsule is dark glass in both themes (the dust needs the dark); the words follow the theme. */
		.rth-stage{--glow:.12;--rim:98,230,242;position:relative;isolation:isolate;width:100%;max-width:300px;display:flex;flex-direction:column;align-items:stretch}
		.rth-stage[data-voice=active]{--glow:.34}
		.rth-stage[data-voice=starting]{--glow:.24}
		.rth-stage[data-voice=ending]{--glow:.16}
		.rth-stage[data-voice=unknown],.rth-stage[data-presence=unknown]{--glow:.05;--rim:150,156,170}
		.rth-bar{display:flex;align-items:center;justify-content:space-between;height:28px;padding:0 2px}
		.rth-name{display:flex;align-items:center;gap:7px;font-size:12px;font-weight:600;letter-spacing:.04em;color:var(--rth-muted)}
		.rth-mic{display:inline-flex;align-items:center;gap:5px;height:26px;min-width:26px;padding:0 7px;border-radius:13px;border:0;background:transparent;color:var(--rth-muted);cursor:pointer;font-size:11px;font-weight:600;transition:background .2s,color .2s}
		.rth-mic:hover{background:var(--rth-hover);color:var(--rth-fg)}
		.rth-mic[data-state=muted],.rth-mic[data-state=unmuting]{color:var(--rth-warn);background:color-mix(in oklab,var(--rth-warn) 13%,transparent)}
		.rth-mic[data-state=unknown]{color:var(--rth-faint)}
		.rth-mic[aria-disabled=true]{cursor:default}
		.rth-mic[aria-disabled=true]:hover{background:transparent}
		.rth-tube{position:relative;display:block;width:76%;margin:10px auto 0;aspect-ratio:3/4.7;padding:0;border:0;border-radius:50%/19%;cursor:pointer;background:#04040a;
		 box-shadow:inset 0 0 0 1px rgba(var(--rim),.26),inset 0 0 34px rgba(var(--rim),calc(var(--glow)*.8)),0 0 38px rgba(var(--rim),calc(var(--glow)*.55));transition:box-shadow .8s var(--rth-ease),transform .25s var(--rth-ease)}
		.rth-tube:hover{transform:translateY(-1px)}
		.rth-tube:active{transform:scale(.985)}
		.rth-tube[aria-disabled=true]{cursor:default;transform:none}
		.rth-tube::after{content:"";position:absolute;inset:0;border-radius:inherit;pointer-events:none;opacity:0;box-shadow:0 0 54px rgba(var(--rim),.30);transition:opacity .8s}
		.rth-stage[data-alive=true] .rth-tube::after{animation:rth-breathe 5s ease-in-out infinite}
		@keyframes rth-breathe{0%,100%{opacity:.15}50%{opacity:.6}}
		.rth-tube canvas{position:absolute;inset:5% 6%;width:88%;height:90%;border-radius:50%/18%;display:block}
		.rth-pres .rth-pres-cap{position:absolute;left:22%;right:22%;height:8px;border-radius:50%;pointer-events:none;background:radial-gradient(50% 50% at 50% 50%,rgba(var(--rim),calc(.22 + var(--glow))),rgba(var(--rim),0));transition:opacity .8s}
		.rth-pres .rth-pres-cap.top{top:.8%}.rth-pres .rth-pres-cap.bot{bottom:.8%}
		.rth-sheen{position:absolute;inset:0;border-radius:inherit;pointer-events:none;background:linear-gradient(90deg,rgba(var(--rim),.10) 0%,rgba(255,255,255,0) 18%,rgba(255,255,255,.045) 30%,rgba(255,255,255,0) 42%,rgba(255,255,255,0) 78%,rgba(var(--rim),.08) 100%)}
		.rth-plinth{width:50%;height:10px;margin:7px auto 0;border-radius:0 0 50% 50%/0 0 100% 100%;background:linear-gradient(#1b1c27,#0b0b11);box-shadow:inset 0 0 0 1px rgba(255,255,255,.05),0 8px 18px rgba(var(--rim),calc(var(--glow)*.25))}
		.rth-tube{outline-offset:4px}
		.rth-stage::before{content:"";position:absolute;left:8%;right:8%;top:6%;height:72%;border-radius:50%;pointer-events:none;background:radial-gradient(50% 50% at 50% 50%,rgba(var(--rim),calc(var(--glow)*.35)),rgba(var(--rim),0) 70%);filter:blur(18px);z-index:-1;transition:opacity .8s}
		.rth-say{min-height:76px;margin-top:10px;padding:0 6px;text-align:center}
		.rth-word{display:block;margin:10px auto 0;width:max-content;padding:2px 9px;border-radius:9px;font-size:10.5px;font-weight:650;letter-spacing:.09em;text-transform:uppercase;color:var(--rth-muted);background:var(--rth-soft);border:1px solid var(--rth-line)}
		.rth-word[data-state=replying],.rth-word[data-state=working],.rth-word[data-state=speaking]{color:var(--rth-accent);border-color:color-mix(in oklab,var(--rth-accent) 35%,transparent)}
		.rth-word[data-state=unknown]{border-style:dashed}
		.rth-cp-real{display:inline-flex;align-items:center;gap:5px;border-style:solid}
		.rth-cp-sw{width:7px;height:7px;border-radius:50%;flex:none}
		.rth-cp-bar{position:relative;width:26px;height:3px;border-radius:2px;background:var(--rth-line);overflow:hidden}
		.rth-cp-bar i{position:absolute;left:0;top:0;bottom:0;background:var(--rth-accent)}
		.rth-tiers{display:inline-flex;gap:3px;vertical-align:middle}
		.rth-tiers i{width:9px;height:9px;border-radius:50%}
		.rth-history{list-style:none;margin:0 0 6px;padding:0;display:flex;flex-direction:column;gap:6px;font-size:12px}
		.rth-cp{font-size:10px;font-weight:600;letter-spacing:.04em;color:var(--rth-faint);padding:1px 6px;border:1px dashed var(--rth-line);border-radius:7px;margin-left:6px}
		/* The production queue: one square slot per thing running, docked to the capsule's right. */
		.rth-queue{position:absolute;right:-2px;top:52px;display:flex;flex-direction:column;gap:6px;z-index:2}
		.rth-slot{position:relative;width:30px;height:36px;padding:0;border-radius:8px;border:1px solid rgba(98,230,242,.35);background:#06070d;color:#bfeff5;cursor:pointer;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:1px;box-shadow:0 0 10px rgba(98,230,242,.12);transition:opacity .9s var(--rth-ease),transform .9s var(--rth-ease),border-color .3s}
		.rth-slot svg{width:14px;height:14px}
		.rth-slot-t{font-size:8.5px;font-variant-numeric:tabular-nums;line-height:1;color:#8fb8c2}
		.rth-slot-bar{position:absolute;left:3px;right:3px;bottom:2px;height:2px;border-radius:1px;overflow:hidden;background:rgba(98,230,242,.12)}
		.rth-slot-bar i{position:absolute;top:0;bottom:0;width:40%;left:0;background:linear-gradient(90deg,transparent,rgba(98,230,242,.9),transparent);animation:rth-shimmer 1.6s linear infinite}
		@keyframes rth-shimmer{from{transform:translateX(-100%)}to{transform:translateX(250%)}}
		.rth-slot[data-phase=ended]{opacity:0;transform:scale(.85)}
		.rth-slot[data-phase=ended] .rth-slot-bar i{animation:none}
		.rth-slot[data-phase=lost],.rth-slot[data-unsure=true]{border-style:dashed;border-color:rgba(255,197,110,.55);color:#ffc56e}
		.rth-slot[data-phase=lost] .rth-slot-bar,.rth-slot[data-unsure=true] .rth-slot-bar i{display:none}
		.rth-slot-more{width:30px;text-align:center;font-size:10px;font-weight:650;color:var(--rth-muted)}
		.rth-slot-pop{position:absolute;right:38px;top:0;width:max-content;max-width:190px;padding:6px 9px;border-radius:8px;background:light-dark(#fff,#0f2129);color:var(--rth-fg);border:1px solid var(--rth-line);font-size:11.5px;line-height:1.35;box-shadow:0 8px 22px -10px rgba(0,0,0,.45);text-align:left;cursor:default}
		.rth-slot-pop button{all:unset;cursor:pointer;color:var(--rth-bad);font-size:11px;margin-top:3px;display:inline-block}
		.rth-slot-pop button:focus-visible{outline:2px solid var(--rth-accent)}
		.rth-head{font-size:17px;font-weight:620;letter-spacing:-.005em;color:var(--rth-fg);transition:color .3s}
		.rth-stage[data-voice=unknown] .rth-head{color:var(--rth-muted)}
		.rth-line{margin-top:4px;font-size:12.5px;color:var(--rth-muted);min-height:19px;transition:opacity .3s}
		.rth-line[data-tone=accent]{color:var(--rth-accent)}
		.rth-line[data-tone=warn]{color:var(--rth-warn)}
		.rth-line[data-tone=bad]{color:var(--rth-bad)}
		.rth-hint{margin-top:8px;font-size:11.5px;color:var(--rth-faint);opacity:0;transition:opacity .3s}
		.rth-stage[data-voice=off] .rth-hint,.rth-stage[data-voice=error] .rth-hint,.rth-stage:hover .rth-hint,.rth-stage:focus-within .rth-hint{opacity:1}
		.rth-pres-more{width:100%;max-width:300px}
		.rth-pres-more .rth-fold{border-top:0}
		.rth-pres-more .rth-fold .rth-fold{border-top:1px solid var(--rth-line);margin-top:10px}
		.rth-tech{display:flex;flex-direction:column;gap:6px}
		.rth-tech ul{margin:0;padding-left:16px}
		`;
		function useGrowth() {
		  const [g, setG] = (0, import_react2.useState)(null);
		  (0, import_react2.useEffect)(() => {
		    let alive = true;
		    const read = async () => {
		      try {
		        const r = await fetch("api/rth-presence/growth", { cache: "no-store", signal: AbortSignal.timeout(4e3) });
		        const body = r.ok ? await r.json() : { unavailable: `route ${r.status}` };
		        if (alive) setG(body);
		      } catch (e) {
		        if (alive) setG({ unavailable: String(e?.message ?? e) });
		      }
		    };
		    void read();
		    const t = setInterval(read, 6e4);
		    return () => {
		      alive = false;
		      clearInterval(t);
		    };
		  }, []);
		  return g;
		}
		var SWATCH = { cyan: "#5fe3ef", magenta: "#ff5fd2", violet: "#a98bff", amber: "#ffc56e", rose: "#ff8fa8", mint: "#7ff0c0", "ice-blue": "#b8e6ff", coral: "#ff8f6a", lime: "#c8f06e", "white-gold": "#ffe9b8" };
		function GrowthChip({ g }) {
		  if (!g) return null;
		  if (g.unavailable || g.state === "unavailable") return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-cp", "data-rth": "avatar-cp", "data-state": "unavailable", title: `growth not available: ${g.unavailable ?? g.why}`, children: "growth unavailable" });
		  if (g.state !== "approved") return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-cp", "data-rth": "avatar-cp", "data-state": "unapproved", title: "Cosmic points start counting once the owner approves the policy", children: "growth: awaiting approval" });
		  const pct = (10 - g.nextLevelIn) / 10 * 100;
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-cp rth-cp-real", "data-rth": "avatar-cp", title: `${g.cp} cosmic points from ${g.entries.length} accepted build(s); next level in ${g.nextLevelIn} CP`, children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-cp-sw", style: { background: SWATCH[g.palette.at(-1)] }, "aria-hidden": "true" }),
		    g.stage,
		    g.availableStage ? ` (${g.availableStage} available)` : "",
		    " \xB7 L",
		    g.level,
		    " \xB7 ",
		    g.cp,
		    " CP",
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-cp-bar", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("i", { style: { width: `${pct}%` } }) })
		  ] });
		}
		function Growth({ g }) {
		  if (!g) return /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-faint", children: "Reading\u2026" });
		  if (g.unavailable || g.state === "unavailable") return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-faint", "data-rth": "avatar-growth", "data-state": "unavailable", children: [
		    "Growth can't be read right now (",
		    g.unavailable ?? g.why,
		    "). Nothing is assumed."
		  ] });
		  if (g.state !== "approved") return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-faint", "data-rth": "avatar-growth", "data-state": "unapproved", children: [
		    "Cosmic points aren't counting yet: the policy and its first entry wait for the owner's approval (",
		    g.source,
		    "). Until then RTH shows no level or points."
		  ] });
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-growth", "data-rth": "avatar-growth", children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dl", { className: "rth-facts", children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Stage" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dd", { children: [
		        g.stage,
		        g.availableStage ? ` \xB7 ${g.availableStage} is available (evolving is an explicit choice)` : g.nextStage ? ` (next: ${g.nextStage.name} at ${g.nextStage.cp} CP)` : ""
		      ] }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Level" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dd", { children: [
		        g.level,
		        " \xB7 next in ",
		        g.nextLevelIn,
		        " CP (one accepted build)"
		      ] }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Cosmic points" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dd", { children: [
		        g.cp,
		        " CP = ",
		        g.grains.toLocaleString("en-US"),
		        " grains of dust"
		      ] }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: "Colours" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("dd", { children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-tiers", children: g.palette.map((c) => /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("i", { title: c, style: { background: SWATCH[c] } }, c)) }),
		        " ",
		        g.colours,
		        " of 10",
		        g.nextColourAtLevel ? ` \xB7 next at level ${g.nextColourAtLevel}` : ""
		      ] })
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-faint", style: { margin: "8px 0 4px" }, children: "What earned them (+10 per activated build; it starts at 100)" }),
		    g.entries.length ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("ul", { className: "rth-history", children: g.entries.map((e, i) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("li", { children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-num", children: e.at.slice(0, 10) }),
		      " \xB7 +",
		      e.cp,
		      " \xB7 ",
		      e.build,
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-mono rth-faint", children: e.evidence.join(" \xB7 ") })
		    ] }, i)) }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-faint", children: "No accepted build yet." }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-mono rth-faint", children: [
		      "source: ",
		      g.source,
		      " \xB7 policy ",
		      g.policy?.id,
		      " v",
		      g.policy?.version,
		      g.notes?.length ? ` \xB7 ${g.notes.length} ledger note(s)` : "",
		      g.incompleteTail ? " \xB7 an incomplete last line was ignored" : ""
		    ] })
		  ] });
		}
		var SlotIcon = ({ kind, phase }) => phase === "lost" ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("svg", { viewBox: "0 0 16 16", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("text", { x: "8", y: "12", textAnchor: "middle", fontSize: "12", fontWeight: "700", fill: "currentColor", children: "?" }) }) : kind === "reply" ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("svg", { viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: "1.5", strokeLinecap: "round", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M3 4.5h10M3 8h7M3 11.5h4.5" }) }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("svg", { viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: "1.5", strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("path", { d: "M8 2.5v7M5 6.5l3 3 3-3M3 12.5h10" }) });
		function useQueue(view) {
		  const q = (0, import_react2.useRef)({ slots: [] });
		  const seen = (0, import_react2.useRef)(null);
		  if (seen.current !== view) {
		    q.current = stepQueue(q.current, view, Date.now());
		    seen.current = view;
		  }
		  const [, tick] = (0, import_react2.useReducer)((n) => n + 1, 0);
		  const running = q.current.slots.some((x) => x.phase === "running");
		  const settle = nextQueueChange(q.current);
		  (0, import_react2.useEffect)(() => {
		    const timers = [];
		    if (running) timers.push(setInterval(tick, 1e3));
		    if (settle) timers.push(setTimeout(() => {
		      q.current = stepQueue(q.current, seen.current, Date.now());
		      tick();
		    }, Math.max(0, settle - Date.now()) + 20));
		    return () => timers.forEach((t) => clearInterval(t));
		  }, [running, settle]);
		  return q.current;
		}
		function Queue({ queue }) {
		  const [open, setOpen] = (0, import_react2.useState)(null);
		  const now = Date.now();
		  const slots = queue.slots;
		  if (!slots.length) return null;
		  const shown = slots.slice(0, MAX_SLOTS);
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-queue", "data-rth": "queue", role: "list", "aria-label": "Running now", children: [
		    shown.map((s) => {
		      const line2 = slotLine(s, now);
		      const age = Math.max(0, Math.round((now - s.since) / 1e3));
		      return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { role: "listitem", style: { position: "relative" }, children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("button", { type: "button", className: "rth-slot", "data-rth": "queue-slot", "data-kind": s.kind, "data-phase": s.phase, "data-unsure": String(Boolean(s.unsure)), "aria-label": line2, "aria-expanded": open === s.id, onClick: () => setOpen(open === s.id ? null : s.id), children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(SlotIcon, { kind: s.kind, phase: s.phase }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-slot-t", children: s.phase === "lost" ? "" : age < 60 ? `${age}s` : `${Math.round(age / 60)}m` }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-slot-bar", "aria-hidden": "true", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("i", {}) })
		        ] }),
		        open === s.id ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-slot-pop", "data-rth": "queue-pop", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { children: line2 }) }) : null
		      ] }, s.id);
		    }),
		    slots.length > MAX_SLOTS ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-slot-more", "data-rth": "queue-more", children: [
		      "+",
		      slots.length - MAX_SLOTS
		    ] }) : null
		  ] });
		}
		function TechLive({ voiceCtl, renderer }) {
		  useNow(1e3);
		  const v = voiceCtl.view();
		  const st = renderer.current?.stats?.() ?? null;
		  const age = v.readAt == null ? "none yet" : `${((performance.now() - v.readAt) / 1e3).toFixed(1)} s ago`;
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-mono", "data-rth": "avatar-live", children: [
		    "page ",
		    typeof document === "undefined" ? "?" : document.visibilityState,
		    " \xB7 voice receipt read ",
		    age,
		    " \xB7 renderer ",
		    st ? `${st.renderer}, ${st.frames} frames, ${st.running ? `running at ${st.fps} fps` : "paused"}` : "none"
		  ] });
		}
		function receiptLine(voice) {
		  if (voice.actual === "unknown") return `rth-host receipt: none fresh${voice.stale ? " (stale)" : ""}`;
		  return `rth-host receipt: actual ${voice.actual} \xB7 desired ${voice.desired ?? "\u2014"}`;
		}
		function PresenceBody() {
		  useStyle("rth-ui-base", BASE_CSS);
		  useStyle("rth-ui-presence", PRESENCE_CSS);
		  const { view: live, stream } = usePresence();
		  const view = stream === "open" && live ? live : STALE;
		  const reduced = useReducedMotion();
		  const voiceCtl = useVoice();
		  const voice = { ...voiceCtl.view() };
		  const scene = sceneFor(view, voice);
		  const words = presenceWords(view, voice);
		  const queue = useQueue(view);
		  const growthView = useGrowth();
		  const approved = growthView?.state === "approved";
		  const cp = approved ? growthView.cp : CP_PREVIEW.cp;
		  const tierColour = approved ? growthView.palette.at(-1) : "cyan";
		  const state = stateOf(view, voice, queue);
		  const lang = langOf(globalThis.document?.documentElement?.lang || globalThis.navigator?.language);
		  const statusId = (0, import_react2.useId)();
		  const canvas = (0, import_react2.useRef)(null);
		  const tube = (0, import_react2.useRef)(null);
		  const renderer = (0, import_react2.useRef)(null);
		  const visible = useVisible(tube);
		  const visibleRef = (0, import_react2.useRef)(visible);
		  visibleRef.current = visible;
		  const sceneRef = (0, import_react2.useRef)(scene);
		  sceneRef.current = scene;
		  (0, import_react2.useEffect)(() => {
		    const r = canvas.current.getBoundingClientRect();
		    const { grains } = grainBudget(r.width, r.height, cp);
		    renderer.current = createRenderer(canvas.current, { reducedMotion: reduced, grains, colour: tierColour });
		    renderer.current.setVisible?.(visibleRef.current);
		    renderer.current.draw(sceneRef.current);
		    return () => renderer.current?.dispose();
		  }, [cp, tierColour]);
		  (0, import_react2.useEffect)(() => {
		    const byes = globalThis.__rthGoodbyes ??= /* @__PURE__ */ new Set();
		    const bye = (ms) => renderer.current?.goodbye?.(ms) ?? false;
		    byes.add(bye);
		    globalThis.__rthGoodbye ??= (ms) => [...globalThis.__rthGoodbyes].filter((f) => f(ms)).length;
		    return () => byes.delete(bye);
		  }, []);
		  (0, import_react2.useEffect)(() => renderer.current?.setReducedMotion(reduced), [reduced]);
		  (0, import_react2.useEffect)(() => renderer.current?.setVisible?.(visible), [visible]);
		  (0, import_react2.useEffect)(() => renderer.current?.draw(scene), [scene.key]);
		  const layers = view.layers;
		  const fault = view.faults?.at(-1);
		  const tap = () => {
		    if (voice.action) void voiceCtl.toggle();
		  };
		  const mute = () => {
		    if (voice.mute.action !== null) void voiceCtl.mute();
		  };
		  const line2 = voice.settling ? { line: voice.settling, tone: "accent" } : voice.note ? { line: voice.note, tone: "bad" } : voice.reason ? { line: voice.reason, tone: "warn" } : (state === "replying" || state === "working") && words?.tone === "accent" ? null : words;
		  return /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-ui rth-pres", "data-rth": "presence", "data-pose": view.pose, "data-scene": scene.form, "data-voice-actual": voice.actual, "data-voice-desired": voice.desired ?? "", children: [
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-stage", "data-rth": "avatar-frame", "data-voice": voice.read ? voice.actual : "unknown", "data-presence": view.pose === "unknown" ? "unknown" : "known", "data-alive": String(lifeFor(view, voice)), children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-bar", children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("span", { className: "rth-name", children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Dot, { tone: layers?.link?.state === "live" ? "ok" : "neutral" }),
		          " Unfold",
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(GrowthChip, { g: growthView })
		        ] }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
		          "button",
		          {
		            type: "button",
		            className: "rth-mic",
		            "data-rth": "avatar-mute",
		            "data-state": voice.mute.state,
		            "aria-label": voice.mute.label,
		            "aria-pressed": voice.mute.muted === null ? void 0 : voice.mute.muted,
		            "aria-disabled": voice.mute.action === null,
		            title: voice.mute.label,
		            onClick: mute,
		            children: [
		              /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(MicGlyph, { state: voice.mute.state }),
		              MIC_WORD[voice.mute.state] ?? null
		            ]
		          }
		        )
		      ] }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(
		        "button",
		        {
		          type: "button",
		          ref: tube,
		          className: "rth-tube",
		          "data-rth": "avatar",
		          "data-voice": voice.actual,
		          "aria-label": voice.label,
		          "aria-describedby": statusId,
		          "aria-disabled": !voice.action,
		          onClick: tap,
		          children: [
		            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-pres-cap top", "aria-hidden": "true" }),
		            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("canvas", { ref: canvas, "data-rth": "presence-canvas", "aria-hidden": "true" }),
		            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-sheen", "aria-hidden": "true" }),
		            /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-pres-cap bot", "aria-hidden": "true" })
		          ]
		        }
		      ),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-plinth", "aria-hidden": "true" }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Queue, { queue }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("span", { className: "rth-word", "data-rth": "avatar-state", "data-state": state, children: WORDS[lang][state] }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-say", id: statusId, children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-head", "data-rth": "avatar-voice", children: voice.status }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-line", "data-rth": "avatar-line", "data-tone": line2?.tone ?? "faint", role: voice.settling || voice.note ? "status" : void 0, children: line2?.line ?? "" }),
		        voice.hint ? /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-hint", "data-rth": "avatar-hint", children: voice.hint }) : null
		      ] })
		    ] }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { "aria-live": "polite", className: "rth-sr", "data-rth": "presence-faults", style: { position: "absolute", width: 1, height: 1, overflow: "hidden", clipPath: "inset(50%)" }, children: fault ? `Problem at ${clock(fault.at)}: ${fault.summary}` : "" }),
		    /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { className: "rth-pres-more", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(Fold, { id: "presence:details", summary: "Details", "data-rth": "presence-layers", children: [
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dl", { className: "rth-facts", children: presenceFacts(view, voice).map(([k, v]) => /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)(import_react2.Fragment, { children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dt", { children: k }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("dd", { children: v })
		      ] }, k)) }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Fold, { id: "presence:growth", summary: "Growth", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Growth, { g: growthView }) }),
		      /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(Fold, { id: "presence:technical", summary: "Technical details", children: /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-tech rth-muted", children: [
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-mono", "data-rth": "avatar-receipt", children: [
		          receiptLine(voice),
		          voice.title ? ` \xB7 ${voice.title}` : ""
		        ] }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-mono", "data-rth": "presence-because", children: [
		          "creature: ",
		          view.label,
		          " (",
		          view.pose,
		          ") because ",
		          view.because,
		          "; scene ",
		          scene.form,
		          scene.face ? `/${scene.face}` : ""
		        ] }),
		        layers ? /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("ul", { className: "rth-mono", children: [
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: layers.link.label }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: layers.request.label }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { children: layers.audio.label }),
		          /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("li", { "data-rth": "presence-mic", "data-state": layers.mic?.state ?? "unknown", children: layers.mic?.label ?? "mic: unknown" })
		        ] }) : /* @__PURE__ */ (0, import_jsx_runtime2.jsxs)("div", { className: "rth-mono", children: [
		          "presence stream: ",
		          stream,
		          " (everything is unknown)"
		        ] }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)(TechLive, { voiceCtl, renderer }),
		        /* @__PURE__ */ (0, import_jsx_runtime2.jsx)("div", { children: "Voice asleep releases only the mic and speaker: RTH, its link and the conversation stay on. \u201CSpeaking\u201D is drawn only from audio rth-host reports handing to the device. A message waiting for a reply is not RTH thinking. There is no listening face: nothing on this feed says someone is speaking to RTH." })
		      ] }) })
		    ] }) })
		  ] });
		}
		return module.exports;
	}
});
