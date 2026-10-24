/*!
 * @rth/dsh-rth-trajectory: a fork of @deepseek-ai/dsh-client-ui-trajectory 0.2.1-rc.2
 * (upstream lib/client.js sha256 b11426f89d3b4302964b491d7826a29fe3de390be4bbfae325375f54fe6919ad).
 * Modified by we unfold; the changed regions are listed in NOTICE.md. Upstream license:
 *
 * MIT License
 *
 * Copyright (c) 2026 DeepSeek
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 * Badge icons: element data from lucide-static 0.460.0 (sha512-X6pIdg7jVxv7YQ/uR241hwhNiztcAfmj181TbcX7HCxxk/3mGaRtAc6b2ftUvQBufbJE6ehgyzO2uVsa604tWg==),
 * pinned in src/icons.generated.mjs. Lucide license:
 *
 * ISC License
 *
 * Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * Portions of Lucide are Feather (feather-icons 4.29.2, sha512-0TaCFTnBTVCz6U+baY2UJNKne5ifGh7sMG4ZC2LoBWCZdIyPa+y6UiR4lEYGws1JOFWdee8KAsAIvu0VcXqiqA==). Feather license:
 *
 * The MIT License (MIT)
 *
 * Copyright (c) 2013-2023 Cole Bemis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */
window.__ModuleLoader__.load({
	id: "@rth/dsh-rth-trajectory",
	factory: (require) => {
		var module = { exports: {} };
		var exports = module.exports;
		Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
		let _deepseek_ai_dsh_client_store = require("@deepseek-ai/dsh-client-store");
		let react_jsx_runtime = require("react/jsx-runtime");
		let react = require("react");
		let _tanstack_react_virtual = require("@tanstack/react-virtual");
		let _deepseek_ai_dsh_client_ui_primitives = require("@deepseek-ai/dsh-client-ui-primitives");
		const RTH = (function (require) {
			var module = { exports: {} };
			var exports = module.exports;
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

// src/runtime.mjs
var runtime_exports = {};
__export(runtime_exports, {
  BETWEEN: () => BETWEEN,
  CSS: () => CSS,
  ClockOverview: () => ClockOverview,
  DEFAULT_GROUPS: () => DEFAULT_GROUPS,
  EN_KEYS: () => EN_KEYS,
  GROUPS: () => GROUPS,
  GROUP_COLOR: () => GROUP_COLOR,
  Icon: () => Icon,
  KINDS: () => KINDS,
  LANES: () => LANES,
  RTH_SESSION: () => RTH_SESSION,
  StripRow: () => StripRow,
  ZH_KEYS: () => ZH_KEYS,
  buildRthTrajectory: () => buildRthTrajectory,
  cardChips: () => cardChips,
  cardLabel: () => cardLabel,
  ensureStyle: () => ensureStyle,
  filterTurns: () => filterTurns,
  inspectorRows: () => inspectorRows,
  kindLabel: () => kindLabel,
  kindTag: () => kindTag,
  layoutClockOverview: () => layoutClockOverview,
  makeHeaderButton: () => makeHeaderButton,
  newSince: () => newSince,
  setCards: () => setCards,
  sourceFor: () => sourceFor,
  stateOf: () => stateOf,
  timelineDetail: () => timelineDetail
});
module.exports = __toCommonJS(runtime_exports);
var import_jsx_runtime = require("react/jsx-runtime");

// ../dsh-mind/src/moments.mjs
function quoted(sentence) {
  if (typeof sentence !== "string") return null;
  const a = sentence.indexOf("\u201C");
  const b = sentence.lastIndexOf("\u201D");
  return a >= 0 && b > a ? sentence.slice(a + 1, b) : null;
}
function decidedTo(sentence) {
  const m = /^Decided to (\S+)/.exec(sentence ?? "");
  return m ? m[1] : null;
}

// ../dsh-mind/src/metrics.mjs
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
function span(name, start, end, clock, why = null) {
  const base = { name, start: start ? { kind: start.kind, row: start.id } : null, end: end ? { kind: end.kind, row: end.id } : null, clock };
  if (why) return { ...base, ms: null, why };
  if (!start || !end) return { ...base, ms: null, why: !start ? "its start was not observed" : "its end was not observed" };
  if (clock === "host") {
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
  const chain = (name, start, end, clock, whyChain, idName) => {
    if (whyChain) return span(name, start.row ?? start.other, end.row ?? end.other, clock, whyChain);
    if (!start.row) return span(name, null, end.row, clock, start.other ? `its start comes from another ${idName}` : "its start was not observed");
    if (!end.row) return span(name, start.row, null, clock, end.other ? `its end comes from another ${idName} than its start` : "its end was not observed");
    return span(name, start.row, end.row, clock);
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

// src/taxonomy.mjs
var LANES = Object.freeze(["Hear", "Think", "Speak", "Act", "Life"]);
var LANE = Object.freeze({ HEAR: 0, THINK: 1, SPEAK: 2, ACT: 3, LIFE: 4 });
var GROUPS = Object.freeze([
  { id: "you", label: "You" },
  { id: "rth", label: "Unfold" },
  { id: "voice", label: "Voice" },
  { id: "control", label: "Control" },
  { id: "signals", label: "Signals" },
  { id: "faults", label: "Faults & gaps" }
]);
var KINDS = Object.freeze({
  voice: { label: "YOU \xB7 VOICE", short: "YOU", icon: "mic", color: ["#2563eb", "#60a5fa"], lane: LANE.HEAR, group: "you", upstream: "user" },
  typed: { label: "YOU \xB7 TYPED", short: "YOU", icon: "keyboard", color: ["#2563eb", "#60a5fa"], outlined: true, lane: LANE.HEAR, group: "you", upstream: "user" },
  vad: { label: "VAD", short: "VAD", icon: "audio-waveform", color: ["#0891b2", "#22d3ee"], lane: LANE.HEAR, group: "voice", upstream: "context" },
  rth: { label: "Unfold", short: "Unfold", icon: "sparkles", color: ["#7c3aed", "#a78bfa"], lane: LANE.THINK, group: "rth", upstream: "message" },
  speaking: { label: "SPEAKING", short: "SPEAKING", icon: "volume-2", color: ["#c026d3", "#e879f9"], lane: LANE.SPEAK, group: "voice", upstream: "context" },
  // Audio the agent generated or received: not a host playback report.
  audio: { label: "AUDIO OUT (generated)", short: "AUDIO", icon: "audio-lines", color: ["#a21caf", "#d8b4fe"], outlined: true, lane: LANE.SPEAK, group: "voice", upstream: "context" },
  interrupt: { label: "INTERRUPT", short: "INTERRUPT", icon: "zap", color: ["#ea580c", "#fb923c"], lane: LANE.SPEAK, group: "voice", upstream: "context" },
  echo: { label: "ECHO GUARD", short: "ECHO", icon: "shield", color: ["#0d9488", "#2dd4bf"], lane: LANE.HEAR, group: "signals", upstream: "context" },
  decision: { label: "DECISION", short: "DECISION", icon: "git-branch", color: ["#b45309", "#fbbf24"], lane: LANE.ACT, group: "rth", upstream: "context" },
  context: { label: "CONTEXT", short: "CONTEXT", icon: "book-open", color: ["#15803d", "#4ade80"], lane: LANE.ACT, group: "rth", upstream: "context" },
  command: { label: "COMMAND", short: "COMMAND", icon: "terminal", color: ["#4f46e5", "#818cf8"], lane: LANE.ACT, group: "control", upstream: "context" },
  mic: { label: "MIC / CALL", short: "MIC", icon: "mic-off", color: ["#475569", "#94a3b8"], lane: LANE.LIFE, group: "control", upstream: "context" },
  call: { label: "MIC / CALL", short: "CALL", icon: "phone", color: ["#475569", "#94a3b8"], lane: LANE.LIFE, group: "control", upstream: "context" },
  link: { label: "LINK", short: "LINK", icon: "plug", color: ["#6b7280", "#9ca3af"], lane: LANE.LIFE, group: "control", upstream: "context" },
  signal: { label: "SIGNAL", short: "SIGNAL", icon: "radio", color: ["#64748b", "#94a3b8"], quiet: true, lane: LANE.LIFE, group: "signals", upstream: "context" },
  fault: { label: "FAULT", short: "FAULT", icon: "triangle-alert", color: ["#dc2626", "#f87171"], lane: LANE.LIFE, group: "faults", upstream: "context" },
  gap: { label: "GAP", short: "GAP", icon: "unlink", color: ["#ca8a04", "#facc15"], dashed: true, lane: LANE.LIFE, group: "faults", upstream: "context" },
  media: { label: "MEDIA", short: "MEDIA", icon: "image", color: ["#6b7280", "#9ca3af"], lane: LANE.LIFE, group: "control", upstream: "context" }
});
var REPLY_PARTS = /* @__PURE__ */ new Set(["response_started", "agent_delta", "agent_done", "response_done", "reply_text", "command_finished"]);
var TRANSCRIPT = /* @__PURE__ */ new Set(["input_transcript"]);
function kindOf(row) {
  const k = row.kind;
  const lane = row.lane;
  if (lane === "mind") {
    if (/^journal_|seq_gap|link_lost|link_resumed|preload|panel_stream_lost/.test(k) || row.status === "gap") return "gap";
    return "signal";
  }
  if (REPLY_PARTS.has(k) || TRANSCRIPT.has(k)) return null;
  switch (k) {
    case "user_text":
    case "command_submitted":
    case "message_submitted":
      return "typed";
    case "local_vad_onset":
    case "user_audio":
    case "user_audio_end":
    case "audio_ingress":
    case "input_audio_start":
    case "input_audio_end":
      return "vad";
    case "playback":
    case "message_playback_reported":
      return "speaking";
    case "agent_audio":
    case "message_audio_added":
      return "audio";
    case "response_cancelled":
    case "cancel":
    case "cancel_written":
    case "speaker_cleared":
    case "message_stop_requested":
    case "stop_requested":
      return "interrupt";
    case "echo_gate":
    case "echo_probe":
      return "echo";
    case "decision":
    case "provider_turn_signaled":
      return "decision";
    case "context_restore":
    case "model_context_checked":
    case "note_added":
      return "context";
    case "command_written":
    case "command_admitted":
      return "command";
    case "mute_request":
    case "mute_applied":
      return "mic";
    case "call_request":
    case "call_devices_opened":
    case "call_devices_released":
    case "call":
      return "call";
    case "started":
    case "link_up":
    case "link_down":
    case "hello":
    case "attach":
    case "ready":
    case "quit_requested":
    case "quit":
      return "link";
    case "provider_signal":
    case "heartbeat":
    case "agent_status_reported":
      return "signal";
    case "rejected":
    case "journal_fault":
    case "request_rejected":
    case "history_save_failed":
      return "fault";
    case "media_stored":
      return "media";
    case "log":
      return row.ref?.level === "error" || row.ref?.level === "warn" ? "fault" : "signal";
    default:
      return row.status === "failed" ? "fault" : "signal";
  }
}

// src/adapter.mjs
var ref2 = (r) => r?.ref ?? {};
var finite = (v) => typeof v === "number" && Number.isFinite(v);
var hostT2 = (r) => finite(r?.clock?.t_ms) ? r.clock.t_ms : null;
var agentT2 = (r) => finite(r?.clock?.t_ns) ? r.clock.t_ns / 1e6 : null;
function pairSpan(start, end) {
  if (!start || !end) return { ms: null, why: !start ? "its start was not observed" : "its end was not observed", clock: null };
  if (start.lane !== end.lane) return { ms: null, why: "start and end come from different producers (clocks are never mixed)", clock: null };
  if (start.lane === "host") {
    const a = ref2(start).host_life;
    const b = ref2(end).host_life;
    if (a == null || b == null) return { ms: null, why: "host process not established (no started record observed for it)", clock: null };
    if (a !== b) return { ms: null, why: "start and end come from different rth-host processes", clock: null };
    const s = hostT2(start);
    const e = hostT2(end);
    if (s == null || e == null) return { ms: null, why: "a time was not reported", clock: null };
    return e >= s ? { ms: e - s, clock: "rth-host" } : { ms: null, why: "clock went backwards", clock: null };
  }
  if (start.lane === "agentd") {
    if (!ref2(start).producer || ref2(start).producer !== ref2(end).producer) return { ms: null, why: "start and end come from different agent boots", clock: null };
    const s = agentT2(start);
    const e = agentT2(end);
    if (s == null || e == null) return { ms: null, why: "a time was not reported", clock: null };
    return e >= s ? { ms: e - s, clock: "agent" } : { ms: null, why: "clock went backwards", clock: null };
  }
  return { ms: null, why: "no producer clock", clock: null };
}
var fmt = (ms) => ms == null ? null : ms < 1e3 ? `${Math.round(ms)} ms` : `${(ms / 1e3).toFixed(ms < 1e4 ? 2 : 1)} s`;
function textOf(row) {
  const q = quoted(row.sentence);
  return q ?? row.sentence ?? row.kind;
}
var shortPaths = (text) => typeof text === "string" ? text.replace(/(?:~|\/(?:Users|home|private|var|tmp|opt))(?:\/[^\s/:;,()]+(?: [^\s/:;,()]+)*)+/g, (p) => `\u2026/${p.split("/").at(-1)}`) : text;
var scrubbed = /* @__PURE__ */ new WeakMap();
function scrubDeep(v) {
  if (typeof v === "string") return shortPaths(v);
  if (Array.isArray(v)) return v.map(scrubDeep);
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, scrubDeep(x)]));
  return v;
}
function scrubRow(row) {
  if (!row || typeof row !== "object") return row;
  let out = scrubbed.get(row);
  if (!out) {
    out = scrubDeep(row);
    scrubbed.set(row, out);
  }
  return out;
}
function rowText(kind, row) {
  return shortPaths(rowTextRaw(kind, row));
}
function rowTextRaw(kind, row) {
  const r = ref2(row);
  switch (kind) {
    case "vad":
      return row.sentence ?? (r.level != null ? `possible speech start \xB7 ${Math.round(r.level)} dBFS (not proof anyone spoke)` : "possible speech start (not proof anyone spoke)");
    case "speaking":
      return r.phase === "started" ? "playback started (host report)" : r.phase === "finished" ? "playback finished (host report; not proof it was heard)" : r.phase === "cleared" ? "playback cleared (host report)" : "playback reported (host)";
    case "audio":
      return "audio generated by the agent (not proof it was played)";
    case "interrupt":
      return row.kind === "speaker_cleared" ? "speaker cleared" : row.kind === "response_cancelled" ? `reply ${r.response_id ?? ""} cancelled`.replace("  ", " ") : "stop requested";
    case "echo":
      return row.sentence ?? "echo check (outcome not stated)";
    case "decision": {
      const to = decidedTo(row.sentence);
      return to ? `decided: ${to.replace(/_/g, " ")}` : row.sentence;
    }
    case "signal":
      return r.signal ?? r.tag ?? row.kind.replace(/_/g, " ");
    default:
      return row.sentence ?? row.kind.replace(/_/g, " ");
  }
}
function cell(base) {
  const k = KINDS[base.rthKind];
  return {
    kind: k.upstream,
    rthLane: k.lane,
    timeSeconds: null,
    rthSpan: "instant",
    ...base,
    text: shortPaths(base.text),
    inputDetail: shortPaths(base.inputDetail)
  };
}
var at = (row) => row ? { kind: row.kind, row: row.id } : null;
var sameLife = (a, b) => a?.lane === "host" && b?.lane === "host" && ref2(a).host_life != null && ref2(a).host_life === ref2(b).host_life;
var sameBoot = (a, b) => a?.lane === "agentd" && b?.lane === "agentd" && ref2(a).producer != null && ref2(a).producer === ref2(b).producer;
function replyCell(epRows, byId, idx, cmdIndex = /* @__PURE__ */ new Map()) {
  const m = { type: "reply", rows: epRows.map((r) => r.id) };
  const met = replyMetrics(m, byId);
  const deltas = epRows.filter((r) => r.kind === "agent_delta" || r.kind === "reply_text");
  const text = deltas.length ? textOf(deltas.at(-1)) : null;
  const hStart = epRows.find((r) => r.kind === "response_started" && r.lane === "host" && ref2(r).response_id === met.reply) ?? null;
  const aStart = epRows.find((r) => r.kind === "response_started" && r.lane === "agentd" && ref2(r).response_id === met.reply) ?? null;
  const started = hStart ?? aStart ?? epRows.find((r) => r.kind === "response_started");
  const duration = met.spans.find((s) => s.name === "reply duration");
  const measured = !met.why && duration?.ms != null;
  const parts = epRows.filter((r) => REPLY_PARTS.has(r.kind));
  const ofR = (r) => met.reply != null && !met.why && ref2(r).response_id === met.reply;
  const mine = (r) => ofR(r) && (sameLife(r, hStart) || sameBoot(r, aStart));
  const stops = epRows.filter((r) => (r.kind === "cancel" || r.kind === "cancel_written") && mine(r));
  const cancels = epRows.filter((r) => r.kind === "response_cancelled" && r.lane === "host" && mine(r));
  const cleared = epRows.filter((r) => r.kind === "playback" && ref2(r).phase === "cleared" && ofR(r) && sameLife(r, hStart));
  const foreignEnds = met.why || met.status ? [] : epRows.filter((r) => ofR(r) && (r.kind === "response_done" || r.kind === "agent_done") && !(sameLife(r, hStart) || sameBoot(r, aStart)));
  const outcome = met.why ? null : met.status ?? (cancels.length ? "cancelled" : null);
  const outcomeWhy = outcome == null && foreignEnds.length ? "its end was reported by another rth-host process or agent boot: not attributed" : null;
  return cell({
    rthKind: "rth",
    recordId: `rth:reply:${started?.id ?? parts[0]?.id}`,
    index: idx,
    text: text ?? (met.reply ? `reply ${met.reply} (no text seen)` : "a reply (no text seen)"),
    outputDetail: text ?? void 0,
    startedAt: (started ?? parts[0])?.observed_at ?? null,
    timeSeconds: measured ? duration.ms / 1e3 : null,
    rthDurationMs: measured ? duration.ms : null,
    rthSpan: measured ? "measured" : "unknown",
    rthClock: measured ? "rth-host" : null,
    rthTiming: met.why ? [{ name: "timing", ms: null, why: met.why }] : met.spans.map((s) => ({ name: s.name, ms: s.ms, why: s.why ?? null, clock: s.clock, start: s.start, end: s.end })),
    rthStatus: met.status,
    rthOutcome: outcome,
    rthOutcomeWhy: outcomeWhy,
    // Typed only when the reply's request names exactly one command written in the reply's own
    // rth-host process (replyMetrics reads the episode; this checks the whole projection).
    rthTyped: !met.why && met.typed === true && hStart != null && ref2(hStart).host_life != null && (cmdIndex.get(ref2(hStart).request_id) ?? []).filter((r) => r.kind === "command_written" && ref2(r).host_life === ref2(hStart).host_life).length === 1,
    rthCancelledBy: cancels.map((r) => r.id),
    rthStopRequested: stops.map((r) => r.id),
    rthPlaybackCleared: cleared.map((r) => r.id),
    rthHostLife: hStart ? ref2(hStart).host_life ?? null : null,
    rthPlayback: met.playback,
    rthReply: met.why ? null : met.reply,
    rthRequest: started ? ref2(started).request_id ?? null : null,
    rthWhy: met.why ?? null,
    isError: !met.why && met.status === "failed",
    rthRows: [...new Set([...parts, ...cancels, ...stops].map((r) => r.id))]
  });
}
function playbackCells(rows) {
  const out = [];
  const groups = /* @__PURE__ */ new Map();
  for (const r of rows) {
    const key = ref2(r).response_id != null && ref2(r).host_life != null ? `${ref2(r).response_id}|${ref2(r).host_life}` : `row:${r.id}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(r);
  }
  for (const group of groups.values()) {
    const R = ref2(group[0]).response_id ?? null;
    const starts = group.filter((r) => ref2(r).phase === "started");
    const ends = group.filter((r) => ref2(r).phase === "finished" || ref2(r).phase === "cleared");
    let span2;
    if (starts.length === 1 && ends.length === 1) span2 = pairSpan(starts[0], ends[0]);
    else if (starts.length > 1 || ends.length > 1) span2 = { ms: null, clock: null, why: `${starts.length} playback start${starts.length === 1 ? "" : "s"} and ${ends.length} end${ends.length === 1 ? "" : "s"} for reply ${R} in one rth-host process: which interval ended is not established` };
    else span2 = { ms: null, clock: null, why: starts.length ? "no end reported" : "its start was not reported" };
    const end = ends.length === 1 ? ends[0] : null;
    const first = group[0];
    out.push(cell({
      rthKind: "speaking",
      recordId: `rth:play:${first.id}`,
      index: 0,
      text: span2.ms != null ? end.kind === "playback" && ref2(end).phase === "cleared" ? "playback cut off (host report: cleared)" : "playback reported (handed to the speaker; not proof it was heard)" : group.length === 1 ? rowTextRaw("speaking", first) : `playback reports for reply ${R} (interval not established)`,
      startedAt: first.observed_at,
      timeSeconds: span2.ms != null ? span2.ms / 1e3 : null,
      rthDurationMs: span2.ms,
      rthSpan: span2.ms != null ? "measured" : "unknown",
      rthClock: span2.clock,
      rthTiming: [{ name: "playback (host report)", ms: span2.ms, why: span2.ms == null ? span2.why : null, clock: span2.clock, start: starts.length === 1 ? at(starts[0]) : null, end: end ? at(end) : null }],
      rthReplyId: R,
      rthConflict: starts.length > 1 || ends.length > 1,
      rthLife: ref2(group[0]).host_life ?? null,
      rthRows: group.map((r) => r.id)
    }));
  }
  return out;
}
function commandCells(rows, cmdIndex) {
  const out = [];
  const byId = /* @__PURE__ */ new Map();
  for (const r of rows) {
    const id = ref2(r).command_id ?? r.id;
    if (!byId.has(id)) byId.set(id, []);
    byId.get(id).push(r);
  }
  for (const [id, group] of byId) {
    const all = cmdIndex.get(id) ?? group;
    const ownWrites = group.filter((r) => r.kind === "command_written");
    const written = ownWrites.length === 1 ? ownWrites[0] : null;
    const life = written ? ref2(written).host_life ?? null : null;
    const writesInLife = life == null ? [] : all.filter((r) => r.kind === "command_written" && ref2(r).host_life === life);
    const finishes = all.filter((r) => r.kind === "command_finished" && r.lane === "host");
    const finishesInLife = life == null ? [] : finishes.filter((r) => ref2(r).host_life === life);
    const admits = all.filter((r) => r.kind === "command_admitted");
    const admitted = admits.length === 1 ? admits[0] : null;
    const why = ownWrites.length > 1 || writesInLife.length > 1 ? `command ${id} was sent more than once in one rth-host process: not attributed` : written && life == null ? "host process not established (no started record observed for it): not attributed" : written && finishesInLife.length > 1 ? `command ${id} finished ${finishesInLife.length} times in its rth-host process: not attributed` : written && finishesInLife.length === 0 && finishes.length > 0 ? "its finish comes from a different rth-host process: not attributed" : !written && finishes.length > 0 ? "its write was not observed: the finish is not attributed" : null;
    const finished = !why && written && finishesInLife.length === 1 ? finishesInLife[0] : null;
    const span2 = why ? { ms: null, why, clock: null } : written && finished ? pairSpan(written, finished) : { ms: null, why: !written ? "not sent by this host log" : "no finish observed", clock: null };
    const outcome = finished ? ref2(finished).status ?? "finished" : null;
    const first = written ?? group.find((r) => r.kind === "command_admitted") ?? group[0];
    const parts = [`command ${id}`, written ? "sent (host)" : null, admitted ? `${ref2(admitted).status ?? "admitted"} (agent)` : null, outcome ? `${outcome} (host)` : why ? "outcome not attributed" : null].filter(Boolean);
    out.push(cell({
      rthKind: "command",
      recordId: `rth:cmd:${first.id}`,
      rthCommand: ref2(first).command_id ?? null,
      index: 0,
      text: parts.join(" \xB7 "),
      startedAt: first.observed_at,
      timeSeconds: span2.ms != null ? span2.ms / 1e3 : null,
      rthDurationMs: span2.ms,
      rthSpan: span2.ms != null ? "measured" : written || finishes.length ? "unknown" : "instant",
      rthClock: span2.clock,
      rthTiming: [{ name: "command (sent -> finished)", ms: span2.ms, why: span2.ms == null ? span2.why : null, clock: span2.clock, start: at(written), end: at(finished) }],
      rthOutcome: outcome,
      rthOutcomeWhy: why,
      isError: outcome === "failed" || !why && admitted != null && ref2(admitted).status === "rejected",
      rthRows: group.map((r) => r.id)
    }));
  }
  return out;
}
function foldedSignals(rows) {
  if (!rows.length) return [];
  const signals = rows.filter((r) => kindOf(r) === "signal").length;
  const echo = rows.filter((r) => kindOf(r) === "echo").length;
  const parts = [signals ? `${signals} signal${signals > 1 ? "s" : ""}` : null, echo ? `${echo} echo check${echo > 1 ? "s" : ""}` : null].filter(Boolean);
  return [cell({
    rthKind: signals ? "signal" : "echo",
    recordId: `rth:fold:${rows[0].id}`,
    index: 0,
    text: `\u2301 ${parts.join(" \xB7 ")}`,
    startedAt: rows[0].observed_at,
    rthFolded: rows.map((r) => ({ id: r.id, kind: kindOf(r), text: rowText(kindOf(r), r) })),
    rthRows: rows.map((r) => r.id)
  }), ...rows.map((r) => cell({
    rthKind: kindOf(r),
    recordId: `rth:row:${r.id}`,
    index: 0,
    text: rowText(kindOf(r), r),
    startedAt: r.observed_at + 1e-6,
    rthQuiet: true,
    rthRows: [r.id]
  }))];
}
var NO_ASSOCIATION = "not available: the projection links no utterance to a reply (a reply names only a stream or request id)";
function sectionCells(rows, byId, idx, cmdIndex) {
  const cells = [];
  const order = (c) => c.startedAt ?? Infinity;
  const hasReply = rows.some((r) => r.kind === "response_started" || r.kind === "agent_delta" || r.kind === "reply_text");
  const utterances = [];
  let open = null;
  for (const r of rows.filter((x) => x.kind === "input_transcript")) {
    const u = ref2(r).utterance_id ?? null;
    if (!open || u !== null && open.u !== u || u === null && open.closed) {
      open = { u, rows: [], closed: false };
      utterances.push(open);
    }
    open.rows.push(r);
    if (/\(final;/.test(r.sentence ?? "")) open.closed = true;
  }
  for (const { rows: group } of utterances) {
    const last = group.at(-1);
    const final = /\(final;/.test(last.sentence ?? "");
    const text = textOf(last);
    cells.push(cell({ rthKind: "voice", recordId: `rth:heard:${group[0].id}`, index: 0, text: final ? text : `${text} (heard, partial)`, inputDetail: text, startedAt: group[0].observed_at, rthAssociation: hasReply ? "same Mind episode as the reply" : NO_ASSOCIATION, rthRows: group.map((r) => r.id) }));
  }
  const command = rows.find((r) => r.kind === "command_written" || r.kind === "command_admitted" && r.lane === "agentd");
  if (command && !rows.some((r) => kindOf(r) === "typed")) cells.push(cell({ rthKind: "typed", recordId: `rth:typed:${command.id}`, index: 0, text: "typed message (its text is in the chat)", inputDetail: `Typed request ${ref2(command).command_id ?? "unknown"}`, startedAt: command.observed_at - 1e-3, rthRows: [command.id] }));
  if (hasReply) cells.push(replyCell(rows, byId, 0, cmdIndex));
  cells.push(...playbackCells(rows.filter((r) => kindOf(r) === "speaking")));
  cells.push(...commandCells(rows.filter((r) => kindOf(r) === "command" || r.kind === "command_finished"), cmdIndex));
  const quiet = [];
  for (const r of rows) {
    const k = kindOf(r);
    if (!k || k === "speaking" || k === "command") continue;
    if (k === "signal" || k === "echo") {
      quiet.push(r);
      continue;
    }
    const typedText = k === "typed" ? textOf(r) : null;
    cells.push(cell({
      rthKind: k,
      recordId: `rth:row:${r.id}`,
      index: 0,
      text: typedText ?? rowText(k, r),
      ...k === "typed" ? { inputDetail: typedText } : {},
      startedAt: r.observed_at,
      isError: k === "fault",
      rthRows: [r.id]
    }));
  }
  cells.push(...foldedSignals(quiet));
  const reply = cells.find((c) => c.rthKind === "rth");
  for (const c of cells) if (c.rthKind === "typed") c.rthAssociation = reply?.rthTyped ? "the reply names this command, sent in its own rth-host process" : reply ? "not established: the reviewed chain does not tie this command to the reply (missing, reused or from another process)" : "no reply in this episode";
  cells.sort((a, b) => order(a) - order(b));
  for (const c of cells) c.index = idx();
  return cells;
}
var hostAt = (r) => r?.lane === "host" && ref2(r).host_life != null && finite(r.clock?.t_ms) ? { clock: "host", life: ref2(r).host_life, t: r.clock.t_ms } : null;
var agentAt = (r) => r?.lane === "agentd" && ref2(r).producer && finite(r.clock?.t_ns) ? { clock: "agent", life: ref2(r).producer, t: r.clock.t_ns / 1e6 } : null;
var clockAt = (r) => hostAt(r) ?? agentAt(r);
function placeInterval(entry, lane, name, byId) {
  if (!entry) return null;
  const s = entry.start ? clockAt(byId.get(entry.start.row)) : null;
  const e = entry.end ? clockAt(byId.get(entry.end.row)) : null;
  const base = { lane, name, startRow: entry.start?.row ?? null, endRow: entry.end?.row ?? null, why: entry.why ?? null };
  if (entry.ms != null && s && e && s.clock === e.clock && s.life === e.life && e.t >= s.t) return { ...base, clock: s.clock, life: s.life, start: s.t, end: e.t, shape: e.t === s.t ? "zero" : "span" };
  if (s) return { ...base, clock: s.clock, life: s.life, start: s.t, end: null, shape: "start", why: entry.why ?? "end not observed" };
  if (e) return { ...base, clock: e.clock, life: e.life, start: null, end: e.t, shape: "end", why: entry.why ?? "start not observed" };
  return null;
}
function placementsOf(c, byId) {
  const lane = KINDS[c.rthKind].lane;
  const timing = (n) => c.rthTiming?.find((x) => x.name === n);
  const out = [];
  if (c.rthKind === "rth") {
    if (c.rthWhy) return { placements: [], off: "reply identity not attributed" };
    const h = placeInterval(timing("reply duration"), lane, "host-observed reply", byId);
    if (h && h.clock === "host") out.push(h);
    const f = placeInterval(timing("first text received"), lane, "first text received", byId);
    if (f && f.clock === "agent") out.push(f);
    if (!h) {
      const aStart = (c.rthRows ?? []).map((id) => byId.get(id)).find((r) => r?.lane === "agentd" && r.kind === "response_started" && ref2(r).response_id === c.rthReply);
      const at2 = agentAt(aStart);
      if (at2) out.push({ lane, name: "agent-observed reply start", clock: "agent", life: at2.life, start: at2.t, end: null, shape: "start", startRow: aStart.id, endRow: null, why: "the reply duration is measured on the rth-host clock; no host start was observed" });
    }
  } else if (c.rthKind === "speaking" || c.rthKind === "command") {
    if (c.rthConflict) return { placements: [], off: "conflicting reports: which interval is meant is not established" };
    const p = placeInterval(c.rthTiming?.[0], lane, c.rthKind === "speaking" ? "playback (host report)" : "command (sent \u2192 finished)", byId);
    if (p) out.push(p);
  } else {
    const first = (c.rthRows ?? []).map((id) => byId.get(id)).find((r) => clockAt(r));
    const at2 = first ? clockAt(first) : null;
    if (at2) out.push({ lane, name: KINDS[c.rthKind].label, clock: at2.clock, life: at2.life, start: at2.t, end: null, shape: "tick", startRow: first.id, endRow: null, why: null });
  }
  if (out.length) return { placements: out, off: null };
  const rows = (c.rthRows ?? []).map((id) => byId.get(id)).filter(Boolean);
  return { placements: [], off: rows.some((r) => r.lane === "host" && ref2(r).host_life == null) ? "host process not established (no started record observed)" : rows.some((r) => r.lane === "host" || r.lane === "agentd") ? "no valid producer time" : "no producer clock (a Mind observation)" };
}
function turnCard(turn, cells) {
  const reply = cells.find((c) => c.rthKind === "rth");
  const typed = reply?.rthTyped === true;
  const voice = !typed && cells.some((c) => c.rthKind === "voice");
  const first = reply?.rthTiming?.find((s) => s.name === "first text received" && s.ms != null);
  const mine = reply?.rthReply != null && reply.rthHostLife != null ? cells.filter((c) => c.rthKind === "speaking" && c.rthReplyId === reply.rthReply && c.rthLife === reply.rthHostLife) : [];
  const speaking = mine.length === 1 && mine[0].rthSpan === "measured" ? mine[0].rthDurationMs : null;
  const cancelled = (reply?.rthCancelledBy?.length ?? 0) > 0 && reply.rthOutcome === "cancelled";
  const chips = [
    typed ? "typed" : voice ? "voice" : null,
    first ? `first text ${fmt(first.ms)}` : null,
    speaking != null ? `speaking ${fmt(speaking)}` : null,
    cancelled ? "\u26A1 cancelled (host)" : null,
    (reply?.rthPlaybackCleared?.length ?? 0) > 0 ? "playback cleared" : null,
    (reply?.rthStopRequested?.length ?? 0) > 0 && !cancelled ? "stop requested" : null,
    reply?.rthWhy ? "timing not attributed" : null
  ].filter(Boolean);
  return { icon: typed ? "keyboard" : voice ? "mic" : "sparkles", label: `Turn ${turn}`, chips };
}
function buildRthTrajectory(state) {
  const rows = (state.rows ?? []).map(scrubRow).sort((a, b) => a.observed_at - b.observed_at);
  const byId = new Map(rows.map((r) => [r.id, r]));
  const arrivals = state.arrivals instanceof Map ? state.arrivals : null;
  let n = 0;
  const idx = () => ++n;
  const cmdIndex = /* @__PURE__ */ new Map();
  for (const r of rows) {
    if (!["command_written", "command_admitted", "command_finished"].includes(r.kind) || ref2(r).command_id == null) continue;
    if (!cmdIndex.has(ref2(r).command_id)) cmdIndex.set(ref2(r).command_id, []);
    cmdIndex.get(ref2(r).command_id).push(r);
  }
  const sections = [];
  const byEpisode = /* @__PURE__ */ new Map();
  for (const r of rows) {
    if (r.episode && r.status !== "gap") {
      if (!byEpisode.has(r.episode)) {
        byEpisode.set(r.episode, []);
        sections.push({ episode: r.episode, rows: byEpisode.get(r.episode) });
      }
      byEpisode.get(r.episode).push(r);
    } else {
      const last = sections.at(-1);
      if (last && last.episode === null) last.rows.push(r);
      else sections.push({ episode: null, rows: [r] });
    }
  }
  const turns = [];
  const cards2 = /* @__PURE__ */ new Map();
  let turnNo = 0;
  for (const sec of sections) {
    const cells = sectionCells(sec.rows, byId, idx, cmdIndex);
    if (!cells.length) continue;
    for (const c of cells) {
      const { placements, off } = placementsOf(c, byId);
      c.rthPlacements = placements;
      c.rthOffAxis = off;
    }
    if (arrivals) for (const c of cells) c.rthArrival = Math.max(-1, ...c.rthRows.map((id) => arrivals.get(id) ?? -1));
    const reply = cells.some((c) => c.rthKind === "rth");
    const turn = sec.episode !== null && reply ? ++turnNo : null;
    if (turn !== null) cards2.set(turn, turnCard(turn, cells));
    const card = turn !== null ? cards2.get(turn) : null;
    const title = card ? card.icon === "keyboard" ? "Typed turn" : card.icon === "mic" ? "Voice turn" : "Reply" : cells.some((c) => c.rthKind === "voice") ? "Heard (no reply linked)" : "Between turns";
    turns.push({ turn, rthEpisode: sec.episode, rthEpisodes: sec.episode === null ? [] : [sec.episode], groups: [{ title, cells }] });
  }
  const all = turns.flatMap((t) => t.groups.flatMap((g) => g.cells));
  const counts = {};
  for (const c of all) if (!c.rthQuiet) counts[KINDS[c.rthKind].group] = (counts[KINDS[c.rthKind].group] ?? 0) + (c.rthFolded?.length ?? 1);
  const firsts = all.filter((c) => c.rthKind === "rth").map((c) => c.rthTiming?.find((s) => s.name === "first text received" && s.ms != null)?.ms).filter((v) => v != null);
  const strip = {
    turns: turnNo,
    avgFirstTextMs: firsts.length ? firsts.reduce((a, b) => a + b, 0) / firsts.length : null,
    firstTextCoverage: `${firsts.length} of ${all.filter((c) => c.rthKind === "rth").length}`,
    interrupts: all.filter((c) => c.rthKind === "interrupt").length,
    faults: all.filter((c) => c.rthKind === "fault").length,
    tokens: "not linked yet",
    usageReports: usageReports(rows).length
  };
  return { turns, cards: cards2, lastIndex: n, counts, strip };
}
function filterTurns(turns, enabled) {
  const on = (group) => !enabled || enabled.has(group);
  const signals = on("signals");
  const keep = (c) => c.rthFolded ? !signals : c.rthQuiet ? signals : on(KINDS[c.rthKind].group);
  return turns.map((t) => ({ ...t, groups: t.groups.map((g) => ({ ...g, cells: g.cells.filter(keep) })) })).filter((t) => t.groups.some((g) => g.cells.length));
}
function newSince(turns, seq, arrivals) {
  if (!(arrivals instanceof Map) || !Number.isFinite(seq)) return 0;
  const ids = /* @__PURE__ */ new Set();
  for (const t of turns) for (const g of t.groups) for (const c of g.cells) for (const id of c.rthRows ?? []) if ((arrivals.get(id) ?? -1) > seq) ids.add(id);
  return ids.size;
}
var DEFAULT_GROUPS = ["you", "rth", "voice", "control", "faults"];

// src/status.mjs
var OUTCOME = Object.freeze({ completed: "complete", cancelled: "rth-cancelled", failed: "error", rejected: "error", interrupted: "rth-other" });
var outcomeState = (s) => s == null ? null : Object.hasOwn(OUTCOME, s) ? OUTCOME[s] : "rth-other";
function stateOf(cell2) {
  switch (cell2.rthKind) {
    case "rth":
      if (cell2.rthWhy || cell2.rthOutcomeWhy) return "rth-unavailable";
      return outcomeState(cell2.rthOutcome) ?? "rth-pending";
    case "command":
      if (cell2.rthOutcomeWhy) return "rth-unavailable";
      return outcomeState(cell2.rthOutcome) ?? "rth-pending";
    case "fault":
      return "error";
    default:
      return "rth-observed";
  }
}
var STATUS_EN = {
  "rth.status.pending": "No end observed yet",
  "rth.status.cancelled": "Cancelled",
  "rth.status.unavailable": "Not attributed",
  "rth.status.other": "Ended (status not classified)",
  "rth.status.observed": "Observed (no outcome claimed)"
};
var STATUS_ZH = {
  "rth.status.pending": "\u5C1A\u672A\u89C2\u5BDF\u5230\u7ED3\u675F",
  "rth.status.cancelled": "\u5DF2\u53D6\u6D88",
  "rth.status.unavailable": "\u65E0\u6CD5\u5F52\u5C5E",
  "rth.status.other": "\u5DF2\u7ED3\u675F\uFF08\u72B6\u6001\u672A\u5206\u7C7B\uFF09",
  "rth.status.observed": "\u5DF2\u89C2\u5BDF\uFF08\u4E0D\u4EE3\u8868\u7ED3\u679C\uFF09"
};
function timelineDetail(cell2) {
  return cell2.rthSpan === "measured" && Number.isFinite(cell2.rthDurationMs) ? { durationMs: Math.max(0, cell2.rthDurationMs) } : {};
}

// src/icons.generated.mjs
var ICONS = { "audio-lines": [["path", { "d": "M2 10v3" }], ["path", { "d": "M6 6v11" }], ["path", { "d": "M10 3v18" }], ["path", { "d": "M14 8v7" }], ["path", { "d": "M18 5v13" }], ["path", { "d": "M22 10v3" }]], "audio-waveform": [["path", { "d": "M2 13a2 2 0 0 0 2-2V7a2 2 0 0 1 4 0v13a2 2 0 0 0 4 0V4a2 2 0 0 1 4 0v13a2 2 0 0 0 4 0v-4a2 2 0 0 1 2-2" }]], "book-open": [["path", { "d": "M12 7v14" }], ["path", { "d": "M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z" }]], "git-branch": [["line", { "x1": "6", "x2": "6", "y1": "3", "y2": "15" }], ["circle", { "cx": "18", "cy": "6", "r": "3" }], ["circle", { "cx": "6", "cy": "18", "r": "3" }], ["path", { "d": "M18 9a9 9 0 0 1-9 9" }]], "image": [["rect", { "width": "18", "height": "18", "x": "3", "y": "3", "rx": "2", "ry": "2" }], ["circle", { "cx": "9", "cy": "9", "r": "2" }], ["path", { "d": "m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" }]], "keyboard": [["path", { "d": "M10 8h.01" }], ["path", { "d": "M12 12h.01" }], ["path", { "d": "M14 8h.01" }], ["path", { "d": "M16 12h.01" }], ["path", { "d": "M18 8h.01" }], ["path", { "d": "M6 8h.01" }], ["path", { "d": "M7 16h10" }], ["path", { "d": "M8 12h.01" }], ["rect", { "width": "20", "height": "16", "x": "2", "y": "4", "rx": "2" }]], "mic": [["path", { "d": "M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" }], ["path", { "d": "M19 10v2a7 7 0 0 1-14 0v-2" }], ["line", { "x1": "12", "x2": "12", "y1": "19", "y2": "22" }]], "mic-off": [["line", { "x1": "2", "x2": "22", "y1": "2", "y2": "22" }], ["path", { "d": "M18.89 13.23A7.12 7.12 0 0 0 19 12v-2" }], ["path", { "d": "M5 10v2a7 7 0 0 0 12 5" }], ["path", { "d": "M15 9.34V5a3 3 0 0 0-5.68-1.33" }], ["path", { "d": "M9 9v3a3 3 0 0 0 5.12 2.12" }], ["line", { "x1": "12", "x2": "12", "y1": "19", "y2": "22" }]], "phone": [["path", { "d": "M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z" }]], "plug": [["path", { "d": "M12 22v-5" }], ["path", { "d": "M9 8V2" }], ["path", { "d": "M15 8V2" }], ["path", { "d": "M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z" }]], "radio": [["path", { "d": "M4.9 19.1C1 15.2 1 8.8 4.9 4.9" }], ["path", { "d": "M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5" }], ["circle", { "cx": "12", "cy": "12", "r": "2" }], ["path", { "d": "M16.2 7.8c2.3 2.3 2.3 6.1 0 8.5" }], ["path", { "d": "M19.1 4.9C23 8.8 23 15.1 19.1 19" }]], "shield": [["path", { "d": "M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z" }]], "sparkles": [["path", { "d": "M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z" }], ["path", { "d": "M20 3v4" }], ["path", { "d": "M22 5h-4" }], ["path", { "d": "M4 17v2" }], ["path", { "d": "M5 18H3" }]], "terminal": [["polyline", { "points": "4 17 10 11 4 5" }], ["line", { "x1": "12", "x2": "20", "y1": "19", "y2": "19" }]], "triangle-alert": [["path", { "d": "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" }], ["path", { "d": "M12 9v4" }], ["path", { "d": "M12 17h.01" }]], "unlink": [["path", { "d": "m18.84 12.25 1.72-1.71h-.02a5.004 5.004 0 0 0-.12-7.07 5.006 5.006 0 0 0-6.95 0l-1.72 1.71" }], ["path", { "d": "m5.17 11.75-1.71 1.71a5.004 5.004 0 0 0 .12 7.07 5.006 5.006 0 0 0 6.95 0l1.71-1.71" }], ["line", { "x1": "8", "x2": "8", "y1": "2", "y2": "5" }], ["line", { "x1": "2", "x2": "5", "y1": "8", "y2": "8" }], ["line", { "x1": "16", "x2": "16", "y1": "19", "y2": "22" }], ["line", { "x1": "19", "x2": "22", "y1": "16", "y2": "16" }]], "volume-2": [["path", { "d": "M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z" }], ["path", { "d": "M16 9a5 5 0 0 1 0 6" }], ["path", { "d": "M19.364 18.364a9 9 0 0 0 0-12.728" }]], "zap": [["path", { "d": "M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z" }]] };

// src/clock-layout.mjs
var LANES2 = ["Hear", "Think", "Speak", "Act", "Life"];
var STEPS = [1, 2, 5];
function ticksFor(extentMs) {
  const span2 = Math.max(1, extentMs);
  let step = 1;
  for (let p = 0; p < 12; p++) {
    const base = 10 ** p;
    const s = STEPS.map((k) => k * base).find((v) => span2 / v <= 6);
    if (s) {
      step = s;
      break;
    }
  }
  const out = [];
  for (let t = 0; t <= span2 + 1e-9; t += step) out.push({ ms: t, x: t / span2, label: t === 0 ? "0" : t < 1e3 ? `+${t} ms` : `+${(t / 1e3).toFixed(t % 1e3 ? 1 : 0)} s` });
  return out;
}
function layoutClockOverview(turns) {
  const groups = /* @__PURE__ */ new Map();
  const unplaced = {};
  for (const t of turns) for (const g of t.groups) for (const c of g.cells) {
    if (!c.rthPlacements?.length) {
      if (c.rthOffAxis) unplaced[c.rthOffAxis] = (unplaced[c.rthOffAxis] ?? 0) + 1;
      continue;
    }
    for (const p of c.rthPlacements) {
      const key = `${p.clock}|${p.life}`;
      if (!groups.has(key)) groups.set(key, { clock: p.clock, life: p.life, items: [] });
      groups.get(key).items.push({ index: c.index, recordId: c.recordId, kind: c.rthKind, lane: p.lane, name: p.name, shape: p.shape, start: p.start, end: p.end, why: p.why, startRow: p.startRow, endRow: p.endRow });
    }
  }
  const out = [];
  const counts = { host: 0, agent: 0 };
  for (const g of groups.values()) {
    const times = g.items.flatMap((i) => [i.start, i.end]).filter((v) => typeof v === "number" && Number.isFinite(v));
    const origin = Math.min(...times);
    const extent = Math.max(1, Math.max(...times) - origin);
    const x = (t) => (t - origin) / extent;
    const n = ++counts[g.clock];
    out.push({
      clock: g.clock,
      life: g.life,
      title: g.clock === "host" ? `rth-host \xB7 process ${n}` : `agent \xB7 boot ${n}`,
      axisLabel: `${g.clock === "host" ? "rth-host clock" : "agent clock"}, its own axis, relative to first retained event`,
      origin,
      extentMs: Math.max(...times) - origin,
      ticks: ticksFor(extent),
      lanes: LANES2.map((name, lane) => ({ name, lane })).filter((l) => g.items.some((i) => i.lane === l.lane)),
      items: g.items.map((i) => ({
        ...i,
        x0: i.start != null ? x(i.start) : x(i.end),
        x1: i.shape === "span" || i.shape === "zero" ? x(i.end) : null,
        durationMs: i.shape === "span" || i.shape === "zero" ? i.end - i.start : null
      }))
    });
  }
  return { groups: out, unplaced };
}

// src/runtime.mjs
var RTH_SESSION = "rth-root";
var initial = { rows: [], episodes: {}, link: null, sources: null, heartbeatMs: 5e3, stream: "opening", lastStreamAt: null };
function reduce(s, event, data, at2) {
  switch (event) {
    case "snapshot":
      return { ...s, rows: data.rows, episodes: Object.fromEntries(data.episodes.map((e) => [e.id, e])), link: data.link, sources: data.sources, heartbeatMs: data.heartbeat_ms ?? s.heartbeatMs, stream: "open", lastStreamAt: at2 };
    case "row":
      return { ...s, rows: [...s.rows, data], lastStreamAt: at2 };
    case "row-updated":
      return { ...s, rows: s.rows.map((r) => r.id === data.id ? data : r), lastStreamAt: at2 };
    case "evicted":
      return { ...s, rows: s.rows.filter((r) => r.id !== data.id), lastStreamAt: at2 };
    case "episode":
      return { ...s, episodes: { ...s.episodes, [data.id]: data }, lastStreamAt: at2 };
    case "episode-removed": {
      const { [data.id]: _, ...rest } = s.episodes;
      return { ...s, episodes: rest, lastStreamAt: at2 };
    }
    case "merged": {
      const { [data.from]: _, ...rest } = s.episodes;
      return { ...s, episodes: rest, rows: s.rows.map((r) => r.episode === data.from ? { ...r, episode: data.into } : r), lastStreamAt: at2 };
    }
    case "link":
      return { ...s, link: data, lastStreamAt: at2 };
    case "heartbeat":
    case "heartbeat-mind":
      return { ...s, lastStreamAt: at2 };
    default:
      return s;
  }
}
var EVENTS = ["snapshot", "row", "row-updated", "evicted", "episode", "episode-removed", "merged", "link", "heartbeat", "heartbeat-mind"];
var STREAM_BEATS = 3;
function streamFreshness(s, now) {
  if (s.stream !== "open" || s.lastStreamAt == null) return { state: s.stream === "opening" ? "opening" : "lost", nextChange: null };
  const lease = s.heartbeatMs * STREAM_BEATS;
  return now - s.lastStreamAt > lease ? { state: "stale", nextChange: null } : { state: "live", nextChange: s.lastStreamAt + lease + 1 };
}
var shared = null;
function sharedStream() {
  if (shared) return shared;
  let state = initial;
  let arrivals = /* @__PURE__ */ new Map();
  let latestArrival = 0;
  let derived = null;
  let derivedFrom = null;
  let derivedFresh = null;
  const listeners = /* @__PURE__ */ new Set();
  let source = null;
  let refs = 0;
  let retry = null;
  let lease = null;
  let pending = false;
  const emit = () => {
    if (pending) return;
    pending = true;
    const run = () => {
      pending = false;
      for (const l of [...listeners]) l();
    };
    if (typeof requestAnimationFrame === "function" && typeof document !== "undefined" && document.visibilityState !== "hidden") requestAnimationFrame(run);
    else setTimeout(run, 50);
  };
  const arm = () => {
    clearTimeout(lease);
    lease = null;
    const f = streamFreshness(state, Date.now());
    if (refs > 0 && f.nextChange != null) lease = setTimeout(() => {
      lease = null;
      emit();
      arm();
    }, Math.max(0, f.nextChange - Date.now()));
  };
  const track = () => {
    const live = /* @__PURE__ */ new Set();
    for (const r of state.rows) {
      live.add(r.id);
      if (!arrivals.has(r.id)) arrivals.set(r.id, ++latestArrival);
    }
    for (const id of [...arrivals.keys()]) if (!live.has(id)) arrivals.delete(id);
  };
  const open = () => {
    source = new EventSource(new URL("api/rth-mind/stream", document.baseURI));
    for (const name of EVENTS) {
      source.addEventListener(name, (e) => {
        state = reduce(state, name, JSON.parse(e.data), Date.now());
        if (name === "snapshot" || name === "row" || name === "row-updated" || name === "evicted") track();
        arm();
        emit();
      });
    }
    source.addEventListener("resnapshot", () => {
      source.close();
      state = { ...state, stream: "opening" };
      arm();
      emit();
      retry = setTimeout(open, 500);
    });
    source.onerror = () => {
      state = { ...state, stream: "lost" };
      arm();
      emit();
    };
  };
  shared = {
    getSnapshot() {
      const fresh = streamFreshness(state, Date.now()).state;
      if (derivedFrom !== state || derivedFresh !== fresh) {
        if (derivedFrom !== state) derived = { ...buildRthTrajectory({ ...state, arrivals }), arrivals, latestArrival, link: state.link, sources: state.sources, rowCount: state.rows.length };
        derivedFrom = state;
        derivedFresh = fresh;
        derived = { ...derived, stream: fresh };
      }
      return derived;
    },
    subscribe(listener) {
      listeners.add(listener);
      if (refs++ === 0) open();
      return () => {
        listeners.delete(listener);
        if (--refs === 0) {
          clearTimeout(retry);
          clearTimeout(lease);
          retry = null;
          lease = null;
          source?.close();
          source = null;
          state = initial;
          arrivals = /* @__PURE__ */ new Map();
          derivedFrom = null;
        }
      };
    }
  };
  return shared;
}
var NOT_RTH = Object.freeze({ notRth: true, turns: [], cards: /* @__PURE__ */ new Map(), lastIndex: 0, counts: {}, strip: null, stream: "none", arrivals: /* @__PURE__ */ new Map(), latestArrival: 0 });
var notRthSource = { getSnapshot: () => NOT_RTH, subscribe: () => () => {
} };
function sourceFor(sessionId) {
  return sessionId === RTH_SESSION ? sharedStream() : notRthSource;
}
function Icon({ name, size = 13 }) {
  const els = ICONS[name] ?? ICONS.radio;
  return (0, import_jsx_runtime.jsx)("svg", { width: size, height: size, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": "true", children: els.map(([tag, attrs], i) => (0, import_jsx_runtime.jsx)(tag, { ...attrs }, i)) });
}
function kindTag(rthKind, css, primitives) {
  const k = KINDS[rthKind] ?? KINDS.signal;
  return (0, import_jsx_runtime.jsxs)("span", {
    className: `${css.kindTag} rth-kind`,
    "data-rth-kind": rthKind,
    "data-role-kind": k.upstream,
    children: [
      (0, import_jsx_runtime.jsx)(primitives.Tooltip, { label: k.label, side: "right", children: (0, import_jsx_runtime.jsx)("span", { className: css.kindTagIcon, "aria-hidden": "true", children: (0, import_jsx_runtime.jsx)(Icon, { name: k.icon }) }) }),
      (0, import_jsx_runtime.jsx)("span", { className: css.kindTagLabel, children: k.short })
    ]
  });
}
function kindLabel(rthKind) {
  return KINDS[rthKind]?.label ?? null;
}
var vars = (i) => Object.entries(KINDS).map(([k, v]) => `--rth-k-${k}:${v.color[i]};`).join("");
var CSS = `
.rth-trajectory{${vars(0)}}
@media (prefers-color-scheme:dark){.rth-trajectory{${vars(1)}}}
[data-theme=dark] .rth-trajectory,.dark .rth-trajectory{${vars(1)}}
.rth-trajectory .rth-kind{--k:var(--rth-k-signal);color:var(--k);background:color-mix(in srgb,var(--k) 14%,transparent);border:1px solid color-mix(in srgb,var(--k) 38%,transparent)}
${Object.keys(KINDS).map((k) => `.rth-trajectory .rth-kind[data-rth-kind=${k}]{--k:var(--rth-k-${k})}`).join("\n")}
.rth-trajectory .rth-kind[data-rth-kind=typed]{background:transparent}
.rth-trajectory .rth-kind[data-rth-kind=gap]{border-style:dashed}
.rth-trajectory .rth-kind[data-rth-kind=signal]{opacity:.75}
${Object.keys(KINDS).map((k) => `.rth-trajectory [data-timeline-span=${k}]{--rth-u:var(--rth-k-${k});background:var(--rth-k-${k})!important;opacity:.92}`).join("\n")}
.rth-trajectory [data-rth-span=instant]{width:3px!important;min-width:3px!important;border-radius:2px}
.rth-trajectory [data-rth-span^=unknown]{background:linear-gradient(var(--rth-u,currentColor) 0 0) left/3px 100% no-repeat,repeating-linear-gradient(135deg,color-mix(in srgb,var(--rth-u,currentColor) 75%,transparent) 0 2px,transparent 2px 4px)!important;opacity:.9;overflow:visible!important}
.rth-trajectory [data-rth-span=unknown]{width:14px!important;min-width:14px!important}
.rth-trajectory [data-rth-span^=unknown]::after{content:"?";position:absolute;left:calc(100% + 2px);top:50%;transform:translateY(-50%);font:600 9px/1 system-ui;color:var(--dsw-alias-label-secondary)}
.rth-trajectory [data-rth-span=unknown-seq]::after{left:6px}
.rth-trajectory .rth-strip{display:flex;flex-wrap:wrap;gap:6px;align-items:center;padding:4px 10px;border-bottom:.5px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-layer-1);font:var(--dsw-font-xxxs-11);color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-chip{display:inline-flex;align-items:center;gap:4px;padding:1px 8px;border-radius:999px;border:1px solid color-mix(in srgb,var(--k,var(--dsw-alias-label-secondary)) 40%,transparent);background:color-mix(in srgb,var(--k,var(--dsw-alias-label-secondary)) 10%,transparent);color:var(--k,var(--dsw-alias-label-secondary));cursor:pointer;user-select:none}
.rth-trajectory .rth-chip[aria-pressed=false]{opacity:.45;background:transparent}
.rth-trajectory .rth-chip b{font-weight:600}
.rth-trajectory .rth-stat{color:var(--dsw-alias-label-caption)}
.rth-trajectory:focus{outline:none}
.rth-trajectory .rth-clock{color:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-live{display:inline-flex;align-items:center;gap:5px;height:18px;padding:0 8px;border-radius:999px;border:1px solid var(--dsw-alias-border-l2);background:transparent;color:var(--dsw-alias-label-secondary);font:inherit;cursor:pointer}
.rth-trajectory .rth-live-dot{width:6px;height:6px;border-radius:50%;background:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-live[aria-pressed=true]{color:var(--rth-k-rth);border-color:currentColor}
.rth-trajectory .rth-live[aria-pressed=true] .rth-live-dot{background:currentColor}
.rth-trajectory:not([data-hidden]) .rth-live[aria-pressed=true][data-stream=live] .rth-live-dot{animation:rth-pulse 1.6s ease-in-out infinite}
.rth-trajectory .rth-live[aria-pressed=true]:not([data-stream=live]){color:var(--dsw-alias-label-secondary);border-color:var(--dsw-alias-border-l2)}
.rth-trajectory .rth-conn{display:inline-flex;align-items:center;gap:4px;color:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-conn[data-stream=stale],.rth-trajectory .rth-conn[data-stream=lost]{color:var(--rth-k-fault)}
.rth-trajectory .rth-live[data-new=yes]{color:var(--rth-k-rth);border-color:currentColor;font-weight:600}
.rth-trajectory .rth-live:focus-visible,.rth-trajectory .rth-chip:focus-visible{outline:1px solid var(--dsw-alias-state-business-primary);outline-offset:1px}
@keyframes rth-pulse{50%{opacity:.35}}
@media (prefers-reduced-motion:reduce){.rth-trajectory .rth-live-dot{animation:none!important}}
.rth-trajectory .rth-stat strong{color:var(--dsw-alias-label-primary);font-weight:600}
.rth-trajectory .rth-sep{flex:1}
.rth-trajectory .rth-clock-ov{padding:4px 8px 6px;border-bottom:.5px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-layer-1);font:var(--dsw-font-xxxs-11);color:var(--dsw-alias-label-caption);max-height:40vh;overflow:auto}
.rth-trajectory .rth-cg{padding:3px 0 2px}
.rth-trajectory .rth-cg+.rth-cg{margin-top:6px;border-top:1px dashed var(--dsw-alias-border-l4);padding-top:5px}
.rth-trajectory .rth-cg-head{font-size:10px;line-height:14px;color:var(--dsw-alias-label-caption);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.rth-trajectory .rth-cg-head strong{font-weight:600;color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-cg-body{display:flex;position:relative}
.rth-trajectory .rth-cg-labels{flex:none;width:38px;display:flex;flex-direction:column;padding-top:2px;font-size:9px;line-height:13px;color:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-cg-track{position:relative;flex:1;min-width:0;margin-right:22px;border-left:.5px solid var(--dsw-alias-border-l2);border-bottom:.5px solid var(--dsw-alias-border-l2)}
.rth-trajectory .rth-cg-ticks{display:flex;height:11px}
.rth-trajectory .rth-cg-tick-pad{flex:none;width:38px}
.rth-trajectory .rth-cg-tickrow{position:relative;flex:1;margin-right:22px}
.rth-trajectory .rth-cg-tickrow span{position:absolute;top:0;transform:translateX(-50%);font-size:9px;line-height:11px;white-space:nowrap;color:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-cg-tickrow span:first-child{transform:none}
.rth-trajectory .rth-ci{position:absolute;height:9px;min-width:3px;padding:0;border:0;border-radius:2px;background:var(--k);opacity:.92;cursor:pointer}
.rth-trajectory .rth-ci[data-shape=tick]{width:3px}
.rth-trajectory .rth-ci[data-shape=zero]{width:3px;outline:1px solid var(--k);outline-offset:1px}
.rth-trajectory .rth-ci[data-shape=start]{width:14px;background:linear-gradient(var(--k) 0 0) left/3px 100% no-repeat,repeating-linear-gradient(135deg,color-mix(in srgb,var(--k) 75%,transparent) 0 2px,transparent 2px 4px)}
.rth-trajectory .rth-ci[data-shape=start]::after{content:"?";position:absolute;left:calc(100% + 2px);top:50%;transform:translateY(-50%);font:600 9px/1 system-ui;color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-ci[data-shape=end]{width:14px;margin-left:-14px;background:linear-gradient(var(--k) 0 0) right/3px 100% no-repeat,repeating-linear-gradient(45deg,color-mix(in srgb,var(--k) 75%,transparent) 0 2px,transparent 2px 4px)}
.rth-trajectory .rth-ci[data-shape=end]::before{content:"?";position:absolute;right:calc(100% + 2px);top:50%;transform:translateY(-50%);font:600 9px/1 system-ui;color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-ci[data-selected]{outline:2px solid var(--dsw-alias-label-primary);outline-offset:1px;z-index:2}
.rth-trajectory .rth-ci[data-match]{box-shadow:0 0 0 2px color-mix(in srgb,var(--dsw-alias-state-business-primary) 70%,transparent)}
.rth-trajectory .rth-ci:focus-visible{outline:2px solid var(--dsw-alias-state-business-primary);outline-offset:1px}
.rth-trajectory .rth-cg-note{margin-top:4px;font-size:10px;line-height:13px;color:var(--dsw-alias-label-caption)}
.rth-trajectory button[hidden]{display:none!important}
.rth-trajectory .rth-kind{gap:3px}
.rth-trajectory .rth-kind>[class*=kindTagIcon]{opacity:1;width:13px;transform:none}
.rth-trajectory [class*="rtjY0dWHa_turnLabel"]{font:600 9px/12px var(--ds-font-family-code,ui-monospace,monospace)}
.rth-trajectory .rth-card{display:inline-flex;gap:4px;align-items:center;margin-left:10px;vertical-align:middle;color:var(--rth-k-rth)}
.rth-trajectory .rth-card-chip{font:var(--dsw-font-xxxs-11);padding:0 6px;border-radius:999px;background:color-mix(in srgb,var(--rth-k-rth) 12%,transparent);color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-card-chip[data-chip=interrupt]{color:var(--rth-k-interrupt);background:color-mix(in srgb,var(--rth-k-interrupt) 14%,transparent)}
.rth-trajectory .rth-card-chip[data-chip=unknown]{color:var(--rth-k-gap);background:transparent;border:1px dashed color-mix(in srgb,var(--rth-k-gap) 50%,transparent)}
.rth-trajectory .rth-notrth{margin:auto;max-width:420px;text-align:center;color:var(--dsw-alias-label-secondary);font:var(--dsw-font-xs-13);padding:24px}
`;
var GROUP_COLOR = { you: "voice", rth: "rth", voice: "speaking", control: "command", signals: "signal", faults: "fault" };
var styled = false;
function ensureStyle() {
  if (styled || typeof document === "undefined") return;
  styled = true;
  const tag = document.createElement("style");
  tag.dataset.plugin = "@rth/dsh-rth-trajectory";
  tag.dataset.pluginCss = "@rth/dsh-rth-trajectory/rth.css";
  tag.textContent = CSS;
  document.head.appendChild(tag);
}
var CONNECTION = { live: "connected", stale: "stream stale (no heartbeat)", lost: "stream lost", opening: "connecting", none: "" };
function StripRow({ data, enabled, onToggle, live = false, newSincePause = 0, onLive }) {
  const s = data.strip;
  const fmt2 = (ms) => ms == null ? null : ms < 1e3 ? `${Math.round(ms)} ms` : `${(ms / 1e3).toFixed(2)} s`;
  return (0, import_jsx_runtime.jsxs)("div", {
    className: "rth-strip",
    "data-rth": "trajectory-strip",
    children: [
      ...GROUPS.map((g) => (0, import_jsx_runtime.jsxs)("span", {
        role: "button",
        tabIndex: 0,
        className: "rth-chip",
        "aria-pressed": enabled.has(g.id),
        style: { "--k": `var(--rth-k-${GROUP_COLOR[g.id]})` },
        onClick: () => onToggle(g.id),
        onKeyDown: (e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle(g.id);
          }
        },
        children: [g.label, " ", (0, import_jsx_runtime.jsx)("b", { children: data.counts[g.id] ?? 0 })]
      }, g.id)),
      (0, import_jsx_runtime.jsx)("span", { className: "rth-sep" }),
      (0, import_jsx_runtime.jsx)("span", {
        className: "rth-conn",
        "data-rth": "trajectory-connection",
        "data-stream": data.stream,
        title: "The Mind stream this view reads: live only while frames arrive within three heartbeats",
        children: CONNECTION[data.stream] ?? data.stream
      }),
      (0, import_jsx_runtime.jsxs)("button", {
        type: "button",
        className: "rth-live",
        "data-rth": "trajectory-live",
        "aria-pressed": live,
        "data-stream": data.stream,
        "data-new": !live && newSincePause > 0 ? "yes" : void 0,
        title: "Follow the newest record (l). Scrolling away pauses it. Keys: j/k move, Enter inspect, / search, f filters, l live",
        onClick: onLive,
        children: [(0, import_jsx_runtime.jsx)("span", { className: "rth-live-dot", "aria-hidden": true }), live ? data.stream === "live" ? "Live" : "Following" : newSincePause > 0 ? `Paused \xB7 ${newSincePause} new` : "Paused"]
      }),
      s ? (0, import_jsx_runtime.jsxs)("span", { className: "rth-stat", children: [(0, import_jsx_runtime.jsx)("strong", { children: s.turns }), " turns"] }) : null,
      s ? (0, import_jsx_runtime.jsxs)("span", { className: "rth-stat", title: `first text received (agent clock: command admitted -> first text); not provider TTFT. Coverage ${s.firstTextCoverage}.`, children: ["first text ", (0, import_jsx_runtime.jsx)("strong", { children: fmt2(s.avgFirstTextMs) ?? "not measured" }), s.avgFirstTextMs == null ? null : (0, import_jsx_runtime.jsx)("span", { className: "rth-clock", children: " (agent clock)" })] }) : null,
      s ? (0, import_jsx_runtime.jsxs)("span", { className: "rth-stat", children: [(0, import_jsx_runtime.jsx)("strong", { children: s.interrupts }), s.interrupts === 1 ? " interrupt" : " interrupts"] }) : null,
      s ? (0, import_jsx_runtime.jsxs)("span", { className: "rth-stat", children: [(0, import_jsx_runtime.jsx)("strong", { children: s.faults }), s.faults === 1 ? " fault" : " faults"] }) : null,
      s ? (0, import_jsx_runtime.jsx)("span", { className: "rth-stat", title: `${s.usageReports} provider usage report(s) seen; they name a connection, not a reply`, children: "tokens: not linked yet" }) : null
    ]
  });
}
var fmtDur = (ms) => ms == null ? "" : ms < 1e3 ? `${Math.round(ms)} ms` : `${(ms / 1e3).toFixed(ms < 1e4 ? 2 : 1)} s`;
var SHAPE_TEXT = { span: (it) => `measured ${fmtDur(it.durationMs)}`, zero: () => "measured 0", start: (it) => `end not observed; duration unavailable${it.why ? ` (${it.why})` : ""}`, end: (it) => `start not observed; duration unavailable${it.why ? ` (${it.why})` : ""}`, tick: () => "an observed instant" };
function ClockOverview({ layout, selectedIndex, searchMatchIndexes, onSelect }) {
  const LANE_H = 13;
  const off = Object.entries(layout.unplaced);
  const offTotal = off.reduce((a, [, n]) => a + n, 0);
  return (0, import_jsx_runtime.jsxs)("section", {
    className: "rth-clock-ov",
    "data-rth": "clock-overview",
    "aria-label": "Duration overview: one group per producer clock and lifetime, each on its own axis; groups are not aligned with each other",
    children: [
      ...layout.groups.map((g) => (0, import_jsx_runtime.jsxs)("div", {
        className: "rth-cg",
        "data-clock": g.clock,
        "data-life": g.life,
        children: [
          (0, import_jsx_runtime.jsxs)("div", { className: "rth-cg-head", children: [(0, import_jsx_runtime.jsx)("strong", { children: g.title }), (0, import_jsx_runtime.jsx)("span", { children: ` \xB7 ${g.axisLabel} \xB7 spans ${fmtDur(g.extentMs)}` })] }),
          (0, import_jsx_runtime.jsxs)("div", {
            className: "rth-cg-body",
            style: { height: g.lanes.length * LANE_H + 4 },
            children: [
              (0, import_jsx_runtime.jsx)("div", { className: "rth-cg-labels", children: g.lanes.map((l) => (0, import_jsx_runtime.jsx)("span", { style: { height: LANE_H }, children: l.name }, l.lane)) }),
              (0, import_jsx_runtime.jsx)("div", {
                className: "rth-cg-track",
                children: g.items.map((it, n) => {
                  const row = g.lanes.findIndex((l) => l.lane === it.lane);
                  const k = KINDS[it.kind];
                  const what = SHAPE_TEXT[it.shape](it);
                  return (0, import_jsx_runtime.jsx)("button", {
                    type: "button",
                    className: "rth-ci",
                    "data-kind": it.kind,
                    "data-shape": it.shape,
                    "data-selected": selectedIndex === it.index || void 0,
                    "data-match": searchMatchIndexes?.has(it.index) || void 0,
                    style: { left: `${it.x0 * 100}%`, width: it.shape === "span" ? `max(2px, ${(it.x1 - it.x0) * 100}%)` : void 0, top: row * LANE_H + 2, "--k": `var(--rth-k-${it.kind})` },
                    title: `${it.name} \xB7 ${what} \xB7 ${g.title}`,
                    "aria-label": `${k?.label ?? it.kind}: ${it.name}, ${what}`,
                    onClick: () => onSelect?.(it.index)
                  }, `${it.recordId}-${n}`);
                })
              })
            ]
          }),
          (0, import_jsx_runtime.jsxs)("div", { className: "rth-cg-ticks", children: [(0, import_jsx_runtime.jsx)("span", { className: "rth-cg-tick-pad" }), (0, import_jsx_runtime.jsx)("div", { className: "rth-cg-tickrow", children: g.ticks.map((t) => (0, import_jsx_runtime.jsx)("span", { style: { left: `${t.x * 100}%` }, children: t.label }, t.ms)) })] })
        ]
      }, `${g.clock}|${g.life}`)),
      layout.groups.length > 1 ? (0, import_jsx_runtime.jsx)("div", { className: "rth-cg-note", children: "Each group has its own clock, origin and scale. Positions in different groups are not comparable in time." }) : null,
      offTotal > 0 ? (0, import_jsx_runtime.jsx)("div", { className: "rth-cg-note", "data-rth": "clock-unplaced", title: off.map(([why, n]) => `${n}: ${why}`).join("\n"), children: `Not placed on a producer clock: ${off.map(([why, n]) => `${n} (${why})`).join("; ")}` }) : null,
      layout.groups.length === 0 ? (0, import_jsx_runtime.jsx)("div", { className: "rth-cg-note", children: "Nothing with a valid producer time yet." }) : null
    ]
  });
}
var BETWEEN = "Between";
var cards = /* @__PURE__ */ new Map();
function setCards(next) {
  cards = next ?? /* @__PURE__ */ new Map();
}
function cardLabel(turn) {
  return cards.get(turn)?.label;
}
function cardChips(turn) {
  const c = cards.get(turn);
  if (!c || !c.chips.length) return null;
  return (0, import_jsx_runtime.jsxs)("span", { className: "rth-card", "data-rth": "turn-card", children: [(0, import_jsx_runtime.jsx)(Icon, { name: c.icon, size: 11 }), ...c.chips.map((chip, i) => (0, import_jsx_runtime.jsx)("span", { className: "rth-card-chip", "data-chip": /interrupt/.test(chip) ? "interrupt" : /not attributed/.test(chip) ? "unknown" : "m", children: chip }, i))] });
}
var fmtMs = (ms) => ms == null ? null : ms < 1e3 ? `${ms.toFixed(ms < 10 ? 2 : 0)} ms` : `${(ms / 1e3).toFixed(2)} s`;
var dl = (key, dt, dd, extra = {}) => (0, import_jsx_runtime.jsxs)("div", { "data-rth-detail": key, children: [(0, import_jsx_runtime.jsx)("dt", { children: dt }), (0, import_jsx_runtime.jsx)("dd", { ...extra, children: dd })] }, key);
function inspectorRows(cell2) {
  const k = KINDS[cell2.rthKind] ?? KINDS.signal;
  const rows = [dl("rth-kind", "Kind", k.label)];
  for (const [i, s] of (cell2.rthTiming ?? []).entries()) {
    const what = s.ms != null ? `${fmtMs(s.ms)}${s.start && s.end ? ` \xB7 ${s.start.kind} \u2192 ${s.end.kind}` : ""}${s.clock ? ` (${s.clock === "host" ? "rth-host" : s.clock} clock)` : ""}` : `not measured: ${s.why ?? "unknown"}`;
    rows.push(dl(`rth-t${i}`, s.name[0].toUpperCase() + s.name.slice(1), what));
  }
  if (cell2.rthKind === "rth") {
    rows.push(dl("rth-play", "Playback", cell2.rthPlayback === "reported" ? "reported by rth-host (handed to the device; not proof it was heard)" : "not reported"));
    rows.push(dl("rth-status", "Outcome", cell2.rthWhy ? `not attributed: ${cell2.rthWhy}` : cell2.rthOutcomeWhy ? cell2.rthOutcomeWhy : cell2.rthOutcome ?? "no end observed"));
    if (cell2.rthCancelledBy?.length) rows.push(dl("rth-cancel", "Cancelled", `the host marked this reply cancelled (local cancellation, not a provider terminal): ${cell2.rthCancelledBy.join(" ")}`));
    if (cell2.rthStopRequested?.length) rows.push(dl("rth-stop", "Stop requested", `a stop was asked or sent (not confirmed; it settles nothing): ${cell2.rthStopRequested.join(" ")}`));
    if (cell2.rthPlaybackCleared?.length) rows.push(dl("rth-cleared", "Playback cleared", `the host cleared this reply's playback (output cleared; not proof the provider stopped): ${cell2.rthPlaybackCleared.join(" ")}`));
    rows.push(dl("rth-tokens", "Tokens", "not available: the agent's usage report names no reply"));
    rows.push(dl("rth-cost", "Cost", "not available: needs per-reply usage with billable categories"));
  }
  rows.push(dl("rth-span", "Overview", cell2.rthSpan === "measured" ? `measured on the ${cell2.rthClock} clock: a bar on that clock's own axis in Duration; a uniform block in Sequence` : cell2.rthSpan === "unknown" ? 'start marker with a hatched "?": its length is not measured' : cell2.rthPlacements?.length ? "an instant: a tick in Duration" : "an observation with no producer time: not placed in Duration"));
  if (cell2.rthKind === "command") rows.push(dl("rth-cmd", "Outcome", cell2.rthOutcomeWhy ? `not attributed: ${cell2.rthOutcomeWhy}` : cell2.rthOutcome ?? "no finish observed"));
  if (cell2.rthKind === "voice" || cell2.rthKind === "typed") rows.push(dl("rth-assoc", "Reply association", cell2.rthAssociation ?? "not available"));
  if (cell2.rthKind === "audio") rows.push(dl("rth-audio", "Playback", "not a playback report: audio the agent generated; whether it played is reported by rth-host separately"));
  const groups = (cell2.rthPlacements ?? []).map((p) => `${p.clock === "host" ? "rth-host" : "agent"} / ${p.life}`);
  rows.push(dl("rth-order", "Position", `Duration: ${groups.length ? `${[...new Set(groups)].join(", ")} (each its own producer axis, relative to its first retained event; groups are not aligned)` : `not placed (${cell2.rthOffAxis ?? "no producer time"})`}. Sequence: observation order on the Mind projection`));
  if (cell2.rthFolded?.length) rows.push(dl("rth-folded", "Folded", (0, import_jsx_runtime.jsx)("span", { children: cell2.rthFolded.map((f) => `${KINDS[f.kind]?.short ?? f.kind}: ${f.text}`).join("\n") }), { style: { whiteSpace: "pre-wrap" } }));
  if (cell2.rthRows?.length) rows.push(dl("rth-ev", "Evidence", cell2.rthRows.join(" "), { style: { fontFamily: "var(--dsw-font-family-mono, ui-monospace, monospace)", fontSize: 11, userSelect: "all", wordBreak: "break-all" } }));
  return rows;
}
var EN_KEYS = {
  ...STATUS_EN,
  "rth.view": "Unfold",
  "rth.tab": "Trajectory",
  "rth.tabGuide": "Every voice, reply, playback, interrupt and signal, on one timeline",
  "rth.notRthSession": "Unfold's trajectory belongs to the Unfold session. Open the Unfold conversation to see what Unfold heard, said and did."
};
var ZH_KEYS = {
  ...STATUS_ZH,
  "rth.view": "Unfold",
  "rth.tab": "\u8F68\u8FF9",
  "rth.tabGuide": "\u6BCF\u4E00\u6BB5\u8BED\u97F3\u3001\u56DE\u590D\u3001\u64AD\u653E\u3001\u6253\u65AD\u548C\u4FE1\u53F7\uFF0C\u90FD\u5728\u540C\u4E00\u6761\u65F6\u95F4\u7EBF\u4E0A",
  "rth.notRthSession": "Unfold \u8F68\u8FF9\u53EA\u5C5E\u4E8E Unfold \u4F1A\u8BDD\u3002\u6253\u5F00 Unfold \u4F1A\u8BDD\u5373\u53EF\u67E5\u770B Unfold \u542C\u5230\u3001\u8BF4\u51FA\u548C\u505A\u8FC7\u7684\u4E8B\u3002"
};
function makeHeaderButton(activate) {
  return function RthTrajectoryHeaderButton({ bot }) {
    const session = bot?.sessionId ?? bot?.id;
    if (session !== RTH_SESSION) return null;
    return (0, import_jsx_runtime.jsxs)("button", {
      type: "button",
      className: "bt-icon-btn rth-traj-btn",
      "data-rth": "trajectory-toggle",
      "aria-label": "Open the Unfold trajectory in the right sidebar",
      title: "Unfold trajectory (what Unfold heard, said and did)",
      onClick: () => activate(),
      style: { display: "inline-flex", alignItems: "center", gap: 4, width: "auto", padding: "0 8px", fontSize: 12 },
      children: [(0, import_jsx_runtime.jsx)(Icon, { name: "audio-waveform", size: 14 }), (0, import_jsx_runtime.jsx)("span", { children: "Trajectory" })]
    });
  };
}

			return module.exports;
		})(require);
		//#region lib/types/client/duration-store.js
		/**
		* Create the browser-wide trajectory duration preference source.
		* @returns a persisted source shared by every session view in one plugin lifecycle.
		*/
		function createTrajectoryDurationStore() {
			return (0, _deepseek_ai_dsh_client_store.createSnapshotStore)(false, { persist: { name: "rth.trajectory.duration" } });
		}
		//#endregion
		//#region lib/types/client/string-wrapping-store.js
		/**
		* Create the browser-wide default for expanded JSON strings.
		* @returns A persisted preference sampled only when a string is expanded.
		*/
		function createTrajectoryStringWrappingStore() {
			return (0, _deepseek_ai_dsh_client_store.createSnapshotStore)(false, { persist: { name: "rth.trajectory.jsonStringWrapping" } });
		}
		//#endregion
		//#region lib/types/client/locales.js
		/** `trajectory` namespace dictionaries for the complete trajectory surface. */
		/** Dictionary namespace owned by this plugin. */
		const NS = "rth-trajectory";
		/** Simplified Chinese dictionary (the key-set source of truth). */
		const zh = {
			...RTH.ZH_KEYS,
			"view.trajectory": "轨迹",
			"toolbar.aria": "轨迹工具栏",
			"toolbar.duration": "时长",
			"toolbar.useActualDuration": "使用实际时长",
			"toolbar.useEqualWidth": "使用等宽操作",
			"toolbar.actualTime": "实际时间",
			"toolbar.turns": "轮次",
			"toolbar.expandTurns": "展开所有轮次",
			"toolbar.collapseTurns": "收起所有轮次",
			"toolbar.calls": "调用",
			"toolbar.expandCalls": "展开所有调用",
			"toolbar.collapseCalls": "收起所有调用",
			"toolbar.search": "搜索轨迹",
			"toolbar.searchPlaceholder": "搜索",
			"kind.system": "系统",
			"kind.user": "用户",
			"kind.context": "上下文",
			"kind.compacted": "已压缩",
			"kind.message": "消息",
			"kind.assistant": "助手",
			"kind.tool": "工具",
			"kind.subtool": "子工具",
			"kind.sub": "子项",
			"column.input": "输入",
			"column.output": "输出",
			"column.think": "思考",
			"column.time": "时间",
			"column.model": "模型",
			"column.tools": "工具",
			"turn.label": "第 {turn} 轮",
			"section.betweenTurns": "轮次之间",
			"group.message": "消息",
			"group.step": "第 {step} 步",
			"group.compaction": "压缩 {seq}",
			"status.failed": "失败",
			"status.pending": "等待中",
			"status.completed": "已完成",
			"timing.notAvailable": "不可用",
			"timing.notRecorded": "未记录",
			"timing.stepStartUnavailable": "步骤开始时间不可用",
			"timing.firstTokenUnavailable": "首 token 时间不可用",
			"timing.usageUnavailable": "用量不可用",
			"timing.outputTokensUnavailable": "输出 token 数不可用",
			"timing.durationTooShort": "时长过短",
			"timing.showLocalTime": "显示本地时间",
			"timing.showUnixTimestamp": "显示 Unix 时间戳",
			"timing.started": "开始时间",
			"timing.totalDuration": "总时长",
			"timing.ttft": "首 token 延迟",
			"timing.generation": "生成",
			"timing.throughput": "吞吐量",
			"timing.duration": "时长",
			"timing.source": "计时来源",
			"timing.sessionTimestamps": "会话时间戳",
			"timing.sessionTimestampsRunning": "会话时间戳（运行中）",
			"timing.request": "请求计时",
			"unit.milliseconds": "{value} 毫秒",
			"unit.seconds": "{value} 秒",
			"unit.tokens": "{value} tok",
			"unit.tokensPerSecond": "{value} tok/s",
			"usage.tokens": "Token",
			"usage.reasoning": "推理",
			"usage.content": "内容",
			"usage.notReported": "未报告用量",
			"usage.input": "输入",
			"usage.cached": "缓存读取",
			"usage.cacheCreated": "缓存写入",
			"usage.other": "其他",
			"usage.output": "输出",
			"usage.thisRequest": "本次请求",
			"usage.sessionCumulative": "会话累计",
			"options.notRecorded": "未记录选项",
			"options.json": "请求选项 JSON",
			"source.unknown": "未知",
			"source.user": "用户",
			"source.plugin": "插件",
			"source.pluginNamed": "插件 · {plugin}",
			"source.goal": "目标",
			"source.goalRound": "目标 · Round {round}",
			"source.notRecorded": "未记录来源",
			"source.messageJson": "消息来源 JSON",
			"tab.summary": "概述",
			"tab.rawOutput": "原始输出",
			"tab.preview": "预览",
			"tab.raw": "原始内容",
			"tab.source": "来源",
			"tab.payload": "参数",
			"tab.result": "结果",
			"tab.schema": "Schema",
			"tab.timing": "计时",
			"tab.diff": "差异",
			"tab.systemPrompt": "系统提示词",
			"tab.tools": "工具",
			"tab.options": "选项",
			"tab.usage": "用量",
			"record.toolCallOnly": "（仅工具调用）",
			"record.noContent": "无内容",
			"record.noPayload": "未捕获参数",
			"record.noResult": "未捕获结果",
			"record.noOutput": "无输出",
			"record.schemaUnavailable": "Schema 不可用",
			"record.parameters": "参数",
			"record.resultJson": "结果 JSON",
			"record.json": "JSON",
			"record.parametersJson": "参数 JSON",
			"record.namedParametersJson": "{name} 参数 JSON",
			"record.payloadJson": "参数 JSON",
			"record.outputJson": "结果 JSON",
			"record.thinking": "思考",
			"record.wrapLines": "自动换行",
			"code.source": "代码",
			"code.output": "输出",
			"code.copySource": "复制代码",
			"code.copyOutput": "复制输出",
			"code.originalJson": "原始 JSON",
			"code.running": "执行中…",
			"record.systemPromptMissing": "本次请求没有系统提示词",
			"record.toolsMissing": "本次请求没有工具",
			"record.systemPrompt": "系统提示词",
			"record.tools": "工具",
			"block.openSummary": "打开第 {index} 个块的工具调用概述",
			"block.openSummaryTitle": "打开工具调用概述",
			"block.label": "块 #{index} {type}",
			"history.loadingTrajectory": "正在加载轨迹…",
			"history.loadingEarlier": "正在加载更早的历史…",
			"history.loadingEarlierAria": "正在加载更早的历史…",
			"history.loadEarlier": "加载更早的历史",
			"history.clickToLoadEarlier": "点击加载更早的历史",
			"request.label": "请求 #{request}",
			"request.labelCompaction": "请求 #{request} · 压缩",
			"request.compaction": "压缩 · {section}",
			"request.compactionPurpose": "压缩",
			"request.retryProgress": "{retry}/{maximum}",
			"request.collapsedSummary": "已收起的{kind}概述，{summary}",
			"request.collapsedTurn": "轮次",
			"request.collapsedAssistant": "助手",
			"request.rowAria": "{request}{kind}，{content}",
			"request.rowPrefix": "请求 {request}，",
			"request.rowAriaCompaction": "请求 {request}，压缩",
			"request.noContent": "无内容",
			"summary.toolCalls.one": "{count} 个工具调用",
			"summary.toolCalls.other": "{count} 个工具调用",
			"summary.steps.one": "{count} 个步骤",
			"summary.steps.other": "{count} 个步骤",
			"details.event": "事件详情",
			"details.resize": "调整事件详情宽度",
			"details.resizeTitle": "拖动调整大小；双击恢复默认值。",
			"details.close": "关闭详情",
			"details.status": "状态",
			"details.purpose": "用途",
			"details.provider": "提供方",
			"details.model": "模型",
			"details.toolCalls": "工具调用",
			"details.subtoolCalls": "子工具调用",
			"details.error": "错误",
			"details.lastAttemptError": "上次尝试错误",
			"details.failure.auth": "API 密钥无效",
			"details.retry": "重试",
			"details.retryDelay": "重试延迟",
			"details.result": "结果",
			"details.compacted": "已压缩",
			"details.assistantMessage": "助手消息",
			"details.source": "来源",
			"details.hierarchy": "层级",
			"details.toolCall": "工具调用",
			"timeline.aria": "轨迹时间线",
			"timeline.overviewAria": "时间线概览；水平拖动可聚焦事件",
			"timeline.noTimingData": "无计时数据",
			"timeline.total": "总计 {duration}",
			"timeline.started": "开始于 {time}",
			"timeline.ttftDecoding": "首 token {ttft} · 解码 {decoding}",
			"layout.compacting": "正在压缩上下文…",
			"layout.compactionFailed": "上下文压缩失败",
			"layout.compacted": "上下文已压缩",
			"layout.toolCallOnly": "仅工具调用",
			"attachment.list": "附件",
			"attachment.imageName": "图片 {index}",
			"layout.imageCount": "图片 ×{count}",
			"layout.fileAttachments": "文件 ×{count}",
			"layout.initialSystemPrompt": "初始系统提示词",
			"layout.systemPromptUpdated": "系统提示词已更新",
			"layout.toolsUpdated": "工具已更新",
			"layout.toolAdded": "已添加工具：{name}",
			"layout.toolRemoved": "已移除工具：{name}",
			"layout.toolUpdateNotice": "工具已更新",
			"layout.toolsAdded": "新增：{names}",
			"layout.toolsAddedCount": "新增 {count} 个",
			"layout.toolsChanged": "新增 {added} 个，移除 {removed} 个",
			"layout.toolsRemoved": "移除：{names}",
			"layout.toolsRemovedCount": "移除 {count} 个",
			"layout.systemPromptAndToolsUpdated": "系统提示词和工具已更新",
			"layout.compactionInterrupted": "上下文压缩在完成前被中断。"
		};
		/** English dictionary, checked complete against the Chinese source of truth. */
		const en = {
			...RTH.EN_KEYS,
			"view.trajectory": "Trajectory",
			"toolbar.aria": "Trajectory toolbar",
			"toolbar.duration": "Duration",
			"toolbar.useActualDuration": "Use actual duration",
			"toolbar.useEqualWidth": "Use equal-width operations",
			"toolbar.actualTime": "Actual time",
			"toolbar.turns": "Turns",
			"toolbar.expandTurns": "Expand turns",
			"toolbar.collapseTurns": "Collapse turns",
			"toolbar.calls": "Calls",
			"toolbar.expandCalls": "Expand calls",
			"toolbar.collapseCalls": "Collapse calls",
			"toolbar.search": "Search trajectory",
			"toolbar.searchPlaceholder": "Search",
			"kind.system": "SYSTEM",
			"kind.user": "USER",
			"kind.context": "CONTEXT",
			"kind.compacted": "COMPACTED",
			"kind.message": "Message",
			"kind.assistant": "ASSISTANT",
			"kind.tool": "TOOL",
			"kind.subtool": "SUBTOOL",
			"kind.sub": "Sub",
			"column.input": "Input",
			"column.output": "Output",
			"column.think": "Think",
			"column.time": "Time",
			"column.model": "Model",
			"column.tools": "Tools",
			"turn.label": "Turn {turn}",
			"section.betweenTurns": "Between turns",
			"group.message": "Message",
			"group.step": "Step {step}",
			"group.compaction": "Compaction {seq}",
			"status.failed": "Failed",
			"status.pending": "Pending",
			"status.completed": "Completed",
			"timing.notAvailable": "Not available",
			"timing.notRecorded": "Not recorded",
			"timing.stepStartUnavailable": "Step start unavailable",
			"timing.firstTokenUnavailable": "First token unavailable",
			"timing.usageUnavailable": "Usage unavailable",
			"timing.outputTokensUnavailable": "Output tokens unavailable",
			"timing.durationTooShort": "Duration too short",
			"timing.showLocalTime": "Show local time",
			"timing.showUnixTimestamp": "Show Unix timestamp",
			"timing.started": "Started",
			"timing.totalDuration": "Total duration",
			"timing.ttft": "TTFT",
			"timing.generation": "Generation",
			"timing.throughput": "Throughput",
			"timing.duration": "Duration",
			"timing.source": "Timing source",
			"timing.sessionTimestamps": "Session timestamps",
			"timing.sessionTimestampsRunning": "Session timestamps (running)",
			"timing.request": "Request Timing",
			"unit.milliseconds": "{value} ms",
			"unit.seconds": "{value} s",
			"unit.tokens": "{value} tok",
			"unit.tokensPerSecond": "{value} tok/s",
			"usage.tokens": "Tokens",
			"usage.reasoning": "Reasoning",
			"usage.content": "Content",
			"usage.notReported": "Usage not reported",
			"usage.input": "Input",
			"usage.cached": "Cached",
			"usage.cacheCreated": "Cache created",
			"usage.other": "Other",
			"usage.output": "Output",
			"usage.thisRequest": "This request",
			"usage.sessionCumulative": "Session cumulative",
			"options.notRecorded": "Options not recorded",
			"options.json": "Request options JSON",
			"source.unknown": "Unknown",
			"source.user": "User",
			"source.plugin": "Plugin",
			"source.pluginNamed": "Plugin · {plugin}",
			"source.goal": "Goal",
			"source.goalRound": "Goal · Round {round}",
			"source.notRecorded": "Source not recorded",
			"source.messageJson": "Message source JSON",
			"tab.summary": "Summary",
			"tab.rawOutput": "Raw Output",
			"tab.preview": "Preview",
			"tab.raw": "Raw",
			"tab.source": "Source",
			"tab.payload": "Payload",
			"tab.result": "Result",
			"tab.schema": "Schema",
			"tab.timing": "Timing",
			"tab.diff": "Diff",
			"tab.systemPrompt": "System Prompt",
			"tab.tools": "Tools",
			"tab.options": "Options",
			"tab.usage": "Usage",
			"record.toolCallOnly": "(tool call only)",
			"record.noContent": "No content",
			"record.noPayload": "No payload captured",
			"record.noResult": "No result captured",
			"record.noOutput": "No output",
			"record.schemaUnavailable": "Schema unavailable",
			"record.parameters": "Parameters",
			"record.resultJson": "Result JSON",
			"record.json": "JSON",
			"record.parametersJson": "parameters JSON",
			"record.namedParametersJson": "{name} parameters JSON",
			"record.payloadJson": "Payload JSON",
			"record.outputJson": "Result JSON",
			"record.thinking": "Thinking",
			"record.wrapLines": "Wrap lines",
			"code.source": "Code",
			"code.output": "Output",
			"code.copySource": "Copy code",
			"code.copyOutput": "Copy output",
			"code.originalJson": "Original JSON",
			"code.running": "Running…",
			"record.systemPromptMissing": "No system prompt in this request",
			"record.toolsMissing": "No tools in this request",
			"record.systemPrompt": "System Prompt",
			"record.tools": "Tools",
			"block.openSummary": "Open Block #{index} tool call summary",
			"block.openSummaryTitle": "Open tool call summary",
			"block.label": "Block #{index} {type}",
			"history.loadingTrajectory": "Loading trajectory…",
			"history.loadingEarlier": "Loading earlier history…",
			"history.loadingEarlierAria": "Loading earlier history…",
			"history.loadEarlier": "Load earlier history",
			"history.clickToLoadEarlier": "Click to load earlier history",
			"request.label": "Request #{request}",
			"request.labelCompaction": "Request #{request} · Compaction",
			"request.compaction": "Compaction · {section}",
			"request.compactionPurpose": "Compaction",
			"request.retryProgress": "{retry} of {maximum}",
			"request.collapsedSummary": "Collapsed {kind} summary, {summary}",
			"request.collapsedTurn": "turn",
			"request.collapsedAssistant": "assistant",
			"request.rowAria": "{request}{kind}, {content}",
			"request.rowPrefix": "Request {request}, ",
			"request.rowAriaCompaction": "Request {request}, compaction",
			"request.noContent": "no content",
			"summary.toolCalls.one": "{count} tool call",
			"summary.toolCalls.other": "{count} tool calls",
			"summary.steps.one": "{count} step",
			"summary.steps.other": "{count} steps",
			"details.event": "Event details",
			"details.resize": "Resize event details",
			"details.resizeTitle": "Drag to resize. Double-click to reset.",
			"details.close": "Close details",
			"details.status": "Status",
			"details.purpose": "Purpose",
			"details.provider": "Provider",
			"details.model": "Model",
			"details.toolCalls": "Tool calls",
			"details.subtoolCalls": "Subtool calls",
			"details.error": "Error",
			"details.lastAttemptError": "Last attempt error",
			"details.failure.auth": "API key is invalid",
			"details.retry": "Retry",
			"details.retryDelay": "Retry delay",
			"details.result": "Result",
			"details.compacted": "Compacted",
			"details.assistantMessage": "Assistant Message",
			"details.source": "Source",
			"details.hierarchy": "Hierarchy",
			"details.toolCall": "Tool Call",
			"timeline.aria": "Trajectory timeline",
			"timeline.overviewAria": "Timeline overview; drag horizontally to focus events",
			"timeline.noTimingData": "No timing data",
			"timeline.total": "Total {duration}",
			"timeline.started": "Started {time}",
			"timeline.ttftDecoding": "TTFT {ttft} · Decoding {decoding}",
			"layout.compacting": "Compacting context…",
			"layout.compactionFailed": "Compaction failed",
			"layout.compacted": "Context compacted",
			"layout.toolCallOnly": "Tool call only",
			"attachment.list": "Attachments",
			"attachment.imageName": "Image {index}",
			"layout.imageCount": "Images ×{count}",
			"layout.fileAttachments": "Files ×{count}",
			"layout.initialSystemPrompt": "Initial System Prompt",
			"layout.systemPromptUpdated": "System Prompt Updated",
			"layout.toolsUpdated": "Tools Updated",
			"layout.toolAdded": "Tool added: {name}",
			"layout.toolRemoved": "Tool removed: {name}",
			"layout.toolUpdateNotice": "Tools updated",
			"layout.toolsAdded": "Added: {names}",
			"layout.toolsAddedCount": "{count} added",
			"layout.toolsChanged": "{added} added, {removed} removed",
			"layout.toolsRemoved": "Removed: {names}",
			"layout.toolsRemovedCount": "{count} removed",
			"layout.systemPromptAndToolsUpdated": "System Prompt and Tools Updated",
			"layout.compactionInterrupted": "Compaction was interrupted before completion."
		};
		//#endregion
		//#region ../../util/values/src/partial-json.ts
		/**
		* Lazily scanned view of one JSON object's top-level fields, built from text
		* that may still be streaming or from an already parsed object. Nothing is
		* scanned until a reader asks; the view remembers every question it answered
		* and reports changed answers when the owner refreshes for publication.
		* Used for model tool-call arguments: a row reads the fields it
		* cares about at whatever granularity it displays, at every stage of the call.
		* @module @deepseek-ai/dsh-util-values/src/partial-json
		*/
		const SIMPLE_ESCAPES = {
			"\"": "\"",
			"\\": "\\",
			"/": "/",
			b: "\b",
			f: "\f",
			n: "\n",
			r: "\r",
			t: "	"
		};
		const CONTENT_ESCAPE = /[\\\u0000-\u001f]/u;
		function isWhitespace(c) {
			return c === " " || c === "\n" || c === "\r" || c === "	";
		}
		function isHex(c) {
			return c >= "0" && c <= "9" || c >= "a" && c <= "f" || c >= "A" && c <= "F";
		}
		/**
		* The view. A streaming instance grows through {@link PartialArguments.append};
		* {@link PartialArguments.fromText} and {@link PartialArguments.fromObject} build
		* sealed instances over a finished call. Every reader is total: an absent or
		* differently typed field answers `undefined` (or `false`), never throws.
		*/
		var PartialArguments = class PartialArguments {
			/** The view of a call with no arguments available. */
			static EMPTY = PartialArguments.fromObject({});
			/**
			* View finished argument text without scanning it until a reader asks.
			* @param text - the complete argument JSON text.
			* @returns a sealed view.
			*/
			static fromText(text) {
				const view = new PartialArguments();
				view.append(text);
				view.sealed = true;
				return view;
			}
			/**
			* View an already parsed argument payload, such as a PTC dispatch object.
			* @param value - the parsed argument value.
			* @returns a sealed view; a non-object payload has no fields.
			*/
			static fromObject(value) {
				const view = new PartialArguments();
				view.object = typeof value === "object" && value !== null && !Array.isArray(value) ? value : {};
				view.sealed = true;
				return view;
			}
			/**
			* The source: text so far or a parsed object, plus whether it can still grow.
			* These are the only enumerable fields, so two views over the same source
			* compare equal structurally however far each has been read.
			*/
			chunks = [];
			object;
			sealed = false;
			#ends = [];
			#size = 0;
			#consumed = 0;
			#mode = "root";
			#escape = false;
			#keyStart = 0;
			#keyEscaped = false;
			#key = "";
			#current = null;
			#nestedEnds = [];
			#nestedInString = false;
			#invalidAt;
			#invalidValue = false;
			#entries = /* @__PURE__ */ new Map();
			#order = [];
			#reads = /* @__PURE__ */ new Map();
			/** Whether this view rejects further appends; does not scan text or register reads. */
			get isSealed() {
				return this.sealed;
			}
			/** Whether indexing or a content read found invalid JSON; unread value contents are not validated. */
			get invalid() {
				this.scan();
				return this.#mode === "invalid" || this.#invalidValue;
			}
			/**
			* Retain streamed argument text without scanning or comparing observed answers.
			* @param fragment - the text following every fragment appended before.
			*/
			append(fragment) {
				if (this.sealed) throw new Error("PartialArguments: cannot append to a sealed view");
				if (fragment.length === 0) return;
				this.chunks.push(fragment);
				this.#size += fragment.length;
				this.#ends.push(this.#size);
			}
			/**
			* Reconcile a streamed prefix with authoritative complete text without joining the fragments.
			* @param text - the final argument text, which replaces missing or conflicting deltas.
			* @returns this view sealed with its caches retained when every character matches; otherwise a new sealed view.
			*/
			settle(text) {
				if (this.object !== void 0 || text.length !== this.#size) return PartialArguments.fromText(text);
				let offset = 0;
				for (const chunk of this.chunks) {
					if (!text.startsWith(chunk, offset)) return PartialArguments.fromText(text);
					offset += chunk.length;
				}
				this.chunks = text.length === 0 ? [] : [text];
				this.#ends = text.length === 0 ? [] : [text.length];
				this.sealed = true;
				return this;
			}
			/**
			* Compare observed answers and advance their publication baseline. Unread views remain unscanned.
			* @returns whether any observed answer changed since its first read or the preceding refresh.
			*/
			refresh() {
				if (this.#reads.size === 0) return false;
				this.scan();
				let changed = false;
				let completions = false;
				for (const read of this.#reads.values()) {
					if (read.completion) {
						completions = true;
						continue;
					}
					changed = this.refreshRead(read) || changed;
				}
				if (completions) {
					for (const read of this.#reads.values()) if (read.completion) changed = this.refreshRead(read) || changed;
				}
				if (this.sealed) this.#reads.clear();
				return changed;
			}
			refreshRead(read) {
				const now = read.answer();
				if (Object.is(now, read.last)) return false;
				read.last = now;
				return true;
			}
			/**
			* Check whether no further fields can arrive.
			* @returns whether the outer object closed, indexing failed, or the view is sealed; unread values are not validated.
			*/
			closed() {
				return this.remember("closed", "", () => this.closedNow());
			}
			/**
			* List discovered fields in first-appearance order.
			* @returns top-level keys seen so far, in first-appearance order.
			*/
			keys() {
				return this.remember("keys", "", () => this.keysNow(), (keys) => keys.length);
			}
			/**
			* Check whether a top-level field has appeared.
			* @param key - argument name.
			* @returns whether the field has appeared (a string opened or another value began).
			*/
			has(key) {
				return this.remember("has", key, () => this.hasNow(key));
			}
			/**
			* Check whether a field's closing delimiter has arrived, without validating its contents.
			* @param key - argument name.
			* @returns whether its delimiter arrived and no content reader has reported an error for this value.
			*/
			complete(key) {
				return this.remember("complete", key, () => this.completeNow(key));
			}
			/**
			* Read string length without materializing its text.
			* @param key - argument name.
			* @param options - change granularity for a streaming string.
			* @returns decoded UTF-16 length of the string field so far; undefined when absent or not a string.
			*/
			stringLength(key, options) {
				const step = Math.max(1, Math.floor(options?.step ?? 1));
				const offset = options?.offset ?? 0;
				return this.remember(`length:${step}:${offset}`, key, () => this.lengthNow(key), (length) => length === void 0 ? void 0 : Math.ceil((length + offset) / step));
			}
			/**
			* Check a string against a decoded UTF-16 length limit without materializing it.
			* @param key - argument name.
			* @param maxLength - decoded UTF-16 limit, floored to at least zero.
			* @returns whether the string is longer than the limit; false when absent or not a string.
			*/
			stringExceeds(key, maxLength) {
				const limit = Math.max(0, Math.floor(maxLength));
				return this.remember(`exceeds:${limit}`, key, () => (this.lengthNow(key, limit + 1) ?? 0) > limit);
			}
			/**
			* Read a decoded string, including a streaming prefix.
			* @param key - argument name.
			* @returns the string field's decoded text so far; undefined when absent or not a string.
			*/
			text(key) {
				return this.remember("text", key, () => this.textNow(key));
			}
			/**
			* Read at most the first decoded UTF-16 units of a string.
			* @param key - argument name.
			* @param maxLength - maximum decoded UTF-16 length, floored to at least one.
			* @returns the bounded string prefix; undefined when absent or not a string.
			*/
			textPrefix(key, maxLength) {
				const limit = Math.max(1, Math.floor(maxLength));
				return this.remember(`prefix:${limit}`, key, () => this.textPrefixNow(key, limit));
			}
			/**
			* Read a completed non-string argument.
			* @param key - argument name.
			* @returns the parsed non-string value once it closed; undefined while open, absent, or a string.
			*/
			value(key) {
				return this.remember("value", key, () => this.valueNow(key));
			}
			/** Answer a question and, on a streaming view, remember it for change detection. */
			remember(kind, key, read, comparison) {
				this.scan();
				const result = read();
				if (!this.sealed) {
					const id = `${kind}/${key}`;
					if (!this.#reads.has(id)) this.#reads.set(id, {
						completion: kind === "complete",
						answer: comparison === void 0 ? read : () => comparison(read()),
						last: comparison === void 0 ? result : comparison(result)
					});
				}
				return result;
			}
			closedNow() {
				return this.sealed || this.#mode === "closed" || this.#mode === "invalid";
			}
			keysNow() {
				return this.object === void 0 ? this.#order : Object.keys(this.object);
			}
			hasNow(key) {
				return this.object === void 0 ? this.#entries.has(key) : Object.hasOwn(this.object, key);
			}
			completeNow(key) {
				if (this.object !== void 0) return Object.hasOwn(this.object, key);
				const entry = this.#entries.get(key);
				return entry !== void 0 && entry.end >= 0 && (entry.kind === "string" ? entry.invalidAt === void 0 : !entry.invalid);
			}
			lengthNow(key, limit = Number.POSITIVE_INFINITY) {
				if (this.object !== void 0) {
					const field = Object.hasOwn(this.object, key) ? this.object[key] : void 0;
					return typeof field === "string" ? field.length : void 0;
				}
				const entry = this.#entries.get(key);
				if (entry?.kind !== "string") return void 0;
				if (entry.text !== void 0 && entry.text.at === entry.end) return entry.text.length;
				const read = entry.length ??= {
					at: entry.start,
					length: 0,
					text: ""
				};
				this.readString(entry, read, limit, false);
				return read.length;
			}
			textNow(key) {
				if (this.object !== void 0) {
					const field = Object.hasOwn(this.object, key) ? this.object[key] : void 0;
					return typeof field === "string" ? field : void 0;
				}
				const entry = this.#entries.get(key);
				if (entry?.kind !== "string") return void 0;
				if (entry.text === void 0 && entry.end >= 0 && entry.needsDecoding && entry.invalidAt === void 0) {
					let text;
					try {
						text = JSON.parse(`"${this.slice(entry.start, entry.end)}"`);
					} catch (_error) {}
					if (text !== void 0) entry.text = {
						at: entry.end,
						length: text.length,
						text
					};
				}
				const read = entry.text ??= {
					at: entry.start,
					length: 0,
					text: ""
				};
				this.readString(entry, read, Number.POSITIVE_INFINITY, true);
				return read.text;
			}
			textPrefixNow(key, maxLength) {
				if (this.object !== void 0) {
					const field = Object.hasOwn(this.object, key) ? this.object[key] : void 0;
					return typeof field === "string" ? field.slice(0, maxLength) : void 0;
				}
				const entry = this.#entries.get(key);
				if (entry?.kind !== "string") return void 0;
				const prefixes = entry.prefixes ??= /* @__PURE__ */ new Map();
				let read = prefixes.get(maxLength);
				if (read === void 0) {
					read = {
						at: entry.start,
						length: 0,
						text: ""
					};
					prefixes.set(maxLength, read);
				}
				this.readString(entry, read, maxLength, true);
				return read.text;
			}
			valueNow(key) {
				if (this.object !== void 0) {
					if (!Object.hasOwn(this.object, key)) return void 0;
					const field = this.object[key];
					return typeof field === "string" ? void 0 : field;
				}
				const entry = this.#entries.get(key);
				if (entry?.kind !== "value" || entry.end < 0 || entry.invalid) return void 0;
				if (entry.parsed === void 0) try {
					entry.parsed = JSON.parse(this.slice(entry.start, entry.end));
				} catch (_error) {
					entry.invalid = true;
					this.#invalidValue = true;
				}
				return entry.parsed;
			}
			chunkAt(at) {
				let low = 0;
				let high = this.#ends.length;
				while (low < high) {
					const mid = low + high >>> 1;
					if (this.#ends[mid] <= at) low = mid + 1;
					else high = mid;
				}
				return low;
			}
			/** Materialize only a requested range, never the cumulative source. */
			slice(start, end) {
				if (start >= end) return "";
				const first = this.chunkAt(start);
				const last = this.chunkAt(end - 1);
				const base = first === 0 ? 0 : this.#ends[first - 1];
				if (first === last) return this.chunks[first].slice(start - base, end - base);
				const parts = [this.chunks[first].slice(start - base)];
				for (let i = first + 1; i < last; i++) parts.push(this.chunks[i]);
				parts.push(this.chunks[last].slice(0, end - this.#ends[last - 1]));
				return parts.join("");
			}
			readString(entry, read, limit, materialize) {
				const end = Math.min(entry.end < 0 ? this.#consumed : entry.end, entry.invalidAt ?? Number.POSITIVE_INFINITY, this.#invalidAt ?? Number.POSITIVE_INFINITY);
				if (!entry.needsDecoding) {
					const length = Math.min(end - read.at, limit - read.length);
					if (length <= 0) return;
					if (materialize) read.text += this.slice(read.at, read.at + length);
					read.at += length;
					read.length += length;
					return;
				}
				let chunkIndex = this.chunkAt(read.at);
				while (read.at < end && read.length < limit) {
					const base = chunkIndex === 0 ? 0 : this.#ends[chunkIndex - 1];
					const chunk = this.chunks[chunkIndex];
					const remaining = chunk.slice(read.at - base, Math.min(chunk.length, end - base));
					const boundary = remaining.search(CONTENT_ESCAPE);
					const length = Math.min(boundary < 0 ? remaining.length : boundary, limit - read.length);
					if (length > 0) {
						if (materialize) read.text += remaining.slice(0, length);
						read.at += length;
						read.length += length;
						if (read.at === base + chunk.length) chunkIndex++;
						continue;
					}
					const type = remaining.length > 1 ? remaining[1] : read.at + 1 < end ? this.chunks[chunkIndex + 1][0] : void 0;
					let decoded;
					let width = 2;
					if (remaining[0] === "\\" && type === void 0 && entry.end < 0) return;
					if (remaining[0] === "\\" && type === "u") {
						const hex = this.slice(read.at + 2, Math.min(end, read.at + 6));
						let valid = true;
						for (let i = 0; i < hex.length; i++) if (!isHex(hex[i])) valid = false;
						if (valid) {
							if (hex.length < 4 && entry.end < 0) return;
							if (hex.length === 4) decoded = String.fromCharCode(Number.parseInt(hex, 16));
						}
						width = 6;
					} else if (remaining[0] === "\\" && type !== void 0) decoded = SIMPLE_ESCAPES[type];
					if (decoded === void 0) {
						entry.invalidAt = read.at;
						this.#invalidValue = true;
						return;
					}
					if (materialize) read.text += decoded;
					read.length++;
					read.at += width;
					while (chunkIndex < this.chunks.length && read.at >= this.#ends[chunkIndex]) chunkIndex++;
				}
			}
			/** Locate new field ranges without decoding or parsing their contents. */
			scan() {
				if (this.object !== void 0 || this.#consumed === this.#size) return;
				for (let i = this.chunkAt(this.#consumed); i < this.chunks.length && this.#invalidAt === void 0; i++) {
					const pending = this.chunks[i];
					const base = i === 0 ? 0 : this.#ends[i - 1];
					for (let index = this.#consumed - base; index < pending.length && this.#mode !== "invalid"; index++) {
						if (this.#mode === "string" || this.#mode === "nested" && this.#nestedInString) {
							const end = this.stringBoundary(pending, index);
							this.#consumed += end - index;
							index = end;
							if (index === pending.length) break;
						}
						this.step(pending[index], this.#consumed);
						this.#consumed++;
					}
				}
			}
			/** Only raw quotes and their preceding backslash runs can terminate a string. */
			stringBoundary(fragment, start) {
				let at = start;
				while (true) {
					const quote = fragment.indexOf("\"", at);
					const end = quote < 0 ? fragment.length : quote;
					if (this.#mode === "string") {
						const entry = this.#current;
						if (!entry.needsDecoding && CONTENT_ESCAPE.test(fragment.slice(at, end))) entry.needsDecoding = true;
					}
					let slashStart = end;
					while (slashStart > at && fragment[slashStart - 1] === "\\") slashStart--;
					const escaped = (end - slashStart) % 2 === 1 !== (slashStart === at && this.#escape);
					this.#escape = quote < 0 && escaped;
					if (quote < 0 || !escaped) return end;
					at = quote + 1;
				}
			}
			step(c, at) {
				switch (this.#mode) {
					case "root":
						if (isWhitespace(c)) return;
						if (c === "{") {
							this.#mode = "key-or-end";
							return;
						}
						this.fail();
						return;
					case "key-or-end":
						if (isWhitespace(c)) return;
						if (c === "}") {
							this.#mode = "closed";
							return;
						}
						if (c === "\"") {
							this.beginKey(at);
							return;
						}
						this.fail();
						return;
					case "key-only":
						if (isWhitespace(c)) return;
						if (c === "\"") {
							this.beginKey(at);
							return;
						}
						this.fail();
						return;
					case "key":
						this.stepKey(c, at);
						return;
					case "colon":
						if (isWhitespace(c)) return;
						if (c === ":") {
							this.#mode = "value";
							return;
						}
						this.fail();
						return;
					case "value":
						this.beginValue(c, at);
						return;
					case "string": {
						const entry = this.#current;
						entry.end = at;
						this.#current = null;
						this.#mode = "comma-or-end";
						return;
					}
					case "scalar":
						this.stepScalar(c, at);
						return;
					case "nested":
						this.stepNested(c, at);
						return;
					case "comma-or-end":
						if (isWhitespace(c)) return;
						if (c === ",") {
							this.#mode = "key-only";
							return;
						}
						if (c === "}") {
							this.#mode = "closed";
							return;
						}
						this.fail();
						return;
					case "closed":
						if (isWhitespace(c)) return;
						this.fail();
						return;
					/* v8 ignore next 2 -- scan() stops stepping once the view is invalid. */
					case "invalid": return;
					/* v8 ignore next 2 -- Every scanner mode has a handler above. */
					default: assertNever(this.#mode);
				}
			}
			fail() {
				this.#invalidAt = this.#consumed;
				this.#mode = "invalid";
				this.#current = null;
			}
			beginKey(at) {
				this.#mode = "key";
				this.#keyStart = at + 1;
				this.#keyEscaped = false;
				this.#escape = false;
			}
			stepKey(c, at) {
				if (c < " ") {
					this.fail();
					return;
				}
				if (this.#escape) {
					this.#escape = false;
					return;
				}
				if (c === "\\") {
					this.#escape = true;
					this.#keyEscaped = true;
					return;
				}
				if (c !== "\"") return;
				const raw = this.slice(this.#keyStart, at);
				if (this.#keyEscaped) try {
					this.#key = JSON.parse(`"${raw}"`);
				} catch (_error) {
					this.fail();
					return;
				}
				else this.#key = raw;
				this.#mode = "colon";
			}
			open(entry) {
				if (!this.#entries.has(this.#key)) this.#order.push(this.#key);
				this.#entries.set(this.#key, entry);
				this.#current = entry;
			}
			beginValue(c, at) {
				if (isWhitespace(c)) return;
				if (c === "\"") {
					this.open({
						kind: "string",
						start: at + 1,
						end: -1,
						needsDecoding: false,
						invalidAt: void 0,
						length: void 0,
						text: void 0,
						prefixes: void 0
					});
					this.#escape = false;
					this.#mode = "string";
					return;
				}
				if (c === "}" || c === "," || c === ":" || c === "]") {
					this.fail();
					return;
				}
				this.open({
					kind: "value",
					start: at,
					end: -1,
					parsed: void 0,
					invalid: false
				});
				if (c === "{" || c === "[") {
					this.#mode = "nested";
					this.#nestedEnds = [c === "{" ? "}" : "]"];
					this.#nestedInString = false;
					this.#escape = false;
					return;
				}
				this.#mode = "scalar";
			}
			stepScalar(c, at) {
				if (c !== "," && c !== "}" && !isWhitespace(c)) return;
				this.closeValue(at);
				this.#mode = c === "," ? "key-only" : c === "}" ? "closed" : "comma-or-end";
			}
			stepNested(c, at) {
				if (this.#nestedInString) {
					this.#nestedInString = false;
					return;
				}
				if (c === "\"") {
					this.#nestedInString = true;
					return;
				}
				if (c === "{" || c === "[") {
					this.#nestedEnds.push(c === "{" ? "}" : "]");
					return;
				}
				if (c === "}" || c === "]") {
					if (this.#nestedEnds.pop() !== c) {
						this.fail();
						return;
					}
					if (this.#nestedEnds.length === 0) {
						this.closeValue(at + 1);
						this.#mode = "comma-or-end";
					}
				}
			}
			closeValue(end) {
				const entry = this.#current;
				entry.end = end;
				this.#current = null;
			}
		};
		//#endregion
		//#region ../../util/values/src/index.ts
		/**
		* Mark an unreachable closed-union branch.
		* @param value - impossible value; an unhandled typed variant fails at the call site.
		* @param context - optional switch-site label included in the failure message.
		* @returns never; a runtime value that escaped its type always throws.
		*/
		function assertNever(value, context) {
			const rendered = JSON.stringify(value) ?? String(value);
			throw new Error(`unreachable variant${context ? ` in ${context}` : ""}: ${rendered}`);
		}
		//#endregion
		//#region ../../llm/llm/src/assistant-stream.ts
		/**
		* Whether one chunk carries the model's first output token for latency measurement.
		* @param chunk - any stream chunk.
		* @returns true for a non-empty text, reasoning, or Tool-call arguments fragment and for
		*   every name-bearing Tool-call delta; false for block, usage, and finish chunks.
		*/
		function isTokenDelta$1(chunk) {
			switch (chunk.type) {
				case "text-delta":
				case "reasoning-delta": return chunk.text !== "";
				case "tool-call-delta": return chunk.argumentsDelta !== "" || chunk.name !== void 0;
				default: return false;
			}
		}
		function firstRunMemberTime(run, predicate) {
			const fragments = run.type === "tool-call-chunks" ? run.args : run.texts;
			let time = run.time0;
			for (let index = 0; index < fragments.length; index += 1) {
				if (index > 0) time += run.dt[index - 1];
				if (predicate(fragments[index])) return time;
			}
		}
		/**
		* Time of the first member of one packed run that {@link isTokenDelta} accepts: a
		* name-bearing Tool-call run starts at its first member, otherwise the first non-empty fragment.
		* Stops scanning at that member.
		* @param run - one packed delta run.
		* @returns the member's reconstructed time, or undefined when no member qualifies.
		*/
		function runFirstTokenTime(run) {
			if (run.type === "tool-call-chunks" && run.name !== void 0) return run.time0;
			return firstRunMemberTime(run, (fragment) => fragment !== "");
		}
		/**
		* Time of the first token in one compact stream per {@link isTokenDelta}, read from the
		* records themselves and stopping at the first qualifying member.
		* @param stream - compact records from one durable Assistant settlement.
		* @returns the first token's time, or undefined when the stream carries no token.
		*/
		function assistantStreamFirstTokenTime(stream) {
			for (const record of stream) {
				const time = record.type === "chunk" ? isTokenDelta$1(record.chunk) ? record.time : void 0 : runFirstTokenTime(record);
				if (time !== void 0) return time;
			}
		}
		//#endregion
		//#region lib/types/client/trajectory-definition-common.js
		/**
		* Wrap one contribution in the Engine-owned target envelope.
		*
		* @param context - Context that owns the contribution identity.
		* @param anchorSeq - Sequence used to order the contribution.
		* @param data - Trajectory-specific contribution payload.
		* @returns The contribution wrapped as a Trajectory view node.
		*/
		function trajectoryNode(context, anchorSeq, data) {
			return {
				key: context.key,
				kind: context.kind,
				id: context.id,
				target: "trajectory",
				anchorSeq,
				location: context.start?.location ?? { kind: "unresolved" },
				data
			};
		}
		//#endregion
		//#region lib/types/client/trajectory-event-projection.js
		/** Trajectory-owned conversion from durable Session events to ledger view data. */
		function asRecord(value) {
			return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
		}
		function readString(record, key) {
			const value = record[key];
			return typeof value === "string" && value.length > 0 ? value : null;
		}
		function collect(source, member, field) {
			const list = source[member];
			if (!Array.isArray(list)) return [];
			const seen = [];
			for (const entry of list) {
				const record = asRecord(entry);
				const value = record === null ? null : readString(record, field);
				if (value !== null && !seen.includes(value)) seen.push(value);
			}
			return seen;
		}
		function joined(names) {
			return names.length > 0 ? names.join(", ") : null;
		}
		/** Forms Trajectory presents structurally; unknown merge-extensible values remain opaque. */
		const KNOWN_FORMS = [
			"instructions",
			"catalog",
			"snapshot",
			"notice",
			"relay",
			"recall"
		];
		/**
		* Read the target-supported presentation form from a durable message source.
		* @param source - Logged `user/message` source.
		* @returns Supported form, or null for the opaque presentation.
		*/
		function contextForm(source) {
			const record = asRecord(source);
			const form = record === null ? null : readString(record, "form");
			return form !== null && KNOWN_FORMS.includes(form) ? form : null;
		}
		/**
		* Project a durable message source to the Trajectory row's role and producer label.
		* @param source - Logged `user/message` source.
		* @returns Role and label rendered by Trajectory.
		*/
		function contextProducer(source) {
			const record = asRecord(source);
			const kind = record === null ? null : readString(record, "kind");
			if (record === null || kind === null) return {
				role: "inject",
				label: null
			};
			switch (kind) {
				case "session-reference": return {
					role: "recall",
					label: joined(collect(record, "references", "label")) ?? kind
				};
				case "agent-instructions": return {
					role: "inject",
					label: joined(collect(record, "changes", "path")) ?? kind
				};
				case "skill-invocation": return {
					role: "inject",
					label: readString(record, "name") ?? kind
				};
				default: return {
					role: "inject",
					label: kind
				};
			}
		}
		/**
		* Classify finalized Assistant content for Trajectory rendering.
		* @param content - Core content blocks.
		* @returns Trajectory blocks in source order.
		*/
		function toAssistantBlocks(content) {
			return content.map(toAssistantBlock);
		}
		/**
		* Classify one finalized Assistant block for Trajectory rendering.
		* @param block - Core content block.
		* @returns Trajectory block.
		*/
		function toAssistantBlock(block) {
			switch (block.type) {
				case "text": return {
					kind: "text",
					text: block.text
				};
				case "reasoning": return {
					kind: "reasoning",
					text: block.text
				};
				case "image": return {
					kind: "image",
					attachment: block.attachment
				};
				case "tool-call": return {
					kind: "tool-call",
					callId: String(block.id),
					name: block.name,
					argsRaw: block.arguments
				};
				default: return {
					kind: "other",
					block
				};
			}
		}
		/**
		* Create the initial Trajectory block for one streamed Assistant block kind.
		* @param blockType - Wire block kind.
		* @returns Empty block ready to receive deltas.
		*/
		function emptyAssistantBlock(blockType) {
			switch (blockType) {
				case "text": return {
					kind: "text",
					text: ""
				};
				case "reasoning": return {
					kind: "reasoning",
					text: ""
				};
				case "tool-call": return {
					kind: "tool-call",
					callId: "",
					name: "",
					argsRaw: ""
				};
				default: return {
					kind: "other",
					block: null
				};
			}
		}
		/**
		* Convert a durable failure to locale-independent fields safe for Trajectory.
		* @param failure - Failure preserved by a Session event.
		* @returns Sanitized message and optional stable provider code.
		*/
		function displayFailure(failure) {
			if (failure === null || typeof failure !== "object") return { message: String(failure) };
			const record = failure;
			const code = typeof record.code === "string" ? record.code : void 0;
			if (code === "AUTH") return {
				code,
				message: ""
			};
			return {
				...code === void 0 ? {} : { code },
				message: typeof record.message === "string" ? record.message : JSON.stringify(failure)
			};
		}
		/**
		* Whether a stream chunk carries visible model output for Trajectory timing.
		* @param chunk - Stream chunk to inspect.
		* @returns true for a non-empty text, reasoning, or Tool-call delta.
		*/
		function isTokenDelta(chunk) {
			switch (chunk.type) {
				case "text-delta":
				case "reasoning-delta": return chunk.text !== "";
				case "tool-call-delta": return chunk.argumentsDelta !== "" || chunk.name !== void 0;
				default: return false;
			}
		}
		//#endregion
		//#region lib/types/client/trajectory-assistant-definition.js
		function initialState(turn, step, startSeq, startTime, started) {
			return {
				turn,
				step,
				startSeq,
				startTime,
				started,
				blocks: [],
				visibleBlocks: 0,
				firstVisibleSeq: void 0,
				firstVisibleTime: void 0,
				firstTokenTime: void 0,
				final: void 0,
				usage: void 0,
				retry: void 0,
				stepEnd: void 0
			};
		}
		function compactBlocks(blocks) {
			return blocks.filter((block) => block !== void 0);
		}
		function blockIsVisible(block) {
			if (block === void 0 || block.kind === "tool-call") return false;
			if (block.kind === "text" || block.kind === "reasoning") return block.text.trim() !== "";
			return true;
		}
		function countVisibleBlocks(blocks) {
			let count = 0;
			for (const block of blocks) if (blockIsVisible(block)) count++;
			return count;
		}
		function hasInterruptionEvidence(blocks) {
			return blocks.some((block) => {
				if (block.kind === "text" || block.kind === "reasoning") return block.text.trim() !== "";
				return true;
			});
		}
		function addUsage$1(current, next) {
			return {
				inputTokens: (current?.inputTokens ?? 0) + next.inputTokens,
				outputTokens: (current?.outputTokens ?? 0) + next.outputTokens,
				...current?.cacheReadTokens === void 0 && next.cacheReadTokens === void 0 ? {} : { cacheReadTokens: (current?.cacheReadTokens ?? 0) + (next.cacheReadTokens ?? 0) },
				...current?.cacheWriteTokens === void 0 && next.cacheWriteTokens === void 0 ? {} : { cacheWriteTokens: (current?.cacheWriteTokens ?? 0) + (next.cacheWriteTokens ?? 0) },
				...current?.reasoningTokens === void 0 && next.reasoningTokens === void 0 ? {} : { reasoningTokens: (current?.reasoningTokens ?? 0) + (next.reasoningTokens ?? 0) }
			};
		}
		function updateChunk(state, chunk, seq, time) {
			if (chunk.type === "usage") return {
				...state,
				usage: addUsage$1(state.usage, chunk.usage)
			};
			const blocks = [...state.blocks];
			let changedIndex = -1;
			let previousVisible = false;
			switch (chunk.type) {
				case "block-start":
					changedIndex = chunk.index;
					previousVisible = blockIsVisible(blocks[chunk.index]);
					blocks[chunk.index] = emptyAssistantBlock(chunk.blockType);
					break;
				case "text-delta": {
					const previous = blocks[chunk.index];
					changedIndex = chunk.index;
					previousVisible = blockIsVisible(previous);
					blocks[chunk.index] = {
						kind: "text",
						text: (previous?.kind === "text" ? previous.text : "") + chunk.text
					};
					break;
				}
				case "reasoning-delta": {
					const previous = blocks[chunk.index];
					changedIndex = chunk.index;
					previousVisible = blockIsVisible(previous);
					blocks[chunk.index] = {
						kind: "reasoning",
						text: (previous?.kind === "reasoning" ? previous.text : "") + chunk.text
					};
					break;
				}
				case "tool-call-delta": {
					const previous = blocks[chunk.index];
					changedIndex = chunk.index;
					previousVisible = blockIsVisible(previous);
					const base = previous?.kind === "tool-call" ? previous : {
						kind: "tool-call",
						callId: "",
						name: "",
						argsRaw: ""
					};
					blocks[chunk.index] = {
						kind: "tool-call",
						callId: base.callId || String(chunk.id),
						name: chunk.name ?? base.name,
						argsRaw: base.argsRaw + chunk.argumentsDelta
					};
					break;
				}
				case "block-end":
					changedIndex = chunk.index;
					previousVisible = blockIsVisible(blocks[chunk.index]);
					blocks[chunk.index] = toAssistantBlock(chunk.block);
					break;
				default: return state;
			}
			const visibleBlocks = state.visibleBlocks - Number(previousVisible) + Number(blockIsVisible(blocks[changedIndex]));
			return {
				...state,
				blocks,
				visibleBlocks,
				...visibleBlocks > 0 && state.firstVisibleSeq === void 0 ? {
					firstVisibleSeq: seq,
					firstVisibleTime: time
				} : {},
				...isTokenDelta(chunk) && state.firstTokenTime === void 0 ? { firstTokenTime: time } : {}
			};
		}
		/** The Step retains its first token across live chunks and settled retry attempts. */
		function settleTiming(state, event) {
			return {
				...state,
				firstTokenTime: state.firstTokenTime ?? assistantStreamFirstTokenTime(event.data.stream)
			};
		}
		function settleMessage(state, match, event) {
			const blocks = toAssistantBlocks(event.data.message.content);
			return {
				...settleTiming(state, event),
				blocks,
				visibleBlocks: countVisibleBlocks(blocks),
				final: match,
				usage: event.data.usage
			};
		}
		function closedBoundary(context) {
			if (context.state?.stepEnd?.event.type === "step/end") return context.state.stepEnd.event;
			const location = context.start?.location ?? context.matches.at(-1)?.location;
			if (location?.kind === "step" && location.step.status === "closed") return location.step.end;
			if ((location?.kind === "step" || location?.kind === "turn") && location.turn.status === "closed") return location.turn.end;
		}
		function fallbackState$1(context) {
			let state;
			for (const match of context.matches) {
				const event = match.event;
				if (event.type === "assistant/live-chunk") {
					state ??= initialState(event.data.turn, event.data.step, event.seq, event.time, false);
					state = updateChunk(state, event.data.chunk, event.seq, event.time);
				} else if (event.type === "assistant/attempt") {
					state ??= initialState(event.data.turn, event.data.step, event.seq, event.time, false);
					state = settleTiming(state, event);
				} else if (event.type === "assistant/message") {
					state ??= initialState(event.data.turn, event.data.step, event.seq, event.time, false);
					state = settleMessage(state, match, event);
				} else if (event.type === "step/end" && state !== void 0) state = {
					...state,
					stepEnd: match
				};
			}
			return state;
		}
		function finalNode(state, context) {
			const final = state.final;
			if (final?.event.type === "assistant/message") {
				const event = final.event;
				return {
					kind: "assistant",
					seq: event.seq,
					messageId: event.data.message.id,
					time: event.time,
					turn: state.turn,
					step: state.step,
					blocks: toAssistantBlocks(event.data.message.content),
					usage: event.data.usage,
					providerMetadata: {
						provider: event.data.message.source.provider,
						model: event.data.message.source.model
					},
					timing: {
						stepStartTime: state.started ? state.startTime : null,
						firstTokenTime: state.firstTokenTime ?? null,
						completedTime: event.time
					},
					...event.data.interrupted === true ? { interrupted: true } : {}
				};
			}
			const boundary = closedBoundary(context);
			if (boundary === void 0) return void 0;
			const blocks = compactBlocks(state.blocks);
			if (!hasInterruptionEvidence(blocks)) return void 0;
			return {
				kind: "assistant",
				seq: boundary.seq - .9,
				time: boundary.time,
				turn: state.turn,
				step: state.step,
				blocks,
				interrupted: true
			};
		}
		function assistantRequest(state, node, boundary) {
			if (!state.started) return void 0;
			const status = node !== void 0 ? node.interrupted === true ? "error" : "complete" : boundary !== void 0 ? "error" : "running";
			return {
				purpose: "assistant",
				startSeq: state.startSeq,
				turn: state.turn,
				step: state.step,
				startedAt: state.startTime,
				completedAt: node?.time ?? boundary?.time ?? null,
				status,
				...state.retry === void 0 ? {} : {
					error: state.retry.message,
					...state.retry.code === void 0 ? {} : { errorCode: state.retry.code },
					retry: state.retry.retry,
					...state.retry.maxRetries === void 0 ? {} : { maxRetries: state.retry.maxRetries },
					retryDelayMs: state.retry.delayMs
				},
				...node?.messageId === void 0 ? {} : {
					resultSeq: node.seq,
					...node.providerMetadata === void 0 ? {} : { providerMetadata: node.providerMetadata }
				},
				...state.usage === void 0 ? {} : { usage: state.usage }
			};
		}
		/** Trajectory-owned Assistant streaming, settlement, and request lifecycle. */
		const trajectoryAssistantDefinition = {
			kind: "trajectory-assistant-step",
			target: "trajectory",
			match: (event) => {
				if (event.type === "step/start") return {
					id: `${event.data.turn}:${event.data.step}`,
					role: "start"
				};
				if (event.type === "assistant/live-chunk" || event.type === "assistant/message" || event.type === "assistant/attempt" || event.type === "llm/retry" || event.type === "step/end") return {
					id: `${event.data.turn}:${event.data.step}`,
					role: "update"
				};
				return null;
			},
			start: (_context, match) => {
				if (match.event.type !== "step/start") throw new Error("trajectory-assistant-step start requires step/start");
				return initialState(match.event.data.turn, match.event.data.step, match.event.seq, match.event.time, true);
			},
			update: (context, match) => {
				if (match.event.type === "assistant/live-chunk") return updateChunk(context.state, match.event.data.chunk, match.event.seq, match.event.time);
				if (match.event.type === "assistant/message") return settleMessage(context.state, match, match.event);
				if (match.event.type === "assistant/attempt") return settleTiming(context.state, match.event);
				if (match.event.type === "step/end") return {
					...context.state,
					stepEnd: match
				};
				if (match.event.type !== "llm/retry") return context.state;
				const data = match.event.data;
				const failure = displayFailure(data.failure);
				return {
					...initialState(context.state.turn, context.state.step, context.state.startSeq, context.state.startTime, true),
					firstTokenTime: context.state.firstTokenTime,
					usage: context.state.usage,
					retry: {
						message: failure.message,
						...failure.code === void 0 ? {} : { code: failure.code },
						retry: data.retry,
						...data.mode === "normal" ? { maxRetries: data.maxRetries } : {},
						delayMs: data.delayMs
					}
				};
			},
			publication: (match) => {
				if (match.event.type === "step/start" || match.event.type === "assistant/attempt") return "none";
				if (match.event.type !== "assistant/live-chunk") return "immediate";
				const type = match.event.data.chunk.type;
				return type === "usage" || type === "finish" ? "none" : "animation-frame";
			},
			buildViewNode: (context) => {
				const state = context.state ?? fallbackState$1(context);
				if (state === void 0) return null;
				const node = finalNode(state, context);
				const boundary = closedBoundary(context);
				const partial = node === void 0 && boundary === void 0 && state.blocks.length > 0 ? {
					turn: state.turn,
					step: state.step,
					blocks: compactBlocks(state.blocks),
					timing: {
						stepStartTime: state.started ? state.startTime : null,
						firstTokenTime: state.firstTokenTime ?? null
					}
				} : null;
				const request = assistantRequest(state, node, boundary);
				if (node === void 0 && partial === null && request === void 0) return null;
				return trajectoryNode(context, state.startSeq, {
					kind: "assistant",
					...node === void 0 ? {} : { node },
					partial,
					...request === void 0 ? {} : { request }
				});
			}
		};
		const trajectoryTurnEndDefinition = {
			kind: "trajectory-turn-end",
			target: "trajectory",
			match: (event) => event.type === "turn/end" ? {
				id: String(event.seq),
				role: "start"
			} : null,
			start: (_context, match) => {
				if (match.event.type !== "turn/end") throw new Error("trajectory-turn-end start requires turn/end");
				const reason = match.event.data.reason;
				const failure = reason.kind === "error" ? displayFailure(reason.error) : void 0;
				return {
					turn: match.event.data.turn,
					seq: match.event.seq,
					time: match.event.time,
					...failure === void 0 ? {} : {
						error: failure.message,
						...failure.code === void 0 ? {} : { errorCode: failure.code }
					}
				};
			},
			update: (context) => context.state,
			buildViewNode: (context) => context.state === void 0 ? null : trajectoryNode(context, context.state.seq, {
				kind: "turn-end",
				turn: context.state.turn,
				time: context.state.time,
				...context.state.error === void 0 ? {} : { error: context.state.error },
				...context.state.errorCode === void 0 ? {} : { errorCode: context.state.errorCode }
			})
		};
		/**
		* Register the Trajectory Assistant lifecycle.
		*
		* @param ctx - Plugin context receiving the Definitions.
		*/
		function registerTrajectoryAssistantDefinition(ctx) {
			ctx.uiConversation.events.register(trajectoryAssistantDefinition);
			ctx.uiConversation.events.register(trajectoryTurnEndDefinition);
		}
		//#endregion
		//#region lib/types/client/trajectory-compaction-definition.js
		function checkpointId(event) {
			if (event.type !== "user/message") return void 0;
			const source = event.data.source;
			return source.kind === "compact-checkpoint" && typeof source.compactionId === "string" && source.compactionId !== "" ? source.compactionId : void 0;
		}
		function eventCompactionId(event) {
			if (event.type !== "compaction/start" && event.type !== "compaction/summary" && event.type !== "compaction/end") return void 0;
			const value = event.data.compactionId;
			return typeof value === "string" && value !== "" ? value : void 0;
		}
		function requestFromState(state) {
			const start = state.start.event;
			if (start.type !== "compaction/start") return void 0;
			const summary = state.summary?.event;
			const end = state.end?.event;
			const checkpoint = state.checkpoint?.event;
			return {
				purpose: "compaction",
				startSeq: start.seq,
				turn: start.data.turn,
				step: 0,
				startedAt: start.time,
				completedAt: end?.type === "compaction/end" ? end.time : null,
				status: end?.type !== "compaction/end" ? "running" : end.data.error === void 0 ? "complete" : "error",
				...end?.type === "compaction/end" && end.data.error !== void 0 ? { error: end.data.error } : {},
				...summary?.type !== "compaction/summary" ? {} : {
					resultSeq: summary.seq,
					summary: summary.data.summary,
					...summary.data.rawOutput === void 0 ? {} : { rawOutput: summary.data.rawOutput },
					providerMetadata: {
						provider: summary.data.provider,
						model: summary.data.model
					},
					requestConfig: {
						provider: summary.data.provider,
						model: summary.data.model,
						purpose: "compaction",
						...summary.data.maxTokens === void 0 ? {} : { maxTokens: summary.data.maxTokens }
					},
					...summary.data.usage === void 0 ? {} : { usage: summary.data.usage }
				},
				...checkpoint?.type === "user/message" ? { replacementSeq: checkpoint.seq } : {}
			};
		}
		const trajectoryCompactionDefinition = {
			kind: "trajectory-compaction",
			target: "trajectory",
			match: (event) => {
				const compactId = eventCompactionId(event);
				if (compactId !== void 0) return {
					id: compactId,
					role: event.type === "compaction/start" ? "start" : "update"
				};
				const checkpoint = checkpointId(event);
				return checkpoint === void 0 ? null : {
					id: checkpoint,
					role: "update"
				};
			},
			start: (_context, match) => {
				if (match.event.type !== "compaction/start") throw new Error("trajectory-compaction start requires compaction/start");
				return { start: match };
			},
			update: (context, match) => {
				if (match.event.type === "compaction/summary") return {
					...context.state,
					summary: match
				};
				if (match.event.type === "compaction/end") return {
					...context.state,
					end: match
				};
				return checkpointId(match.event) === void 0 ? context.state : {
					...context.state,
					checkpoint: match
				};
			},
			buildViewNode: (context) => {
				if (context.state === void 0) return null;
				const request = requestFromState(context.state);
				return request === void 0 ? null : trajectoryNode(context, request.startSeq, {
					kind: "compaction",
					request
				});
			}
		};
		const trajectorySessionEndDefinition = {
			kind: "trajectory-session-end",
			target: "trajectory",
			match: (event) => event.type === "session/end-seed" ? {
				id: String(event.seq),
				role: "start"
			} : null,
			start: (_context, match) => ({
				seq: match.event.seq,
				time: match.event.time
			}),
			update: (context) => context.state,
			buildViewNode: (context) => context.state === void 0 ? null : trajectoryNode(context, context.state.seq, {
				kind: "session-end",
				seq: context.state.seq,
				time: context.state.time
			})
		};
		/**
		* Register Trajectory compaction requests and session boundaries.
		*
		* @param ctx - Plugin context receiving the Definitions.
		*/
		function registerTrajectoryCompactionDefinitions(ctx) {
			ctx.uiConversation.events.register(trajectoryCompactionDefinition);
			ctx.uiConversation.events.register(trajectorySessionEndDefinition);
		}
		//#endregion
		//#region lib/types/client/trajectory-message-definitions.js
		const EMPTY_PENDING = {
			kind: "snapshot",
			ids: []
		};
		const EMPTY_CURRENT_CLAIMED = /* @__PURE__ */ new Set();
		function materializePending(state) {
			const splices = [];
			let current = state;
			while (current.kind === "splice") {
				splices.push(current);
				current = current.previous;
			}
			const pending = [...current.ids];
			for (const splice of splices.reverse()) pending.splice(splice.start, splice.removedCount, ...splice.inserted);
			return pending;
		}
		function withoutInserted(claimed, inserted) {
			let next;
			for (const id of inserted) {
				if (!claimed.has(id)) continue;
				next ??= new Set(claimed);
				next.delete(id);
			}
			return next ?? claimed;
		}
		/**
		* Apply one next-step splice under the AgentLoop's durable event ordering.
		* An entered claim logs its complete message batch before another claim; a
		* rejected claim logs no messages, so only the current claim can classify a
		* later `user/message`.
		*/
		function applySplice(previous, splice) {
			const priorPending = previous?.state.pending ?? EMPTY_PENDING;
			const inserted = splice.inserted.map((identity) => identity.id);
			const removedCount = splice.removedCount ?? 0;
			if (removedCount > 0 && splice.outcome !== "canceled") {
				const pending = materializePending(priorPending);
				const removed = pending.splice(splice.start, removedCount, ...inserted);
				return {
					pending: {
						kind: "snapshot",
						ids: pending
					},
					currentClaimed: new Set(removed)
				};
			}
			const currentClaimed = withoutInserted(previous?.state.currentClaimed ?? EMPTY_CURRENT_CLAIMED, inserted);
			return {
				pending: {
					kind: "splice",
					previous: priorPending,
					start: splice.start,
					removedCount,
					inserted
				},
				currentClaimed
			};
		}
		const trajectoryInboxDefinition = {
			kind: "trajectory-inbox-next-step",
			match: (event) => {
				if (event.type === "agent/inbox/spliced" && event.data.target === "next-step") return {
					id: String(event.seq),
					role: "start"
				};
				return null;
			},
			start: (_context, match, reader) => {
				if (match.event.type !== "agent/inbox/spliced") throw new Error("trajectory-inbox-next-step start requires agent/inbox/spliced");
				return applySplice(reader.previous("trajectory-inbox-next-step"), match.event.data);
			},
			update: (context) => context.state,
			publication: () => "none"
		};
		const trajectoryMessageDefinition = {
			kind: "trajectory-input-message",
			target: "trajectory",
			match: (event) => {
				return event.type === "user/message" || event.type === "developer/message" ? {
					id: String(event.seq),
					role: "start"
				} : null;
			},
			start: (_context, match, reader) => {
				if (match.event.type === "developer/message") {
					const { seq, time, data: { message } } = match.event;
					return {
						kind: "context",
						seq,
						time,
						content: message.content,
						source: message.source,
						producer: contextProducer(message.source),
						form: contextForm(message.source)
					};
				}
				if (match.event.type !== "user/message") throw new Error("trajectory-input-message start requires user/message");
				const event = match.event;
				if (event.data.source.kind !== "user") return {
					kind: "context",
					seq: event.seq,
					time: event.time,
					content: event.data.content,
					source: event.data.source,
					producer: contextProducer(event.data.source),
					form: contextForm(event.data.source)
				};
				return reader.previous("trajectory-inbox-next-step")?.state.currentClaimed.has(String(event.data.id)) === true ? {
					kind: "steering",
					messageId: event.data.id,
					seq: event.seq,
					time: event.time,
					content: event.data.content,
					source: event.data.source
				} : {
					kind: "user",
					seq: event.seq,
					time: event.time,
					content: event.data.content,
					source: event.data.source
				};
			},
			update: (context) => context.state,
			buildViewNode: (context) => context.state === void 0 ? null : trajectoryNode(context, context.state.seq, {
				kind: "node",
				node: context.state
			})
		};
		/**
		* Register Trajectory-owned inbox classification and message records.
		*
		* @param ctx - Plugin context receiving the Definitions.
		*/
		function registerTrajectoryMessageDefinitions(ctx) {
			ctx.uiConversation.events.register(trajectoryInboxDefinition);
			ctx.uiConversation.events.register(trajectoryMessageDefinition);
		}
		//#endregion
		//#region lib/types/client/trajectory-request-header-definition.js
		/**
		* Definition retaining each `system/message` surface node for the Trajectory
		* request-header Definition, which reads it through `reader.previous` and
		* presents the prompt through the request's `system` cell. A node that
		* lacks a loaded header contributes known text without request config or tools; an in-history
		* update contributes a request-header fact at its own position, since no
		* `request/header` follows a prompt change that keeps the cached history.
		* Surface replacements also contribute prompt changes when they remove the
		* effective node; earlier request facts remain historical.
		* @param inspect - Pure surface interpretation supplied by uiConversation.
		* @returns The Trajectory system-prompt Definition.
		*/
		function trajectorySystemMessageDefinition(inspect) {
			return {
				kind: "trajectory-system-message",
				target: "trajectory",
				match: (event) => event.type === "system/message" || "surfaceOp" in event && event.surfaceOp !== "append" ? {
					id: String(event.seq),
					role: "start"
				} : null,
				start: (_context, match, reader) => {
					if (match.event.type === "assistant/live-chunk") throw new Error("system-message requires a durable event");
					const prior = reader.previous("trajectory-system-message")?.state;
					const state = inspect(prior, match.event);
					const node = state.effective;
					if (state.uncertain) {
						const header = reader.previous("trajectory-request-header")?.state;
						if (header === void 0) return state;
						return {
							...state,
							header: {
								seq: match.event.seq,
								time: match.event.time,
								location: match.location,
								prompt: {
									...header.prompt,
									system: ""
								}
							}
						};
					}
					if (node === void 0 || node.text === prior?.effective?.text || state.introduced !== void 0 && !state.introduced.update) return {
						...state,
						...prior?.header === void 0 ? {} : { header: prior.header }
					};
					const header = reader.previous("trajectory-request-header")?.state;
					const systemHeader = prior?.header;
					const previous = systemHeader !== void 0 && (header === void 0 || systemHeader.seq > header.seq) ? systemHeader : header;
					if (previous === void 0) return state;
					return {
						...state,
						header: {
							seq: node.seq,
							time: node.time,
							prompt: {
								...previous.prompt,
								system: node.text
							},
							change: {
								seq: node.seq,
								time: node.time,
								kind: "system",
								previous: previous.prompt
							},
							location: match.location
						}
					};
				},
				update: (context) => context.state,
				buildViewNode: (context) => {
					const state = context.state;
					if (state?.header !== void 0 && state.header.seq === context.start?.event.seq) return trajectoryNode(context, state.header.seq, {
						kind: "request-header",
						header: state.header
					});
					const prompt = state?.introduced;
					return prompt !== void 0 && prompt.text !== "" && context.start?.event.type === "system/message" && context.start.event.surfaceOp === "append" ? trajectoryNode(context, prompt.seq, {
						kind: "system-prompt",
						prompt
					}) : null;
				}
			};
		}
		/**
		* Request-header fact Definition for the Trajectory target.
		* @param inspect - the shared prompt interpretation, supplied by the
		* uiConversation service (a client bundle cannot value-import it).
		* @returns the Trajectory request-header Definition.
		*/
		function trajectoryRequestHeaderDefinition(inspect) {
			return {
				kind: "trajectory-request-header",
				target: "trajectory",
				match: (event) => event.type === "request/header" ? {
					id: String(event.seq),
					role: "start"
				} : null,
				start: (_context, match, reader) => {
					if (match.event.type !== "request/header") throw new Error("trajectory-request-header start requires request/header");
					const header = reader.previous("trajectory-request-header")?.state;
					const state = reader.previous("trajectory-system-message")?.state;
					const systemHeader = state?.header;
					const previous = systemHeader !== void 0 && (header === void 0 || systemHeader.seq > header.seq) ? systemHeader.prompt : header?.prompt;
					const system = state?.effective;
					const inspection = inspect(previous, match.event, system);
					const { prompt } = inspection;
					const change = inspection.change ?? (systemHeader !== void 0 && (header === void 0 || systemHeader.seq > header.seq) ? systemHeader.change : void 0);
					return {
						seq: match.event.seq,
						time: match.event.time,
						prompt,
						location: match.location,
						...change === void 0 ? {} : { change }
					};
				},
				update: (context) => context.state,
				buildViewNode: (context) => context.state === void 0 ? null : trajectoryNode(context, context.state.seq, {
					kind: "request-header",
					header: context.state
				})
			};
		}
		/**
		* Register Trajectory system-prompt node and request-header facts.
		*
		* @param ctx - Plugin context receiving the Definitions.
		*/
		function registerTrajectoryRequestHeaderDefinition(ctx) {
			ctx.uiConversation.events.register(trajectorySystemMessageDefinition((previous, event) => ctx.uiConversation.inspectSystemPrompt(previous, event)));
			ctx.uiConversation.events.register(trajectoryRequestHeaderDefinition((previous, event, system) => ctx.uiConversation.inspectRequestPrompt(previous, event, system)));
		}
		//#endregion
		//#region lib/types/client/copy-codes.js
		/** Stable non-display markers for client-synthesized trajectory copy. */
		/** Error marker localized only when the trajectory renders it. */
		const COMPACTION_INTERRUPTED_ERROR = "trajectory.compaction-interrupted";
		//#endregion
		//#region lib/types/client/trajectory-snapshot-builder.js
		const EMPTY_LIST = [];
		/** Stable empty target used until a Session has assembled Trajectory records. */
		const EMPTY_TRAJECTORY_SNAPSHOT = {
			eventNodes: EMPTY_LIST,
			eventLocations: /* @__PURE__ */ new Map(),
			requests: EMPTY_LIST,
			callSchemas: /* @__PURE__ */ new Map(),
			partial: null,
			runningCalls: EMPTY_LIST
		};
		function stepKey(turn, step) {
			return `${turn}\u0000${step}`;
		}
		function headerStepKey(header) {
			const location = header.location;
			return location.kind === "step" ? stepKey(location.turn.turn, location.step.step) : void 0;
		}
		function headerFor(request, headersByStep, previous) {
			return headersByStep.get(stepKey(request.turn, request.step)) ?? (previous !== void 0 && previous.seq < request.startSeq ? {
				latest: previous,
				...previous.change === void 0 ? {} : { change: previous.change }
			} : void 0);
		}
		function applyHeader(request, header, includeChange) {
			return header === void 0 ? request : {
				...request,
				prompt: header.latest.prompt,
				requestConfig: header.latest.prompt.config,
				...includeChange && header.change !== void 0 ? { promptChange: header.change } : {}
			};
		}
		function withRequestConfig(node, prompt) {
			return prompt === void 0 ? node : {
				...node,
				requestConfig: prompt.config
			};
		}
		function captureSchemas(block, toolsByName, output) {
			const name = "kind" in block ? block.call?.name : block.name;
			const schema = name === void 0 ? void 0 : toolsByName.get(name);
			if (schema !== void 0) output.set(block.callId, schema);
			for (const child of block.subCalls) captureSchemas(child, toolsByName, output);
		}
		function indexTools(tools) {
			return new Map(tools.map((tool) => [tool.name, tool]));
		}
		function interruptCompactions(requests, boundaries) {
			let nextRequest = 0;
			const runningCompactions = [];
			for (const boundary of boundaries) {
				while (nextRequest < requests.length) {
					const request = requests[nextRequest];
					if (request === void 0 || request.startSeq >= boundary.seq) break;
					if (request.purpose === "compaction" && request.status === "running") runningCompactions.push(nextRequest);
					nextRequest++;
				}
				let index = runningCompactions.pop();
				while (index !== void 0 && requests[index]?.status !== "running") index = runningCompactions.pop();
				if (index === void 0) continue;
				const request = requests[index];
				if (request?.purpose !== "compaction") continue;
				requests[index] = {
					...request,
					completedAt: boundary.time,
					status: "error",
					error: COMPACTION_INTERRUPTED_ERROR
				};
			}
		}
		function applyTurnErrors(requests, endings) {
			const lastAssistantByTurn = /* @__PURE__ */ new Map();
			for (const [index, request] of requests.entries()) if (request.purpose === "assistant") lastAssistantByTurn.set(request.turn, index);
			for (const ending of endings) {
				if (ending.error === void 0) continue;
				const index = lastAssistantByTurn.get(ending.turn);
				if (index === void 0) continue;
				const request = requests[index];
				if (request?.purpose !== "assistant") continue;
				requests[index] = {
					...request,
					completedAt: request.completedAt ?? ending.time,
					status: "error",
					error: ending.error,
					...ending.errorCode === void 0 ? {} : { errorCode: ending.errorCode }
				};
			}
		}
		/** Simple keyed adapter retaining the old Trajectory snapshot and stage layout. */
		var TrajectorySnapshotBuilder = class {
			nodes = /* @__PURE__ */ new Map();
			positions = /* @__PURE__ */ new Map();
			contributions = [];
			empty = EMPTY_TRAJECTORY_SNAPSHOT;
			replace(input) {
				this.nodes.clear();
				for (const node of input.nodes) this.nodes.set(node.key, node);
				this.rebuildContributions();
				return this.snapshot();
			}
			apply(input) {
				let structural = false;
				for (const node of input.upserts) {
					const previous = this.nodes.get(node.key);
					this.nodes.set(node.key, node);
					if (previous === void 0 || previous.anchorSeq !== node.anchorSeq) {
						structural = true;
						continue;
					}
					const position = this.positions.get(node.key);
					if (position === void 0) structural = true;
					else this.contributions[position] = node;
				}
				if (structural) this.rebuildContributions();
				return this.snapshot();
			}
			snapshot() {
				const headersByStep = /* @__PURE__ */ new Map();
				for (const contribution of this.contributions) {
					if (contribution.data.kind !== "request-header") continue;
					const key = headerStepKey(contribution.data.header);
					if (key === void 0) continue;
					const previous = headersByStep.get(key);
					headersByStep.set(key, {
						latest: contribution.data.header,
						...contribution.data.header.change !== void 0 ? { change: contribution.data.header.change } : previous?.change === void 0 ? {} : { change: previous.change }
					});
				}
				const representedPrompts = new Set(this.contributions.flatMap((node) => node.data.kind === "request-header" && node.data.header.change !== void 0 ? [node.data.header.change.seq] : []));
				const systemPrompts = [];
				const finalized = [];
				const eventLocations = /* @__PURE__ */ new Map();
				const requests = [];
				const boundaries = [];
				const turnEndings = [];
				const callSchemas = /* @__PURE__ */ new Map();
				const consumedPromptChanges = /* @__PURE__ */ new Set();
				let previousHeader;
				let previousTools = /* @__PURE__ */ new Map();
				let partial = null;
				const runningCalls = [];
				for (const contribution of this.contributions) {
					const data = contribution.data;
					if (data.kind === "system-prompt") {
						if (!representedPrompts.has(data.prompt.seq)) systemPrompts.push(data.prompt);
						continue;
					}
					if (data.kind === "request-header") {
						previousHeader = data.header;
						previousTools = indexTools(data.header.prompt.tools);
						continue;
					}
					if (data.kind === "node") {
						finalized.push(data.node);
						eventLocations.set(data.node.seq, contribution.location);
						continue;
					}
					if (data.kind === "assistant") {
						const header = data.request === void 0 ? void 0 : headerFor(data.request, headersByStep, previousHeader);
						if (data.node !== void 0) finalized.push(withRequestConfig(data.node, header?.latest.prompt));
						if (data.partial !== null) partial = data.partial;
						if (data.request !== void 0) {
							const change = header?.change;
							const includeChange = change !== void 0 && !consumedPromptChanges.has(change.seq);
							requests.push(applyHeader(data.request, header, includeChange));
							if (includeChange) consumedPromptChanges.add(change.seq);
						}
						continue;
					}
					if (data.kind === "tool") {
						if ("kind" in data.root) finalized.push(data.root);
						else runningCalls.push(data.root);
						if (previousHeader !== void 0 && previousHeader.seq < contribution.anchorSeq) captureSchemas(data.root, previousTools, callSchemas);
						continue;
					}
					if (data.kind === "compaction") {
						requests.push(data.request);
						continue;
					}
					if (data.kind === "session-end") {
						boundaries.push({
							seq: data.seq,
							time: data.time
						});
						continue;
					}
					turnEndings.push({
						turn: data.turn,
						time: data.time,
						...data.error === void 0 ? {} : { error: data.error },
						...data.errorCode === void 0 ? {} : { errorCode: data.errorCode }
					});
				}
				requests.sort((left, right) => left.startSeq - right.startSeq);
				interruptCompactions(requests, boundaries);
				applyTurnErrors(requests, turnEndings);
				finalized.sort((left, right) => left.seq - right.seq);
				return {
					eventNodes: finalized,
					...systemPrompts.length > 0 ? { systemPrompts } : {},
					eventLocations,
					requests,
					callSchemas,
					partial,
					runningCalls
				};
			}
			rebuildContributions() {
				this.contributions = [...this.nodes.values()].sort((left, right) => left.anchorSeq - right.anchorSeq || left.key.localeCompare(right.key));
				this.positions.clear();
				for (const [index, contribution] of this.contributions.entries()) this.positions.set(contribution.key, index);
			}
		};
		/** Trajectory target factory preserving the existing stage-oriented view model. */
		const trajectoryViewDefinition = {
			target: "trajectory",
			toolCallFocus: (callId) => callId,
			create: () => new TrajectorySnapshotBuilder()
		};
		/**
		* Register the stage-oriented Trajectory target builder.
		*
		* @param ctx - Plugin context receiving the view Definition.
		*/
		function registerTrajectoryConversationView(ctx) {
			ctx.uiConversation.views.register(trajectoryViewDefinition);
		}
		//#endregion
		//#region lib/types/client/trajectory-tool-definition.js
		const MAX_DEPTH = 256;
		function rootCall(match) {
			if (match.event.type !== "tool/call") throw new Error("trajectory-tool-call start requires tool/call");
			return {
				phase: "start",
				callId: String(match.event.data.callId),
				name: match.event.data.name,
				argsRaw: match.event.data.arguments,
				args: PartialArguments.fromText(match.event.data.arguments),
				turn: match.event.data.turn,
				step: match.event.data.step,
				time: match.event.time,
				subCalls: []
			};
		}
		function rootResult(match, previous) {
			if (match.event.type !== "tool/result") return void 0;
			const message = match.event.data.message;
			return {
				kind: "tool-result",
				seq: match.event.seq,
				time: match.event.time,
				callId: String(message.source.callId),
				name: previous?.name ?? "",
				args: previous?.args ?? PartialArguments.EMPTY,
				call: previous === void 0 ? null : {
					name: previous.name,
					argsRaw: previous.argsRaw
				},
				callTime: previous?.time ?? null,
				content: message.content,
				isError: message.isError === true,
				...match.event.data.error === void 0 ? {} : { error: match.event.data.error },
				meta: match.event.data.meta,
				subCalls: []
			};
		}
		function locationTurn(match) {
			return match.location.kind === "step" || match.location.kind === "turn" ? match.location.turn.turn : 0;
		}
		function locationStep(match) {
			return match.location.kind === "step" ? match.location.step.step : 0;
		}
		function childCall(match, data) {
			return {
				phase: "start",
				callId: data.subCallId,
				parentCallId: data.parentCallId,
				name: data.name,
				argsRaw: JSON.stringify(data.arguments),
				args: PartialArguments.fromObject(data.arguments),
				turn: locationTurn(match),
				step: locationStep(match),
				time: match.event.time,
				subCalls: []
			};
		}
		function childResult(match, data, previous) {
			return {
				kind: "tool-result",
				seq: match.event.seq,
				time: match.event.time,
				callId: data.subCallId,
				parentCallId: data.parentCallId,
				name: data.name,
				args: previous?.args ?? PartialArguments.fromObject(data.arguments),
				call: {
					name: data.name,
					argsRaw: JSON.stringify(data.arguments)
				},
				callTime: previous === void 0 || "kind" in previous ? null : previous.time,
				content: data.content ?? [],
				meta: data.meta,
				isError: data.isError === true,
				...data.error === void 0 ? {} : { error: data.error },
				subCalls: []
			};
		}
		function acceptsEdge(state, parent, child) {
			if (parent === child || state.parents.has(child)) return false;
			let cursor = parent;
			let parentDepth = 0;
			const ancestors = /* @__PURE__ */ new Set();
			while (cursor !== void 0) {
				if (cursor === child || ancestors.has(cursor)) return false;
				ancestors.add(cursor);
				parentDepth++;
				cursor = state.parents.get(cursor);
			}
			const pending = [{
				callId: child,
				depth: 1
			}];
			const descendants = /* @__PURE__ */ new Set();
			let subtreeDepth = 0;
			for (const candidate of pending) {
				if (descendants.has(candidate.callId)) return false;
				descendants.add(candidate.callId);
				subtreeDepth = Math.max(subtreeDepth, candidate.depth);
				for (const nested of state.children.get(candidate.callId) ?? []) pending.push({
					callId: nested,
					depth: candidate.depth + 1
				});
			}
			return parentDepth + subtreeDepth <= MAX_DEPTH;
		}
		function updateDispatch(state, match) {
			const event = match.event;
			if (event.type !== "tool/ptc-dispatch-start" && event.type !== "tool/ptc-dispatch") return state;
			const data = event.data;
			const parentId = String(data.parentCallId);
			const childId = String(data.subCallId);
			const siblings = state.children.get(parentId) ?? [];
			const index = siblings.indexOf(childId);
			if (index < 0 && !acceptsEdge(state, parentId, childId)) return state;
			if (event.type === "tool/ptc-dispatch-start" && index >= 0) return state;
			const calls = new Map(state.calls);
			calls.set(childId, event.type === "tool/ptc-dispatch-start" ? childCall(match, data) : childResult(match, data, calls.get(childId)));
			if (index >= 0) return {
				...state,
				calls
			};
			const children = new Map(state.children);
			children.set(parentId, [...siblings, childId]);
			const parents = new Map(state.parents);
			parents.set(childId, parentId);
			return {
				...state,
				calls,
				children,
				parents
			};
		}
		function interruption(context) {
			const location = context.start?.location;
			if (location?.kind === "step" && location.step.status === "closed") return location.step.end;
			if ((location?.kind === "step" || location?.kind === "turn") && location.turn.status === "closed") return location.turn.end;
		}
		function projectCall(state, callId, interruptedAt, visited = /* @__PURE__ */ new Set(), depth = 1) {
			const block = state.calls.get(callId);
			if (block === void 0) return void 0;
			if (visited.has(callId) || depth > MAX_DEPTH) return {
				...block,
				subCalls: []
			};
			const nextVisited = new Set(visited);
			nextVisited.add(callId);
			const subCalls = (state.children.get(callId) ?? []).flatMap((childId) => {
				const child = projectCall(state, childId, interruptedAt, nextVisited, depth + 1);
				return child === void 0 ? [] : [child];
			});
			if ("kind" in block || interruptedAt === void 0) return {
				...block,
				subCalls
			};
			return {
				kind: "tool-result",
				seq: interruptedAt.seq - .8,
				time: interruptedAt.time,
				callId: block.callId,
				...block.parentCallId === void 0 ? {} : { parentCallId: block.parentCallId },
				name: block.name,
				args: block.args,
				call: {
					name: block.name,
					argsRaw: block.argsRaw
				},
				callTime: block.time,
				content: [],
				isError: true,
				error: {
					name: "Interrupted",
					code: "interrupted"
				},
				subCalls
			};
		}
		function fallbackState(context) {
			const resultMatch = context.matches.find((match) => match.event.type === "tool/result");
			const root = resultMatch === void 0 ? void 0 : rootResult(resultMatch);
			if (root === void 0) return void 0;
			let state = {
				rootId: root.callId,
				calls: new Map([[root.callId, root]]),
				children: /* @__PURE__ */ new Map(),
				parents: /* @__PURE__ */ new Map()
			};
			for (const match of context.matches) state = updateDispatch(state, match);
			return state;
		}
		/** Trajectory-owned root Tool lifecycle with nested PTC dispatch calls. */
		const trajectoryToolDefinition = {
			kind: "trajectory-tool-call",
			target: "trajectory",
			match: (event) => {
				if (event.type === "tool/call") return {
					id: String(event.data.callId),
					role: "start"
				};
				if (event.type === "tool/result") return {
					id: String(event.data.message.source.callId),
					role: "update"
				};
				if (event.type === "tool/ptc-dispatch-start" || event.type === "tool/ptc-dispatch") {
					const rootCallId = event.data.rootCallId;
					return typeof rootCallId === "string" && rootCallId !== "" ? {
						id: rootCallId,
						role: "update"
					} : null;
				}
				return null;
			},
			start: (_context, match) => {
				const root = rootCall(match);
				return {
					rootId: root.callId,
					calls: new Map([[root.callId, root]]),
					children: /* @__PURE__ */ new Map(),
					parents: /* @__PURE__ */ new Map()
				};
			},
			update: (context, match) => {
				if (match.event.type !== "tool/result") return updateDispatch(context.state, match);
				const previous = context.state.calls.get(context.state.rootId);
				const result = rootResult(match, previous !== void 0 && !("kind" in previous) ? previous : void 0);
				if (result === void 0) return context.state;
				const calls = new Map(context.state.calls);
				calls.set(context.state.rootId, result);
				return {
					...context.state,
					calls
				};
			},
			buildViewNode: (context) => {
				const state = context.state ?? fallbackState(context);
				if (state === void 0) return null;
				const root = projectCall(state, state.rootId, interruption(context));
				if (root === void 0) return null;
				return trajectoryNode(context, context.start?.event.seq ?? ("kind" in root ? root.seq : context.matches[0]?.event.seq ?? 0), {
					kind: "tool",
					root
				});
			}
		};
		/**
		* Register the Trajectory Tool lifecycle with raw native and PTC error details.
		*
		* @param ctx - Plugin context receiving the Definition.
		*/
		function registerTrajectoryToolDefinition(ctx) {
			ctx.uiConversation.events.register(trajectoryToolDefinition);
		}
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/diff/base.js
		var Diff = class {
			diff(oldStr, newStr, options = {}) {
				let callback;
				if (typeof options === "function") {
					callback = options;
					options = {};
				} else if ("callback" in options) callback = options.callback;
				const oldString = this.castInput(oldStr, options);
				const newString = this.castInput(newStr, options);
				const oldTokens = this.removeEmpty(this.tokenize(oldString, options));
				const newTokens = this.removeEmpty(this.tokenize(newString, options));
				return this.diffWithOptionsObj(oldTokens, newTokens, options, callback);
			}
			diffWithOptionsObj(oldTokens, newTokens, options, callback) {
				var _a;
				const done = (value) => {
					value = this.postProcess(value, options);
					if (callback) {
						setTimeout(function() {
							callback(value);
						}, 0);
						return;
					} else return value;
				};
				const newLen = newTokens.length, oldLen = oldTokens.length;
				let editLength = 1;
				let maxEditLength = newLen + oldLen;
				if (options.maxEditLength != null) maxEditLength = Math.min(maxEditLength, options.maxEditLength);
				const maxExecutionTime = (_a = options.timeout) !== null && _a !== void 0 ? _a : Infinity;
				const abortAfterTimestamp = Date.now() + maxExecutionTime;
				const bestPath = [{
					oldPos: -1,
					lastComponent: void 0
				}];
				let newPos = this.extractCommon(bestPath[0], newTokens, oldTokens, 0, options);
				if (bestPath[0].oldPos + 1 >= oldLen && newPos + 1 >= newLen) return done(this.buildValues(bestPath[0].lastComponent, newTokens, oldTokens));
				let minDiagonalToConsider = -Infinity, maxDiagonalToConsider = Infinity;
				const execEditLength = () => {
					for (let diagonalPath = Math.max(minDiagonalToConsider, -editLength); diagonalPath <= Math.min(maxDiagonalToConsider, editLength); diagonalPath += 2) {
						let basePath;
						const removePath = bestPath[diagonalPath - 1], addPath = bestPath[diagonalPath + 1];
						if (removePath) bestPath[diagonalPath - 1] = void 0;
						let canAdd = false;
						if (addPath) {
							const addPathNewPos = addPath.oldPos - diagonalPath;
							canAdd = addPath && 0 <= addPathNewPos && addPathNewPos < newLen;
						}
						const canRemove = removePath && removePath.oldPos + 1 < oldLen;
						if (!canAdd && !canRemove) {
							bestPath[diagonalPath] = void 0;
							continue;
						}
						if (!canRemove || canAdd && removePath.oldPos < addPath.oldPos) basePath = this.addToPath(addPath, true, false, 0, options);
						else basePath = this.addToPath(removePath, false, true, 1, options);
						newPos = this.extractCommon(basePath, newTokens, oldTokens, diagonalPath, options);
						if (basePath.oldPos + 1 >= oldLen && newPos + 1 >= newLen) return done(this.buildValues(basePath.lastComponent, newTokens, oldTokens)) || true;
						else {
							bestPath[diagonalPath] = basePath;
							if (basePath.oldPos + 1 >= oldLen) maxDiagonalToConsider = Math.min(maxDiagonalToConsider, diagonalPath - 1);
							if (newPos + 1 >= newLen) minDiagonalToConsider = Math.max(minDiagonalToConsider, diagonalPath + 1);
						}
					}
					editLength++;
				};
				if (callback) (function exec() {
					setTimeout(function() {
						if (editLength > maxEditLength || Date.now() > abortAfterTimestamp) return callback(void 0);
						if (!execEditLength()) exec();
					}, 0);
				})();
				else while (editLength <= maxEditLength && Date.now() <= abortAfterTimestamp) {
					const ret = execEditLength();
					if (ret) return ret;
				}
			}
			addToPath(path, added, removed, oldPosInc, options) {
				const last = path.lastComponent;
				if (last && !options.oneChangePerToken && last.added === added && last.removed === removed) return {
					oldPos: path.oldPos + oldPosInc,
					lastComponent: {
						count: last.count + 1,
						added,
						removed,
						previousComponent: last.previousComponent
					}
				};
				else return {
					oldPos: path.oldPos + oldPosInc,
					lastComponent: {
						count: 1,
						added,
						removed,
						previousComponent: last
					}
				};
			}
			extractCommon(basePath, newTokens, oldTokens, diagonalPath, options) {
				const newLen = newTokens.length, oldLen = oldTokens.length;
				let oldPos = basePath.oldPos, newPos = oldPos - diagonalPath, commonCount = 0;
				while (newPos + 1 < newLen && oldPos + 1 < oldLen && this.equals(oldTokens[oldPos + 1], newTokens[newPos + 1], options)) {
					newPos++;
					oldPos++;
					commonCount++;
					if (options.oneChangePerToken) basePath.lastComponent = {
						count: 1,
						previousComponent: basePath.lastComponent,
						added: false,
						removed: false
					};
				}
				if (commonCount && !options.oneChangePerToken) basePath.lastComponent = {
					count: commonCount,
					previousComponent: basePath.lastComponent,
					added: false,
					removed: false
				};
				basePath.oldPos = oldPos;
				return newPos;
			}
			equals(left, right, options) {
				if (options.comparator) return options.comparator(left, right);
				else return left === right || !!options.ignoreCase && left.toLowerCase() === right.toLowerCase();
			}
			removeEmpty(array) {
				const ret = [];
				for (let i = 0; i < array.length; i++) if (array[i]) ret.push(array[i]);
				return ret;
			}
			castInput(value, options) {
				return value;
			}
			tokenize(value, options) {
				return Array.from(value);
			}
			join(chars) {
				return chars.join("");
			}
			postProcess(changeObjects, options) {
				return changeObjects;
			}
			get useLongestToken() {
				return false;
			}
			buildValues(lastComponent, newTokens, oldTokens) {
				const components = [];
				let nextComponent;
				while (lastComponent) {
					components.push(lastComponent);
					nextComponent = lastComponent.previousComponent;
					delete lastComponent.previousComponent;
					lastComponent = nextComponent;
				}
				components.reverse();
				const componentLen = components.length;
				let componentPos = 0, newPos = 0, oldPos = 0;
				for (; componentPos < componentLen; componentPos++) {
					const component = components[componentPos];
					if (!component.removed) {
						if (!component.added && this.useLongestToken) {
							let value = newTokens.slice(newPos, newPos + component.count);
							value = value.map(function(value, i) {
								const oldValue = oldTokens[oldPos + i];
								return oldValue.length > value.length ? oldValue : value;
							});
							component.value = this.join(value);
						} else component.value = this.join(newTokens.slice(newPos, newPos + component.count));
						newPos += component.count;
						if (!component.added) oldPos += component.count;
					} else {
						component.value = this.join(oldTokens.slice(oldPos, oldPos + component.count));
						oldPos += component.count;
					}
				}
				return components;
			}
		};
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/diff/character.js
		var CharacterDiff = class extends Diff {};
		new CharacterDiff();
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/util/string.js
		function longestCommonPrefix(str1, str2) {
			let i;
			for (i = 0; i < str1.length && i < str2.length; i++) if (str1[i] != str2[i]) return str1.slice(0, i);
			return str1.slice(0, i);
		}
		function longestCommonSuffix(str1, str2) {
			let i;
			if (!str1 || !str2 || str1[str1.length - 1] != str2[str2.length - 1]) return "";
			for (i = 0; i < str1.length && i < str2.length; i++) if (str1[str1.length - (i + 1)] != str2[str2.length - (i + 1)]) return str1.slice(-i);
			return str1.slice(-i);
		}
		function replacePrefix(string, oldPrefix, newPrefix) {
			if (string.slice(0, oldPrefix.length) != oldPrefix) throw Error(`string ${JSON.stringify(string)} doesn't start with prefix ${JSON.stringify(oldPrefix)}; this is a bug`);
			return newPrefix + string.slice(oldPrefix.length);
		}
		function replaceSuffix(string, oldSuffix, newSuffix) {
			if (!oldSuffix) return string + newSuffix;
			if (string.slice(-oldSuffix.length) != oldSuffix) throw Error(`string ${JSON.stringify(string)} doesn't end with suffix ${JSON.stringify(oldSuffix)}; this is a bug`);
			return string.slice(0, -oldSuffix.length) + newSuffix;
		}
		function removePrefix(string, oldPrefix) {
			return replacePrefix(string, oldPrefix, "");
		}
		function removeSuffix(string, oldSuffix) {
			return replaceSuffix(string, oldSuffix, "");
		}
		function maximumOverlap(string1, string2) {
			return string2.slice(0, overlapCount(string1, string2));
		}
		function overlapCount(a, b) {
			let startA = 0;
			if (a.length > b.length) startA = a.length - b.length;
			let endB = b.length;
			if (a.length < b.length) endB = a.length;
			const map = Array(endB);
			let k = 0;
			map[0] = 0;
			for (let j = 1; j < endB; j++) {
				if (b[j] == b[k]) map[j] = map[k];
				else map[j] = k;
				while (k > 0 && b[j] != b[k]) k = map[k];
				if (b[j] == b[k]) k++;
			}
			k = 0;
			for (let i = startA; i < a.length; i++) {
				while (k > 0 && a[i] != b[k]) k = map[k];
				if (a[i] == b[k]) k++;
			}
			return k;
		}
		/**
		* Split a string into segments using a word segmenter, merging consecutive
		* segments if they are both whitespace segments. Whitespace segments can
		* appear adjacent to one another for two reasons:
		* - newlines always get their own segment
		* - where a diacritic is attached to a whitespace character in the text, the
		*   segment ends after the diacritic, so e.g. " \u0300 " becomes two segments.
		* This function therefore runs the segmenter's .segment() method and then
		* merges consecutive segments of whitespace into a single part.
		*/
		function segment(string, segmenter) {
			const parts = [];
			for (const segmentObj of Array.from(segmenter.segment(string))) {
				const segment = segmentObj.segment;
				if (parts.length && /\s/.test(parts[parts.length - 1]) && /\s/.test(segment)) parts[parts.length - 1] += segment;
				else parts.push(segment);
			}
			return parts;
		}
		function trailingWs(string, segmenter) {
			if (segmenter) return leadingAndTrailingWs(string, segmenter)[1];
			let i;
			for (i = string.length - 1; i >= 0; i--) if (!string[i].match(/\s/)) break;
			return string.substring(i + 1);
		}
		function leadingWs(string, segmenter) {
			if (segmenter) return leadingAndTrailingWs(string, segmenter)[0];
			const match = string.match(/^\s*/);
			return match ? match[0] : "";
		}
		function leadingAndTrailingWs(string, segmenter) {
			if (!segmenter) return [leadingWs(string), trailingWs(string)];
			if (segmenter.resolvedOptions().granularity != "word") throw new Error("The segmenter passed must have a granularity of \"word\"");
			const segments = segment(string, segmenter);
			const firstSeg = segments[0];
			const lastSeg = segments[segments.length - 1];
			return [/\s/.test(firstSeg) ? firstSeg : "", /\s/.test(lastSeg) ? lastSeg : ""];
		}
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/diff/word.js
		const extendedWordChars = "a-zA-Z0-9_\\u{AD}\\u{C0}-\\u{D6}\\u{D8}-\\u{F6}\\u{F8}-\\u{2C6}\\u{2C8}-\\u{2D7}\\u{2DE}-\\u{2FF}\\u{1E00}-\\u{1EFF}";
		const tokenizeIncludingWhitespace = new RegExp(`[${extendedWordChars}]+|\\s+|[^${extendedWordChars}]`, "ug");
		var WordDiff = class extends Diff {
			equals(left, right, options) {
				if (options.ignoreCase) {
					left = left.toLowerCase();
					right = right.toLowerCase();
				}
				return left.trim() === right.trim();
			}
			tokenize(value, options = {}) {
				let parts;
				if (options.intlSegmenter) {
					const segmenter = options.intlSegmenter;
					if (segmenter.resolvedOptions().granularity != "word") throw new Error("The segmenter passed must have a granularity of \"word\"");
					parts = segment(value, segmenter);
				} else parts = value.match(tokenizeIncludingWhitespace) || [];
				const tokens = [];
				let prevPart = null;
				parts.forEach((part) => {
					if (/\s/.test(part)) if (prevPart == null) tokens.push(part);
					else tokens.push(tokens.pop() + part);
					else if (prevPart != null && /\s/.test(prevPart)) if (tokens[tokens.length - 1] == prevPart) tokens.push(tokens.pop() + part);
					else tokens.push(prevPart + part);
					else tokens.push(part);
					prevPart = part;
				});
				return tokens;
			}
			join(tokens) {
				return tokens.map((token, i) => {
					if (i == 0) return token;
					else return token.replace(/^\s+/, "");
				}).join("");
			}
			postProcess(changes, options) {
				if (!changes || options.oneChangePerToken) return changes;
				let lastKeep = null;
				let insertion = null;
				let deletion = null;
				changes.forEach((change) => {
					if (change.added) insertion = change;
					else if (change.removed) deletion = change;
					else {
						if (insertion || deletion) dedupeWhitespaceInChangeObjects(lastKeep, deletion, insertion, change, options.intlSegmenter);
						lastKeep = change;
						insertion = null;
						deletion = null;
					}
				});
				if (insertion || deletion) dedupeWhitespaceInChangeObjects(lastKeep, deletion, insertion, null, options.intlSegmenter);
				return changes;
			}
		};
		new WordDiff();
		function dedupeWhitespaceInChangeObjects(startKeep, deletion, insertion, endKeep, segmenter) {
			if (deletion && insertion) {
				const [oldWsPrefix, oldWsSuffix] = leadingAndTrailingWs(deletion.value, segmenter);
				const [newWsPrefix, newWsSuffix] = leadingAndTrailingWs(insertion.value, segmenter);
				if (startKeep) {
					const commonWsPrefix = longestCommonPrefix(oldWsPrefix, newWsPrefix);
					startKeep.value = replaceSuffix(startKeep.value, newWsPrefix, commonWsPrefix);
					deletion.value = removePrefix(deletion.value, commonWsPrefix);
					insertion.value = removePrefix(insertion.value, commonWsPrefix);
				}
				if (endKeep) {
					const commonWsSuffix = longestCommonSuffix(oldWsSuffix, newWsSuffix);
					endKeep.value = replacePrefix(endKeep.value, newWsSuffix, commonWsSuffix);
					deletion.value = removeSuffix(deletion.value, commonWsSuffix);
					insertion.value = removeSuffix(insertion.value, commonWsSuffix);
				}
			} else if (insertion) {
				if (startKeep) {
					const ws = leadingWs(insertion.value, segmenter);
					insertion.value = insertion.value.substring(ws.length);
				}
				if (endKeep) {
					const ws = leadingWs(endKeep.value, segmenter);
					endKeep.value = endKeep.value.substring(ws.length);
				}
			} else if (startKeep && endKeep) {
				const newWsFull = leadingWs(endKeep.value, segmenter), [delWsStart, delWsEnd] = leadingAndTrailingWs(deletion.value, segmenter);
				const newWsStart = longestCommonPrefix(newWsFull, delWsStart);
				deletion.value = removePrefix(deletion.value, newWsStart);
				const newWsEnd = longestCommonSuffix(removePrefix(newWsFull, newWsStart), delWsEnd);
				deletion.value = removeSuffix(deletion.value, newWsEnd);
				endKeep.value = replacePrefix(endKeep.value, newWsFull, newWsEnd);
				startKeep.value = replaceSuffix(startKeep.value, newWsFull, newWsFull.slice(0, newWsFull.length - newWsEnd.length));
			} else if (endKeep) {
				const endKeepWsPrefix = leadingWs(endKeep.value, segmenter);
				const overlap = maximumOverlap(trailingWs(deletion.value, segmenter), endKeepWsPrefix);
				deletion.value = removeSuffix(deletion.value, overlap);
			} else if (startKeep) {
				const overlap = maximumOverlap(trailingWs(startKeep.value, segmenter), leadingWs(deletion.value, segmenter));
				deletion.value = removePrefix(deletion.value, overlap);
			}
		}
		var WordsWithSpaceDiff = class extends Diff {
			tokenize(value) {
				const regex = new RegExp(`(\\r?\\n)|[${extendedWordChars}]+|[^\\S\\n\\r]+|[^${extendedWordChars}]`, "ug");
				return value.match(regex) || [];
			}
		};
		new WordsWithSpaceDiff();
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/diff/line.js
		var LineDiff = class extends Diff {
			constructor() {
				super(...arguments);
				this.tokenize = tokenize;
			}
			equals(left, right, options) {
				if (options.ignoreWhitespace) {
					if (!options.newlineIsToken || !left.includes("\n")) left = left.trim();
					if (!options.newlineIsToken || !right.includes("\n")) right = right.trim();
				} else if (options.ignoreNewlineAtEof && !options.newlineIsToken) {
					if (left.endsWith("\n")) left = left.slice(0, -1);
					if (right.endsWith("\n")) right = right.slice(0, -1);
				}
				return super.equals(left, right, options);
			}
		};
		const lineDiff = new LineDiff();
		function diffLines(oldStr, newStr, options) {
			return lineDiff.diff(oldStr, newStr, options);
		}
		function tokenize(value, options) {
			if (options.stripTrailingCr) value = value.replace(/\r\n/g, "\n");
			const retLines = [], linesAndNewlines = value.split(/(\n|\r\n)/);
			if (!linesAndNewlines[linesAndNewlines.length - 1]) linesAndNewlines.pop();
			for (let i = 0; i < linesAndNewlines.length; i++) {
				const line = linesAndNewlines[i];
				if (i % 2 && !options.newlineIsToken) retLines[retLines.length - 1] += line;
				else retLines.push(line);
			}
			return retLines;
		}
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/diff/sentence.js
		function isSentenceEndPunct(char) {
			return char == "." || char == "!" || char == "?";
		}
		var SentenceDiff = class extends Diff {
			tokenize(value) {
				var _a;
				const result = [];
				let tokenStartI = 0;
				for (let i = 0; i < value.length; i++) {
					if (i == value.length - 1) {
						result.push(value.slice(tokenStartI));
						break;
					}
					if (isSentenceEndPunct(value[i]) && value[i + 1].match(/\s/)) {
						result.push(value.slice(tokenStartI, i + 1));
						i = tokenStartI = i + 1;
						while ((_a = value[i + 1]) === null || _a === void 0 ? void 0 : _a.match(/\s/)) i++;
						result.push(value.slice(tokenStartI, i + 1));
						tokenStartI = i + 1;
					}
				}
				return result;
			}
		};
		new SentenceDiff();
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/diff/css.js
		var CssDiff = class extends Diff {
			tokenize(value) {
				return value.split(/([{}:;,]|\s+)/);
			}
		};
		new CssDiff();
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/diff/json.js
		var JsonDiff = class extends Diff {
			constructor() {
				super(...arguments);
				this.tokenize = tokenize;
			}
			get useLongestToken() {
				return true;
			}
			castInput(value, options) {
				const { undefinedReplacement, stringifyReplacer = (k, v) => typeof v === "undefined" ? undefinedReplacement : v } = options;
				return typeof value === "string" ? value : JSON.stringify(canonicalize(value, null, null, stringifyReplacer), null, "  ");
			}
			equals(left, right, options) {
				return super.equals(left.replace(/,([\r\n])/g, "$1"), right.replace(/,([\r\n])/g, "$1"), options);
			}
		};
		new JsonDiff();
		function canonicalize(obj, stack, replacementStack, replacer, key) {
			stack = stack || [];
			replacementStack = replacementStack || [];
			if (replacer) obj = replacer(key === void 0 ? "" : key, obj);
			let i;
			for (i = 0; i < stack.length; i += 1) if (stack[i] === obj) return replacementStack[i];
			let canonicalizedObj;
			if ("[object Array]" === Object.prototype.toString.call(obj)) {
				stack.push(obj);
				canonicalizedObj = new Array(obj.length);
				replacementStack.push(canonicalizedObj);
				for (i = 0; i < obj.length; i += 1) canonicalizedObj[i] = canonicalize(obj[i], stack, replacementStack, replacer, String(i));
				stack.pop();
				replacementStack.pop();
				return canonicalizedObj;
			}
			if (obj && obj.toJSON) obj = obj.toJSON();
			if (typeof obj === "object" && obj !== null) {
				stack.push(obj);
				canonicalizedObj = {};
				replacementStack.push(canonicalizedObj);
				const sortedKeys = [];
				let key;
				for (key in obj)
 /* istanbul ignore else */
				if (Object.prototype.hasOwnProperty.call(obj, key)) sortedKeys.push(key);
				sortedKeys.sort();
				for (i = 0; i < sortedKeys.length; i += 1) {
					key = sortedKeys[i];
					canonicalizedObj[key] = canonicalize(obj[key], stack, replacementStack, replacer, key);
				}
				stack.pop();
				replacementStack.pop();
			} else canonicalizedObj = obj;
			return canonicalizedObj;
		}
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/diff/array.js
		var ArrayDiff = class extends Diff {
			tokenize(value) {
				return value.slice();
			}
			join(value) {
				return value;
			}
			removeEmpty(value) {
				return value;
			}
		};
		new ArrayDiff();
		//#endregion
		//#region ../../../node_modules/.pnpm/diff@9.0.0/node_modules/diff/libesm/patch/create.js
		function structuredPatch(oldFileName, newFileName, oldStr, newStr, oldHeader, newHeader, options) {
			let optionsObj;
			if (!options) optionsObj = {};
			else if (typeof options === "function") optionsObj = { callback: options };
			else optionsObj = options;
			if (typeof optionsObj.context === "undefined") optionsObj.context = 4;
			const context = optionsObj.context;
			if (optionsObj.newlineIsToken) throw new Error("newlineIsToken may not be used with patch-generation functions, only with diffing functions");
			if (!optionsObj.callback) return diffLinesResultToPatch(diffLines(oldStr, newStr, optionsObj));
			else {
				const { callback } = optionsObj;
				diffLines(oldStr, newStr, Object.assign(Object.assign({}, optionsObj), { callback: (diff) => {
					callback(diffLinesResultToPatch(diff));
				} }));
			}
			function diffLinesResultToPatch(diff) {
				if (!diff) return;
				diff.push({
					value: "",
					lines: []
				});
				function contextLines(lines) {
					return lines.map(function(entry) {
						return " " + entry;
					});
				}
				const hunks = [];
				let oldRangeStart = 0, newRangeStart = 0, curRange = [], oldLine = 1, newLine = 1;
				for (let i = 0; i < diff.length; i++) {
					const current = diff[i], lines = current.lines || splitLines(current.value);
					current.lines = lines;
					if (current.added || current.removed) {
						if (!oldRangeStart) {
							const prev = diff[i - 1];
							oldRangeStart = oldLine;
							newRangeStart = newLine;
							if (prev) {
								curRange = context > 0 ? contextLines(prev.lines.slice(-context)) : [];
								oldRangeStart -= curRange.length;
								newRangeStart -= curRange.length;
							}
						}
						for (const line of lines) curRange.push((current.added ? "+" : "-") + line);
						if (current.added) newLine += lines.length;
						else oldLine += lines.length;
					} else {
						if (oldRangeStart) if (lines.length <= context * 2 && i < diff.length - 2) for (const line of contextLines(lines)) curRange.push(line);
						else {
							const contextSize = Math.min(lines.length, context);
							for (const line of contextLines(lines.slice(0, contextSize))) curRange.push(line);
							const hunk = {
								oldStart: oldRangeStart,
								oldLines: oldLine - oldRangeStart + contextSize,
								newStart: newRangeStart,
								newLines: newLine - newRangeStart + contextSize,
								lines: curRange
							};
							hunks.push(hunk);
							oldRangeStart = 0;
							newRangeStart = 0;
							curRange = [];
						}
						oldLine += lines.length;
						newLine += lines.length;
					}
				}
				for (const hunk of hunks) for (let i = 0; i < hunk.lines.length; i++) if (hunk.lines[i].endsWith("\n")) hunk.lines[i] = hunk.lines[i].slice(0, -1);
				else {
					hunk.lines.splice(i + 1, 0, "\\ No newline at end of file");
					i++;
				}
				return {
					oldFileName,
					newFileName,
					oldHeader,
					newHeader,
					hunks
				};
			}
		}
		/**
		* Split `text` into an array of lines, including the trailing newline character (where present)
		*/
		function splitLines(text) {
			const hasTrailingNl = text.endsWith("\n");
			const result = text.split("\n").map((line) => line + "\n");
			if (hasTrailingNl) result.pop();
			else result.push(result.pop().slice(0, -1));
			return result;
		}
		//#endregion
		//#region lib/types/client/trajectory-record.js
		/** Shared trajectory record data and formatting contracts. */
		/**
		* Resolve the identity that survives prepending older projected records.
		* @param cell - Projected trajectory record.
		* @returns Stable identity from the owning event or tool call, with a fixture fallback.
		*/
		function trajectoryRecordId(cell) {
			if (cell.recordId !== void 0) return cell.recordId;
			if (cell.callId !== void 0) return `${cell.kind}\u0000call\u0000${cell.callId}`;
			if (cell.sourceSeq !== void 0) return `${cell.kind}\u0000seq\u0000${cell.sourceSeq}`;
			return `${cell.kind}\u0000index\u0000${cell.index}`;
		}
		/**
		* Format a duration in milliseconds with thousands separators.
		* @param milliseconds - Duration in milliseconds, or `null` when absent.
		* @param t - Trajectory locale translator.
		* @returns `—` when unknown, otherwise an integer-millisecond label.
		*/
		function formatDurationMillis(milliseconds, t) {
			if (milliseconds === null || !Number.isFinite(milliseconds)) return "—";
			return t("unit.milliseconds", { value: String(Math.round(milliseconds)).replace(/\B(?=(\d{3})+(?!\d))/g, ",") });
		}
		/**
		* Format an elapsed duration given in seconds as a millisecond label.
		* @param seconds - Duration seconds, or `null` when absent.
		* @param t - Trajectory locale translator.
		* @returns `—` when unknown, otherwise an integer-millisecond label.
		*/
		function formatElapsedSeconds(seconds, t) {
			return formatDurationMillis(seconds === null ? null : seconds * 1e3, t);
		}
		//#endregion
		//#region lib/types/client/trajectory-virtual-rows.js
		/** Pure projection from trajectory records to measurable virtual ledger rows. */
		const CONTENT_ROW_HEIGHT = 30;
		const COLLAPSED_SUMMARY_HEIGHT = 20;
		const TERMINAL_BOUNDARY_HEIGHT = 9;
		/**
		* Derive the DOM-safe row identity shared by React, the virtualizer, and
		* browser scroll contracts.
		* @param record - Display record whose identity is required.
		* @returns Stable record identity with a suffix for synthetic fold summaries.
		*/
		function trajectoryVirtualRecordKey(record) {
			const identity = encodeURIComponent(trajectoryRecordId(record.cell));
			return record.collapsedSummaryKind === void 0 ? identity : `${identity}\u0000summary\u0000${record.collapsedSummaryKind}`;
		}
		/**
		* Attach separator-only records to the next content row so the virtualizer
		* never owns a zero-height item. A terminal separator retains its CSS-owned
		* lower-marker clearance as a standalone item.
		* @param records - Final search/fold projection in ledger order.
		* @returns Measurable virtual rows with original logical positions retained.
		*/
		function groupTrajectoryVirtualRows(records) {
			const rows = [];
			let pending = [];
			for (const [logicalIndex, record] of records.entries()) {
				const entry = {
					logicalIndex,
					record
				};
				if (record.cell.requestOnly === true) {
					pending.push(entry);
					continue;
				}
				const entries = [...pending, entry];
				pending = [];
				rows.push({
					entries,
					height: record.collapsedSummaryKind === void 0 ? CONTENT_ROW_HEIGHT : COLLAPSED_SUMMARY_HEIGHT,
					key: trajectoryVirtualRecordKey(record)
				});
			}
			if (pending.length > 0) rows.push({
				entries: pending,
				height: TERMINAL_BOUNDARY_HEIGHT,
				key: pending.map((candidate) => trajectoryVirtualRecordKey(candidate.record)).join("|")
			});
			return rows;
		}
		//#endregion
		//#region lib/types/client/trajectory-preview.js
		/** Bounded Markdown-to-text projection shared by trajectory consumers. */
		const PREVIEW_SOURCE_CHARACTERS = 2048;
		const PREVIEW_OUTPUT_CHARACTERS = 512;
		/**
		* Build a bounded one-line preview without parsing the complete Markdown document.
		* @param text - Untrusted message, reasoning, payload, or result text.
		* @returns A compact preview capped independently from the retained source.
		*/
		function trajectoryPreviewText(text) {
			const source = text.slice(0, PREVIEW_SOURCE_CHARACTERS);
			const compact = (0, _deepseek_ai_dsh_client_ui_primitives.extractMarkdownPlainText)(source).replace(/\s+/g, " ").trim();
			const preview = compact.slice(0, PREVIEW_OUTPUT_CHARACTERS).trimEnd();
			return source.length < text.length || preview.length < compact.length ? `${preview}…` : preview;
		}
		//#endregion
		//#region lib/types/client/code-program.js
		/** Recorded name of the programmatic tool-calling entry point. */
		const PTC_TOOL_NAME = "run_code";
		function record(value) {
			return typeof value === "object" && value !== null && !Array.isArray(value) ? value : void 0;
		}
		function parseRecord(value) {
			if (value === void 0) return void 0;
			try {
				return record(JSON.parse(value));
			} catch {
				return;
			}
		}
		function recordedLanguage(schemaRaw) {
			const description = record(record(record(parseRecord(schemaRaw)?.parameters)?.properties)?.code)?.description;
			if (typeof description !== "string") return void 0;
			const typescript = /\bTypeScript\b/i.test(description);
			if (typescript === /\bPython\b/i.test(description)) return void 0;
			return typescript ? "typescript" : "python";
		}
		/**
		* Resolve a PTC program without guessing its language from source or current runtime settings.
		* @param cell - Recorded tool arguments and the schema visible at call time.
		* @returns The program, or undefined for another tool or unsupported arguments.
		*/
		function codeProgram(cell) {
			if (cell.kind !== "tool" && cell.kind !== "subtool") return void 0;
			if (cell.toolName !== "run_code" || cell.inputDetail === void 0) return void 0;
			const args = parseRecord(cell.inputDetail);
			if (typeof args?.code !== "string") return void 0;
			if (args.description !== void 0 && typeof args.description !== "string") return void 0;
			return {
				rawInput: cell.inputDetail,
				source: args.code,
				description: typeof args.description === "string" && args.description.trim() !== "" ? args.description : args.code.split(/\r?\n/).find((line) => line.trim() !== "")?.trim() ?? "",
				arguments: args,
				language: recordedLanguage(cell.schemaDetail)
			};
		}
		//#endregion
		//#region \0dsh-css:/home/runner/work/deepseek-harness/deepseek-harness/packages/client/ui-trajectory/src/client/TrajectoryTable.module.css.mjs
		const css$3 = ".rtjY0dWHa_split{--dsh-scrollbar-thumb:var(--dsw-alias-scrollbar-bg-l2);--dsh-scrollbar-thumb-hover:var(--dsw-alias-scrollbar-hover-l2);background:var(--dsw-alias-bg-layer-1);flex:1;width:100%;min-height:0;display:flex;position:relative;overflow:hidden;container-type:inline-size}.rtjY0dWHa_tablePane{min-width:0;padding-bottom:var(--dsh-trajectory-bottom-clearance,0px);flex:1;position:relative;overflow:hidden auto;container:rtjY0dWHa_trajectory-table/inline-size}.rtjY0dWHa_historyLoading{z-index:5;pointer-events:none;height:0;position:sticky;top:0;overflow:visible}.rtjY0dWHa_historyLoadingBar{box-sizing:border-box;border-bottom:.5px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-layer-1);width:100%;height:30px;color:var(--dsw-alias-label-secondary);font:var(--dsw-font-xxs-12);justify-content:center;align-items:center;gap:6px;display:flex}.rtjY0dWHa_table tbody .rtjY0dWHa_historyLoadRow td{height:30px;padding:0}.rtjY0dWHa_table tbody .rtjY0dWHa_historyLoadRow+tr[data-turn-start=true] td:before{content:none}.rtjY0dWHa_historyLoadButton{background:var(--dsw-alias-bg-layer-1);width:100%;height:29px;color:var(--dsw-alias-label-secondary);cursor:pointer;font:var(--dsw-font-xxs-12);border:0;justify-content:center;align-items:center;gap:6px;display:flex}.rtjY0dWHa_historyLoadButton:hover:not(:disabled){background:var(--dsw-alias-interactive-bg-hover);color:var(--dsw-alias-label-primary)}.rtjY0dWHa_historyLoadButton:focus-visible{outline:var(--dsw-focus-ring-width) solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:-2px}.rtjY0dWHa_historyLoadButton:disabled{cursor:default}.rtjY0dWHa_visuallyHidden{clip:rect(0 0 0 0);white-space:nowrap;width:1px;height:1px;position:absolute;overflow:hidden}.rtjY0dWHa_table:not([data-scroll-ready=true]){visibility:hidden}.rtjY0dWHa_table{--trajectory-turn-accent:color-mix(in srgb, var(--dsw-static-blue-500) 22%, var(--dsw-alias-bg-layer-1));border-spacing:0;table-layout:fixed;width:100%;min-width:0;color:var(--dsw-alias-label-primary);background:var(--dsw-alias-bg-layer-1);font:var(--dsw-font-xxs-12)}.rtjY0dWHa_eventColumn{width:122px}.rtjY0dWHa_eventColumn:where(:lang(zh)){width:84px}.rtjY0dWHa_contentColumn{width:auto}.rtjY0dWHa_table th{z-index:3;box-sizing:border-box;border-bottom:.5px solid var(--dsw-alias-border-l2);height:30px;color:var(--dsw-alias-label-tertiary);background:var(--dsw-specific-sidebar-fill);font:var(--dsw-font-xxs-12);text-align:left;text-overflow:ellipsis;user-select:none;white-space:nowrap;padding:0 8px;font-weight:500;position:sticky;top:0;overflow:hidden}.rtjY0dWHa_eventHeader{text-align:right!important;padding-right:4px!important}.rtjY0dWHa_table td{box-sizing:border-box;border-bottom:.5px solid var(--dsw-alias-border-l1);text-overflow:ellipsis;white-space:nowrap;height:30px;padding:0 8px;overflow:hidden}.rtjY0dWHa_table tbody .rtjY0dWHa_virtualSpacer{pointer-events:none}.rtjY0dWHa_table tbody .rtjY0dWHa_virtualSpacer td{height:var(--trajectory-virtual-spacer-height);border:0;padding:0}.rtjY0dWHa_table tbody tr:not([data-collapsed-summary]):not([data-virtual-spacer]):not([data-history-load]){cursor:default;transition:background-color .12s var(--ds-ease-in-out), opacity .12s var(--ds-ease-in-out);outline:none}.rtjY0dWHa_table tbody tr[data-timeline-focus=outside]{opacity:.24}.rtjY0dWHa_table tbody tr:not([data-collapsed-summary]):not([data-virtual-spacer]):not([data-history-load]):not([data-selected=true]):hover{background:var(--dsw-alias-interactive-bg-hover)}.rtjY0dWHa_table tbody tr[data-request-only=true]:hover{background:0 0}.rtjY0dWHa_table tbody tr[data-request-only=true] td{border-bottom:0;height:0;padding-top:0;padding-bottom:0}.rtjY0dWHa_table tbody tr[data-terminal-request-boundary=true] td{height:9px}.rtjY0dWHa_table tbody tr[data-request-only=true] .rtjY0dWHa_turnRail{top:-15px;bottom:0}.rtjY0dWHa_table tbody tr:not([data-collapsed-summary]):focus-visible{box-shadow:inset 0 0 0 1px var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary))}.rtjY0dWHa_table tbody tr[data-selected=true]{background:var(--dsw-alias-interactive-bg-active)}.rtjY0dWHa_requestBoundaryControl{--request-boundary-base-left:12px;z-index:6;top:-8px;left:calc(var(--request-boundary-base-left) + var(--request-boundary-offset,0px));cursor:pointer;background:0 0;border:0;width:16px;height:16px;padding:0;position:absolute}.rtjY0dWHa_requestBoundaryControl:before{corner-shape:round;background:var(--dsw-alias-label-caption);width:5px;height:5px;box-shadow:0 0 0 2px var(--dsw-alias-bg-layer-1), 0 0 0 3px transparent;content:\"\";transition:background .12s var(--ds-ease-in-out), box-shadow .12s var(--ds-ease-in-out);border-radius:50%;position:absolute;top:5.5px;left:5.5px}.rtjY0dWHa_requestBoundaryControl:after{border:.5px solid var(--dsw-alias-border-l1);width:max-content;color:var(--dsw-alias-label-secondary);background:var(--dsw-alias-bg-layer-1);content:attr(data-label);font:9px/12px var(--ds-font-family-code);opacity:0;pointer-events:none;transition:opacity .12s var(--ds-ease-in-out), transform .12s var(--ds-ease-in-out);user-select:none;white-space:nowrap;border-radius:2px;padding:0 4px;position:absolute;top:2px;left:17px;transform:translate(-2px);box-shadow:0 2px 6px #0000001f}.rtjY0dWHa_requestBoundaryControl:hover:before,.rtjY0dWHa_requestBoundaryControl:focus-visible:before{background:var(--dsw-alias-brand-primary-new-colorprimary-new-color)}.rtjY0dWHa_requestBoundaryControlActive:before,.rtjY0dWHa_requestBoundaryControlActive:hover:before,.rtjY0dWHa_requestBoundaryControlActive:focus-visible:before{background:color-mix(in srgb, var(--dsw-alias-brand-primary-new-colorprimary-new-color) 18%, var(--dsw-alias-bg-layer-1));box-shadow:0 0 0 1.5px var(--dsw-alias-state-business-primary)}.rtjY0dWHa_requestBoundaryControl[data-request-status=error]:before,.rtjY0dWHa_requestBoundaryControl[data-request-status=error]:hover:before,.rtjY0dWHa_requestBoundaryControl[data-request-status=error]:focus-visible:before{background:var(--dsw-alias-state-error-primary)}.rtjY0dWHa_requestBoundaryControl:hover:after,.rtjY0dWHa_requestBoundaryControl:focus-visible:after{opacity:1;transform:translate(0)}.rtjY0dWHa_requestBoundaryControl:focus-visible{outline:none}.rtjY0dWHa_table tbody tr:has(.rtjY0dWHa_requestBoundaryControl:hover):not([data-selected=true]){background:0 0}.rtjY0dWHa_event{position:relative}.rtjY0dWHa_turnRail,.rtjY0dWHa_selectionRail{background:var(--dsw-alias-brand-primary-new-colorprimary-new-color);pointer-events:none;position:absolute;left:0}.rtjY0dWHa_turnRail{z-index:4;background:var(--trajectory-turn-accent);width:2px;top:-1px;bottom:-1px}.rtjY0dWHa_table tbody tr[data-turn-end=true] .rtjY0dWHa_turnRail{bottom:0}.rtjY0dWHa_selectionRail{z-index:5;width:3px;top:0;bottom:0}.rtjY0dWHa_table tbody tr[data-error=true] .rtjY0dWHa_turnRail{background:color-mix(in srgb, var(--dsw-alias-state-error-primary) 22%, var(--dsw-alias-bg-layer-1))}.rtjY0dWHa_table tbody tr[data-error=true] .rtjY0dWHa_selectionRail{background:var(--dsw-alias-state-error-primary)}.rtjY0dWHa_table tbody tr[data-turn-start=true] td{position:relative;overflow:visible}.rtjY0dWHa_table tbody tr[data-turn-start=true]:not(:first-child) td:before{z-index:1;background:var(--dsw-alias-border-l1);content:\"\";pointer-events:none;height:2px;position:absolute;top:0;left:0;right:0;transform:translateY(-50%)}.rtjY0dWHa_event{padding-left:36px!important;padding-right:4px!important;overflow:visible!important}.rtjY0dWHa_turnLabel{z-index:3;box-sizing:border-box;width:max-content;color:var(--dsw-alias-label-tertiary);background:var(--dsw-alias-bg-module-platform);font:8px/10px var(--ds-font-family-code);font-variant-numeric:tabular-nums;user-select:none;white-space:nowrap;border-radius:0 0 2px;flex:none;align-items:center;padding:1px 5px;display:inline-grid;position:absolute;top:0;left:0}.rtjY0dWHa_turnLabelFull,.rtjY0dWHa_turnLabelCompact{opacity:1;white-space:nowrap;grid-area:1/1;max-width:64px;overflow:hidden}.rtjY0dWHa_turnLabelCompact{opacity:0;max-width:0}.rtjY0dWHa_turnLabelActive{color:color-mix(in srgb, var(--dsw-static-blue-500) 55%, var(--dsw-alias-label-tertiary));background:var(--trajectory-turn-accent)}.rtjY0dWHa_eventInner{justify-content:flex-start;align-items:center;min-width:0;height:100%;display:flex}.rtjY0dWHa_kindSlot{flex:none;justify-content:flex-end;align-items:flex-end;width:76px;display:flex}.rtjY0dWHa_kindSlot:where(:lang(zh)){width:44px}.rtjY0dWHa_kindSlot [role=tooltip]{background:var(--dsw-alias-bg-layer-2);box-shadow:var(--dsw-elevation-panel);color:var(--dsw-alias-label-primary);border:0}.rtjY0dWHa_content{color:var(--dsw-alias-label-primary);padding-left:4px!important}.rtjY0dWHa_kindTag{box-sizing:border-box;border-radius:var(--dsw-radius-xs);letter-spacing:.035em;user-select:none;border:1px solid #0000;flex:none;align-items:center;height:19px;padding:0 5px;font-size:10px;font-weight:650;line-height:16px;display:inline-flex}.rtjY0dWHa_kindTagIcon{opacity:0;flex:none;justify-content:center;align-items:center;width:0;height:13px;display:inline-flex;overflow:hidden;transform:scale(.8)}.rtjY0dWHa_kindTagLabel{opacity:1;white-space:nowrap;max-width:72px;display:inline-block;overflow:hidden}.rtjY0dWHa_table .rtjY0dWHa_kindSlot .rtjY0dWHa_message{justify-content:center;width:100%}@container rtjY0dWHa_trajectory-table (width<=620px){.rtjY0dWHa_eventColumn{width:50px}.rtjY0dWHa_event{padding-left:28px!important;padding-right:3px!important}.rtjY0dWHa_requestBoundaryControl{--request-boundary-base-left:6px}.rtjY0dWHa_kindSlot{width:19px}.rtjY0dWHa_kindTag,.rtjY0dWHa_table .rtjY0dWHa_kindSlot .rtjY0dWHa_message{justify-content:center;width:19px;padding-left:0;padding-right:0}.rtjY0dWHa_kindTagIcon{opacity:1;width:13px;transform:scale(1)}.rtjY0dWHa_kindTagLabel,.rtjY0dWHa_turnLabelFull{opacity:0;max-width:0}.rtjY0dWHa_turnLabelCompact{opacity:1;max-width:64px}}@media (prefers-reduced-motion:no-preference){.rtjY0dWHa_eventColumn,.rtjY0dWHa_event,.rtjY0dWHa_requestBoundaryControl,.rtjY0dWHa_kindSlot,.rtjY0dWHa_kindTag,.rtjY0dWHa_kindTagIcon,.rtjY0dWHa_kindTagLabel,.rtjY0dWHa_turnLabelFull,.rtjY0dWHa_turnLabelCompact{transition-duration:.18s;transition-timing-function:var(--ds-ease-in-out)}.rtjY0dWHa_eventColumn,.rtjY0dWHa_kindSlot{transition-property:width}.rtjY0dWHa_event{transition-property:padding-right,padding-left}.rtjY0dWHa_requestBoundaryControl{transition-property:left}.rtjY0dWHa_kindTag{transition-property:padding-right,padding-left}.rtjY0dWHa_kindTagIcon{transition-property:width,opacity,transform}.rtjY0dWHa_kindTagLabel,.rtjY0dWHa_turnLabelFull,.rtjY0dWHa_turnLabelCompact{transition-property:max-width,opacity}}.rtjY0dWHa_user{color:var(--dsw-alias-state-business-primary);background:var(--dsw-alias-state-business-tertiary)}.rtjY0dWHa_systemNeutral{color:var(--dsw-alias-label-secondary);background:var(--dsw-alias-bg-module-platform)}.rtjY0dWHa_contextGreen{color:color-mix(in srgb, var(--dsw-alias-state-success-primary) 68%, var(--dsw-alias-label-secondary));background:var(--dsw-alias-state-success-tertiary)}.rtjY0dWHa_compacted{color:var(--dsw-alias-label-secondary);background:var(--dsw-alias-bg-module-platform)}.rtjY0dWHa_compactedSummary{font:var(--dsw-font-xs-13);margin-top:12px;padding:0 0 14px}.rtjY0dWHa_promptDiffSections{flex-direction:column;gap:14px;max-height:100%;padding:10px 14px 14px;display:flex;overflow:auto}.rtjY0dWHa_promptDiffSection{min-width:0}.rtjY0dWHa_promptDiffTitle{color:var(--dsw-alias-label-secondary);font:var(--dsw-font-xs-strong-13);user-select:none;margin:0 0 6px}.rtjY0dWHa_promptDiff{color:var(--dsw-alias-label-primary);background:var(--dsw-alias-bg-layer-1);font:11px/17px var(--ds-font-family-code);white-space:pre;margin:0;overflow:auto}.rtjY0dWHa_promptDiff span{min-width:max-content;padding:0 6px;display:block}.rtjY0dWHa_promptDiffLinemeta{color:var(--dsw-alias-label-caption);background:var(--dsw-alias-bg-module-platform);user-select:none}.rtjY0dWHa_promptDiffLinecontext{color:var(--dsw-alias-label-secondary)}.rtjY0dWHa_promptDiffLineadded{color:color-mix(in srgb, var(--dsw-alias-state-success-primary) 72%, var(--dsw-alias-label-primary));background:var(--dsw-alias-state-success-tertiary)}.rtjY0dWHa_promptDiffLineremoved{color:var(--dsw-alias-state-error-primary);background:color-mix(in srgb, var(--dsw-alias-state-error-primary) 12%, var(--dsw-alias-bg-layer-1))}.rtjY0dWHa_assistantVioletBright{color:color-mix(in srgb, var(--dsw-alias-brand-primary-new-colorprimary-new-color) 60%, var(--dsw-alias-state-error-secondary));background:color-mix(in srgb, color-mix(in srgb, var(--dsw-alias-brand-primary-new-colorprimary-new-color) 55%, var(--dsw-alias-state-error-secondary)) 15%, var(--dsw-alias-bg-layer-1))}.rtjY0dWHa_toolAmber{color:var(--dsw-alias-state-warn-label);background:var(--dsw-alias-state-warn-tertiary)}.rtjY0dWHa_subtoolAmber{color:color-mix(in srgb, var(--dsw-alias-state-warn-label) 62%, var(--dsw-alias-label-tertiary));background:color-mix(in srgb, var(--dsw-alias-state-warn-tertiary) 58%, var(--dsw-alias-bg-layer-1))}.rtjY0dWHa_contentText{text-overflow:ellipsis;white-space:nowrap;min-width:0;display:block;overflow:hidden}.rtjY0dWHa_toolCallOnly{color:var(--dsw-alias-label-tertiary)}.rtjY0dWHa_table tbody tr[data-collapsed-summary=turn] td,.rtjY0dWHa_table tbody tr[data-collapsed-summary=assistant] td{height:20px}.rtjY0dWHa_table tbody tr[data-collapsed-summary]{cursor:pointer;transition:background-color .12s var(--ds-ease-in-out);outline:none}.rtjY0dWHa_table tbody tr[data-collapsed-summary]:hover{background:var(--dsw-alias-interactive-bg-hover)}.rtjY0dWHa_table tbody tr[data-collapsed-summary]:focus-visible{box-shadow:inset 0 0 0 1px var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary))}.rtjY0dWHa_collapsedTurnContent{min-width:0;color:var(--dsw-alias-label-secondary);align-items:center;font-size:12px;line-height:16px;display:flex}.rtjY0dWHa_collapsedTurnEllipsis{color:var(--dsw-alias-label-tertiary);user-select:none;flex:none;margin-right:6px;font-weight:600}.rtjY0dWHa_collapsedTurnText{text-overflow:ellipsis;white-space:nowrap;min-width:0;overflow:hidden}.rtjY0dWHa_resultPreview{grid-template-columns:clamp(180px, var(--trajectory-tool-request-width,calc(36cqw - 56px)), 480px) minmax(0, 1fr);align-items:center;gap:8px;min-width:0;display:grid}.rtjY0dWHa_resultRequest,.rtjY0dWHa_inlineResultText{text-overflow:ellipsis;white-space:nowrap;min-width:0;overflow:hidden}.rtjY0dWHa_toolCallNameTypeface{color:var(--dsw-alias-label-primary);font:400 12px/18px Menlo,Consolas,Liberation Mono,PingFang SC,Microsoft YaHei}.rtjY0dWHa_toolCallPayload{color:var(--dsw-alias-label-secondary);font:400 12px/18px var(--ds-font-family-code);margin-left:7px}.rtjY0dWHa_table tbody tr[data-kind=tool] .rtjY0dWHa_contentText,.rtjY0dWHa_table tbody tr[data-kind=subtool] .rtjY0dWHa_contentText,.rtjY0dWHa_table tbody tr[data-kind=tool] .rtjY0dWHa_resultPreview,.rtjY0dWHa_table tbody tr[data-kind=subtool] .rtjY0dWHa_resultPreview{font-family:var(--ds-font-family-code);font-size:12px}.rtjY0dWHa_table tbody tr[data-kind=subtool] .rtjY0dWHa_content{padding-left:26px}.rtjY0dWHa_inlineResult{min-width:0;color:var(--dsw-alias-label-secondary);align-items:center;display:flex}.rtjY0dWHa_noOutputText{color:var(--dsw-alias-label-caption)}.rtjY0dWHa_arrow{color:var(--dsw-alias-label-caption);flex:none;margin-right:8px}.rtjY0dWHa_error,.rtjY0dWHa_overview dd.rtjY0dWHa_error,.rtjY0dWHa_details .rtjY0dWHa_errorPayload{color:var(--dsw-alias-state-error-primary)}.rtjY0dWHa_details .rtjY0dWHa_errorPayload .rtjY0dWHa_resultBlockText{color:inherit}.rtjY0dWHa_details .rtjY0dWHa_jsonPayload.rtjY0dWHa_errorPayload,.rtjY0dWHa_details .rtjY0dWHa_jsonPreview.rtjY0dWHa_errorPayload{--json-tree-property:var(--dsw-alias-state-error-primary);--json-tree-string:var(--dsw-alias-state-error-primary);--json-tree-number:var(--dsw-alias-state-error-primary);--json-tree-keyword:var(--dsw-alias-state-error-primary);--json-tree-punctuation:var(--dsw-alias-state-error-primary);--json-tree-icon:var(--dsw-alias-state-error-primary)}.rtjY0dWHa_details{border-left:.5px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-layer-1);flex-direction:column;flex:none;width:clamp(320px,38%,440px);min-width:0;max-width:calc(100% - 280px);min-height:0;display:flex;position:relative}.rtjY0dWHa_detailsResizeHandle{z-index:6;cursor:col-resize;touch-action:none;user-select:none;background:0 0;border:0;width:8px;padding:0;position:absolute;top:0;bottom:0;left:-4px}.rtjY0dWHa_detailsResizeHandle:focus-visible{outline:none}.rtjY0dWHa_detailsHeader{box-sizing:border-box;border-bottom:.5px solid var(--dsw-alias-border-l2);flex:none;justify-content:space-between;align-items:center;height:42px;padding:0 8px 0 12px;display:flex}.rtjY0dWHa_detailsTitle{min-width:0;color:var(--dsw-alias-label-primary);align-items:center;gap:8px;display:flex}.rtjY0dWHa_requestDetailsDot{corner-shape:round;background:var(--dsw-alias-label-secondary);border-radius:50%;flex:none;width:5px;height:5px}.rtjY0dWHa_requestDetailsName{font:500 12px/16px var(--ds-font-family-code);flex:none}.rtjY0dWHa_detailsLocation{min-width:0;color:var(--dsw-alias-label-tertiary);font:11px/16px var(--ds-font-family-code);text-overflow:ellipsis;white-space:nowrap;overflow:hidden}.rtjY0dWHa_close{border-radius:var(--dsw-radius-sm);width:28px;height:28px;color:var(--dsw-alias-label-secondary);cursor:pointer;background:0 0;border:0;flex:none;justify-content:center;align-items:center;padding:0;font-size:18px;line-height:18px;display:inline-flex}.rtjY0dWHa_close:hover{color:var(--dsw-alias-label-primary);background:var(--dsw-alias-interactive-bg-hover)}.rtjY0dWHa_detailTabs{box-sizing:border-box;border-bottom:.5px solid var(--dsw-alias-border-l2);overscroll-behavior-x:contain;scrollbar-width:none;white-space:nowrap;flex:none;gap:1px;width:100%;min-width:0;max-width:100%;height:34px;padding:0 8px;display:flex;overflow:auto hidden}.rtjY0dWHa_detailTabs::-webkit-scrollbar{display:none}.rtjY0dWHa_detailTab{color:var(--dsw-alias-label-tertiary);cursor:pointer;font:var(--dsw-font-xs-13);background:0 0;border:0;flex:none;padding:0 9px;position:relative}.rtjY0dWHa_detailTab:hover{color:var(--dsw-alias-label-primary);background:var(--dsw-alias-interactive-bg-hover)}.rtjY0dWHa_detailTabActive{color:var(--dsw-alias-state-business-primary)}.rtjY0dWHa_detailTabActive:after{background:var(--dsw-alias-state-business-primary);content:\"\";border-radius:1px 1px 0 0;height:2px;position:absolute;bottom:0;left:9px;right:9px}.rtjY0dWHa_close:focus-visible,.rtjY0dWHa_detailTab:focus-visible{outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:-1px}.rtjY0dWHa_detailBody{min-height:0;padding-bottom:var(--dsh-trajectory-bottom-clearance,0px);scrollbar-gutter:stable;flex:1;overflow:hidden auto}.rtjY0dWHa_detailBodySummary{box-sizing:border-box;padding-bottom:calc(12px + var(--dsh-trajectory-bottom-clearance,0px));flex-direction:column;display:flex;overflow:hidden}.rtjY0dWHa_detailBodySummary>.rtjY0dWHa_overview{overscroll-behavior:contain;flex:0 auto;min-height:0;padding-bottom:0;overflow:auto}.rtjY0dWHa_detailBodySummary>.rtjY0dWHa_compactedSummary{flex:1;min-height:0;overflow:auto}.rtjY0dWHa_summaryScrollRegion{--dsh-scrollbar-thumb:transparent;--dsh-scrollbar-thumb-hover:transparent}.rtjY0dWHa_summaryScrollRegion:hover,.rtjY0dWHa_summaryScrollRegion:focus-within{--dsh-scrollbar-thumb:var(--dsw-alias-scrollbar-bg-l2);--dsh-scrollbar-thumb-hover:var(--dsw-alias-scrollbar-hover-l2)}.rtjY0dWHa_compactedSummary .rtjY0dWHa_markdownPayload{padding-right:18px}.rtjY0dWHa_overview{font:var(--dsw-font-xs-13);margin:0;padding:8px 0}.rtjY0dWHa_overviewParentLinks{gap:14px;display:flex}.rtjY0dWHa_overviewHierarchyNavLink{all:unset;color:var(--dsw-alias-label-secondary);cursor:pointer;font:inherit;opacity:1;align-items:center;gap:1px;display:inline-flex}.rtjY0dWHa_overviewHierarchyNavLink:hover{color:var(--dsw-alias-label-primary)}.rtjY0dWHa_overviewHierarchyJumpIconTight{color:var(--dsw-alias-label-caption);flex:none}.rtjY0dWHa_overviewHierarchyNavLink:hover .rtjY0dWHa_overviewHierarchyJumpIconTight,.rtjY0dWHa_overviewHierarchyNavLink:focus-visible .rtjY0dWHa_overviewHierarchyJumpIconTight{color:var(--dsw-alias-label-primary)}.rtjY0dWHa_overviewHierarchyNavLink:focus-visible{color:var(--dsw-alias-label-primary);outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:2px}.rtjY0dWHa_overview>div{grid-template-columns:94px minmax(0,1fr);align-items:center;min-height:22px;padding:0 14px;display:grid}.rtjY0dWHa_overview>.rtjY0dWHa_requestTokenDetail dt{padding-left:12px}.rtjY0dWHa_usagePanel{padding:4px 0 10px}.rtjY0dWHa_usageGroup+.rtjY0dWHa_usageGroup{margin-top:8px}.rtjY0dWHa_usageHeading{color:var(--dsw-alias-label-secondary);font:var(--dsw-font-xs-strong-13);user-select:none;margin:0;padding:4px 14px 1px}.rtjY0dWHa_usageGroup .rtjY0dWHa_overview{padding:0}.rtjY0dWHa_overview dt{color:var(--dsw-alias-label-tertiary)}.rtjY0dWHa_overview dd{min-width:0;color:var(--dsw-alias-label-primary);text-overflow:ellipsis;white-space:nowrap;margin:0;overflow:hidden}.rtjY0dWHa_timestampToggle{all:unset;color:inherit;cursor:pointer;font:inherit;user-select:text}.rtjY0dWHa_timestampToggle:focus-visible{outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:2px}.rtjY0dWHa_overviewSections{border-top:0;flex-direction:column;flex:1;min-height:min-content;display:flex;overflow:hidden}.rtjY0dWHa_overviewSection{flex-direction:column;flex:1 1 0;min-height:24px;max-height:max-content;display:flex;overflow:hidden}.rtjY0dWHa_overviewSection+.rtjY0dWHa_overviewSection{padding-top:2px}.rtjY0dWHa_overviewHeading{box-sizing:border-box;width:100%;height:24px;color:var(--dsw-alias-label-secondary);background:var(--dsw-alias-bg-layer-1);font:var(--dsw-font-xs-strong-13);user-select:none;flex:none;align-items:flex-end;margin:0;padding:0 0 3px 14px;display:flex}.rtjY0dWHa_overviewTitle{width:max-content;height:auto;color:inherit;cursor:pointer;font:inherit;user-select:none;background:0 0;border:0;flex:none;align-items:center;gap:3px;padding:0;line-height:1;display:inline-flex}.rtjY0dWHa_overviewTitleIcon{color:var(--dsw-alias-label-caption);flex:none}.rtjY0dWHa_overviewTitle:hover .rtjY0dWHa_overviewTitleIcon{color:var(--dsw-alias-label-primary)}.rtjY0dWHa_overviewTitle:focus-visible{outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:2px}.rtjY0dWHa_overviewTitle:focus-visible .rtjY0dWHa_overviewTitleIcon{color:var(--dsw-alias-state-business-primary)}.rtjY0dWHa_overviewPreview{overscroll-behavior:contain;background:var(--dsw-alias-bg-layer-1);flex:1;min-height:0;overflow:auto}.rtjY0dWHa_overviewPreview[data-scroll-more]{mask-image:linear-gradient(#000 calc(100% - 12px),#0000)}.rtjY0dWHa_overviewPreview>:last-child{margin-bottom:0;padding-bottom:0}.rtjY0dWHa_programDescription{color:var(--dsw-alias-label-primary);font:var(--dsw-font-xs-13);white-space:pre-wrap;overflow-wrap:anywhere;flex:none;margin:0;padding:8px 14px 2px}.rtjY0dWHa_programIcon{vertical-align:-2px;margin-right:4px}.rtjY0dWHa_programSummary{font-family:var(--dsw-font-family);margin-left:5px}.rtjY0dWHa_detailBodyProgram,.rtjY0dWHa_programPanel{flex-direction:column;min-height:0;display:flex;overflow:hidden}.rtjY0dWHa_programPanel{background:var(--dsw-alias-bg-layer-1);flex:1}.rtjY0dWHa_programActions{flex:none;align-items:center;gap:4px;height:16px;margin-left:auto;padding-right:14px;display:inline-flex}.rtjY0dWHa_programLanguage{color:var(--dsw-alias-label-tertiary);font:11px/16px var(--ds-font-family-code);padding-right:4px}.rtjY0dWHa_programAction{box-sizing:border-box;width:20px;height:16px;color:var(--dsw-alias-label-secondary);cursor:pointer;background:0 0;border:0;justify-content:center;align-items:center;padding:0;display:inline-flex}.rtjY0dWHa_programAction:hover{color:var(--dsw-alias-label-primary)}.rtjY0dWHa_programAction[aria-pressed=true]{color:var(--dsw-alias-state-business-primary)}.rtjY0dWHa_programAction[data-state=failed]{color:var(--dsw-alias-state-error-primary)}.rtjY0dWHa_programAction:focus-visible{outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:-1px}.rtjY0dWHa_programContent{min-width:0;min-height:0}.rtjY0dWHa_programPanel>.rtjY0dWHa_programContent{overscroll-behavior:contain;flex:1;overflow:auto}.rtjY0dWHa_programSource{background:var(--dsw-alias-bg-layer-1);--dsl-code-block-line-white-space:pre;border-radius:0;margin:0!important}.rtjY0dWHa_programSource :where(pre){font:12px/16px var(--ds-font-family-code);overflow-wrap:normal;word-break:normal;tab-size:2;border-radius:0;padding:2px 14px 8px;overflow:visible;background:var(--dsw-alias-bg-layer-1)!important}.rtjY0dWHa_programSource pre code>.line{box-sizing:border-box;width:max-content;min-width:100%}.rtjY0dWHa_programContent[data-wrap=true] .rtjY0dWHa_programSource{--dsl-code-block-line-white-space:pre-wrap}.rtjY0dWHa_programContent[data-wrap=true] .rtjY0dWHa_programSource pre code>.line{overflow-wrap:anywhere;word-break:break-all;width:auto}.rtjY0dWHa_programOutput{color:var(--dsw-alias-label-primary);font:12px/16px var(--ds-font-family-code);white-space:pre-wrap;overflow-wrap:anywhere;margin:0;padding:2px 14px 8px}.rtjY0dWHa_programError:first-line{color:var(--dsw-alias-state-error-primary);font-weight:600}.rtjY0dWHa_overviewPreview .rtjY0dWHa_jsonPreview>:first-child,.rtjY0dWHa_overviewPreview .rtjY0dWHa_schemaTree>:first-child{padding-top:2px;padding-bottom:0}.rtjY0dWHa_overviewPreview .rtjY0dWHa_schemaTree{margin-bottom:0}.rtjY0dWHa_overviewPreview .rtjY0dWHa_overview{padding:2px 0 0}.rtjY0dWHa_overviewPreview .rtjY0dWHa_overview>div{min-height:22px}.rtjY0dWHa_markdownPreview,.rtjY0dWHa_markdownPayload{color:var(--dsw-alias-label-primary)}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]),.rtjY0dWHa_markdownPayload>div:not([data-markdown-variant=compact]){font:var(--dsw-font-xs-13);gap:8px}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) h1,.rtjY0dWHa_markdownPayload>div:not([data-markdown-variant=compact]) h1{font:600 16px/22px var(--dsw-font-family)}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) h2,.rtjY0dWHa_markdownPayload>div:not([data-markdown-variant=compact]) h2{font:600 15px/22px var(--dsw-font-family)}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) :where(h3,h4,h5,h6),.rtjY0dWHa_markdownPayload>div:not([data-markdown-variant=compact]) :where(h3,h4,h5,h6){font:600 14px/20px var(--dsw-font-family)}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) :not(pre)>code,.rtjY0dWHa_markdownPayload>div:not([data-markdown-variant=compact]) :not(pre)>code{font:12px/18px var(--ds-font-family-code)}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) table,.rtjY0dWHa_markdownPayload>div:not([data-markdown-variant=compact]) table{font:var(--dsw-font-xs-13)}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact])>:first-child{margin-top:0}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact])>:last-child{margin-bottom:0}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) :where(h1,h2,h3){margin:12px 0 6px}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) :where(h4,h5,h6){margin:10px 0 5px}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) :where(p,ul,ol){margin:8px 0}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) li:not(:first-child){margin-top:3px}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) blockquote{margin-top:8px}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact]) hr{margin:14px 0}.rtjY0dWHa_markdownPreview>div:not([data-markdown-variant=compact])>.md-code-block{margin:10px 0}.rtjY0dWHa_assistantContent .rtjY0dWHa_markdownPayload,.rtjY0dWHa_assistantContent .rtjY0dWHa_payload{min-height:0}.rtjY0dWHa_thinkingQuote{--dsh-content-font-size-secondary:13px;--dsh-content-font-delta-secondary:0px;border-left:2px solid var(--dsw-alias-markdown-citation);color:var(--dsw-alias-label-secondary);background:0 0;margin:6px 14px 0 12px;padding-left:6px}.rtjY0dWHa_thinkingQuoteOnlyPreview{margin-bottom:8px}.rtjY0dWHa_detailBody>.rtjY0dWHa_assistantContentRendered .rtjY0dWHa_thinkingQuote{margin-top:14px}.rtjY0dWHa_thinkingToggle{width:max-content;height:18px;color:var(--dsw-alias-label-tertiary);cursor:pointer;font:600 12px/18px var(--dsw-font-family);user-select:none;background:0 0;border:0;align-items:center;gap:2px;padding:0;display:flex}.rtjY0dWHa_thinkingChevron{transition:transform .12s var(--ds-ease-in-out);flex:none}.rtjY0dWHa_thinkingToggle[aria-expanded=true] .rtjY0dWHa_thinkingChevron{transform:rotate(90deg)}.rtjY0dWHa_thinkingToggle:hover{color:var(--dsw-alias-label-secondary)}.rtjY0dWHa_thinkingToggle:focus-visible{outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:2px}.rtjY0dWHa_thinkingQuote .rtjY0dWHa_markdownPreview,.rtjY0dWHa_thinkingQuote .rtjY0dWHa_markdownPayload,.rtjY0dWHa_thinkingQuote .rtjY0dWHa_payload{min-height:0;color:var(--dsw-alias-label-secondary);background:0 0;padding:2px 0}.rtjY0dWHa_assistantOutput{background:var(--dsw-alias-bg-layer-1)}.rtjY0dWHa_markdownPreview{padding:2px 14px 8px}.rtjY0dWHa_markdownPayload{min-height:100%;padding:14px}.rtjY0dWHa_jsonPayload{min-height:100%}.rtjY0dWHa_jsonPreview{min-height:0}.rtjY0dWHa_schema{background:var(--dsw-alias-bg-layer-1);min-height:100%}.rtjY0dWHa_schemaPreview{min-height:0}.rtjY0dWHa_schemaIntro{padding:12px 14px 6px}.rtjY0dWHa_schemaPreview .rtjY0dWHa_schemaIntro{padding-top:2px}.rtjY0dWHa_schemaName{color:var(--dsw-alias-label-primary);font:600 12px/18px var(--ds-font-family-code);margin:0}.rtjY0dWHa_schemaDescription{color:var(--dsw-alias-label-secondary);font:var(--dsw-font-xs-13);white-space:pre-wrap;margin:2px 0 0}.rtjY0dWHa_schemaParameters{min-width:0}.rtjY0dWHa_schemaParametersTitle{color:var(--dsw-alias-label-tertiary);font:600 11px/16px var(--dsw-font-family);user-select:none;margin:0;padding:4px 14px 2px}.rtjY0dWHa_schemaTree{margin:0 6px 8px 0}.rtjY0dWHa_payload{box-sizing:border-box;overflow-wrap:anywhere;min-height:100%;color:var(--dsw-alias-label-primary);background:var(--dsw-alias-markdown-code-block);font:12px/19px var(--ds-font-family-code);tab-size:2;white-space:pre-wrap;margin:0;padding:14px}.rtjY0dWHa_payloadPreview{min-height:0;padding:2px 14px 8px}.rtjY0dWHa_sourceBlocks{box-sizing:border-box;min-height:100%;padding:12px 14px}.rtjY0dWHa_sourceBlock+.rtjY0dWHa_sourceBlock{margin-top:14px}.rtjY0dWHa_sourceBlockHeader,.rtjY0dWHa_sourceBlockJumpTarget{user-select:none;align-items:center;gap:3px;width:max-content;margin-bottom:3px;display:flex}.rtjY0dWHa_sourceBlockJumpTarget{all:unset;cursor:pointer;user-select:none;align-items:center;gap:3px;width:max-content;margin-bottom:3px;display:flex}.rtjY0dWHa_sourceBlockLabel{color:var(--dsw-alias-label-tertiary);font:11px/16px var(--ds-font-family-code)}.rtjY0dWHa_sourceBlockJumpIcon{color:var(--dsw-alias-label-caption);flex:none}.rtjY0dWHa_sourceBlockJumpTarget:hover .rtjY0dWHa_sourceBlockJumpIcon{color:var(--dsw-alias-label-primary)}.rtjY0dWHa_sourceBlockJumpTarget:focus-visible{outline:none}.rtjY0dWHa_sourceBlockJumpTarget:focus-visible .rtjY0dWHa_sourceBlockJumpIcon{color:var(--dsw-alias-state-business-primary)}.rtjY0dWHa_sourceBlockContent{overflow-wrap:anywhere;color:var(--dsw-alias-label-primary);font:12px/19px var(--ds-font-family-code);tab-size:2;white-space:pre-wrap;background:0 0;margin:0}.rtjY0dWHa_messageImages{flex-direction:column;gap:8px;margin:8px 14px 14px;display:flex}.rtjY0dWHa_attachments{flex-direction:column;gap:8px;margin:8px 14px 14px;padding:0;list-style:none;display:flex}.rtjY0dWHa_attachmentsPreview{gap:6px;margin-block:6px 8px}.rtjY0dWHa_attachmentRow{border:.5px solid var(--dsw-alias-border-l2-darkmode-thin);border-radius:var(--dsw-radius-lg);align-items:center;gap:10px;min-width:0;padding:8px;display:flex}.rtjY0dWHa_attachmentIcon{flex:0 0 48px;place-items:center;width:48px;height:48px;display:grid}.rtjY0dWHa_attachmentInfo{flex-direction:column;flex:1;gap:3px;min-width:0;display:flex}.rtjY0dWHa_attachmentName{color:var(--dsw-alias-label-primary);text-overflow:ellipsis;white-space:nowrap;font-size:12px;line-height:18px;overflow:hidden}.rtjY0dWHa_attachmentMetadata{color:var(--dsw-alias-label-tertiary);text-overflow:ellipsis;white-space:nowrap;font-size:11px;line-height:16px;overflow:hidden}.rtjY0dWHa_attachmentDisclosure{border:.5px solid var(--dsw-alias-border-l2-darkmode-thin);border-radius:var(--dsw-radius-md);margin-block:8px}.rtjY0dWHa_attachmentDisclosure>summary{cursor:pointer;padding:8px}.rtjY0dWHa_attachmentDisclosure>summary .rtjY0dWHa_attachmentName{display:block}.rtjY0dWHa_attachmentDisclosure>summary:focus-visible{outline:var(--dsw-focus-ring-width) solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:2px}.rtjY0dWHa_attachmentDisclosure>pre{padding:0 8px 8px}.rtjY0dWHa_assistantToolCalls{box-sizing:border-box;max-width:100%;color:var(--dsw-alias-label-secondary);font:11px/17px var(--ds-font-family-code);margin:2px 14px 12px;padding:0;list-style:none}.rtjY0dWHa_assistantToolCallsPreview{margin-top:2px;margin-bottom:8px}.rtjY0dWHa_assistantToolCalls li{min-width:0}.rtjY0dWHa_assistantToolCallButton{all:unset;box-sizing:border-box;border-radius:var(--dsw-radius-xs);cursor:pointer;align-items:center;width:calc(100% + 4px);min-width:0;height:19px;margin-left:-4px;padding:0 4px;display:flex}.rtjY0dWHa_assistantToolCallButton:hover{background:var(--dsw-alias-interactive-bg-hover)}.rtjY0dWHa_assistantToolCallButton:focus-visible{background:var(--dsw-alias-interactive-bg-hover);outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:-1px}.rtjY0dWHa_assistantToolCallIcon{color:var(--dsw-alias-label-caption);flex:none;margin-right:5px}.rtjY0dWHa_assistantToolCallButton:hover .rtjY0dWHa_assistantToolCallIcon,.rtjY0dWHa_assistantToolCallButton:focus-visible .rtjY0dWHa_assistantToolCallIcon{color:var(--dsw-alias-label-secondary)}.rtjY0dWHa_assistantToolCallText{white-space:nowrap;flex:1;min-width:0;display:flex;overflow:hidden}.rtjY0dWHa_assistantToolCallName{color:var(--dsw-alias-label-secondary);flex:none;margin-right:5px;font-weight:500}.rtjY0dWHa_assistantToolCallArgs{min-width:0;color:var(--dsw-alias-label-tertiary);text-overflow:ellipsis;white-space:nowrap;overflow:hidden}.rtjY0dWHa_systemPrompt{box-sizing:border-box}.rtjY0dWHa_toolCatalog{min-height:100%;padding:8px 0 16px}.rtjY0dWHa_toolCatalogItem{border-bottom:.5px solid var(--dsw-alias-border-l1)}.rtjY0dWHa_toolCatalogSummary{box-sizing:border-box;cursor:pointer;user-select:none;grid-template-columns:12px 12px max-content minmax(0,1fr);align-items:center;gap:5px;min-height:30px;padding:4px 12px;list-style:none;display:grid}.rtjY0dWHa_toolCatalogSummary::-webkit-details-marker{display:none}.rtjY0dWHa_toolCatalogSummary:hover{background:var(--dsw-alias-interactive-bg-hover)}.rtjY0dWHa_toolCatalogSummary:focus-visible{background:var(--dsw-alias-interactive-bg-hover);outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:-1px}.rtjY0dWHa_toolCatalogChevron,.rtjY0dWHa_toolCatalogIcon{color:var(--dsw-alias-label-caption)}.rtjY0dWHa_toolCatalogChevron{transition:transform .1s var(--ds-ease-in-out)}.rtjY0dWHa_toolCatalogItem[open] .rtjY0dWHa_toolCatalogChevron{transform:rotate(90deg)}.rtjY0dWHa_toolCatalogName{color:var(--dsw-alias-label-primary);font:500 12px/18px var(--ds-font-family-code)}.rtjY0dWHa_toolCatalogDescription{min-width:0;color:var(--dsw-alias-label-tertiary);font:var(--dsw-font-xs-13);text-overflow:ellipsis;white-space:nowrap;overflow:hidden}.rtjY0dWHa_toolCatalogDefinition{background:var(--dsw-alias-bg-base);padding:0 0 8px 29px}.rtjY0dWHa_toolCatalogFullDescription{color:var(--dsw-alias-label-secondary);font:var(--dsw-font-xs-13);white-space:pre-wrap;margin:0;padding:7px 14px 4px 0}.rtjY0dWHa_toolCatalogTree{margin-left:-14px;margin-right:6px}.rtjY0dWHa_resultBlocks{box-sizing:border-box;flex-direction:column;gap:10px;min-height:100%;padding:14px;display:flex}.rtjY0dWHa_resultBlocksPreview{gap:6px;min-height:0;padding:2px 14px 8px}.rtjY0dWHa_resultBlockText{overflow-wrap:anywhere;color:var(--dsw-alias-label-primary);font:12px/19px var(--ds-font-family-code);tab-size:2;white-space:pre-wrap;background:0 0;margin:0}.rtjY0dWHa_overviewPreview .rtjY0dWHa_noPayload{padding:2px 14px 0}.rtjY0dWHa_noPayload{color:var(--dsw-alias-label-tertiary);font:var(--dsw-font-xs-13);margin:0;padding:18px 14px}@media (width<=760px){.rtjY0dWHa_details{z-index:5;border-left-color:var(--dsw-alias-border-l3);width:min(92%,420px);max-width:92%;position:absolute;top:0;bottom:0;right:0;box-shadow:-12px 0 32px #00000024}}.rtjY0dWHa_toolUpdatePayload{white-space:pre;overflow-x:auto}";
		const tagId$3 = "@rth/dsh-rth-trajectory/TrajectoryTable.module.css";
		if (typeof document !== "undefined" && document.querySelector("style[data-plugin-css=" + JSON.stringify(tagId$3) + "]") === null) {
			const tag = document.createElement("style");
			tag.dataset.plugin = "@rth/dsh-rth-trajectory";
			tag.dataset.pluginCss = tagId$3;
			tag.textContent = css$3;
			document.head.appendChild(tag);
		}
		var TrajectoryTable_module_css_default = {
			"arrow": "rtjY0dWHa_arrow",
			"assistantContent": "rtjY0dWHa_assistantContent",
			"assistantContentRendered": "rtjY0dWHa_assistantContentRendered",
			"assistantOutput": "rtjY0dWHa_assistantOutput",
			"assistantToolCallArgs": "rtjY0dWHa_assistantToolCallArgs",
			"assistantToolCallButton": "rtjY0dWHa_assistantToolCallButton",
			"assistantToolCallIcon": "rtjY0dWHa_assistantToolCallIcon",
			"assistantToolCallName": "rtjY0dWHa_assistantToolCallName",
			"assistantToolCallText": "rtjY0dWHa_assistantToolCallText",
			"assistantToolCalls": "rtjY0dWHa_assistantToolCalls",
			"assistantToolCallsPreview": "rtjY0dWHa_assistantToolCallsPreview",
			"assistantVioletBright": "rtjY0dWHa_assistantVioletBright",
			"attachmentDisclosure": "rtjY0dWHa_attachmentDisclosure",
			"attachmentIcon": "rtjY0dWHa_attachmentIcon",
			"attachmentInfo": "rtjY0dWHa_attachmentInfo",
			"attachmentMetadata": "rtjY0dWHa_attachmentMetadata",
			"attachmentName": "rtjY0dWHa_attachmentName",
			"attachmentRow": "rtjY0dWHa_attachmentRow",
			"attachments": "rtjY0dWHa_attachments",
			"attachmentsPreview": "rtjY0dWHa_attachmentsPreview",
			"close": "rtjY0dWHa_close",
			"collapsedTurnContent": "rtjY0dWHa_collapsedTurnContent",
			"collapsedTurnEllipsis": "rtjY0dWHa_collapsedTurnEllipsis",
			"collapsedTurnText": "rtjY0dWHa_collapsedTurnText",
			"compacted": "rtjY0dWHa_compacted",
			"compactedSummary": "rtjY0dWHa_compactedSummary",
			"content": "rtjY0dWHa_content",
			"contentColumn": "rtjY0dWHa_contentColumn",
			"contentText": "rtjY0dWHa_contentText",
			"contextGreen": "rtjY0dWHa_contextGreen",
			"detailBody": "rtjY0dWHa_detailBody",
			"detailBodyProgram": "rtjY0dWHa_detailBodyProgram",
			"detailBodySummary": "rtjY0dWHa_detailBodySummary",
			"detailTab": "rtjY0dWHa_detailTab",
			"detailTabActive": "rtjY0dWHa_detailTabActive",
			"detailTabs": "rtjY0dWHa_detailTabs",
			"details": "rtjY0dWHa_details",
			"detailsHeader": "rtjY0dWHa_detailsHeader",
			"detailsLocation": "rtjY0dWHa_detailsLocation",
			"detailsResizeHandle": "rtjY0dWHa_detailsResizeHandle",
			"detailsTitle": "rtjY0dWHa_detailsTitle",
			"error": "rtjY0dWHa_error",
			"errorPayload": "rtjY0dWHa_errorPayload",
			"event": "rtjY0dWHa_event",
			"eventColumn": "rtjY0dWHa_eventColumn",
			"eventHeader": "rtjY0dWHa_eventHeader",
			"eventInner": "rtjY0dWHa_eventInner",
			"historyLoadButton": "rtjY0dWHa_historyLoadButton",
			"historyLoadRow": "rtjY0dWHa_historyLoadRow",
			"historyLoading": "rtjY0dWHa_historyLoading",
			"historyLoadingBar": "rtjY0dWHa_historyLoadingBar",
			"inlineResult": "rtjY0dWHa_inlineResult",
			"inlineResultText": "rtjY0dWHa_inlineResultText",
			"jsonPayload": "rtjY0dWHa_jsonPayload",
			"jsonPreview": "rtjY0dWHa_jsonPreview",
			"kindSlot": "rtjY0dWHa_kindSlot",
			"kindTag": "rtjY0dWHa_kindTag",
			"kindTagIcon": "rtjY0dWHa_kindTagIcon",
			"kindTagLabel": "rtjY0dWHa_kindTagLabel",
			"markdownPayload": "rtjY0dWHa_markdownPayload",
			"markdownPreview": "rtjY0dWHa_markdownPreview",
			"message": "rtjY0dWHa_message",
			"messageImages": "rtjY0dWHa_messageImages",
			"noOutputText": "rtjY0dWHa_noOutputText",
			"noPayload": "rtjY0dWHa_noPayload",
			"overview": "rtjY0dWHa_overview",
			"overviewHeading": "rtjY0dWHa_overviewHeading",
			"overviewHierarchyJumpIconTight": "rtjY0dWHa_overviewHierarchyJumpIconTight",
			"overviewHierarchyNavLink": "rtjY0dWHa_overviewHierarchyNavLink",
			"overviewParentLinks": "rtjY0dWHa_overviewParentLinks",
			"overviewPreview": "rtjY0dWHa_overviewPreview",
			"overviewSection": "rtjY0dWHa_overviewSection",
			"overviewSections": "rtjY0dWHa_overviewSections",
			"overviewTitle": "rtjY0dWHa_overviewTitle",
			"overviewTitleIcon": "rtjY0dWHa_overviewTitleIcon",
			"payload": "rtjY0dWHa_payload",
			"payloadPreview": "rtjY0dWHa_payloadPreview",
			"programAction": "rtjY0dWHa_programAction",
			"programActions": "rtjY0dWHa_programActions",
			"programContent": "rtjY0dWHa_programContent",
			"programDescription": "rtjY0dWHa_programDescription",
			"programError": "rtjY0dWHa_programError",
			"programIcon": "rtjY0dWHa_programIcon",
			"programLanguage": "rtjY0dWHa_programLanguage",
			"programOutput": "rtjY0dWHa_programOutput",
			"programPanel": "rtjY0dWHa_programPanel",
			"programSource": "rtjY0dWHa_programSource",
			"programSummary": "rtjY0dWHa_programSummary",
			"promptDiff": "rtjY0dWHa_promptDiff",
			"promptDiffLineadded": "rtjY0dWHa_promptDiffLineadded",
			"promptDiffLinecontext": "rtjY0dWHa_promptDiffLinecontext",
			"promptDiffLinemeta": "rtjY0dWHa_promptDiffLinemeta",
			"promptDiffLineremoved": "rtjY0dWHa_promptDiffLineremoved",
			"promptDiffSection": "rtjY0dWHa_promptDiffSection",
			"promptDiffSections": "rtjY0dWHa_promptDiffSections",
			"promptDiffTitle": "rtjY0dWHa_promptDiffTitle",
			"requestBoundaryControl": "rtjY0dWHa_requestBoundaryControl",
			"requestBoundaryControlActive": "rtjY0dWHa_requestBoundaryControlActive",
			"requestDetailsDot": "rtjY0dWHa_requestDetailsDot",
			"requestDetailsName": "rtjY0dWHa_requestDetailsName",
			"requestTokenDetail": "rtjY0dWHa_requestTokenDetail",
			"resultBlockText": "rtjY0dWHa_resultBlockText",
			"resultBlocks": "rtjY0dWHa_resultBlocks",
			"resultBlocksPreview": "rtjY0dWHa_resultBlocksPreview",
			"resultPreview": "rtjY0dWHa_resultPreview",
			"resultRequest": "rtjY0dWHa_resultRequest",
			"schema": "rtjY0dWHa_schema",
			"schemaDescription": "rtjY0dWHa_schemaDescription",
			"schemaIntro": "rtjY0dWHa_schemaIntro",
			"schemaName": "rtjY0dWHa_schemaName",
			"schemaParameters": "rtjY0dWHa_schemaParameters",
			"schemaParametersTitle": "rtjY0dWHa_schemaParametersTitle",
			"schemaPreview": "rtjY0dWHa_schemaPreview",
			"schemaTree": "rtjY0dWHa_schemaTree",
			"selectionRail": "rtjY0dWHa_selectionRail",
			"sourceBlock": "rtjY0dWHa_sourceBlock",
			"sourceBlockContent": "rtjY0dWHa_sourceBlockContent",
			"sourceBlockHeader": "rtjY0dWHa_sourceBlockHeader",
			"sourceBlockJumpIcon": "rtjY0dWHa_sourceBlockJumpIcon",
			"sourceBlockJumpTarget": "rtjY0dWHa_sourceBlockJumpTarget",
			"sourceBlockLabel": "rtjY0dWHa_sourceBlockLabel",
			"sourceBlocks": "rtjY0dWHa_sourceBlocks",
			"split": "rtjY0dWHa_split",
			"subtoolAmber": "rtjY0dWHa_subtoolAmber",
			"summaryScrollRegion": "rtjY0dWHa_summaryScrollRegion",
			"systemNeutral": "rtjY0dWHa_systemNeutral",
			"systemPrompt": "rtjY0dWHa_systemPrompt",
			"table": "rtjY0dWHa_table",
			"tablePane": "rtjY0dWHa_tablePane",
			"thinkingChevron": "rtjY0dWHa_thinkingChevron",
			"thinkingQuote": "rtjY0dWHa_thinkingQuote",
			"thinkingQuoteOnlyPreview": "rtjY0dWHa_thinkingQuoteOnlyPreview",
			"thinkingToggle": "rtjY0dWHa_thinkingToggle",
			"timestampToggle": "rtjY0dWHa_timestampToggle",
			"toolAmber": "rtjY0dWHa_toolAmber",
			"toolCallNameTypeface": "rtjY0dWHa_toolCallNameTypeface",
			"toolCallOnly": "rtjY0dWHa_toolCallOnly",
			"toolCallPayload": "rtjY0dWHa_toolCallPayload",
			"toolCatalog": "rtjY0dWHa_toolCatalog",
			"toolCatalogChevron": "rtjY0dWHa_toolCatalogChevron",
			"toolCatalogDefinition": "rtjY0dWHa_toolCatalogDefinition",
			"toolCatalogDescription": "rtjY0dWHa_toolCatalogDescription",
			"toolCatalogFullDescription": "rtjY0dWHa_toolCatalogFullDescription",
			"toolCatalogIcon": "rtjY0dWHa_toolCatalogIcon",
			"toolCatalogItem": "rtjY0dWHa_toolCatalogItem",
			"toolCatalogName": "rtjY0dWHa_toolCatalogName",
			"toolCatalogSummary": "rtjY0dWHa_toolCatalogSummary",
			"toolCatalogTree": "rtjY0dWHa_toolCatalogTree",
			"toolUpdatePayload": "rtjY0dWHa_toolUpdatePayload",
			"trajectory-table": "rtjY0dWHa_trajectory-table",
			"turnLabel": "rtjY0dWHa_turnLabel",
			"turnLabelActive": "rtjY0dWHa_turnLabelActive",
			"turnLabelCompact": "rtjY0dWHa_turnLabelCompact",
			"turnLabelFull": "rtjY0dWHa_turnLabelFull",
			"turnRail": "rtjY0dWHa_turnRail",
			"usageGroup": "rtjY0dWHa_usageGroup",
			"usageHeading": "rtjY0dWHa_usageHeading",
			"usagePanel": "rtjY0dWHa_usagePanel",
			"user": "rtjY0dWHa_user",
			"virtualSpacer": "rtjY0dWHa_virtualSpacer",
			"visuallyHidden": "rtjY0dWHa_visuallyHidden"
		};
		//#endregion
		//#region lib/types/client/TrajectoryTable.js
		/** Turn-aware trajectory event ledger with a local record inspector. */
		const BOTTOM_FOLLOW_THRESHOLD_PX = 2;
		const OLDER_LOAD_THRESHOLD_PX = 48;
		const HISTORY_LOAD_ROW_HEIGHT_PX = 30;
		const VIRTUALIZATION_THRESHOLD = 100;
		const VIRTUAL_OVERSCAN_ROWS = 12;
		const VIRTUAL_INITIAL_VIEWPORT_HEIGHT_PX = 600;
		const KIND_LABEL_KEY = {
			system: "kind.system",
			user: "kind.user",
			context: "kind.context",
			compacted: "kind.compacted",
			message: "kind.assistant",
			tool: "kind.tool",
			subtool: "kind.subtool"
		};
		function ToolWrenchIcon() {
			return (0, react_jsx_runtime.jsx)("svg", {
				width: "13",
				height: "13",
				viewBox: "0 0 16 16",
				fill: "none",
				stroke: "currentColor",
				strokeWidth: "1.5",
				strokeLinecap: "round",
				strokeLinejoin: "round",
				"data-role-icon": "wrench",
				"aria-hidden": "true",
				children: (0, react_jsx_runtime.jsx)("path", { d: "M14 3.3a3.8 3.8 0 0 1-4.8 4.8l-5.1 5.1a1.6 1.6 0 1 1-2.3-2.3l5.1-5.1A3.8 3.8 0 0 1 11.7 1l-2.3 2.3 2.3 2.3L14 3.3Z" })
			});
		}
		function InformationIcon() {
			return (0, react_jsx_runtime.jsxs)("svg", {
				width: "14",
				height: "14",
				viewBox: "0 0 16 16",
				fill: "none",
				stroke: "currentColor",
				strokeWidth: "1.4",
				strokeLinecap: "round",
				"data-role-icon": "information",
				"aria-hidden": "true",
				children: [
					(0, react_jsx_runtime.jsx)("circle", {
						cx: "8",
						cy: "8",
						r: "6.7"
					}),
					(0, react_jsx_runtime.jsx)("circle", {
						cx: "8",
						cy: "5.5",
						r: ".85",
						fill: "currentColor",
						stroke: "none"
					}),
					(0, react_jsx_runtime.jsx)("path", {
						d: "M8 7.75v3.4",
						strokeWidth: "1.8"
					})
				]
			});
		}
		function CompactedIcon() {
			return (0, react_jsx_runtime.jsxs)("svg", {
				width: "13",
				height: "13",
				viewBox: "0 0 16 16",
				fill: "none",
				stroke: "currentColor",
				strokeWidth: "1.5",
				strokeLinecap: "round",
				strokeLinejoin: "round",
				"data-role-icon": "compacted",
				"aria-hidden": "true",
				children: [
					(0, react_jsx_runtime.jsx)("path", { d: "m2.5 2.5 3.75 3.75M3 6.25h3.25V3" }),
					(0, react_jsx_runtime.jsx)("path", { d: "m13.5 2.5-3.75 3.75M13 6.25H9.75V3" }),
					(0, react_jsx_runtime.jsx)("path", { d: "m2.5 13.5 3.75-3.75M3 9.75h3.25V13" }),
					(0, react_jsx_runtime.jsx)("path", { d: "m13.5 13.5-3.75-3.75M13 9.75H9.75V13" })
				]
			});
		}
		const KIND_ICON = {
			system: (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconSettingsOutlineRegular, { size: 13 }),
			user: (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconUserOutlineRegular, { size: 13 }),
			context: (0, react_jsx_runtime.jsx)(InformationIcon, {}),
			compacted: (0, react_jsx_runtime.jsx)(CompactedIcon, {}),
			message: (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconSparkleRegular, { size: 13 }),
			tool: (0, react_jsx_runtime.jsx)(ToolWrenchIcon, {}),
			subtool: (0, react_jsx_runtime.jsx)(ToolWrenchIcon, {})
		};
		function useStableVirtualRowStructure(rows) {
			const cache = (0, react.useRef)({
				rows: [],
				structure: []
			});
			if (cache.current.rows === rows) return cache.current.structure;
			const structure = cache.current.structure.length === rows.length && rows.every((row, index) => {
				const previous = cache.current.structure[index];
				return previous?.key === row.key && previous.height === row.height;
			}) ? cache.current.structure : rows.map((row) => ({
				key: row.key,
				height: row.height
			}));
			cache.current = {
				rows,
				structure
			};
			return structure;
		}
		const DETAILS_MIN_WIDTH = 320;
		const DETAILS_MAX_WIDTH = 720;
		const TABLE_MIN_WIDTH = 280;
		const DETAILS_RESIZE_STEP = 16;
		const TOOL_REQUEST_SHARE = .58;
		const TOOL_REQUEST_MIN_WIDTH = 180;
		const TOOL_REQUEST_MAX_WIDTH = 480;
		const DEFAULT_TOOL_REQUEST_SHARE = .36;
		const DEFAULT_TOOL_REQUEST_OFFSET = 56;
		const SYSTEM_PROMPT_TABS = [{
			id: "system-prompt",
			labelKey: "tab.systemPrompt"
		}, {
			id: "tools",
			labelKey: "tab.tools"
		}];
		const SYSTEM_UPDATE_TABS = [{
			id: "diff",
			labelKey: "tab.diff"
		}, ...SYSTEM_PROMPT_TABS];
		const REQUEST_TABS = [
			{
				id: "overview",
				labelKey: "tab.summary"
			},
			{
				id: "options",
				labelKey: "tab.options"
			},
			{
				id: "usage",
				labelKey: "tab.usage"
			},
			{
				id: "timing",
				labelKey: "tab.timing"
			}
		];
		function jsonTreeLabels(t) {
			return {
				copyValue: t("copy.value"),
				copyJson: t("copy.json"),
				copyPath: t("copy.path"),
				copyPrettyJson: t("copy.prettyJson"),
				copyCompactJson: t("copy.compactJson"),
				copied: t("copied"),
				copyFailed: t("copy.failed"),
				collapseNode: t("collapse"),
				expandNode: t("expand"),
				copyButtonTitle: (action) => t("copy.optionsHint", { action })
			};
		}
		function markdownLabels(t) {
			return {
				code: {
					copyLabel: t("copy"),
					copiedLabel: t("copied")
				},
				footnotes: t("markdown.footnotes")
			};
		}
		function clampDetailsWidth(width, splitWidth) {
			const maxWidth = Math.max(DETAILS_MIN_WIDTH, Math.min(DETAILS_MAX_WIDTH, splitWidth - TABLE_MIN_WIDTH));
			return Math.round(Math.min(Math.max(width, DETAILS_MIN_WIDTH), maxWidth));
		}
		function defaultToolRequestWidth(splitWidth) {
			return Math.min(Math.max(splitWidth * DEFAULT_TOOL_REQUEST_SHARE - DEFAULT_TOOL_REQUEST_OFFSET, TOOL_REQUEST_MIN_WIDTH), TOOL_REQUEST_MAX_WIDTH);
		}
		function formatDurationMs(milliseconds, t) {
			if (milliseconds < 1e3) return t("unit.milliseconds", { value: Math.round(milliseconds) });
			return t("unit.seconds", { value: (milliseconds / 1e3).toFixed(milliseconds < 1e4 ? 2 : 1) });
		}
		function formatStartedAt(timestamp, t) {
			if (timestamp === null || !Number.isFinite(timestamp)) return t("timing.notAvailable");
			const date = new Date(timestamp);
			const two = (value) => String(value).padStart(2, "0");
			const three = (value) => String(value).padStart(3, "0");
			const time = `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}.${three(date.getMilliseconds())}`;
			return `${`${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())}`} ${time}`;
		}
		/** Whether a click lands on an active text selection and should keep it. */
		function clickSelectsText(target) {
			const selection = window.getSelection();
			return selection !== null && !selection.isCollapsed && selection.rangeCount > 0 && selection.getRangeAt(0).intersectsNode(target);
		}
		function StartedAtValue({ timestamp, t }) {
			const [showUnix, setShowUnix] = (0, react.useState)(false);
			if (timestamp === null || !Number.isFinite(timestamp)) return (0, react_jsx_runtime.jsx)("dd", { children: t("timing.notAvailable") });
			return (0, react_jsx_runtime.jsx)("dd", { children: (0, react_jsx_runtime.jsx)("button", {
				type: "button",
				className: TrajectoryTable_module_css_default.timestampToggle,
				title: showUnix ? t("timing.showLocalTime") : t("timing.showUnixTimestamp"),
				onClick: (event) => {
					if (clickSelectsText(event.currentTarget)) return;
					setShowUnix((current) => !current);
				},
				children: showUnix ? (timestamp / 1e3).toFixed(3) : formatStartedAt(timestamp, t)
			}) });
		}
		function totalTime(metrics, t) {
			if (!metrics.timingRecorded) return t("timing.notRecorded");
			if (metrics.stepStartTime === null) return t("timing.stepStartUnavailable");
			if (metrics.completedTime === null) return t("status.pending");
			return formatDurationMs(Math.max(0, metrics.completedTime - metrics.stepStartTime), t);
		}
		function ttft(metrics, t) {
			if (!metrics.timingRecorded) return t("timing.notRecorded");
			if (metrics.stepStartTime === null) return t("timing.stepStartUnavailable");
			if (metrics.firstTokenTime === null) return t("timing.firstTokenUnavailable");
			return formatDurationMs(Math.max(0, metrics.firstTokenTime - metrics.stepStartTime), t);
		}
		function generationTime(metrics, t) {
			if (!metrics.timingRecorded || metrics.firstTokenTime === null) return t("timing.firstTokenUnavailable");
			if (metrics.completedTime === null) return t("status.pending");
			return formatDurationMs(Math.max(0, metrics.completedTime - metrics.firstTokenTime), t);
		}
		function throughput(metrics, t) {
			if (!metrics.usageProvided) return t("timing.usageUnavailable");
			if (metrics.outputTokens === null) return t("timing.outputTokensUnavailable");
			if (!metrics.timingRecorded || metrics.firstTokenTime === null) return t("timing.firstTokenUnavailable");
			if (metrics.completedTime === null) return t("status.pending");
			const generationSeconds = (metrics.completedTime - metrics.firstTokenTime) / 1e3;
			if (generationSeconds <= 0) return t("timing.durationTooShort");
			return t("unit.tokensPerSecond", { value: (metrics.outputTokens / generationSeconds).toFixed(1) });
		}
		function AssistantTimingPanel({ metrics, t }) {
			return (0, react_jsx_runtime.jsxs)("dl", {
				className: TrajectoryTable_module_css_default.overview,
				children: [
					(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.started") }), (0, react_jsx_runtime.jsx)(StartedAtValue, {
						timestamp: metrics.stepStartTime,
						t
					})] }),
					(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.totalDuration") }), (0, react_jsx_runtime.jsx)("dd", { children: totalTime(metrics, t) })] }),
					(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.ttft") }), (0, react_jsx_runtime.jsx)("dd", { children: ttft(metrics, t) })] }),
					(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.generation") }), (0, react_jsx_runtime.jsx)("dd", { children: generationTime(metrics, t) })] }),
					(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.throughput") }), (0, react_jsx_runtime.jsx)("dd", { children: throughput(metrics, t) })] })
				]
			});
		}
		function flattenRecords(turns) {
			return turns.flatMap((turn, section) => {
				let firstInSection = true;
				const records = turn.groups.flatMap((group) => {
					return group.cells.map((cell, index) => {
						const turnStart = firstInSection && cell.requestOnly !== true && cell.kind !== "system" && (cell.kind !== "compacted" || turn.turn === null);
						if (turnStart) firstInSection = false;
						return {
							turn: turn.turn,
							section,
							group: group.title,
							groupStart: index === 0,
							turnStart,
							cell,
							turnEnd: false
						};
					});
				});
				const last = records.at(-1);
				if (last !== void 0) last.turnEnd = true;
				return records;
			});
		}
		function filterRecords(records, matches) {
			const filtered = records.filter((record) => record.cell.requestOnly !== true && matches.has(record.cell.index)).map((record) => ({
				...record,
				groupStart: false,
				turnStart: false,
				turnEnd: false
			}));
			const startedSections = /* @__PURE__ */ new Set();
			for (const [index, record] of filtered.entries()) {
				const previous = filtered[index - 1];
				const next = filtered[index + 1];
				record.groupStart = previous === void 0 || previous.section !== record.section || previous.group !== record.group;
				record.turnStart = !startedSections.has(record.section) && record.cell.kind !== "system" && (record.cell.kind !== "compacted" || record.turn === null);
				if (record.turnStart) startedSections.add(record.section);
				record.turnEnd = next === void 0 || next.section !== record.section;
			}
			return filtered;
		}
		function requestKey(turn, group) {
			return `${turn}\u0000${group}`;
		}
		function requestIdentity(request) {
			return request.purpose === "compaction" ? `compaction\u0000${request.seq}` : `assistant\u0000${request.turn}\u0000${request.step}`;
		}
		function indexRequestBoundaries(records, requestGroups) {
			const boundaries = /* @__PURE__ */ new Map();
			for (const record of records) {
				const key = requestKey(record.turn, record.group);
				if (!requestGroups.has(key)) continue;
				if (boundaries.has(key)) continue;
				if (record.cell.kind === "user" || record.cell.kind === "context") continue;
				boundaries.set(key, record.cell.index);
			}
			return boundaries;
		}
		function sectionLabel(turn, t) {
			return turn === null ? RTH.BETWEEN : RTH.cardLabel(turn) ?? t("turn.label", { turn });
		}
		function indexRequestNumbers(sessionNumbers) {
			const numbers = /* @__PURE__ */ new Map();
			for (const request of sessionNumbers ?? []) numbers.set(requestKey(request.turn, request.group), request.number);
			return numbers;
		}
		function indexRequestBoundaryRuns(records, requestGroups) {
			const indexes = /* @__PURE__ */ new Map();
			let runLength = 0;
			for (const record of records) {
				if (record.cell.requestOnly === true) {
					indexes.set(record.cell.index, runLength++);
					continue;
				}
				if (runLength > 0 && record.groupStart && requestGroups.has(requestKey(record.turn, record.group))) indexes.set(record.cell.index, runLength);
				runLength = 0;
			}
			return indexes;
		}
		function summarizeTurn(records, requestGroups, t) {
			const steps = new Set(records.map((record) => requestKey(record.turn, record.group)).filter((key) => requestGroups.has(key))).size;
			const toolCalls = records.filter((record) => record.cell.kind === "tool" || record.cell.kind === "subtool").length;
			return [t(steps === 1 ? "summary.steps.one" : "summary.steps.other", { count: steps }), t(toolCalls === 1 ? "summary.toolCalls.one" : "summary.toolCalls.other", { count: toolCalls })].join(" · ");
		}
		function collapseTurnRecords(records, collapsedTurns, requestGroups, t) {
			const recordsByTurn = /* @__PURE__ */ new Map();
			for (const record of records) {
				if (record.turn === null) continue;
				const turnRecords = recordsByTurn.get(record.turn) ?? [];
				turnRecords.push(record);
				recordsByTurn.set(record.turn, turnRecords);
			}
			return records.flatMap((record) => {
				if (record.turn === null || !collapsedTurns.has(record.turn)) return [record];
				const turnRecords = recordsByTurn.get(record.turn) ?? [record];
				if (record.cell.requestOnly === true || record.cell.kind === "system") return [record];
				const contentRecords = turnRecords.filter((candidate) => candidate.cell.requestOnly !== true && candidate.cell.kind !== "system");
				if (contentRecords.length <= 1) return [record];
				if (record.cell.index !== contentRecords[0]?.cell.index) return [];
				return [{
					...record,
					turnEnd: false
				}, {
					...record,
					groupStart: false,
					turnStart: false,
					turnEnd: true,
					collapsedSummary: summarizeTurn(contentRecords.slice(1), requestGroups, t),
					collapsedSummaryKind: "turn"
				}];
			});
		}
		function assistantToolCalls(records, assistantIndex) {
			const at = records.findIndex((record) => record.cell.index === assistantIndex);
			if (at === -1 || records[at]?.cell.kind !== "message") return [];
			const calls = [];
			for (let i = at + 1; i < records.length; i++) {
				const record = records[i];
				if (record === void 0) break;
				if (record.cell.kind !== "tool" && record.cell.kind !== "subtool") break;
				calls.push(record);
			}
			return calls;
		}
		function summarizeAssistantTools(records, t) {
			const names = [...new Set(records.map((record) => {
				const separator = record.cell.text.indexOf(" · ");
				return separator === -1 ? record.cell.text : record.cell.text.slice(0, separator);
			}).filter((name) => name !== ""))];
			const count = records.length;
			const summary = t(count === 1 ? "summary.toolCalls.one" : "summary.toolCalls.other", { count });
			return names.length > 0 ? `${summary} · ${names.join(", ")}` : summary;
		}
		function collapseAssistantRecords(records, collapsedAssistants, t) {
			const out = [];
			for (let i = 0; i < records.length; i++) {
				const record = records[i];
				if (record === void 0) continue;
				out.push(record);
				if (record.cell.kind !== "message" || !collapsedAssistants.has(trajectoryRecordId(record.cell))) continue;
				const calls = [];
				for (let j = i + 1; j < records.length; j++) {
					const candidate = records[j];
					if (candidate === void 0 || candidate.collapsedSummary !== void 0 || candidate.cell.kind !== "tool" && candidate.cell.kind !== "subtool") break;
					calls.push(candidate);
				}
				if (calls.length === 0) continue;
				const last = calls.at(-1);
				out[out.length - 1] = {
					...record,
					turnEnd: false
				};
				out.push({
					...record,
					groupStart: false,
					turnStart: false,
					turnEnd: last?.turnEnd ?? false,
					collapsedSummary: summarizeAssistantTools(calls, t),
					collapsedSummaryKind: "assistant"
				});
				i += calls.length;
			}
			return out;
		}
		function stateOf(record) {
			if (record.cell.rthKind !== void 0) return RTH.stateOf(record.cell);
			if (record.cell.isError) return "error";
			if (record.cell.kind === "compacted" && record.cell.timeSeconds === null) return "running";
			if (record.cell.assistantMetrics?.completedTime === null) return "running";
			if ((record.cell.kind === "tool" || record.cell.kind === "subtool") && record.cell.outputDetail === void 0) return "running";
			return "complete";
		}
		function statusLabel(state, t) {
			if (typeof state === "string" && state.startsWith("rth-")) return t(`rth.status.${state.slice(4)}`);
			if (state === "error") return t("status.failed");
			if (state === "running") return t("status.pending");
			return t("status.completed");
		}
		function requestErrorMessage(request, t) {
			if (request.errorCode === "AUTH") return t("details.failure.auth");
			if (request.error === "trajectory.compaction-interrupted") return t("layout.compactionInterrupted");
			return request.error;
		}
		function TokenRows({ cell, t }) {
			const content = cell.output !== void 0 && cell.think !== void 0 ? Math.max(0, cell.output - cell.think) : void 0;
			return (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [
				(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.tokens") }), (0, react_jsx_runtime.jsx)("dd", { children: cell.output === void 0 ? "—" : t("unit.tokens", { value: cell.output }) })] }),
				cell.think !== void 0 && (0, react_jsx_runtime.jsxs)("div", {
					className: TrajectoryTable_module_css_default.requestTokenDetail,
					children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.reasoning") }), (0, react_jsx_runtime.jsx)("dd", { children: t("unit.tokens", { value: cell.think }) })]
				}),
				content !== void 0 && (0, react_jsx_runtime.jsxs)("div", {
					className: TrajectoryTable_module_css_default.requestTokenDetail,
					children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.content") }), (0, react_jsx_runtime.jsx)("dd", { children: t("unit.tokens", { value: content }) })]
				})
			] });
		}
		function inputTotal(usage) {
			if (usage.input === void 0 && usage.cacheRead === void 0 && usage.cacheWrite === void 0) return void 0;
			return (usage.input ?? 0) + (usage.cacheRead ?? 0) + (usage.cacheWrite ?? 0);
		}
		function UsageRows({ usage, t }) {
			if (usage === void 0) return (0, react_jsx_runtime.jsx)("p", {
				className: TrajectoryTable_module_css_default.noPayload,
				children: t("usage.notReported")
			});
			const totalInput = inputTotal(usage);
			const otherOutput = usage.output !== void 0 && usage.reasoning !== void 0 ? usage.output - usage.reasoning : void 0;
			return (0, react_jsx_runtime.jsxs)("dl", {
				className: TrajectoryTable_module_css_default.overview,
				children: [
					totalInput !== void 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.input") }), (0, react_jsx_runtime.jsx)("dd", { children: t("unit.tokens", { value: totalInput }) })] }),
					usage.cacheRead !== void 0 && (0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryTable_module_css_default.requestTokenDetail,
						children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.cached") }), (0, react_jsx_runtime.jsx)("dd", { children: t("unit.tokens", { value: usage.cacheRead }) })]
					}),
					usage.cacheWrite !== void 0 && (0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryTable_module_css_default.requestTokenDetail,
						children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.cacheCreated") }), (0, react_jsx_runtime.jsx)("dd", { children: t("unit.tokens", { value: usage.cacheWrite }) })]
					}),
					usage.input !== void 0 && (0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryTable_module_css_default.requestTokenDetail,
						children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.other") }), (0, react_jsx_runtime.jsx)("dd", { children: t("unit.tokens", { value: usage.input }) })]
					}),
					usage.output !== void 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.output") }), (0, react_jsx_runtime.jsx)("dd", { children: t("unit.tokens", { value: usage.output }) })] }),
					usage.reasoning !== void 0 && (0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryTable_module_css_default.requestTokenDetail,
						children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.reasoning") }), (0, react_jsx_runtime.jsx)("dd", { children: t("unit.tokens", { value: usage.reasoning }) })]
					}),
					otherOutput !== void 0 && (0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryTable_module_css_default.requestTokenDetail,
						children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.content") }), (0, react_jsx_runtime.jsx)("dd", { children: t("unit.tokens", { value: otherOutput }) })]
					})
				]
			});
		}
		function RequestUsagePanel({ usage, cumulative, t }) {
			return (0, react_jsx_runtime.jsxs)("div", {
				className: TrajectoryTable_module_css_default.usagePanel,
				children: [(0, react_jsx_runtime.jsxs)("section", {
					className: TrajectoryTable_module_css_default.usageGroup,
					children: [(0, react_jsx_runtime.jsx)("h4", {
						className: TrajectoryTable_module_css_default.usageHeading,
						children: t("usage.thisRequest")
					}), (0, react_jsx_runtime.jsx)(UsageRows, {
						usage,
						t
					})]
				}), (0, react_jsx_runtime.jsxs)("section", {
					className: TrajectoryTable_module_css_default.usageGroup,
					children: [(0, react_jsx_runtime.jsx)("h4", {
						className: TrajectoryTable_module_css_default.usageHeading,
						children: t("usage.sessionCumulative")
					}), (0, react_jsx_runtime.jsx)(UsageRows, {
						usage: cumulative,
						t
					})]
				})]
			});
		}
		function RequestOptions({ options, preview = false, stringWrapping, t }) {
			if (options === void 0) return (0, react_jsx_runtime.jsx)("p", {
				className: TrajectoryTable_module_css_default.noPayload,
				children: t("options.notRecorded")
			});
			return (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.JsonTree, {
				data: options,
				stringWrapping,
				collapsedStringLines: preview ? 3 : 12,
				label: t("options.json"),
				labels: jsonTreeLabels(t),
				className: preview ? TrajectoryTable_module_css_default.jsonPreview : TrajectoryTable_module_css_default.jsonPayload
			});
		}
		function messageSourceLabel(source, t) {
			if (typeof source !== "object" || source === null || Array.isArray(source)) return t("source.unknown");
			const properties = source;
			const kind = properties.kind;
			if (kind === "user") return t("source.user");
			if (kind === "goal") {
				const round = properties.round;
				return typeof round === "number" && round > 0 ? t("source.goalRound", { round }) : t("source.goal");
			}
			if (typeof kind !== "string" || kind === "") return t("source.unknown");
			return `${kind[0]?.toUpperCase() ?? ""}${kind.slice(1)}`;
		}
		function MessageSource({ record, stringWrapping, t }) {
			const source = record.cell.messageSource;
			if (source === void 0) return (0, react_jsx_runtime.jsx)("p", {
				className: TrajectoryTable_module_css_default.noPayload,
				children: t("source.notRecorded")
			});
			return (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.JsonTree, {
				data: typeof source === "object" && source !== null ? source : { value: source },
				stringWrapping,
				collapsedStringLines: 12,
				label: t("source.messageJson"),
				labels: jsonTreeLabels(t),
				className: TrajectoryTable_module_css_default.jsonPayload
			});
		}
		function isMarkdownRecord(record) {
			return record.cell.kind === "user" || record.cell.kind === "context" || record.cell.kind === "message";
		}
		function parentRecords(records, record) {
			if (record.cell.kind !== "tool" && record.cell.kind !== "subtool") return {};
			const at = records.findIndex((candidate) => candidate.cell.index === record.cell.index);
			if (at === -1) return {};
			let tool;
			if (record.cell.kind === "subtool") for (let i = at - 1; i >= 0; i--) {
				const candidate = records[i];
				if (candidate === void 0 || candidate.turn !== record.turn || candidate.group !== record.group) break;
				if (candidate.cell.kind === "tool") {
					tool = candidate;
					break;
				}
			}
			const parentCallId = tool?.cell.callId ?? record.cell.callId;
			let message;
			if (parentCallId !== void 0) message = records.find((candidate) => candidate.turn === record.turn && candidate.cell.kind === "message" && candidate.cell.sourceBlocks?.some((block) => block.callId === parentCallId) === true);
			return {
				...message === void 0 ? {} : { message },
				...tool === void 0 ? {} : { tool }
			};
		}
		function markdownSource(record) {
			if (record.cell.kind === "user" || record.cell.kind === "context") return record.cell.inputDetail;
			if (record.cell.kind === "message" || record.cell.kind === "compacted") return record.cell.outputDetail;
		}
		function detailTabs(record) {
			if (record.cell.kind === "system") {
				if (record.cell.promptDetail === void 0 && record.cell.systemPromptDetail !== void 0) return SYSTEM_PROMPT_TABS.filter((tab) => tab.id === "system-prompt");
				return record.cell.previousPromptDetail === void 0 ? SYSTEM_PROMPT_TABS : SYSTEM_UPDATE_TABS;
			}
			if (record.cell.kind === "compacted") return [{
				id: "overview",
				labelKey: "tab.summary"
			}, {
				id: "raw",
				labelKey: "tab.rawOutput"
			}];
			if (isMarkdownRecord(record)) return [
				{
					id: "overview",
					labelKey: "tab.summary"
				},
				{
					id: "rendered",
					labelKey: "tab.preview"
				},
				{
					id: "raw",
					labelKey: "tab.raw"
				},
				...record.cell.messageSource === void 0 ? [] : [{
					id: "source",
					labelKey: "tab.source"
				}]
			];
			return [
				{
					id: "overview",
					labelKey: "tab.summary"
				},
				...record.cell.inputDetail ? [{
					id: "input",
					labelKey: codeProgram(record.cell) === void 0 ? "tab.payload" : "code.source"
				}] : [],
				...record.cell.outputDetail || codeProgram(record.cell) !== void 0 ? [{
					id: "output",
					labelKey: "tab.result"
				}] : [],
				{
					id: "schema",
					labelKey: "tab.schema"
				},
				{
					id: "timing",
					labelKey: "tab.timing"
				}
			];
		}
		function recordDisplayText(cell, t) {
			if (isToolCallOnly(cell, t)) return "";
			const program = codeProgram(cell);
			if (program !== void 0) return `${PTC_TOOL_NAME} · ${program.description.replace(/\s+/g, " ")}`;
			if (cell.previewMarkdown !== void 0) {
				const preview = trajectoryPreviewText(cell.previewMarkdown);
				if (cell.text === "") return preview;
				return preview === "" ? cell.text : `${cell.text} · ${preview}`;
			}
			if (cell.text !== "") return cell.text;
			const markdown = cell.kind === "user" || cell.kind === "context" ? cell.inputDetail : cell.kind === "message" ? cell.outputDetail ?? cell.thinkingDetail : void 0;
			return markdown === void 0 ? "" : trajectoryPreviewText(markdown);
		}
		function recordResultText(cell) {
			return cell.resultPreviewMarkdown === void 0 ? cell.result : trajectoryPreviewText(cell.resultPreviewMarkdown);
		}
		function toolCallTextParts(cell, text) {
			if (cell.kind !== "tool" && cell.kind !== "subtool") return void 0;
			const program = cell.toolName === PTC_TOOL_NAME;
			const separator = text.indexOf(" · ");
			if (separator === -1) return {
				name: text,
				program
			};
			return {
				program,
				name: text.slice(0, separator),
				args: text.slice(separator + 3)
			};
		}
		function isToolCallOnly(cell, t) {
			return cell.kind === "message" && !cell.outputDetail && !cell.thinkingDetail && cell.text === t("layout.toolCallOnly");
		}
		function RecordPresentation({ cell, children, t }) {
			const displayText = (0, react.useMemo)(() => recordDisplayText(cell, t), [
				cell.kind,
				cell.text,
				cell.toolName,
				cell.previewMarkdown,
				cell.inputDetail,
				cell.outputDetail,
				cell.thinkingDetail,
				t
			]);
			const resultText = (0, react.useMemo)(() => recordResultText(cell), [cell.result, cell.resultPreviewMarkdown]);
			const toolCallOnly = isToolCallOnly(cell, t);
			const toolCallText = toolCallTextParts(cell, displayText);
			return children({
				displayText,
				listDisplayText: toolCallOnly ? t("record.toolCallOnly") : toolCallText === void 0 ? displayText : [toolCallText.name, toolCallText.args].filter(Boolean).join(" "),
				resultText,
				toolCallOnly,
				toolCallText
			});
		}
		function RecordListText({ displayText, toolCallOnly, toolCallText, t }) {
			if (toolCallOnly) return (0, react_jsx_runtime.jsx)("span", {
				className: TrajectoryTable_module_css_default.toolCallOnly,
				children: t("record.toolCallOnly")
			});
			if (toolCallText === void 0) return displayText || "—";
			return (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [(0, react_jsx_runtime.jsxs)("span", {
				className: TrajectoryTable_module_css_default.toolCallNameTypeface,
				children: [toolCallText.program && (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconCodeOutlineRegular, {
					className: TrajectoryTable_module_css_default.programIcon,
					size: 12
				}), toolCallText.name || "—"]
			}), toolCallText.args !== void 0 && (0, react_jsx_runtime.jsx)("span", {
				className: toolCallText.program ? TrajectoryTable_module_css_default.programSummary : TrajectoryTable_module_css_default.toolCallPayload,
				children: toolCallText.args
			})] });
		}
		function MarkdownFragment({ text, rendered, preview, variant = "body", t }) {
			const labels = (0, react.useMemo)(() => markdownLabels(t), [t]);
			if (rendered) return (0, react_jsx_runtime.jsx)("div", {
				className: preview ? TrajectoryTable_module_css_default.markdownPreview : TrajectoryTable_module_css_default.markdownPayload,
				children: (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.MarkdownText, {
					text,
					labels,
					variant
				})
			});
			return (0, react_jsx_runtime.jsx)("pre", {
				className: `${TrajectoryTable_module_css_default.payload} ${preview ? TrajectoryTable_module_css_default.payloadPreview : ""}`,
				children: text
			});
		}
		function SourceBlocks({ blocks, onOpenCall, t }) {
			const attachments = new Map(recordAttachments(blocks, t).map((entry) => [entry.index, entry]));
			return (0, react_jsx_runtime.jsx)("div", {
				className: TrajectoryTable_module_css_default.sourceBlocks,
				children: blocks.map((block, index) => {
					const attachment = attachments.get(index);
					return attachment !== void 0 ? (0, react_jsx_runtime.jsxs)("details", {
						className: TrajectoryTable_module_css_default.attachmentDisclosure,
						children: [(0, react_jsx_runtime.jsxs)("summary", { children: [(0, react_jsx_runtime.jsx)("span", {
							className: TrajectoryTable_module_css_default.sourceBlockLabel,
							children: t("block.label", {
								index: index + 1,
								type: block.type
							})
						}), (0, react_jsx_runtime.jsx)("span", {
							className: TrajectoryTable_module_css_default.attachmentName,
							title: attachment.name,
							children: attachment.name
						})] }), (0, react_jsx_runtime.jsx)("pre", {
							className: TrajectoryTable_module_css_default.sourceBlockContent,
							children: block.content
						})]
					}, index) : (0, react_jsx_runtime.jsxs)("section", {
						className: TrajectoryTable_module_css_default.sourceBlock,
						children: [block.callId !== void 0 ? (0, react_jsx_runtime.jsxs)("button", {
							type: "button",
							className: TrajectoryTable_module_css_default.sourceBlockJumpTarget,
							"aria-label": t("block.openSummary", { index: index + 1 }),
							title: t("block.openSummaryTitle"),
							onClick: () => {
								if (block.callId !== void 0) onOpenCall(block.callId);
							},
							children: [(0, react_jsx_runtime.jsx)("span", {
								className: TrajectoryTable_module_css_default.sourceBlockLabel,
								children: t("block.label", {
									index: index + 1,
									type: block.type
								})
							}), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconChevronRightOutlineRegular, {
								className: TrajectoryTable_module_css_default.sourceBlockJumpIcon,
								size: 12
							})]
						}) : (0, react_jsx_runtime.jsx)("div", {
							className: TrajectoryTable_module_css_default.sourceBlockHeader,
							children: (0, react_jsx_runtime.jsx)("span", {
								className: TrajectoryTable_module_css_default.sourceBlockLabel,
								children: t("block.label", {
									index: index + 1,
									type: block.type
								})
							})
						}), (0, react_jsx_runtime.jsx)("pre", {
							className: TrajectoryTable_module_css_default.sourceBlockContent,
							children: block.content
						})]
					}, index);
				})
			});
		}
		function recordAttachments(blocks, t) {
			let imageIndex = 0;
			return (blocks ?? []).flatMap((block, index) => {
				const ref = block.attachment ?? block.file;
				if (ref === void 0) return [];
				if (block.attachment !== void 0) imageIndex += 1;
				return [{
					block,
					index,
					name: ref.name ?? t("attachment.imageName", { index: imageIndex }),
					metadata: [
						block.attachment?.mediaType ?? (0, _deepseek_ai_dsh_client_ui_primitives.fileExtension)(ref.name ?? "").toUpperCase(),
						(0, _deepseek_ai_dsh_client_ui_primitives.fileSizeText)(ref.bytes),
						...block.attachment === void 0 ? [] : [`${block.attachment.width} × ${block.attachment.height}`]
					].filter(Boolean).join(" · ")
				}];
			});
		}
		function RecordAttachments({ blocks, preview, renderSlot, loadImage, t }) {
			const attachments = recordAttachments(blocks, t);
			if (attachments.length === 0) return null;
			return (0, react_jsx_runtime.jsx)("ul", {
				className: preview ? `${TrajectoryTable_module_css_default.attachments} ${TrajectoryTable_module_css_default.attachmentsPreview}` : TrajectoryTable_module_css_default.attachments,
				"aria-label": t("attachment.list"),
				children: attachments.map(({ block, index, name, metadata }) => (0, react_jsx_runtime.jsxs)("li", {
					className: TrajectoryTable_module_css_default.attachmentRow,
					children: [block.attachment !== void 0 ? renderSlot("conversation.trajectory.images", {
						images: [{
							attachment: block.attachment,
							label: name
						}],
						align: "start",
						thumbnail: true,
						loadImage
					}) : (0, react_jsx_runtime.jsx)("span", {
						className: TrajectoryTable_module_css_default.attachmentIcon,
						children: (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.FileTypeIcon, { path: name })
					}), (0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryTable_module_css_default.attachmentInfo,
						children: [(0, react_jsx_runtime.jsx)("span", {
							className: TrajectoryTable_module_css_default.attachmentName,
							title: name,
							children: name
						}), (0, react_jsx_runtime.jsx)("span", {
							className: TrajectoryTable_module_css_default.attachmentMetadata,
							children: metadata
						})]
					})]
				}, index))
			});
		}
		function AssistantToolCalls({ blocks, preview, onOpenCall, t }) {
			const calls = blocks?.filter((block) => block.type === "tool-call") ?? [];
			if (calls.length === 0) return null;
			return (0, react_jsx_runtime.jsx)("ul", {
				className: preview ? `${TrajectoryTable_module_css_default.assistantToolCalls} ${TrajectoryTable_module_css_default.assistantToolCallsPreview}` : TrajectoryTable_module_css_default.assistantToolCalls,
				children: calls.map((call, index) => (0, react_jsx_runtime.jsx)("li", { children: (0, react_jsx_runtime.jsxs)("button", {
					type: "button",
					className: TrajectoryTable_module_css_default.assistantToolCallButton,
					title: t("block.openSummaryTitle"),
					onClick: () => {
						if (call.callId !== void 0) onOpenCall(call.callId);
					},
					children: [(0, react_jsx_runtime.jsx)("svg", {
						className: TrajectoryTable_module_css_default.assistantToolCallIcon,
						width: "12",
						height: "12",
						viewBox: "0 0 24 24",
						fill: "none",
						"aria-hidden": "true",
						children: (0, react_jsx_runtime.jsx)("path", {
							d: "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94z",
							stroke: "currentColor",
							strokeWidth: "1.8",
							strokeLinecap: "round",
							strokeLinejoin: "round"
						})
					}), (0, react_jsx_runtime.jsxs)("span", {
						className: TrajectoryTable_module_css_default.assistantToolCallText,
						children: [(0, react_jsx_runtime.jsx)("span", {
							className: TrajectoryTable_module_css_default.assistantToolCallName,
							children: call.toolName ?? t("details.toolCall")
						}), call.content !== "" && (0, react_jsx_runtime.jsx)("span", {
							className: TrajectoryTable_module_css_default.assistantToolCallArgs,
							children: call.content
						})]
					})]
				}) }, call.callId ?? index))
			});
		}
		function ToolGlyph() {
			return (0, react_jsx_runtime.jsx)("svg", {
				className: TrajectoryTable_module_css_default.toolCatalogIcon,
				width: "12",
				height: "12",
				viewBox: "0 0 24 24",
				fill: "none",
				"aria-hidden": "true",
				children: (0, react_jsx_runtime.jsx)("path", {
					d: "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94z",
					stroke: "currentColor",
					strokeWidth: "1.8",
					strokeLinecap: "round",
					strokeLinejoin: "round"
				})
			});
		}
		function ToolCatalog({ tools, stringWrapping, t }) {
			if (tools.length === 0) return (0, react_jsx_runtime.jsx)("p", {
				className: TrajectoryTable_module_css_default.noPayload,
				children: t("record.toolsMissing")
			});
			return (0, react_jsx_runtime.jsx)("div", {
				className: TrajectoryTable_module_css_default.toolCatalog,
				children: tools.map((tool, index) => (0, react_jsx_runtime.jsxs)("details", {
					className: TrajectoryTable_module_css_default.toolCatalogItem,
					children: [(0, react_jsx_runtime.jsxs)("summary", {
						className: TrajectoryTable_module_css_default.toolCatalogSummary,
						children: [
							(0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconChevronRightOutlineRegular, {
								className: TrajectoryTable_module_css_default.toolCatalogChevron,
								size: 12
							}),
							(0, react_jsx_runtime.jsx)(ToolGlyph, {}),
							(0, react_jsx_runtime.jsx)("span", {
								className: TrajectoryTable_module_css_default.toolCatalogName,
								children: tool.name
							}),
							(0, react_jsx_runtime.jsx)("span", {
								className: TrajectoryTable_module_css_default.toolCatalogDescription,
								children: tool.description
							})
						]
					}), (0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryTable_module_css_default.toolCatalogDefinition,
						children: [tool.description !== "" && (0, react_jsx_runtime.jsx)("p", {
							className: TrajectoryTable_module_css_default.toolCatalogFullDescription,
							children: tool.description
						}), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.JsonTree, {
							data: tool.parameters,
							stringWrapping,
							label: t("record.namedParametersJson", { name: tool.name }),
							labels: jsonTreeLabels(t),
							className: TrajectoryTable_module_css_default.toolCatalogTree
						})]
					})]
				}, `${tool.name}:${index}`))
			});
		}
		function promptDiffLines(before, after) {
			return structuredPatch("", "", before, after, void 0, void 0, { context: 3 }).hunks.flatMap((hunk, hunkIndex) => [
				...hunkIndex === 0 ? [] : [{
					kind: "meta",
					text: ""
				}],
				{
					kind: "meta",
					text: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`
				},
				...hunk.lines.flatMap((line) => {
					if (line.startsWith("\\")) return [];
					if (line.startsWith("+")) return [{
						kind: "added",
						text: line
					}];
					if (line.startsWith("-")) return [{
						kind: "removed",
						text: line
					}];
					return [{
						kind: "context",
						text: line
					}];
				})
			]);
		}
		function PromptDiffSection({ title, before, after }) {
			const lines = promptDiffLines(before, after);
			if (lines.length === 0) return null;
			return (0, react_jsx_runtime.jsxs)("section", {
				className: TrajectoryTable_module_css_default.promptDiffSection,
				children: [(0, react_jsx_runtime.jsx)("h3", {
					className: TrajectoryTable_module_css_default.promptDiffTitle,
					children: title
				}), (0, react_jsx_runtime.jsx)("pre", {
					className: TrajectoryTable_module_css_default.promptDiff,
					children: lines.map((line, index) => (0, react_jsx_runtime.jsxs)("span", {
						className: TrajectoryTable_module_css_default[`promptDiffLine${line.kind}`],
						children: [line.text || " ", "\n"]
					}, index))
				})]
			});
		}
		function SystemPromptDiff({ before, after, t }) {
			const toolsBefore = JSON.stringify(before.tools, null, 2);
			const toolsAfter = JSON.stringify(after.tools, null, 2);
			return (0, react_jsx_runtime.jsxs)("div", {
				className: TrajectoryTable_module_css_default.promptDiffSections,
				children: [before.system !== after.system && (0, react_jsx_runtime.jsx)(PromptDiffSection, {
					title: t("record.systemPrompt"),
					before: before.system,
					after: after.system
				}), toolsBefore !== toolsAfter && (0, react_jsx_runtime.jsx)(PromptDiffSection, {
					title: t("record.tools"),
					before: toolsBefore,
					after: toolsAfter
				})]
			});
		}
		function ToolOutputBlocks({ blocks, error, errorDetail, preview, renderSlot, loadImage }) {
			return (0, react_jsx_runtime.jsxs)("div", {
				className: [
					TrajectoryTable_module_css_default.resultBlocks,
					preview ? TrajectoryTable_module_css_default.resultBlocksPreview : void 0,
					error ? TrajectoryTable_module_css_default.errorPayload : void 0
				].filter((value) => value !== void 0).join(" "),
				children: [error && errorDetail !== void 0 && errorDetail !== "" && (0, react_jsx_runtime.jsx)("pre", {
					className: TrajectoryTable_module_css_default.resultBlockText,
					children: errorDetail
				}), blocks.map((block, index) => block.attachment !== void 0 ? (0, react_jsx_runtime.jsx)("div", {
					className: TrajectoryTable_module_css_default.messageImages,
					children: renderSlot("conversation.trajectory.images", {
						images: [{ attachment: block.attachment }],
						align: "start",
						loadImage
					})
				}, index) : block.content !== "" ? (0, react_jsx_runtime.jsx)("pre", {
					className: TrajectoryTable_module_css_default.resultBlockText,
					children: block.content
				}, index) : null)]
			});
		}
		function MarkdownRecordContent({ record, rendered, preview = false, thinkingExpanded, onThinkingExpandedChange, onOpenCall, renderSlot, loadImage, t }) {
			if (record.cell.sourceBlocks?.length && record.cell.sourceBlocks.every((block) => block.type === "tool-addition" || block.type === "tool-removal")) return (0, react_jsx_runtime.jsx)("pre", {
				className: `${TrajectoryTable_module_css_default.payload} ${TrajectoryTable_module_css_default.toolUpdatePayload}`,
				children: record.cell.inputDetail
			});
			if (!rendered && record.cell.sourceBlocks && record.cell.sourceBlocks.length > 0) return (0, react_jsx_runtime.jsx)(SourceBlocks, {
				blocks: record.cell.sourceBlocks,
				onOpenCall,
				t
			});
			if (record.cell.thinkingDetail) {
				if (!rendered) return (0, react_jsx_runtime.jsx)(MarkdownFragment, {
					text: [record.cell.thinkingDetail, record.cell.outputDetail].filter((value) => value !== void 0 && value !== "").join("\n\n"),
					rendered: false,
					preview,
					t
				});
				return (0, react_jsx_runtime.jsxs)("div", {
					className: `${TrajectoryTable_module_css_default.assistantContent} ${TrajectoryTable_module_css_default.assistantContentRendered}`,
					children: [
						(0, react_jsx_runtime.jsxs)("div", {
							className: preview && !record.cell.outputDetail ? `${TrajectoryTable_module_css_default.thinkingQuote} ${TrajectoryTable_module_css_default.thinkingQuoteOnlyPreview}` : TrajectoryTable_module_css_default.thinkingQuote,
							children: [(0, react_jsx_runtime.jsxs)("button", {
								type: "button",
								className: TrajectoryTable_module_css_default.thinkingToggle,
								"aria-expanded": thinkingExpanded,
								onClick: () => {
									onThinkingExpandedChange(!thinkingExpanded);
								},
								children: [t("record.thinking"), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconChevronRightOutlineRegular, {
									className: TrajectoryTable_module_css_default.thinkingChevron,
									size: 12
								})]
							}), thinkingExpanded && (0, react_jsx_runtime.jsx)(MarkdownFragment, {
								text: record.cell.thinkingDetail,
								rendered,
								preview,
								variant: "compact",
								t
							})]
						}),
						record.cell.outputDetail && (0, react_jsx_runtime.jsx)("div", {
							className: TrajectoryTable_module_css_default.assistantOutput,
							children: (0, react_jsx_runtime.jsx)(MarkdownFragment, {
								text: record.cell.outputDetail,
								rendered,
								preview,
								t
							})
						}),
						(0, react_jsx_runtime.jsx)(AssistantToolCalls, {
							blocks: record.cell.sourceBlocks,
							preview,
							onOpenCall,
							t
						}),
						(0, react_jsx_runtime.jsx)(RecordAttachments, {
							blocks: record.cell.sourceBlocks,
							preview,
							renderSlot,
							loadImage,
							t
						})
					]
				});
			}
			const source = markdownSource(record);
			const hasAttachments = record.cell.sourceBlocks?.some((block) => block.attachment !== void 0 || block.file !== void 0) === true;
			const hasToolCalls = record.cell.kind === "message" && record.cell.sourceBlocks?.some((block) => block.type === "tool-call") === true;
			if (!source && !hasAttachments && !hasToolCalls) {
				const emptyLabel = isToolCallOnly(record.cell, t) ? t("record.toolCallOnly") : record.cell.text || t("record.noContent");
				return (0, react_jsx_runtime.jsx)("p", {
					className: TrajectoryTable_module_css_default.noPayload,
					children: emptyLabel
				});
			}
			if (!rendered || !hasAttachments && !hasToolCalls) return (0, react_jsx_runtime.jsx)(MarkdownFragment, {
				text: source ?? "",
				rendered,
				preview,
				t
			});
			return (0, react_jsx_runtime.jsxs)("div", { children: [
				source && (0, react_jsx_runtime.jsx)(MarkdownFragment, {
					text: source,
					rendered: true,
					preview,
					t
				}),
				record.cell.kind === "message" && (0, react_jsx_runtime.jsx)(AssistantToolCalls, {
					blocks: record.cell.sourceBlocks,
					preview,
					onOpenCall,
					t
				}),
				(0, react_jsx_runtime.jsx)(RecordAttachments, {
					blocks: record.cell.sourceBlocks,
					preview,
					renderSlot,
					loadImage,
					t
				})
			] });
		}
		function RecordTiming({ record, preview = false, t }) {
			if (record.cell.rthKind !== void 0) return null;
			return record.cell.kind === "message" && record.cell.assistantMetrics !== void 0 ? (0, react_jsx_runtime.jsx)(AssistantTimingPanel, {
				metrics: record.cell.assistantMetrics,
				t
			}) : (0, react_jsx_runtime.jsxs)("dl", {
				className: TrajectoryTable_module_css_default.overview,
				children: [
					(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.started") }), (0, react_jsx_runtime.jsx)(StartedAtValue, {
						timestamp: record.cell.startedAt ?? null,
						t
					})] }),
					(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.duration") }), (0, react_jsx_runtime.jsx)("dd", { children: formatElapsedSeconds(record.cell.timeSeconds, t) })] }),
					!preview && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.source") }), (0, react_jsx_runtime.jsx)("dd", { children: record.cell.timeSeconds === null ? t("timing.notAvailable") : t("timing.sessionTimestamps") })] })
				]
			});
		}
		function RequestTiming({ assistant, anchor, request, preview = false, t }) {
			if (assistant !== void 0) return (0, react_jsx_runtime.jsx)(RecordTiming, {
				record: assistant,
				preview,
				t
			});
			if (request?.startedAt !== void 0) {
				const duration = request.completedAt === null || request.completedAt === void 0 ? null : Math.max(0, (request.completedAt - request.startedAt) / 1e3);
				return (0, react_jsx_runtime.jsxs)("dl", {
					className: TrajectoryTable_module_css_default.overview,
					children: [
						(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.started") }), (0, react_jsx_runtime.jsx)(StartedAtValue, {
							timestamp: request.startedAt,
							t
						})] }),
						(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.duration") }), (0, react_jsx_runtime.jsx)("dd", { children: formatElapsedSeconds(duration, t) })] }),
						!preview && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.source") }), (0, react_jsx_runtime.jsx)("dd", { children: duration === null ? t("timing.sessionTimestampsRunning") : t("timing.sessionTimestamps") })] })
					]
				});
			}
			return (0, react_jsx_runtime.jsxs)("dl", {
				className: TrajectoryTable_module_css_default.overview,
				children: [(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.started") }), (0, react_jsx_runtime.jsx)(StartedAtValue, {
					timestamp: anchor?.cell.startedAt ?? null,
					t
				})] }), (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.duration") }), (0, react_jsx_runtime.jsx)("dd", { children: formatElapsedSeconds(null, t) })] })]
			});
		}
		function RecordPayload({ record, direction, preview = false, renderSlot, loadImage, stringWrapping, t }) {
			const value = direction === "input" ? record.cell.inputDetail : record.cell.outputDetail;
			const missing = direction === "input" ? t("record.noPayload") : t("record.noResult");
			if (!value) return (0, react_jsx_runtime.jsx)("p", {
				className: TrajectoryTable_module_css_default.noPayload,
				children: missing
			});
			const error = direction === "output" && record.cell.isError === true;
			const payloadClass = preview ? TrajectoryTable_module_css_default.jsonPreview : TrajectoryTable_module_css_default.jsonPayload;
			const payloadClassName = error ? `${payloadClass} ${TrajectoryTable_module_css_default.errorPayload}` : payloadClass;
			const json = parseJsonContainer(value);
			if (direction === "output" && record.cell.outputBlocks?.length === 1 && record.cell.outputBlocks[0]?.type === "text" && json !== void 0) return (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.JsonTree, {
				data: json,
				stringWrapping,
				collapsedStringLines: preview ? 3 : 12,
				label: t("record.resultJson"),
				labels: jsonTreeLabels(t),
				className: payloadClassName
			});
			if (direction === "output" && record.cell.outputBlocks?.some((block) => block.attachment !== void 0 || block.content !== "") === true) return (0, react_jsx_runtime.jsx)(ToolOutputBlocks, {
				blocks: record.cell.outputBlocks,
				error,
				errorDetail: error ? value : void 0,
				preview,
				renderSlot,
				loadImage
			});
			if (direction === "input" && (record.cell.kind === "user" || record.cell.kind === "context") || direction === "output" && record.cell.kind === "message") return (0, react_jsx_runtime.jsx)("div", {
				className: [preview ? TrajectoryTable_module_css_default.markdownPreview : TrajectoryTable_module_css_default.markdownPayload, error ? TrajectoryTable_module_css_default.errorPayload : void 0].filter((className) => className !== void 0).join(" "),
				children: (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.MarkdownText, {
					text: value,
					labels: markdownLabels(t)
				})
			});
			if (json !== void 0) return (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.JsonTree, {
				data: json,
				stringWrapping,
				collapsedStringLines: preview ? 3 : 12,
				label: t(direction === "input" ? "record.payloadJson" : "record.outputJson"),
				labels: jsonTreeLabels(t),
				className: payloadClassName
			});
			return (0, react_jsx_runtime.jsx)("pre", {
				className: [
					TrajectoryTable_module_css_default.payload,
					preview ? TrajectoryTable_module_css_default.payloadPreview : void 0,
					error ? TrajectoryTable_module_css_default.errorPayload : void 0,
					value === t("record.noOutput") ? TrajectoryTable_module_css_default.noOutputText : void 0
				].filter((value) => value !== void 0).join(" "),
				children: value
			});
		}
		function RecordSchema({ record, preview = false, stringWrapping, t }) {
			if (!record.cell.schemaDetail) return (0, react_jsx_runtime.jsx)("p", {
				className: TrajectoryTable_module_css_default.noPayload,
				children: t("record.schemaUnavailable")
			});
			const schema = parseToolSchema(record.cell.schemaDetail);
			if (schema !== void 0) return (0, react_jsx_runtime.jsxs)("div", {
				className: preview ? `${TrajectoryTable_module_css_default.schema} ${TrajectoryTable_module_css_default.schemaPreview}` : TrajectoryTable_module_css_default.schema,
				children: [(0, react_jsx_runtime.jsxs)("header", {
					className: TrajectoryTable_module_css_default.schemaIntro,
					children: [(0, react_jsx_runtime.jsx)("h3", {
						className: TrajectoryTable_module_css_default.schemaName,
						children: schema.name
					}), (0, react_jsx_runtime.jsx)("p", {
						className: TrajectoryTable_module_css_default.schemaDescription,
						children: schema.description
					})]
				}), (0, react_jsx_runtime.jsxs)("section", {
					className: TrajectoryTable_module_css_default.schemaParameters,
					children: [(0, react_jsx_runtime.jsx)("h4", {
						className: TrajectoryTable_module_css_default.schemaParametersTitle,
						children: t("record.parameters")
					}), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.JsonTree, {
						data: schema.parameters,
						stringWrapping,
						collapsedStringLines: preview ? 3 : 12,
						label: t("record.namedParametersJson", { name: schema.name }),
						labels: jsonTreeLabels(t),
						className: TrajectoryTable_module_css_default.schemaTree
					})]
				})]
			});
			return (0, react_jsx_runtime.jsx)("pre", {
				className: `${TrajectoryTable_module_css_default.payload} ${preview ? TrajectoryTable_module_css_default.payloadPreview : ""}`,
				children: record.cell.schemaDetail
			});
		}
		function parseToolSchema(value) {
			try {
				const parsed = JSON.parse(value);
				if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return void 0;
				const schema = parsed;
				if (typeof schema.name !== "string" || typeof schema.description !== "string" || typeof schema.parameters !== "object" || schema.parameters === null || Array.isArray(schema.parameters)) return void 0;
				return {
					name: schema.name,
					description: schema.description,
					parameters: schema.parameters
				};
			} catch {
				return;
			}
		}
		function parseJsonContainer(value) {
			try {
				const parsed = JSON.parse(value);
				return typeof parsed === "object" && parsed !== null ? parsed : void 0;
			} catch {
				return;
			}
		}
		function InspectorCopyButton({ text, label, t }) {
			const [state, setState] = (0, react.useState)("idle");
			(0, react.useEffect)(() => {
				if (state === "idle") return;
				const timer = setTimeout(() => {
					setState("idle");
				}, 1500);
				return () => {
					clearTimeout(timer);
				};
			}, [state]);
			const title = state === "idle" ? label : t(state === "copied" ? "copied" : "copy.failed");
			return (0, react_jsx_runtime.jsx)("button", {
				type: "button",
				className: TrajectoryTable_module_css_default.programAction,
				"data-state": state,
				"aria-label": title,
				title,
				onClick: () => {
					(0, _deepseek_ai_dsh_client_ui_primitives.writeClipboard)(text).then((ok) => {
						setState(ok ? "copied" : "failed");
					});
				},
				children: state === "copied" ? (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconCheckOutlineRegular, { size: 12 }) : (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconCopyOutlineRegular, { size: 12 })
			});
		}
		function ProgramInput({ program, initialWrapped, stringWrapping, onOpen, t }) {
			const contentsId = (0, react.useId)();
			const [wrapped, setWrapped] = (0, react.useState)(initialWrapped);
			const [showJson, setShowJson] = (0, react.useState)(false);
			const actions = (0, react_jsx_runtime.jsxs)("span", {
				className: TrajectoryTable_module_css_default.programActions,
				children: [
					!showJson && program.language !== void 0 && (0, react_jsx_runtime.jsx)("span", {
						className: TrajectoryTable_module_css_default.programLanguage,
						children: program.language
					}),
					!showJson && (0, react_jsx_runtime.jsx)("button", {
						type: "button",
						className: TrajectoryTable_module_css_default.programAction,
						"aria-label": t("record.wrapLines"),
						title: t("record.wrapLines"),
						"aria-pressed": wrapped,
						"aria-controls": contentsId,
						onClick: () => {
							const next = !wrapped;
							setWrapped(next);
							stringWrapping?.setDefault(next);
						},
						children: (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconWrapLinesOutlineRegular, { size: 12 })
					}),
					onOpen === void 0 && (0, react_jsx_runtime.jsx)("button", {
						type: "button",
						className: TrajectoryTable_module_css_default.programAction,
						"aria-label": t("code.originalJson"),
						title: t("code.originalJson"),
						"aria-pressed": showJson,
						"aria-controls": contentsId,
						onClick: () => {
							setShowJson((value) => !value);
						},
						children: (0, react_jsx_runtime.jsxs)("svg", {
							width: "12",
							height: "12",
							viewBox: "0 0 16 16",
							fill: "none",
							stroke: "currentColor",
							strokeWidth: "1.5",
							strokeLinecap: "round",
							strokeLinejoin: "round",
							"aria-hidden": "true",
							children: [(0, react_jsx_runtime.jsx)("path", { d: "M6 2H5a2 2 0 0 0-2 2v2a2 2 0 0 1-2 2 2 2 0 0 1 2 2v2a2 2 0 0 0 2 2h1" }), (0, react_jsx_runtime.jsx)("path", { d: "M10 2h1a2 2 0 0 1 2 2v2a2 2 0 0 0 2 2 2 2 0 0 0-2 2v2a2 2 0 0 1-2 2h-1" })]
						})
					}),
					(0, react_jsx_runtime.jsx)(InspectorCopyButton, {
						text: showJson ? program.rawInput : program.source,
						label: t(showJson ? "copy.json" : "code.copySource"),
						t
					})
				]
			});
			const body = (0, react_jsx_runtime.jsx)("div", {
				id: contentsId,
				className: TrajectoryTable_module_css_default.programContent,
				"data-wrap": wrapped,
				children: showJson ? (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.JsonTree, {
					data: program.arguments,
					label: t("record.parametersJson"),
					labels: jsonTreeLabels(t),
					stringWrapping
				}) : (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.CodeBlock, {
					code: program.source,
					lang: program.language,
					lineNumbers: true,
					showHeader: false,
					className: TrajectoryTable_module_css_default.programSource,
					copyLabel: t("code.copySource"),
					copiedLabel: t("copied")
				})
			});
			return onOpen === void 0 ? (0, react_jsx_runtime.jsxs)("section", {
				className: TrajectoryTable_module_css_default.programPanel,
				children: [(0, react_jsx_runtime.jsxs)("header", {
					className: TrajectoryTable_module_css_default.overviewHeading,
					children: [(0, react_jsx_runtime.jsx)("span", { children: t("code.source") }), actions]
				}), body]
			}) : (0, react_jsx_runtime.jsx)(OverviewSection, {
				label: t("code.source"),
				onOpen,
				actions,
				children: body
			});
		}
		function ProgramOutput({ record, stringWrapping, onOpen, t }) {
			const output = record.cell.outputDetail;
			const json = output === void 0 || record.cell.isError === true ? void 0 : parseJsonContainer(output);
			const actions = output === void 0 ? void 0 : (0, react_jsx_runtime.jsx)("span", {
				className: TrajectoryTable_module_css_default.programActions,
				children: (0, react_jsx_runtime.jsx)(InspectorCopyButton, {
					text: output,
					label: t("code.copyOutput"),
					t
				})
			});
			const body = (0, react_jsx_runtime.jsx)("div", {
				className: TrajectoryTable_module_css_default.programContent,
				children: output === void 0 || output === "" ? (0, react_jsx_runtime.jsx)("p", {
					className: TrajectoryTable_module_css_default.noPayload,
					children: t(stateOf(record) === "running" ? "code.running" : "record.noOutput")
				}) : json !== void 0 ? (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.JsonTree, {
					data: json,
					label: t("record.outputJson"),
					labels: jsonTreeLabels(t),
					stringWrapping,
					collapsedStringLines: onOpen === void 0 ? 12 : 3
				}) : (0, react_jsx_runtime.jsx)("pre", {
					className: `${TrajectoryTable_module_css_default.programOutput} ${record.cell.isError === true ? TrajectoryTable_module_css_default.programError : ""}`,
					children: output
				})
			});
			return onOpen === void 0 ? (0, react_jsx_runtime.jsxs)("section", {
				className: TrajectoryTable_module_css_default.programPanel,
				children: [(0, react_jsx_runtime.jsxs)("header", {
					className: TrajectoryTable_module_css_default.overviewHeading,
					children: [(0, react_jsx_runtime.jsx)("span", { children: t("code.output") }), actions]
				}), body]
			}) : (0, react_jsx_runtime.jsx)(OverviewSection, {
				label: t("code.output"),
				onOpen,
				actions,
				children: body
			});
		}
		function OverviewSection({ label, onOpen, actions, children }) {
			const previewRef = (0, react.useRef)(null);
			const [hasMore, setHasMore] = (0, react.useState)(false);
			const measureOverflow = (0, react.useCallback)((preview) => {
				setHasMore(preview.scrollHeight - preview.clientHeight - preview.scrollTop > 1);
			}, []);
			(0, react.useLayoutEffect)(() => {
				const preview = previewRef.current;
				const measure = () => {
					measureOverflow(preview);
				};
				measure();
				if (typeof ResizeObserver === "undefined") return;
				const observer = new ResizeObserver(measure);
				observer.observe(preview);
				for (const child of preview.children) observer.observe(child);
				return () => {
					observer.disconnect();
				};
			}, [children, measureOverflow]);
			return (0, react_jsx_runtime.jsxs)("section", {
				className: TrajectoryTable_module_css_default.overviewSection,
				children: [(0, react_jsx_runtime.jsxs)("h3", {
					className: TrajectoryTable_module_css_default.overviewHeading,
					children: [(0, react_jsx_runtime.jsxs)("button", {
						type: "button",
						className: TrajectoryTable_module_css_default.overviewTitle,
						onClick: onOpen,
						children: [(0, react_jsx_runtime.jsx)("span", { children: label }), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconChevronRightOutlineRegular, {
							className: TrajectoryTable_module_css_default.overviewTitleIcon,
							size: 12
						})]
					}), actions]
				}), (0, react_jsx_runtime.jsx)("div", {
					ref: previewRef,
					className: `${TrajectoryTable_module_css_default.overviewPreview} ${TrajectoryTable_module_css_default.summaryScrollRegion}`,
					"data-summary-scroll-region": "",
					"data-scroll-more": hasMore || void 0,
					onScroll: (event) => {
						measureOverflow(event.currentTarget);
					},
					children
				})]
			});
		}
		/**
		* Render trajectory events as a dense ledger with turn and step separators.
		* Clicking ledger whitespace clears the active record or request selection.
		* @param props - Grouped trajectory data and whole-ledger fold state.
		* @returns The ledger and an optional local record inspector.
		*/
		function TrajectoryTable({ t, stringWrapping, renderSlot, loadImage, requestNumbers: sessionRequestNumbers, turns, streamingCells = [], timelineFocusIndexes = null, searchMatchIndexes = null, onSelectedIndexChange, onRecordSelect, recordSelection = null, recordFocus = null, historyLoading = false, olderHistoryLoading = false, historyStartSeq, hasOlderRecords = false, onLoadOlder, onClearSelection, collapsedTurns, onToggleTurn, collapsedAssistants, onToggleAssistant, inspectCallId = null, onInspectApplied }) {
			const [selectedRecordId, setSelectedRecordId] = (0, react.useState)(null);
			const [selectedRequest, setSelectedRequest] = (0, react.useState)(null);
			const [activeTab, setActiveTab] = (0, react.useState)("overview");
			const [codeWrappingOnOpen, setCodeWrappingOnOpen] = (0, react.useState)(false);
			const [thinkingDisclosure, setThinkingDisclosure] = (0, react.useState)();
			const [detailsWidth, setDetailsWidth] = (0, react.useState)(null);
			const [toolRequestOffset, setToolRequestOffset] = (0, react.useState)(null);
			const detailsResizeDrag = (0, react.useRef)(null);
			const appliedRecordSelection = (0, react.useRef)(null);
			const appliedRecordFocus = (0, react.useRef)(null);
			const tabHistory = (0, react.useRef)(new Set(["overview"]));
			const rootRef = (0, react.useRef)(null);
			const tablePaneRef = (0, react.useRef)(null);
			const followsTableTail = (0, react.useRef)(false);
			const tableScrollInitialized = (0, react.useRef)(false);
			const [tableScrollReady, setTableScrollReady] = (0, react.useState)(false);
			const pendingScrollRecordId = (0, react.useRef)(null);
			const loadingOlder = (0, react.useRef)(false);
			const [olderLoading, setOlderLoading] = (0, react.useState)(false);
			const olderLoadAnchor = (0, react.useRef)(null);
			const allRecords = (0, react.useMemo)(() => flattenRecords(turns), [turns]);
			const streamingCellsByIndex = (0, react.useMemo)(() => new Map(streamingCells.map((cell) => [cell.index, cell])), [streamingCells]);
			const currentRecord = (0, react.useCallback)((record) => {
				const cell = streamingCellsByIndex.get(record.cell.index);
				return cell === void 0 ? record : {
					...record,
					cell
				};
			}, [streamingCellsByIndex]);
			const selectedTemplate = (0, react.useMemo)(() => selectedRecordId === null ? void 0 : allRecords.find((record) => trajectoryRecordId(record.cell) === selectedRecordId), [allRecords, selectedRecordId]);
			const selected = selectedTemplate === void 0 ? void 0 : currentRecord(selectedTemplate);
			const selectedProgram = selected === void 0 ? void 0 : codeProgram(selected.cell);
			const thinkingExpanded = thinkingDisclosure?.recordId === selectedRecordId ? thinkingDisclosure.expanded : selected?.cell.kind === "message" && Boolean(selected.cell.thinkingDetail?.trim());
			const setThinkingExpanded = (expanded) => {
				if (selectedRecordId !== null) setThinkingDisclosure({
					recordId: selectedRecordId,
					expanded
				});
			};
			const selectedIndex = selected?.cell.index ?? null;
			(0, react.useEffect)(() => {
				onSelectedIndexChange?.(selectedIndex);
			}, [onSelectedIndexChange, selectedIndex]);
			const requestGroups = (0, react.useMemo)(() => new Set((sessionRequestNumbers ?? []).map((request) => requestKey(request.turn, request.group))), [sessionRequestNumbers]);
			const requestBoundaries = (0, react.useMemo)(() => indexRequestBoundaries(allRecords, requestGroups), [allRecords, requestGroups]);
			const requestNumbers = (0, react.useMemo)(() => indexRequestNumbers(sessionRequestNumbers), [sessionRequestNumbers]);
			const records = (0, react.useMemo)(() => {
				if (searchMatchIndexes !== null) return filterRecords(allRecords, searchMatchIndexes);
				const turnRecords = collapsedTurns.size === 0 ? allRecords : collapseTurnRecords(allRecords, collapsedTurns, requestGroups, t);
				return collapsedAssistants.size === 0 ? turnRecords : collapseAssistantRecords(turnRecords, collapsedAssistants, t);
			}, [
				allRecords,
				collapsedAssistants,
				collapsedTurns,
				requestGroups,
				searchMatchIndexes,
				t
			]);
			const projectedVirtualRows = (0, react.useMemo)(() => groupTrajectoryVirtualRows(records), [records]);
			const virtualRowStructure = useStableVirtualRowStructure(projectedVirtualRows);
			const virtualizationEnabled = hasOlderRecords || records.length > VIRTUALIZATION_THRESHOLD;
			const virtualScrollMargin = hasOlderRecords ? HISTORY_LOAD_ROW_HEIGHT_PX : 0;
			const estimateVirtualRowSize = (0, react.useCallback)((index) => virtualRowStructure[index]?.height ?? 30, [virtualRowStructure]);
			const getVirtualRowKey = (0, react.useCallback)((index) => virtualRowStructure[index]?.key ?? index, [virtualRowStructure]);
			const getTableScrollElement = (0, react.useCallback)(() => tablePaneRef.current, []);
			const rowVirtualizer = (0, _tanstack_react_virtual.useVirtualizer)({
				count: virtualizationEnabled ? virtualRowStructure.length : 0,
				enabled: virtualizationEnabled,
				estimateSize: estimateVirtualRowSize,
				getItemKey: getVirtualRowKey,
				getScrollElement: getTableScrollElement,
				initialRect: {
					width: 0,
					height: VIRTUAL_INITIAL_VIEWPORT_HEIGHT_PX
				},
				anchorTo: "end",
				overscan: VIRTUAL_OVERSCAN_ROWS,
				scrollMargin: virtualScrollMargin,
				scrollEndThreshold: BOTTOM_FOLLOW_THRESHOLD_PX,
				followOnAppend: "auto"
			});
			const virtualIndexByRecordId = (0, react.useMemo)(() => {
				const indexes = /* @__PURE__ */ new Map();
				for (const [virtualIndex, row] of projectedVirtualRows.entries()) for (const entry of row.entries) if (entry.record.collapsedSummary === void 0) indexes.set(trajectoryRecordId(entry.record.cell), virtualIndex);
				return indexes;
			}, [projectedVirtualRows]);
			const virtualItems = virtualizationEnabled ? rowVirtualizer.getVirtualItems() : [];
			const virtualTop = Math.max(0, (virtualItems[0]?.start ?? 0) - virtualScrollMargin);
			const virtualBottom = virtualItems.length === 0 ? 0 : Math.max(0, rowVirtualizer.getTotalSize() + virtualScrollMargin - (virtualItems.at(-1)?.end ?? 0));
			const renderedRecords = virtualizationEnabled ? virtualItems.flatMap((item) => {
				const row = projectedVirtualRows[item.index];
				if (row === void 0) return [];
				return row.entries.map((entry, entryIndex) => ({
					record: currentRecord(entry.record),
					position: entry.logicalIndex,
					terminalRequestBoundary: entry.record.cell.requestOnly === true && row.entries.at(-1)?.record.cell.requestOnly === true && entryIndex === row.entries.length - 1
				}));
			}) : records.map((record, position) => ({
				record: currentRecord(record),
				position,
				terminalRequestBoundary: record.cell.requestOnly === true && position === records.length - 1
			}));
			const requestBoundaryRuns = (0, react.useMemo)(() => indexRequestBoundaryRuns(records, requestGroups), [records, requestGroups]);
			const selectedPrompt = selected?.cell.kind === "system" ? selected.cell.promptDetail : void 0;
			const selectedPreviousPrompt = selected?.cell.kind === "system" ? selected.cell.previousPromptDetail : void 0;
			const selectedSystemPrompt = selectedPrompt?.system ?? selected?.cell.systemPromptDetail;
			const promptSelected = selectedSystemPrompt !== void 0;
			const selectedState = selected === void 0 ? void 0 : stateOf(selected);
			const selectedRequestInfo = selectedRequest === null ? void 0 : sessionRequestNumbers?.find((request) => requestIdentity(request) === selectedRequest.identity);
			const selectedRequestRecords = (0, react.useMemo)(() => selectedRequestInfo === void 0 ? [] : allRecords.filter((record) => record.turn === selectedRequestInfo.turn && record.group === selectedRequestInfo.group), [allRecords, selectedRequestInfo]).map(currentRecord);
			const selectedRequestAssistant = selectedRequestRecords.find((record) => record.cell.kind === "message" && record.cell.requestOnly !== true);
			const selectedRequestAnchor = selectedRequestAssistant ?? selectedRequestRecords[0];
			const selectedRequestNumber = selectedRequestInfo?.number;
			const selectedRequestState = selectedRequestInfo === void 0 ? void 0 : selectedRequestInfo.status ?? (selectedRequestAssistant?.cell.assistantMetrics?.completedTime === null ? "running" : selectedRequestAssistant === void 0 && selectedRequestRecords.some((record) => stateOf(record) === "running") ? "running" : "complete");
			const selectedRequestToolCalls = selectedRequestRecords.filter((record) => record.cell.kind === "tool").length;
			const selectedRequestSubtoolCalls = selectedRequestRecords.filter((record) => record.cell.kind === "subtool").length;
			const selectedRequestResultTemplate = selectedRequestInfo?.resultSeq === void 0 ? selectedRequestAssistant : allRecords.find((record) => record.cell.sourceSeq === selectedRequestInfo.resultSeq);
			const selectedRequestResult = selectedRequestResultTemplate === void 0 ? void 0 : currentRecord(selectedRequestResultTemplate);
			const selectedRequestUsage = selectedRequestInfo?.usage ?? (selectedRequestAssistant === void 0 ? void 0 : {
				...selectedRequestAssistant.cell.input === void 0 ? {} : { input: selectedRequestAssistant.cell.input },
				...selectedRequestAssistant.cell.cacheRead === void 0 ? {} : { cacheRead: selectedRequestAssistant.cell.cacheRead },
				...selectedRequestAssistant.cell.cacheWrite === void 0 ? {} : { cacheWrite: selectedRequestAssistant.cell.cacheWrite },
				...selectedRequestAssistant.cell.output === void 0 ? {} : { output: selectedRequestAssistant.cell.output },
				...selectedRequestAssistant.cell.think === void 0 ? {} : { reasoning: selectedRequestAssistant.cell.think }
			});
			const selectedRequestCumulativeUsage = selectedRequestInfo?.cumulativeUsage ?? selectedRequestUsage;
			const selectedRequestOptions = selectedRequestInfo?.requestConfig;
			const activeTurn = selectedRequestInfo === void 0 ? selected?.turn : selectedRequestInfo.turn;
			const activeSection = selectedRequestInfo === void 0 ? selected?.section : selectedRequestRecords[0]?.section;
			const selectedTabs = selectedRequestInfo !== void 0 ? REQUEST_TABS.filter((tab) => tab.id !== "options" || selectedRequestOptions !== void 0) : selected === void 0 ? [] : detailTabs(selected);
			const selectedParents = selected === void 0 ? {} : parentRecords(allRecords, selected);
			const selectedParentMessage = selectedParents.message;
			const selectedParentTool = selectedParents.tool;
			const selectedAssistantRequest = selected?.cell.kind === "message" ? requestNumbers.get(requestKey(selected.turn, selected.group)) : void 0;
			const selectedAssistantRequestInfo = selectedAssistantRequest === void 0 ? void 0 : sessionRequestNumbers?.find((request) => request.number === selectedAssistantRequest);
			const selectedAssistantRequestTarget = selectedAssistantRequestInfo === void 0 ? void 0 : { identity: requestIdentity(selectedAssistantRequestInfo) };
			const hasSelectedHierarchy = selectedAssistantRequestTarget !== void 0 || selectedParents.message !== void 0 || selectedParents.tool !== void 0;
			const splitStyle = toolRequestOffset === null ? void 0 : { "--trajectory-tool-request-width": `calc(58cqw - ${toolRequestOffset}px)` };
			const activateTab = (tab) => {
				setCodeWrappingOnOpen(stringWrapping?.getDefault() ?? false);
				tabHistory.current.delete(tab);
				tabHistory.current.add(tab);
				setActiveTab(tab);
			};
			const clearInspectorSelection = () => {
				setSelectedRecordId(null);
				setSelectedRequest(null);
			};
			const clearAllSelections = () => {
				clearInspectorSelection();
				onClearSelection?.();
			};
			const selectRecord = (0, react.useCallback)((index) => {
				setCodeWrappingOnOpen(stringWrapping?.getDefault() ?? false);
				const record = allRecords.find((candidate) => candidate.cell.index === index);
				onRecordSelect?.(index);
				setSelectedRequest(null);
				setSelectedRecordId(record === void 0 ? null : trajectoryRecordId(record.cell));
				if (record === void 0) return;
				const tabs = detailTabs(record);
				const available = new Set(tabs.map((tab) => tab.id));
				setActiveTab([...tabHistory.current].reverse().find((tab) => available.has(tab)) ?? tabs[0]?.id ?? "overview");
			}, [
				allRecords,
				onRecordSelect,
				stringWrapping
			]);
			(0, react.useEffect)(() => {
				if (recordSelection === null || appliedRecordSelection.current === recordSelection) return;
				appliedRecordSelection.current = recordSelection;
				selectRecord(recordSelection.index);
				const record = allRecords.find((candidate) => candidate.cell.index === recordSelection.index);
				pendingScrollRecordId.current = record === void 0 ? null : trajectoryRecordId(record.cell);
			}, [
				allRecords,
				recordSelection,
				selectRecord
			]);
			(0, react.useEffect)(() => {
				if (recordFocus === null || appliedRecordFocus.current === recordFocus) return;
				appliedRecordFocus.current = recordFocus;
				const record = allRecords.find((candidate) => candidate.cell.index === recordFocus.index);
				pendingScrollRecordId.current = record === void 0 ? null : trajectoryRecordId(record.cell);
			}, [allRecords, recordFocus]);
			const selectRequest = (request, tab = "overview") => {
				setSelectedRecordId(null);
				setSelectedRequest(request);
				activateTab(tab);
			};
			const openRecordSummary = (target) => {
				const targetAt = allRecords.findIndex((record) => record.cell.index === target.cell.index);
				if (target.turn !== null && collapsedTurns.has(target.turn)) onToggleTurn(target.turn);
				if (target.cell.kind === "tool" || target.cell.kind === "subtool") for (let i = targetAt - 1; i >= 0; i--) {
					const candidate = allRecords[i];
					if (candidate === void 0 || candidate.turn !== target.turn) break;
					if (candidate.cell.kind !== "message") continue;
					const assistantId = trajectoryRecordId(candidate.cell);
					if (collapsedAssistants.has(assistantId)) onToggleAssistant(assistantId);
					break;
				}
				setSelectedRequest(null);
				setSelectedRecordId(trajectoryRecordId(target.cell));
				activateTab("overview");
			};
			const openCallSummary = (callId) => {
				const target = allRecords.find((record) => record.cell.callId === callId);
				if (target !== void 0) openRecordSummary(target);
			};
			const openRecordSummaryRef = (0, react.useRef)(openRecordSummary);
			openRecordSummaryRef.current = openRecordSummary;
			(0, react.useEffect)(() => {
				if (inspectCallId === null) return;
				const target = flattenRecords(turns).find((record) => record.cell.callId === inspectCallId);
				if (target === void 0) return;
				openRecordSummaryRef.current(target);
				pendingScrollRecordId.current = trajectoryRecordId(target.cell);
				onInspectApplied?.();
			}, [
				inspectCallId,
				turns,
				onInspectApplied
			]);
			(0, react.useEffect)(() => {
				const id = pendingScrollRecordId.current;
				if (id === null) return;
				const position = records.findIndex((record) => trajectoryRecordId(record.cell) === id && record.collapsedSummary === void 0);
				if (position === -1) return;
				if (virtualizationEnabled) {
					const virtualIndex = virtualIndexByRecordId.get(id);
					if (virtualIndex === void 0) return;
					pendingScrollRecordId.current = null;
					followsTableTail.current = false;
					rowVirtualizer.scrollToIndex(virtualIndex, {
						behavior: "smooth",
						align: "center"
					});
					return;
				}
				pendingScrollRecordId.current = null;
				followsTableTail.current = false;
				const recordIndex = records[position]?.cell.index;
				const row = recordIndex === void 0 ? null : rootRef.current?.querySelector(`tr[data-record-index="${recordIndex}"]`);
				/* v8 ignore next -- jsdom lacks scrollIntoView; browsers always have it. */
				if (row !== void 0 && row !== null && typeof row.scrollIntoView === "function") row.scrollIntoView({
					behavior: "smooth",
					block: "center"
				});
			}, [
				records,
				rowVirtualizer,
				virtualIndexByRecordId,
				virtualizationEnabled
			]);
			(0, react.useEffect)(() => {
				if (timelineFocusIndexes === null || timelineFocusIndexes.size === 0) return;
				const focusedPositions = records.flatMap((record, position) => record.collapsedSummary === void 0 && record.cell.requestOnly !== true && timelineFocusIndexes.has(record.cell.index) ? [position] : []);
				const first = focusedPositions.at(0);
				const last = focusedPositions.at(-1);
				if (first === void 0 || last === void 0) return;
				if (!virtualizationEnabled) {
					const ledger = rootRef.current;
					if (ledger === null) return;
					const focusedRows = [...ledger.querySelectorAll("tr[data-timeline-focus=\"inside\"]")];
					const firstRow = focusedRows.at(0);
					const lastRow = focusedRows.at(-1);
					if (firstRow === void 0 || lastRow === void 0) return;
					const focusHeight = lastRow.getBoundingClientRect().bottom - firstRow.getBoundingClientRect().top;
					const target = focusHeight > ledger.clientHeight ? firstRow : focusedRows[Math.floor((focusedRows.length - 1) / 2)];
					/* v8 ignore next -- jsdom lacks scrollIntoView; browsers always have it. */
					if (target !== void 0 && typeof target.scrollIntoView === "function") {
						followsTableTail.current = false;
						target.scrollIntoView({
							behavior: "smooth",
							block: focusHeight > ledger.clientHeight ? "start" : "center"
						});
					}
					return;
				}
				const focusedVirtualIndexes = [...new Set(focusedPositions.flatMap((position) => {
					const record = records[position];
					if (record === void 0) return [];
					const virtualIndex = virtualIndexByRecordId.get(trajectoryRecordId(record.cell));
					return virtualIndex === void 0 ? [] : [virtualIndex];
				}))].sort((left, right) => left - right);
				const firstVirtual = focusedVirtualIndexes.at(0);
				const lastVirtual = focusedVirtualIndexes.at(-1);
				if (firstVirtual === void 0 || lastVirtual === void 0) return;
				const paneHeight = tablePaneRef.current?.clientHeight ?? 0;
				const focusHeight = projectedVirtualRows.slice(firstVirtual, lastVirtual + 1).reduce((height, row) => height + row.height, 0);
				followsTableTail.current = false;
				rowVirtualizer.scrollToIndex(focusHeight > paneHeight ? firstVirtual : focusedVirtualIndexes[Math.floor((focusedVirtualIndexes.length - 1) / 2)] ?? firstVirtual, {
					behavior: "smooth",
					align: focusHeight > paneHeight ? "start" : "center"
				});
			}, [
				projectedVirtualRows,
				records,
				rowVirtualizer,
				timelineFocusIndexes,
				virtualIndexByRecordId,
				virtualizationEnabled
			]);
			const requestOlder = (0, react.useCallback)((pane, requireTop) => {
				if (!hasOlderRecords || onLoadOlder === void 0 || loadingOlder.current || olderHistoryLoading || requireTop && pane.scrollTop > OLDER_LOAD_THRESHOLD_PX) return;
				loadingOlder.current = true;
				setOlderLoading(true);
				olderLoadAnchor.current = {
					historyStartSeq,
					scrollHeight: pane.scrollHeight,
					scrollTop: pane.scrollTop
				};
				onLoadOlder().then((advanced) => {
					if (!advanced) olderLoadAnchor.current = null;
				}).finally(() => {
					loadingOlder.current = false;
					setOlderLoading(false);
				});
			}, [
				hasOlderRecords,
				historyStartSeq,
				olderHistoryLoading,
				onLoadOlder
			]);
			(0, react.useLayoutEffect)(() => {
				const pane = tablePaneRef.current;
				if (pane === null) return;
				const anchor = olderLoadAnchor.current;
				if (anchor !== null && anchor.historyStartSeq !== historyStartSeq) {
					if (!virtualizationEnabled) pane.scrollTop = anchor.scrollTop + pane.scrollHeight - anchor.scrollHeight;
					olderLoadAnchor.current = null;
					followsTableTail.current = false;
					return;
				}
				if (!tableScrollInitialized.current) {
					if (historyLoading) return;
					tableScrollInitialized.current = true;
					followsTableTail.current = true;
					if (virtualizationEnabled) rowVirtualizer.scrollToEnd({ behavior: "auto" });
					else pane.scrollTop = pane.scrollHeight;
					setTableScrollReady(true);
					return;
				}
				if (!followsTableTail.current) return;
				if (!virtualizationEnabled) pane.scrollTop = pane.scrollHeight;
			}, [
				historyLoading,
				historyStartSeq,
				rowVirtualizer,
				virtualRowStructure,
				virtualizationEnabled
			]);
			const olderBusy = olderHistoryLoading || olderLoading;
			const showInitialLoading = historyLoading || !tableScrollReady;
			const historyRowOffset = hasOlderRecords ? 1 : 0;
			return (0, react_jsx_runtime.jsxs)("div", {
				ref: rootRef,
				className: TrajectoryTable_module_css_default.split,
				style: splitStyle,
				children: [(0, react_jsx_runtime.jsxs)("div", {
					ref: tablePaneRef,
					className: TrajectoryTable_module_css_default.tablePane,
					"data-trajectory-scroll": "",
					onScroll: (event) => {
						const pane = event.currentTarget;
						followsTableTail.current = pane.scrollHeight - pane.clientHeight - pane.scrollTop <= BOTTOM_FOLLOW_THRESHOLD_PX;
						requestOlder(pane, true);
					},
					onClick: (event) => {
						if (event.target === event.currentTarget) clearAllSelections();
					},
					children: [showInitialLoading && (0, react_jsx_runtime.jsx)("div", {
						className: TrajectoryTable_module_css_default.historyLoading,
						role: "status",
						"aria-live": "polite",
						children: (0, react_jsx_runtime.jsxs)("span", {
							className: TrajectoryTable_module_css_default.historyLoadingBar,
							children: [(0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.StateDot, { state: "ongoing" }), t("history.loadingTrajectory")]
						})
					}), (0, react_jsx_runtime.jsxs)("table", {
						className: TrajectoryTable_module_css_default.table,
						"data-scroll-ready": tableScrollReady || void 0,
						"aria-rowcount": records.length + historyRowOffset,
						children: [(0, react_jsx_runtime.jsxs)("colgroup", { children: [(0, react_jsx_runtime.jsx)("col", { className: TrajectoryTable_module_css_default.eventColumn }), (0, react_jsx_runtime.jsx)("col", { className: TrajectoryTable_module_css_default.contentColumn })] }), (0, react_jsx_runtime.jsxs)("tbody", { children: [
							hasOlderRecords && (0, react_jsx_runtime.jsx)("tr", {
								className: TrajectoryTable_module_css_default.historyLoadRow,
								"data-history-load": "",
								"aria-rowindex": 1,
								children: (0, react_jsx_runtime.jsx)("td", {
									colSpan: 2,
									children: (0, react_jsx_runtime.jsxs)("button", {
										type: "button",
										className: TrajectoryTable_module_css_default.historyLoadButton,
										disabled: olderBusy || onLoadOlder === void 0,
										"aria-label": olderBusy ? t("history.loadingEarlierAria") : t("history.loadEarlier"),
										onClick: () => {
											const pane = tablePaneRef.current;
											if (pane !== null) requestOlder(pane, false);
										},
										children: [
											olderBusy && (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.StateDot, { state: "ongoing" }),
											(0, react_jsx_runtime.jsx)("span", {
												"aria-hidden": "true",
												children: olderBusy ? t("history.loadingEarlier") : t("history.loadEarlier")
											}),
											(0, react_jsx_runtime.jsx)("span", {
												className: TrajectoryTable_module_css_default.visuallyHidden,
												role: "status",
												"aria-live": "polite",
												children: olderBusy ? t("history.loadingEarlier") : ""
											})
										]
									})
								})
							}),
							virtualTop > 0 && (0, react_jsx_runtime.jsx)("tr", {
								className: TrajectoryTable_module_css_default.virtualSpacer,
								"data-virtual-spacer": "top",
								"aria-hidden": "true",
								children: (0, react_jsx_runtime.jsx)("td", {
									colSpan: 2,
									style: { "--trajectory-virtual-spacer-height": `${virtualTop}px` }
								})
							}),
							renderedRecords.map(({ record, position, terminalRequestBoundary }) => (0, react_jsx_runtime.jsx)(RecordPresentation, {
								cell: record.cell,
								t,
								children: ({ displayText, listDisplayText, resultText, toolCallOnly, toolCallText }) => {
									const isCollapsedSummary = record.collapsedSummary !== void 0;
									const isRequestOnly = record.cell.requestOnly === true;
									const isInitialSystem = record.cell.kind === "system" && record.cell.index === allRecords[0]?.cell.index;
									const key = requestKey(record.turn, record.group);
									const request = requestBoundaries.get(key) === record.cell.index && !isCollapsedSummary && (record.turn === null || !collapsedTurns.has(record.turn)) ? requestNumbers.get(key) : void 0;
									const requestInfo = request === void 0 ? void 0 : sessionRequestNumbers?.find((candidate) => candidate.number === request);
									const requestStatus = requestInfo?.status ?? (record.cell.isError === true ? "error" : void 0);
									const requestRunIndex = requestBoundaryRuns.get(record.cell.index) ?? 0;
									const requestBoundaryStyle = { "--request-boundary-offset": `${requestRunIndex * 8}px` };
									const requestLabel = request === void 0 ? void 0 : t(requestInfo?.purpose === "compaction" ? "request.labelCompaction" : "request.label", { request });
									const requestSelected = requestInfo !== void 0 && selectedRequest?.identity === requestIdentity(requestInfo);
									const sectionActive = record.turn === null ? activeSection === record.section : activeTurn === record.turn;
									const singleToolNotice = record.cell.kind === "context" && record.cell.sourceBlocks?.length === 1 && record.cell.sourceBlocks.every((block) => block.type === "tool-addition" || block.type === "tool-removal");
									return (0, react_jsx_runtime.jsxs)("tr", {
										tabIndex: isRequestOnly || singleToolNotice ? -1 : 0,
										"aria-rowindex": position + 1 + historyRowOffset,
										"aria-label": isCollapsedSummary ? t("request.collapsedSummary", {
											kind: t(record.collapsedSummaryKind === "turn" ? "request.collapsedTurn" : "request.collapsedAssistant"),
											summary: record.collapsedSummary
										}) : isRequestOnly ? t("request.rowAriaCompaction", { request: request ?? "" }) : t("request.rowAria", {
											request: request === void 0 ? "" : t("request.rowPrefix", { request }),
											kind: t(KIND_LABEL_KEY[record.cell.kind]),
											content: listDisplayText || t("request.noContent")
										}),
										"aria-selected": !isCollapsedSummary && !isRequestOnly && selectedIndex === record.cell.index,
										"data-kind": record.cell.kind,
										"data-trajectory-row-key": trajectoryVirtualRecordKey(record),
										"data-virtual-position": virtualizationEnabled ? position : void 0,
										"data-record-index": !isCollapsedSummary && !isRequestOnly ? record.cell.index : void 0,
										"data-request-only": isRequestOnly || void 0,
										"data-terminal-request-boundary": terminalRequestBoundary || void 0,
										"data-group-start": record.groupStart || void 0,
										"data-turn-start": record.turnStart || void 0,
										"data-error": record.cell.isError || void 0,
										"data-running": stateOf(record) === "running" || void 0,
										"data-turn-end": record.turnEnd || void 0,
										"data-collapsed-summary": record.collapsedSummaryKind,
										"data-selected": !isCollapsedSummary && selectedIndex === record.cell.index || void 0,
										"data-timeline-focus": isCollapsedSummary || timelineFocusIndexes === null ? void 0 : timelineFocusIndexes.has(record.cell.index) ? "inside" : "outside",
										onClick: isRequestOnly || singleToolNotice ? void 0 : isCollapsedSummary ? () => {
											if (record.collapsedSummaryKind === "turn" && record.turn !== null) onToggleTurn(record.turn);
											else onToggleAssistant(trajectoryRecordId(record.cell));
										} : () => {
											selectRecord(record.cell.index);
										},
										onDoubleClick: (event) => {
											if (isCollapsedSummary || isRequestOnly || singleToolNotice) return;
											if (record.turn !== null && collapsedTurns.has(record.turn)) {
												event.preventDefault();
												onToggleTurn(record.turn);
												return;
											}
											if (record.cell.kind === "message" && assistantToolCalls(allRecords, record.cell.index).length > 0) {
												event.preventDefault();
												onToggleAssistant(trajectoryRecordId(record.cell));
												return;
											}
											if (!record.turnStart) return;
											if (record.turn === null) return;
											if (allRecords.filter((candidate) => candidate.turn === record.turn && candidate.cell.requestOnly !== true && candidate.cell.kind !== "system").length <= 1) return;
											event.preventDefault();
											onToggleTurn(record.turn);
										},
										onKeyDown: (event) => {
											if (isRequestOnly || singleToolNotice) return;
											if (event.key !== "Enter" && event.key !== " ") return;
											event.preventDefault();
											if (isCollapsedSummary) {
												if (record.collapsedSummaryKind === "turn" && record.turn !== null) onToggleTurn(record.turn);
												else onToggleAssistant(trajectoryRecordId(record.cell));
												return;
											}
											selectRecord(record.cell.index);
										},
										children: [(0, react_jsx_runtime.jsxs)("td", {
											className: TrajectoryTable_module_css_default.event,
											children: [
												request !== void 0 && (0, react_jsx_runtime.jsx)("button", {
													type: "button",
													className: requestSelected ? `${TrajectoryTable_module_css_default.requestBoundaryControl} ${TrajectoryTable_module_css_default.requestBoundaryControlActive}` : TrajectoryTable_module_css_default.requestBoundaryControl,
													"aria-label": requestLabel,
													"aria-pressed": requestSelected,
													"data-label": requestLabel,
													"data-request-run-index": requestRunIndex,
													"data-request-status": requestStatus,
													style: requestBoundaryStyle,
													onClick: (event) => {
														event.stopPropagation();
														if (requestInfo !== void 0) selectRequest({ identity: requestIdentity(requestInfo) });
													},
													onDoubleClick: (event) => {
														event.stopPropagation();
													}
												}),
												record.turn !== null && activeTurn === record.turn && !isInitialSystem && (0, react_jsx_runtime.jsx)("span", {
													className: TrajectoryTable_module_css_default.turnRail,
													"aria-hidden": "true"
												}),
												!isCollapsedSummary && selectedIndex === record.cell.index && (0, react_jsx_runtime.jsx)("span", {
													className: TrajectoryTable_module_css_default.selectionRail,
													"aria-hidden": "true"
												}),
												!isCollapsedSummary && !isRequestOnly && record.turnStart && (0, react_jsx_runtime.jsx)("span", {
													className: sectionActive ? `${TrajectoryTable_module_css_default.turnLabel} ${TrajectoryTable_module_css_default.turnLabelActive}` : TrajectoryTable_module_css_default.turnLabel,
													"aria-label": sectionLabel(record.turn, t),
													children: record.turn === null ? sectionLabel(record.turn, t) : (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [(0, react_jsx_runtime.jsx)("span", {
														className: TrajectoryTable_module_css_default.turnLabelFull,
														"aria-hidden": "true",
														children: sectionLabel(record.turn, t)
													}), (0, react_jsx_runtime.jsxs)("span", {
														className: TrajectoryTable_module_css_default.turnLabelCompact,
														"aria-hidden": "true",
														children: ["#", record.turn]
													})] })
												}),
												(0, react_jsx_runtime.jsx)("div", {
													className: TrajectoryTable_module_css_default.eventInner,
													children: !isCollapsedSummary && !isRequestOnly && (0, react_jsx_runtime.jsx)("span", {
														className: TrajectoryTable_module_css_default.kindSlot,
														children: record.cell.rthKind !== void 0 ? RTH.kindTag(record.cell.rthKind, TrajectoryTable_module_css_default, _deepseek_ai_dsh_client_ui_primitives) : (0, react_jsx_runtime.jsxs)("span", {
															className: `${TrajectoryTable_module_css_default.kindTag} ${record.cell.kind === "system" ? TrajectoryTable_module_css_default.systemNeutral : record.cell.kind === "context" ? TrajectoryTable_module_css_default.contextGreen : record.cell.kind === "compacted" ? TrajectoryTable_module_css_default.compacted : record.cell.kind === "tool" ? TrajectoryTable_module_css_default.toolAmber : record.cell.kind === "message" ? TrajectoryTable_module_css_default.assistantVioletBright : record.cell.kind === "subtool" ? TrajectoryTable_module_css_default.subtoolAmber : TrajectoryTable_module_css_default[record.cell.kind]}`,
															"data-role-kind": record.cell.kind,
															children: [(0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.Tooltip, {
																label: t(KIND_LABEL_KEY[record.cell.kind]),
																side: "right",
																children: (0, react_jsx_runtime.jsx)("span", {
																	className: TrajectoryTable_module_css_default.kindTagIcon,
																	"aria-hidden": "true",
																	children: KIND_ICON[record.cell.kind]
																})
															}), (0, react_jsx_runtime.jsx)("span", {
																className: TrajectoryTable_module_css_default.kindTagLabel,
																children: t(KIND_LABEL_KEY[record.cell.kind])
															})]
														})
													})
												})
											]
										}), (0, react_jsx_runtime.jsx)("td", {
											className: TrajectoryTable_module_css_default.content,
											children: isRequestOnly ? null : record.collapsedSummary !== void 0 ? (0, react_jsx_runtime.jsxs)("span", {
												className: TrajectoryTable_module_css_default.collapsedTurnContent,
												title: record.collapsedSummary,
												children: [(0, react_jsx_runtime.jsx)("span", {
													className: TrajectoryTable_module_css_default.collapsedTurnEllipsis,
													children: "…"
												}), (0, react_jsx_runtime.jsx)("span", {
													className: TrajectoryTable_module_css_default.collapsedTurnText,
													children: record.collapsedSummary
												})]
											}) : (0, react_jsx_runtime.jsxs)("span", {
												className: resultText === void 0 ? TrajectoryTable_module_css_default.contentText : TrajectoryTable_module_css_default.resultPreview,
												title: resultText === void 0 ? listDisplayText : `${listDisplayText} → ${resultText}`,
												children: [(0, react_jsx_runtime.jsx)("span", {
													className: resultText === void 0 ? void 0 : TrajectoryTable_module_css_default.resultRequest,
													children: (0, react_jsx_runtime.jsx)(RecordListText, {
														displayText,
														toolCallOnly,
														toolCallText,
														t
													})
												}), resultText !== void 0 && (0, react_jsx_runtime.jsxs)("span", {
													className: record.cell.isError ? `${TrajectoryTable_module_css_default.inlineResult} ${TrajectoryTable_module_css_default.error}` : TrajectoryTable_module_css_default.inlineResult,
													children: [(0, react_jsx_runtime.jsx)("span", {
														className: TrajectoryTable_module_css_default.arrow,
														children: "→"
													}), (0, react_jsx_runtime.jsx)("span", {
														className: resultText === t("record.noOutput") ? `${TrajectoryTable_module_css_default.inlineResultText} ${TrajectoryTable_module_css_default.noOutputText}` : TrajectoryTable_module_css_default.inlineResultText,
														children: resultText
													})]
												}), record.turnStart === true && record.cell.rthKind !== void 0 ? RTH.cardChips(record.turn) : null]
											})
										})]
									});
								}
							}, trajectoryVirtualRecordKey(record))),
							virtualBottom > 0 && (0, react_jsx_runtime.jsx)("tr", {
								className: TrajectoryTable_module_css_default.virtualSpacer,
								"data-virtual-spacer": "bottom",
								"aria-hidden": "true",
								children: (0, react_jsx_runtime.jsx)("td", {
									colSpan: 2,
									style: { "--trajectory-virtual-spacer-height": `${virtualBottom}px` }
								})
							})
						] })]
					})]
				}), (selectedRequestInfo !== void 0 || promptSelected || selected !== void 0 && selectedState !== void 0) && (0, react_jsx_runtime.jsxs)("aside", {
					className: TrajectoryTable_module_css_default.details,
					"aria-label": t("details.event"),
					style: detailsWidth === null ? void 0 : { width: detailsWidth },
					children: [
						(0, react_jsx_runtime.jsx)("div", {
							className: TrajectoryTable_module_css_default.detailsResizeHandle,
							role: "separator",
							"aria-label": t("details.resize"),
							"aria-controls": "trajectory-detail-panel",
							"aria-orientation": "vertical",
							tabIndex: 0,
							title: t("details.resizeTitle"),
							onDoubleClick: () => {
								setDetailsWidth(null);
								setToolRequestOffset(null);
							},
							onPointerDown: (event) => {
								if (event.button !== 0) return;
								const details = event.currentTarget.parentElement;
								if (details === null) return;
								const split = details.parentElement;
								if (split === null) return;
								const splitWidth = split.getBoundingClientRect().width;
								detailsResizeDrag.current = {
									pointerId: event.pointerId,
									startX: event.clientX,
									startWidth: details.getBoundingClientRect().width,
									splitWidth,
									startToolRequestOffset: toolRequestOffset ?? splitWidth * TOOL_REQUEST_SHARE - defaultToolRequestWidth(splitWidth)
								};
								event.currentTarget.setPointerCapture(event.pointerId);
								event.preventDefault();
							},
							onPointerMove: (event) => {
								const drag = detailsResizeDrag.current;
								if (drag === null || drag.pointerId !== event.pointerId) return;
								const nextDetailsWidth = clampDetailsWidth(drag.startWidth + drag.startX - event.clientX, drag.splitWidth);
								setDetailsWidth(nextDetailsWidth);
								setToolRequestOffset(drag.startToolRequestOffset + (nextDetailsWidth - drag.startWidth) * TOOL_REQUEST_SHARE);
							},
							onPointerUp: (event) => {
								if (detailsResizeDrag.current?.pointerId !== event.pointerId) return;
								detailsResizeDrag.current = null;
								event.currentTarget.releasePointerCapture(event.pointerId);
							},
							onPointerCancel: () => {
								detailsResizeDrag.current = null;
							},
							onKeyDown: (event) => {
								if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
								const details = event.currentTarget.parentElement;
								if (details === null) return;
								const split = details.parentElement;
								if (split === null) return;
								const direction = event.key === "ArrowLeft" ? 1 : -1;
								const currentDetailsWidth = details.getBoundingClientRect().width;
								const splitWidth = split.getBoundingClientRect().width;
								const nextDetailsWidth = clampDetailsWidth(currentDetailsWidth + direction * DETAILS_RESIZE_STEP, splitWidth);
								const currentToolRequestOffset = toolRequestOffset ?? splitWidth * TOOL_REQUEST_SHARE - defaultToolRequestWidth(splitWidth);
								setDetailsWidth(nextDetailsWidth);
								setToolRequestOffset(currentToolRequestOffset + (nextDetailsWidth - currentDetailsWidth) * TOOL_REQUEST_SHARE);
								event.preventDefault();
							}
						}),
						(0, react_jsx_runtime.jsxs)("div", {
							className: TrajectoryTable_module_css_default.detailsHeader,
							children: [(0, react_jsx_runtime.jsx)("div", {
								className: TrajectoryTable_module_css_default.detailsTitle,
								children: selectedRequestInfo !== void 0 ? (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [
									(0, react_jsx_runtime.jsx)("span", {
										className: TrajectoryTable_module_css_default.requestDetailsDot,
										"aria-hidden": "true"
									}),
									(0, react_jsx_runtime.jsx)("span", {
										className: TrajectoryTable_module_css_default.requestDetailsName,
										children: t("request.label", { request: selectedRequestNumber ?? "—" })
									}),
									(0, react_jsx_runtime.jsx)("span", {
										className: TrajectoryTable_module_css_default.detailsLocation,
										children: selectedRequestInfo.purpose === "compaction" ? t("request.compaction", { section: sectionLabel(selectedRequestInfo.turn, t) }) : sectionLabel(selectedRequestInfo.turn, t)
									})
								] }) : promptSelected ? (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [(0, react_jsx_runtime.jsx)("span", {
									className: `${TrajectoryTable_module_css_default.kindTag} ${TrajectoryTable_module_css_default.systemNeutral}`,
									children: t("kind.system")
								}), (0, react_jsx_runtime.jsx)("span", {
									className: TrajectoryTable_module_css_default.detailsLocation,
									children: selected?.cell.text
								})] }) : selected !== void 0 && (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [selected.cell.rthKind !== void 0 ? RTH.kindTag(selected.cell.rthKind, TrajectoryTable_module_css_default, _deepseek_ai_dsh_client_ui_primitives) : (0, react_jsx_runtime.jsx)("span", {
									className: `${TrajectoryTable_module_css_default.kindTag} ${selected.cell.kind === "context" ? TrajectoryTable_module_css_default.contextGreen : selected.cell.kind === "compacted" ? TrajectoryTable_module_css_default.compacted : selected.cell.kind === "tool" ? TrajectoryTable_module_css_default.toolAmber : selected.cell.kind === "message" ? TrajectoryTable_module_css_default.assistantVioletBright : selected.cell.kind === "subtool" ? TrajectoryTable_module_css_default.subtoolAmber : TrajectoryTable_module_css_default[selected.cell.kind]}`,
									children: t(KIND_LABEL_KEY[selected.cell.kind])
								}), (0, react_jsx_runtime.jsx)("span", {
									className: TrajectoryTable_module_css_default.detailsLocation,
									children: selected.cell.kind === "compacted" ? sectionLabel(selected.turn, t) : `${sectionLabel(selected.turn, t)} · ${selected.group}`
								})] })
							}), (0, react_jsx_runtime.jsx)("button", {
								type: "button",
								className: TrajectoryTable_module_css_default.close,
								"aria-label": t("details.close"),
								onClick: clearInspectorSelection,
								children: (0, react_jsx_runtime.jsx)("span", {
									"aria-hidden": "true",
									children: "×"
								})
							})]
						}),
						(0, react_jsx_runtime.jsx)("div", {
							className: TrajectoryTable_module_css_default.detailTabs,
							role: "tablist",
							"aria-label": t("details.event"),
							children: selectedTabs.map((tab) => (0, react_jsx_runtime.jsx)("button", {
								id: `trajectory-detail-${tab.id}`,
								type: "button",
								role: "tab",
								"aria-controls": "trajectory-detail-panel",
								"aria-selected": activeTab === tab.id,
								className: activeTab === tab.id ? `${TrajectoryTable_module_css_default.detailTab} ${TrajectoryTable_module_css_default.detailTabActive}` : TrajectoryTable_module_css_default.detailTab,
								onClick: () => {
									activateTab(tab.id);
								},
								children: t(tab.labelKey)
							}, tab.id))
						}),
						(0, react_jsx_runtime.jsxs)("div", {
							id: "trajectory-detail-panel",
							className: activeTab === "overview" ? `${TrajectoryTable_module_css_default.detailBody} ${TrajectoryTable_module_css_default.detailBodySummary}` : selectedProgram !== void 0 && (activeTab === "input" || activeTab === "output") ? `${TrajectoryTable_module_css_default.detailBody} ${TrajectoryTable_module_css_default.detailBodyProgram}` : TrajectoryTable_module_css_default.detailBody,
							role: "tabpanel",
							"aria-labelledby": `trajectory-detail-${activeTab}`,
							children: [
								selectedRequestInfo !== void 0 && selectedRequestState !== void 0 && activeTab === "overview" && (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [(0, react_jsx_runtime.jsxs)("dl", {
									className: `${TrajectoryTable_module_css_default.overview} ${TrajectoryTable_module_css_default.summaryScrollRegion}`,
									"data-summary-scroll-region": "",
									children: [
										(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.status") }), (0, react_jsx_runtime.jsx)("dd", {
											className: selectedRequestState === "error" ? TrajectoryTable_module_css_default.error : void 0,
											children: statusLabel(selectedRequestState, t)
										})] }),
										selectedRequestInfo.purpose === "compaction" && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.purpose") }), (0, react_jsx_runtime.jsx)("dd", { children: t("request.compactionPurpose") })] }),
										(selectedRequestInfo.provider ?? selectedRequestInfo.requestConfig?.provider) !== void 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.provider") }), (0, react_jsx_runtime.jsx)("dd", { children: selectedRequestInfo.provider ?? selectedRequestInfo.requestConfig?.provider })] }),
										(selectedRequestInfo.model ?? selectedRequestInfo.requestConfig?.model) !== void 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.model") }), (0, react_jsx_runtime.jsx)("dd", { children: selectedRequestInfo.model ?? selectedRequestInfo.requestConfig?.model })] }),
										(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.toolCalls") }), (0, react_jsx_runtime.jsx)("dd", { children: selectedRequestToolCalls })] }),
										selectedRequestSubtoolCalls > 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.subtoolCalls") }), (0, react_jsx_runtime.jsx)("dd", { children: selectedRequestSubtoolCalls })] }),
										selectedRequestInfo.error !== void 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t(selectedRequestState !== "error" && selectedRequestInfo.retry !== void 0 ? "details.lastAttemptError" : "details.error") }), (0, react_jsx_runtime.jsx)("dd", {
											className: selectedRequestState === "error" ? TrajectoryTable_module_css_default.error : void 0,
											children: requestErrorMessage(selectedRequestInfo, t)
										})] }),
										selectedRequestInfo.retry !== void 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.retry") }), (0, react_jsx_runtime.jsx)("dd", { children: selectedRequestInfo.maxRetries === void 0 ? selectedRequestInfo.retry : t("request.retryProgress", {
											retry: selectedRequestInfo.retry,
											maximum: selectedRequestInfo.maxRetries
										}) })] }),
										selectedRequestInfo.retryDelayMs !== void 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.retryDelay") }), (0, react_jsx_runtime.jsx)("dd", { children: formatDurationMs(selectedRequestInfo.retryDelayMs, t) })] }),
										selectedRequestResult !== void 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.result") }), (0, react_jsx_runtime.jsx)("dd", {
											className: TrajectoryTable_module_css_default.overviewParentLinks,
											children: (0, react_jsx_runtime.jsxs)("button", {
												type: "button",
												className: TrajectoryTable_module_css_default.overviewHierarchyNavLink,
												onClick: () => {
													openRecordSummary(selectedRequestResult);
												},
												children: [(0, react_jsx_runtime.jsx)("span", { children: selectedRequestInfo.purpose === "compaction" ? t("details.compacted") : t("details.assistantMessage") }), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconChevronRightOutlineRegular, {
													className: TrajectoryTable_module_css_default.overviewHierarchyJumpIconTight,
													size: 11
												})]
											})
										})] })
									]
								}), (0, react_jsx_runtime.jsxs)("div", {
									className: TrajectoryTable_module_css_default.overviewSections,
									children: [
										selectedRequestOptions !== void 0 && (0, react_jsx_runtime.jsx)(OverviewSection, {
											label: t("tab.options"),
											onOpen: () => {
												activateTab("options");
											},
											children: (0, react_jsx_runtime.jsx)(RequestOptions, {
												options: selectedRequestOptions,
												preview: true,
												stringWrapping,
												t
											})
										}),
										(0, react_jsx_runtime.jsx)(OverviewSection, {
											label: t("tab.usage"),
											onOpen: () => {
												activateTab("usage");
											},
											children: (0, react_jsx_runtime.jsx)(UsageRows, {
												usage: selectedRequestUsage,
												t
											})
										}),
										(0, react_jsx_runtime.jsx)(OverviewSection, {
											label: t("tab.timing"),
											onOpen: () => {
												activateTab("timing");
											},
											children: (0, react_jsx_runtime.jsx)(RequestTiming, {
												assistant: selectedRequestAssistant,
												anchor: selectedRequestAnchor,
												request: selectedRequestInfo,
												preview: true,
												t
											})
										})
									]
								})] }),
								selectedRequestInfo !== void 0 && activeTab === "options" && (0, react_jsx_runtime.jsx)(RequestOptions, {
									options: selectedRequestOptions,
									stringWrapping,
									t
								}),
								selectedRequestInfo !== void 0 && activeTab === "usage" && (0, react_jsx_runtime.jsx)(RequestUsagePanel, {
									usage: selectedRequestUsage,
									cumulative: selectedRequestCumulativeUsage,
									t
								}),
								selectedRequestInfo !== void 0 && activeTab === "timing" && (0, react_jsx_runtime.jsx)(RequestTiming, {
									assistant: selectedRequestAssistant,
									anchor: selectedRequestAnchor,
									request: selectedRequestInfo,
									t
								}),
								selectedPrompt !== void 0 && selectedPreviousPrompt !== void 0 && activeTab === "diff" && (0, react_jsx_runtime.jsx)(SystemPromptDiff, {
									before: selectedPreviousPrompt,
									after: selectedPrompt,
									t
								}),
								promptSelected && activeTab === "system-prompt" && (selectedSystemPrompt === "" ? (0, react_jsx_runtime.jsx)("p", {
									className: TrajectoryTable_module_css_default.noPayload,
									children: t("record.systemPromptMissing")
								}) : (0, react_jsx_runtime.jsx)("div", {
									className: `${TrajectoryTable_module_css_default.markdownPayload} ${TrajectoryTable_module_css_default.systemPrompt}`,
									children: (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.MarkdownText, {
										text: selectedSystemPrompt,
										labels: markdownLabels(t)
									})
								})),
								selectedPrompt !== void 0 && activeTab === "tools" && (0, react_jsx_runtime.jsx)(ToolCatalog, {
									tools: selectedPrompt.tools,
									stringWrapping,
									t
								}),
								!promptSelected && selected?.cell.kind === "compacted" && selectedState !== void 0 && activeTab === "overview" && (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [(0, react_jsx_runtime.jsxs)("dl", {
									className: `${TrajectoryTable_module_css_default.overview} ${TrajectoryTable_module_css_default.summaryScrollRegion}`,
									"data-summary-scroll-region": "",
									children: [
										(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.status") }), (0, react_jsx_runtime.jsx)("dd", {
											className: selectedState === "error" ? TrajectoryTable_module_css_default.error : void 0,
											children: statusLabel(selectedState, t)
										})] }),
										(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.duration") }), (0, react_jsx_runtime.jsx)("dd", { children: formatElapsedSeconds(selected.cell.timeSeconds, t) })] }),
										(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("usage.tokens") }), (0, react_jsx_runtime.jsx)("dd", { children: "—" })] })
									]
								}), selected.cell.outputDetail !== void 0 && (0, react_jsx_runtime.jsx)("div", {
									className: `${TrajectoryTable_module_css_default.compactedSummary} ${TrajectoryTable_module_css_default.summaryScrollRegion}`,
									"data-summary-scroll-region": "",
									children: (0, react_jsx_runtime.jsx)(MarkdownRecordContent, {
										record: selected,
										renderSlot,
										loadImage,
										rendered: true,
										thinkingExpanded,
										onThinkingExpandedChange: setThinkingExpanded,
										onOpenCall: openCallSummary,
										t
									})
								})] }),
								!promptSelected && selected !== void 0 && selected.cell.kind !== "compacted" && selectedState !== void 0 && activeTab === "overview" && (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [
									selectedProgram !== void 0 && selectedProgram.description !== "" && (0, react_jsx_runtime.jsx)("p", {
										className: TrajectoryTable_module_css_default.programDescription,
										children: selectedProgram.description
									}),
									(0, react_jsx_runtime.jsxs)("dl", {
										className: `${TrajectoryTable_module_css_default.overview} ${TrajectoryTable_module_css_default.summaryScrollRegion}`,
										"data-summary-scroll-region": "",
										children: [
											selected.cell.messageSource !== void 0 && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.source") }), (0, react_jsx_runtime.jsx)("dd", {
												className: TrajectoryTable_module_css_default.overviewParentLinks,
												children: (0, react_jsx_runtime.jsxs)("button", {
													type: "button",
													className: TrajectoryTable_module_css_default.overviewHierarchyNavLink,
													onClick: () => {
														activateTab("source");
													},
													children: [(0, react_jsx_runtime.jsx)("span", { children: messageSourceLabel(selected.cell.messageSource, t) }), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconChevronRightOutlineRegular, {
														className: TrajectoryTable_module_css_default.overviewHierarchyJumpIconTight,
														size: 11
													})]
												})
											})] }),
											hasSelectedHierarchy && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: selectedAssistantRequestTarget !== void 0 ? t("details.source") : t("details.hierarchy") }), (0, react_jsx_runtime.jsxs)("dd", {
												className: TrajectoryTable_module_css_default.overviewParentLinks,
												children: [
													selectedAssistantRequestTarget !== void 0 && (0, react_jsx_runtime.jsxs)("button", {
														type: "button",
														className: TrajectoryTable_module_css_default.overviewHierarchyNavLink,
														onClick: () => {
															selectRequest(selectedAssistantRequestTarget);
														},
														children: [(0, react_jsx_runtime.jsx)("span", { children: t("request.label", { request: selectedAssistantRequest ?? "—" }) }), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconChevronRightOutlineRegular, {
															className: TrajectoryTable_module_css_default.overviewHierarchyJumpIconTight,
															size: 11
														})]
													}),
													selectedParentMessage !== void 0 && (0, react_jsx_runtime.jsxs)("button", {
														type: "button",
														className: TrajectoryTable_module_css_default.overviewHierarchyNavLink,
														onClick: () => {
															openRecordSummary(selectedParentMessage);
														},
														children: [(0, react_jsx_runtime.jsx)("span", { children: t("details.assistantMessage") }), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconChevronRightOutlineRegular, {
															className: TrajectoryTable_module_css_default.overviewHierarchyJumpIconTight,
															size: 11
														})]
													}),
													selectedParentTool !== void 0 && (0, react_jsx_runtime.jsxs)("button", {
														type: "button",
														className: TrajectoryTable_module_css_default.overviewHierarchyNavLink,
														onClick: () => {
															openRecordSummary(selectedParentTool);
														},
														children: [(0, react_jsx_runtime.jsx)("span", { children: t("details.toolCall") }), (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconChevronRightOutlineRegular, {
															className: TrajectoryTable_module_css_default.overviewHierarchyJumpIconTight,
															size: 11
														})]
													})
												]
											})] }),
											(0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("details.status") }), (0, react_jsx_runtime.jsx)("dd", {
												className: selectedState === "error" ? TrajectoryTable_module_css_default.error : void 0,
												children: statusLabel(selectedState, t)
											})] }),
											selected.cell.rthKind === void 0 && selected.cell.kind === "message" && (0, react_jsx_runtime.jsx)(TokenRows, {
												cell: selected.cell,
												t
											}),
											selected.cell.rthKind === void 0 && (selected.cell.kind === "user" || selected.cell.kind === "context") && (0, react_jsx_runtime.jsxs)("div", { children: [(0, react_jsx_runtime.jsx)("dt", { children: t("timing.duration") }), (0, react_jsx_runtime.jsx)("dd", { children: formatElapsedSeconds(selected.cell.timeSeconds, t) })] }),
											...(selected.cell.rthKind !== void 0 ? RTH.inspectorRows(selected.cell, react_jsx_runtime) : [])
										]
									}),
									(0, react_jsx_runtime.jsxs)("div", {
										className: TrajectoryTable_module_css_default.overviewSections,
										children: [
											selectedProgram !== void 0 ? (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [(0, react_jsx_runtime.jsx)(ProgramInput, {
												program: selectedProgram,
												initialWrapped: codeWrappingOnOpen,
												stringWrapping,
												onOpen: () => {
													activateTab("input");
												},
												t
											}, `preview:${selectedRecordId}`), (0, react_jsx_runtime.jsx)(ProgramOutput, {
												record: selected,
												stringWrapping,
												onOpen: () => {
													activateTab("output");
												},
												t
											})] }) : isMarkdownRecord(selected) ? (0, react_jsx_runtime.jsx)(react_jsx_runtime.Fragment, { children: (0, react_jsx_runtime.jsx)(OverviewSection, {
												label: t("tab.preview"),
												onOpen: () => {
													activateTab("rendered");
												},
												children: (0, react_jsx_runtime.jsx)(MarkdownRecordContent, {
													record: selected,
													renderSlot,
													loadImage,
													rendered: true,
													preview: true,
													thinkingExpanded,
													onThinkingExpandedChange: setThinkingExpanded,
													onOpenCall: openCallSummary,
													t
												})
											}) }) : (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [
												selected.cell.inputDetail && (0, react_jsx_runtime.jsx)(OverviewSection, {
													label: t("tab.payload"),
													onOpen: () => {
														activateTab("input");
													},
													children: (0, react_jsx_runtime.jsx)(RecordPayload, {
														record: selected,
														direction: "input",
														preview: true,
														renderSlot,
														loadImage,
														stringWrapping,
														t
													})
												}),
												selected.cell.outputDetail && (0, react_jsx_runtime.jsx)(OverviewSection, {
													label: t("tab.result"),
													onOpen: () => {
														activateTab("output");
													},
													children: (0, react_jsx_runtime.jsx)(RecordPayload, {
														record: selected,
														direction: "output",
														preview: true,
														renderSlot,
														loadImage,
														stringWrapping,
														t
													})
												}),
												(0, react_jsx_runtime.jsx)(OverviewSection, {
													label: t("tab.schema"),
													onOpen: () => {
														activateTab("schema");
													},
													children: (0, react_jsx_runtime.jsx)(RecordSchema, {
														record: selected,
														preview: true,
														stringWrapping,
														t
													})
												})
											] }),
											selectedAssistantRequestTarget !== void 0 && (0, react_jsx_runtime.jsx)(OverviewSection, {
												label: t("timing.request"),
												onOpen: () => {
													selectRequest(selectedAssistantRequestTarget, "timing");
												},
												children: (0, react_jsx_runtime.jsx)(RecordTiming, {
													record: selected,
													preview: true,
													t
												})
											}),
											(selected.cell.kind === "tool" || selected.cell.kind === "subtool") && (0, react_jsx_runtime.jsx)(OverviewSection, {
												label: t("tab.timing"),
												onOpen: () => {
													activateTab("timing");
												},
												children: (0, react_jsx_runtime.jsx)(RecordTiming, {
													record: selected,
													preview: true,
													t
												})
											})
										]
									})
								] }),
								!promptSelected && selected !== void 0 && activeTab === "rendered" && (0, react_jsx_runtime.jsx)(MarkdownRecordContent, {
									record: selected,
									renderSlot,
									loadImage,
									rendered: true,
									thinkingExpanded,
									onThinkingExpandedChange: setThinkingExpanded,
									onOpenCall: openCallSummary,
									t
								}),
								!promptSelected && selected !== void 0 && activeTab === "raw" && (0, react_jsx_runtime.jsx)(MarkdownRecordContent, {
									record: selected,
									renderSlot,
									loadImage,
									rendered: false,
									thinkingExpanded,
									onThinkingExpandedChange: setThinkingExpanded,
									onOpenCall: openCallSummary,
									t
								}),
								!promptSelected && selected !== void 0 && activeTab === "source" && (0, react_jsx_runtime.jsx)(MessageSource, {
									record: selected,
									stringWrapping,
									t
								}),
								!promptSelected && selected !== void 0 && activeTab === "input" && (selectedProgram === void 0 ? (0, react_jsx_runtime.jsx)(RecordPayload, {
									record: selected,
									direction: "input",
									renderSlot,
									loadImage,
									stringWrapping,
									t
								}) : (0, react_jsx_runtime.jsx)(ProgramInput, {
									program: selectedProgram,
									initialWrapped: codeWrappingOnOpen,
									stringWrapping,
									t
								}, `input:${selectedRecordId}`)),
								!promptSelected && selected !== void 0 && activeTab === "output" && (selectedProgram === void 0 ? (0, react_jsx_runtime.jsx)(RecordPayload, {
									record: selected,
									direction: "output",
									renderSlot,
									loadImage,
									stringWrapping,
									t
								}) : (0, react_jsx_runtime.jsx)(ProgramOutput, {
									record: selected,
									stringWrapping,
									t
								})),
								!promptSelected && selected !== void 0 && activeTab === "schema" && (0, react_jsx_runtime.jsx)(RecordSchema, {
									record: selected,
									stringWrapping,
									t
								}),
								!promptSelected && selected !== void 0 && activeTab === "timing" && (0, react_jsx_runtime.jsx)(RecordTiming, {
									record: selected,
									t
								})
							]
						})
					]
				})]
			});
		}
		//#endregion
		//#region \0dsh-css:/home/runner/work/deepseek-harness/deepseek-harness/packages/client/ui-trajectory/src/client/TrajectoryToolbar.module.css.mjs
		const css$2 = ".rtjfV0t5q_root{z-index:4;box-sizing:border-box;width:100%;height:var(--dsh-trajectory-toolbar-height);border-bottom:.5px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-layer-1);position:sticky;top:0}.rtjfV0t5q_inner{box-sizing:border-box;align-items:center;gap:8px;width:100%;height:100%;padding:0 6px;display:flex}.rtjfV0t5q_actions{flex:none;align-items:center;gap:2px;display:flex}.rtjfV0t5q_toggle{border-radius:var(--dsw-radius-sm);height:20px;color:var(--dsw-alias-label-tertiary);cursor:pointer;font:var(--dsw-font-xxs-12);background:0 0;border:0;flex:none;align-items:center;gap:4px;padding:0 7px;display:inline-flex}.rtjfV0t5q_toggle:hover,.rtjfV0t5q_toggle[aria-pressed=true]{color:var(--dsw-alias-label-primary);background:var(--dsw-alias-interactive-bg-hover)}.rtjfV0t5q_toggle:focus-visible{outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:1px}.rtjfV0t5q_toggleIcon{stroke:currentColor;stroke-width:1.25px;stroke-linecap:round;stroke-linejoin:round;flex:none;width:12px;height:12px}.rtjfV0t5q_control{box-sizing:border-box;width:88px;height:20px;color:var(--dsw-alias-label-tertiary);cursor:pointer;font:var(--dsw-font-xxs-12);background:0 0;border:0;border-radius:0;flex:none;justify-content:center;align-items:center;gap:4px;padding:0 5px;display:inline-flex}.rtjfV0t5q_control[hidden]{display:none}.rtjfV0t5q_control:hover:not(:disabled),.rtjfV0t5q_control[aria-checked=true],.rtjfV0t5q_control[aria-pressed=true]{color:var(--dsw-alias-label-primary)}.rtjfV0t5q_control:focus-visible{outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:1px}.rtjfV0t5q_control:disabled{color:var(--dsw-alias-label-dimmed);cursor:not-allowed}.rtjfV0t5q_controlTrack{background:var(--dsw-alias-border-l2);width:20px;height:10px;transition:background-color .12s var(--ds-ease-in-out);border-radius:5px;flex:none;display:inline-block;position:relative}.rtjfV0t5q_controlThumb{corner-shape:round;background:var(--dsw-alias-bg-layer-1);width:6px;height:6px;transition:transform .12s var(--ds-ease-in-out);border-radius:50%;position:absolute;top:2px;left:2px}.rtjfV0t5q_controlTrack[data-on=true]{background:var(--dsw-alias-state-business-primary)}.rtjfV0t5q_controlTrack[data-on=true] .rtjfV0t5q_controlThumb{transform:translate(10px)}.rtjfV0t5q_action{border-radius:var(--dsw-radius-sm);height:20px;color:var(--dsw-alias-label-tertiary);cursor:pointer;font:var(--dsw-font-xxs-12);background:0 0;border:0;flex:none;align-items:center;gap:4px;padding:0 5px;display:inline-flex}.rtjfV0t5q_action:hover{color:var(--dsw-alias-label-primary);background:var(--dsw-alias-interactive-bg-hover)}.rtjfV0t5q_action:focus-visible{outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:1px}.rtjfV0t5q_actionIcon{color:var(--dsw-alias-label-tertiary);font:14px/14px var(--ds-font-family-code)}.rtjfV0t5q_search{border:.5px solid var(--dsw-alias-border-l4);border-radius:var(--dsw-radius-sm);min-width:84px;height:22px;color:var(--dsw-alias-label-caption);background:var(--dsw-alias-bg-layer-2);flex:0 164px;align-items:center;gap:4px;margin-left:auto;padding:0 6px;display:flex}.rtjfV0t5q_search:hover{border-color:var(--dsw-alias-label-caption)}.rtjfV0t5q_search:focus-within{border-color:var(--dsw-alias-state-business-primary);background:var(--dsw-alias-bg-layer-1)}.rtjfV0t5q_searchIcon{flex:none}.rtjfV0t5q_searchInput{width:100%;min-width:0;color:var(--dsw-alias-label-primary);font:var(--dsw-font-xxs-12);background:0 0;border:0;outline:0;padding:0}.rtjfV0t5q_searchInput::placeholder{color:var(--dsw-alias-label-caption)}.rtjfV0t5q_searchInput::-webkit-search-cancel-button{cursor:pointer;width:12px;height:12px}";
		const tagId$2 = "@rth/dsh-rth-trajectory/TrajectoryToolbar.module.css";
		if (typeof document !== "undefined" && document.querySelector("style[data-plugin-css=" + JSON.stringify(tagId$2) + "]") === null) {
			const tag = document.createElement("style");
			tag.dataset.plugin = "@rth/dsh-rth-trajectory";
			tag.dataset.pluginCss = tagId$2;
			tag.textContent = css$2;
			document.head.appendChild(tag);
		}
		var TrajectoryToolbar_module_css_default = {
			"action": "rtjfV0t5q_action",
			"actionIcon": "rtjfV0t5q_actionIcon",
			"actions": "rtjfV0t5q_actions",
			"control": "rtjfV0t5q_control",
			"controlThumb": "rtjfV0t5q_controlThumb",
			"controlTrack": "rtjfV0t5q_controlTrack",
			"inner": "rtjfV0t5q_inner",
			"root": "rtjfV0t5q_root",
			"search": "rtjfV0t5q_search",
			"searchIcon": "rtjfV0t5q_searchIcon",
			"searchInput": "rtjfV0t5q_searchInput",
			"toggle": "rtjfV0t5q_toggle",
			"toggleIcon": "rtjfV0t5q_toggleIcon"
		};
		//#endregion
		//#region lib/types/client/TrajectoryToolbar.js
		/**
		* Render the sticky trajectory toolbar.
		* @param props - rendered counts and whole-list fold state.
		* @returns the toolbar element.
		*/
		function TrajectoryToolbar({ rthSequenceOnly, actualDuration, onActualDurationChange, actualTime, onActualTimeChange, allTurnsCollapsed, onToggleAllTurns, allAssistantsCollapsed, onToggleAllAssistants, searchQuery, onSearchQueryChange, t }) {
			return (0, react_jsx_runtime.jsx)("div", {
				className: TrajectoryToolbar_module_css_default.root,
				role: "toolbar",
				"aria-label": t("toolbar.aria"),
				children: (0, react_jsx_runtime.jsxs)("div", {
					className: TrajectoryToolbar_module_css_default.inner,
					children: [(0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryToolbar_module_css_default.actions,
						children: [
							(0, react_jsx_runtime.jsxs)("button", {
								type: "button",
								className: TrajectoryToolbar_module_css_default.toggle,
								"aria-label": t("toolbar.useActualDuration"),
								hidden: rthSequenceOnly === true,
								"aria-pressed": actualDuration,
								title: actualDuration ? t("toolbar.useEqualWidth") : t("toolbar.useActualDuration"),
								onClick: () => {
									onActualDurationChange(!actualDuration);
								},
								children: [(0, react_jsx_runtime.jsxs)("svg", {
									className: TrajectoryToolbar_module_css_default.toggleIcon,
									viewBox: "0 0 16 16",
									fill: "none",
									"aria-hidden": "true",
									children: [(0, react_jsx_runtime.jsx)("circle", {
										cx: "8",
										cy: "8",
										r: "5.25"
									}), (0, react_jsx_runtime.jsx)("path", { d: "M8 4.75V8l2.25 1.5" })]
								}), t("toolbar.duration")]
							}),
							(0, react_jsx_runtime.jsxs)("button", {
								type: "button",
								className: TrajectoryToolbar_module_css_default.control,
								role: "switch",
								"aria-checked": actualTime,
								hidden: true,
								onClick: () => {
									onActualTimeChange(!actualTime);
								},
								children: [(0, react_jsx_runtime.jsx)("span", { children: t("toolbar.actualTime") }), (0, react_jsx_runtime.jsx)("span", {
									className: TrajectoryToolbar_module_css_default.controlTrack,
									"data-on": actualTime || void 0,
									"aria-hidden": "true",
									children: (0, react_jsx_runtime.jsx)("span", { className: TrajectoryToolbar_module_css_default.controlThumb })
								})]
							}),
							(0, react_jsx_runtime.jsxs)("button", {
								type: "button",
								className: TrajectoryToolbar_module_css_default.action,
								"aria-label": allTurnsCollapsed ? t("toolbar.expandTurns") : t("toolbar.collapseTurns"),
								"aria-pressed": allTurnsCollapsed,
								title: allTurnsCollapsed ? t("toolbar.expandTurns") : t("toolbar.collapseTurns"),
								onClick: onToggleAllTurns,
								children: [(0, react_jsx_runtime.jsx)("span", {
									className: TrajectoryToolbar_module_css_default.actionIcon,
									"aria-hidden": "true",
									children: allTurnsCollapsed ? "⊞" : "⊟"
								}), t("toolbar.turns")]
							}),
							(0, react_jsx_runtime.jsxs)("button", {
								type: "button",
								className: TrajectoryToolbar_module_css_default.action,
								"aria-label": allAssistantsCollapsed ? t("toolbar.expandCalls") : t("toolbar.collapseCalls"),
								"aria-pressed": allAssistantsCollapsed,
								title: allAssistantsCollapsed ? t("toolbar.expandCalls") : t("toolbar.collapseCalls"),
								onClick: onToggleAllAssistants,
								children: [(0, react_jsx_runtime.jsx)("span", {
									className: TrajectoryToolbar_module_css_default.actionIcon,
									"aria-hidden": "true",
									children: allAssistantsCollapsed ? "⊞" : "⊟"
								}), t("toolbar.calls")]
							})
						]
					}), (0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryToolbar_module_css_default.search,
						children: [(0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.IconSearchOutlineRegular, {
							size: 11,
							className: TrajectoryToolbar_module_css_default.searchIcon
						}), (0, react_jsx_runtime.jsx)("input", {
							type: "search",
							className: TrajectoryToolbar_module_css_default.searchInput,
							"aria-label": t("toolbar.search"),
							placeholder: t("toolbar.searchPlaceholder"),
							value: searchQuery,
							onChange: (event) => {
								onSearchQueryChange(event.currentTarget.value);
							}
						})]
					})]
				})
			});
		}
		//#endregion
		//#region lib/types/client/timeline.js
		/** Operation-sequence and recorded-time projections for the trajectory overview. */
		/**
		* Format a timeline duration as an integer-millisecond label.
		* @param milliseconds - Non-negative duration in milliseconds.
		* @param t - Trajectory locale translator.
		* @returns Millisecond label with thousands separators.
		*/
		function formatTimelineOffset(milliseconds, t) {
			return formatDurationMillis(milliseconds, t);
		}
		function laneFor(kind) {
			if (kind === "tool" || kind === "subtool") return 2;
			if (kind === "message" || kind === "compacted") return 1;
			return 0;
		}
		function finite(value) {
			return value !== null && value !== void 0 && Number.isFinite(value);
		}
		function cellRange(cell) {
			if (!finite(cell.startedAt)) return null;
			if (cell.rthKind !== void 0) return {
				start: cell.startedAt,
				end: cell.startedAt
			};
			const durationMs = finite(cell.timeSeconds) ? Math.max(0, cell.timeSeconds * 1e3) : 0;
			return {
				start: cell.startedAt,
				end: cell.startedAt + durationMs
			};
		}
		/**
		* Project every visible record into a stable three-lane timeline.
		* @param turns - Unfiltered trajectory layout.
		* @param mode - Independent equal/recorded duration and compressed/complete time projection.
		* @returns Timeline model, or `null` when no record is visible.
		*/
		function deriveTrajectoryTimeline(turns, mode = "sequence") {
			if (mode !== "sequence") return deriveTimedTimeline(turns, mode === "duration" || mode === "actual", mode === "duration");
			const spans = [];
			const turnBoundaries = [];
			for (const turn of turns) {
				const cells = turn.groups.flatMap((group) => group.cells.filter((cell) => cell.requestOnly !== true));
				if (cells.length === 0) continue;
				if (turn.turn !== null) turnBoundaries.push({
					turn: turn.turn,
					time: spans.length
				});
				spans.push(...cells.map((cell, offset) => ({
					start: spans.length + offset,
					end: spans.length + offset + 1,
					index: cell.index,
					isError: cell.isError === true,
					kind: cell.rthKind ?? cell.kind,
					label: cell.text,
					lane: cell.rthLane ?? laneFor(cell.kind),
					rthSpan: cell.rthSpan
				})));
			}
			if (spans.length === 0) return null;
			return {
				start: 0,
				end: spans.length,
				spans,
				turnBoundaries
			};
		}
		function deriveTimedTimeline(turns, actualDuration, compressIdle) {
			const timedTurns = turns.flatMap((turn) => {
				const rawSpans = turn.groups.flatMap((group) => group.cells.flatMap((cell) => {
					if (cell.requestOnly === true) return [];
					const range = cellRange(cell);
					return range === null ? [] : [{
						...range,
						index: cell.index,
						isError: cell.isError === true,
						kind: cell.rthKind ?? cell.kind,
						label: cell.text,
						lane: cell.rthLane ?? laneFor(cell.kind),
						rthSpan: cell.rthSpan
					}];
				}));
				return rawSpans.length === 0 ? [] : [{
					turn: turn.turn,
					rawSpans
				}];
			});
			const rawSpans = timedTurns.flatMap((turn) => turn.rawSpans);
			if (rawSpans.length === 0) return null;
			const removedIdleBySpan = /* @__PURE__ */ new Map();
			let removedIdle = 0;
			let coveredUntil = null;
			for (const span of [...rawSpans].sort((left, right) => left.start - right.start || left.end - right.end)) {
				if (compressIdle && coveredUntil !== null && span.start > coveredUntil) removedIdle += span.start - coveredUntil;
				removedIdleBySpan.set(span, removedIdle);
				coveredUntil = coveredUntil === null ? span.end : Math.max(coveredUntil, span.end);
			}
			const spans = [];
			const turnBoundaries = [];
			for (const turn of timedTurns) {
				const projected = turn.rawSpans.map((span) => {
					const offset = removedIdleBySpan.get(span) ?? 0;
					return {
						...span,
						start: span.start - offset,
						end: (actualDuration ? span.end : span.start) - offset
					};
				});
				spans.push(...projected);
				if (turn.turn !== null) turnBoundaries.push({
					turn: turn.turn,
					time: Math.min(...projected.map((span) => span.start))
				});
			}
			return {
				start: Math.min(...spans.map((span) => span.start)),
				end: Math.max(...spans.map((span) => span.end)),
				spans,
				turnBoundaries
			};
		}
		/**
		* Identify records active at any point inside an inclusive selected interval.
		* @param turns - Unfiltered trajectory layout.
		* @param range - Selected interval in the active projection.
		* @param mode - Independent equal/recorded duration and compressed/complete time projection.
		* @returns Record indexes inside the focus interval.
		*/
		function trajectoryTimelineFocusIndexes(turns, range, mode = "sequence") {
			const model = deriveTrajectoryTimeline(turns, mode);
			return new Set(model?.spans.filter((span) => span.start <= range.end && span.end >= range.start).map((span) => span.index));
		}
		//#endregion
		//#region \0dsh-css:/home/runner/work/deepseek-harness/deepseek-harness/packages/client/ui-trajectory/src/client/TrajectoryTimeline.module.css.mjs
		const css$1 = ".rtj1p9O6q_root{z-index:1;isolation:isolate;border-bottom:.5px solid var(--dsw-alias-border-l2);user-select:none;flex:none;position:relative}.rtj1p9O6q_root [role=tooltip]{font:var(--dsw-font-xxxs-11)}.rtj1p9O6q_plot{background:var(--dsw-alias-bg-layer-2);grid-template-columns:44px minmax(0,1fr);height:50px;display:grid;overflow:hidden}.rtj1p9O6q_labels{border-right:.5px solid var(--dsw-alias-border-l1);color:var(--dsw-alias-label-caption);font:var(--dsw-font-xs-13);font-size:10px;line-height:1;position:relative}.rtj1p9O6q_labels span{text-align:right;justify-content:flex-end;align-items:center;height:8px;display:flex;position:absolute;right:3px}.rtj1p9O6q_labels span:first-child{top:7px}.rtj1p9O6q_labels span:nth-child(2){top:21px}.rtj1p9O6q_labels span:nth-child(3){top:35px}.rtj1p9O6q_track{cursor:crosshair;touch-action:none;position:relative;overflow:hidden}.rtj1p9O6q_track[data-panning=true]{cursor:grabbing}.rtj1p9O6q_earlierHistory{z-index:5;appearance:none;box-sizing:border-box;background:linear-gradient(to right, var(--dsw-alias-bg-layer-2) 0, var(--dsw-alias-bg-layer-2) 38%, transparent 100%);width:28px;color:var(--dsw-alias-label-secondary);font:var(--dsw-font-xs-13);opacity:.72;cursor:pointer;border:0;outline:none;justify-content:flex-start;align-items:center;padding-left:3px;line-height:1;display:flex;position:absolute;top:0;bottom:0;left:0}.rtj1p9O6q_earlierHistory:hover{opacity:1}.rtj1p9O6q_earlierHistory[aria-disabled=true]{cursor:default}.rtj1p9O6q_earlierHistory:focus-visible{box-shadow:inset 0 0 0 1px var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary))}.rtj1p9O6q_empty{color:var(--dsw-alias-label-caption);font:var(--dsw-font-xs-13);position:absolute;top:50%;left:50%;transform:translate(-50%,-50%)}.rtj1p9O6q_track:focus-visible{outline:1px solid var(--dsw-focus-ring-color,var(--dsw-alias-state-business-primary));outline-offset:-1px}.rtj1p9O6q_lanes{z-index:2;top:7px;bottom:7px;left:var(--trajectory-domain-left);width:var(--trajectory-domain-width);position:absolute}.rtj1p9O6q_turnBoundaries{z-index:3;top:0;bottom:0;left:var(--trajectory-domain-left);width:var(--trajectory-domain-width);pointer-events:none;position:absolute}@media (prefers-reduced-motion:no-preference){.rtj1p9O6q_lanes[data-animate-viewport=true],.rtj1p9O6q_turnBoundaries[data-animate-viewport=true]{transition:left .18s ease-out}}.rtj1p9O6q_turnBoundary{top:0;bottom:0;left:var(--trajectory-turn-left);background:var(--dsw-alias-border-l2);width:.5px;position:absolute}.rtj1p9O6q_span{top:calc(var(--trajectory-span-lane) * 14px);left:calc(var(--trajectory-span-left) + var(--trajectory-span-gap));width:max(2px, calc(var(--trajectory-span-width) - var(--trajectory-span-gap) - var(--trajectory-span-gap)));background:var(--dsw-alias-label-secondary);opacity:.78;border-radius:1px;min-width:2px;height:8px;position:absolute}.rtj1p9O6q_span[data-timeline-span=user]{background:var(--dsw-alias-state-business-primary)}.rtj1p9O6q_span[data-timeline-span=context]{background:color-mix(in srgb, var(--dsw-alias-state-success-primary) 68%, var(--dsw-alias-label-secondary))}.rtj1p9O6q_span[data-timeline-span=message]{--trajectory-assistant-decoding-color:color-mix(in srgb, var(--dsw-alias-brand-primary-new-colorprimary-new-color) 60%, var(--dsw-alias-state-error-secondary));--trajectory-assistant-ttft-color:color-mix(in srgb, var(--trajectory-assistant-decoding-color) 54%, var(--dsw-alias-bg-layer-2));background:var(--trajectory-assistant-decoding-color);opacity:1}.rtj1p9O6q_span[data-timeline-span=message][data-assistant-timing=true]{background:linear-gradient(to right, var(--trajectory-assistant-ttft-color) 0, var(--trajectory-assistant-ttft-color) var(--trajectory-assistant-ttft), var(--trajectory-assistant-decoding-color) var(--trajectory-assistant-ttft), var(--trajectory-assistant-decoding-color) 100%)}.rtj1p9O6q_span[data-timeline-span=tool],.rtj1p9O6q_span[data-timeline-span=subtool]{background:var(--dsw-alias-state-warn-label);opacity:1}.rtj1p9O6q_span[data-error=true]{background:var(--dsw-alias-state-error-primary)}.rtj1p9O6q_span[data-equal-duration=true]{width:8px;min-width:8px}.rtj1p9O6q_span[data-selected=false]{opacity:.2}.rtj1p9O6q_span[data-hovered=true]:not([data-current=true]){z-index:1;opacity:1;box-shadow:0 0 0 1px var(--dsw-alias-bg-layer-2), 0 0 0 2px color-mix(in srgb, var(--dsw-alias-state-business-primary) 80%, transparent)}.rtj1p9O6q_span[data-current=true]{z-index:1;opacity:1;box-shadow:0 0 0 1px var(--dsw-alias-bg-layer-2), 0 0 0 2px var(--dsw-alias-state-business-primary)}.rtj1p9O6q_span[data-search-match=false]{opacity:.14}.rtj1p9O6q_selection{z-index:1;top:0;bottom:0;left:var(--trajectory-selection-left);width:var(--trajectory-selection-width);background:color-mix(in srgb, var(--dsw-alias-state-business-primary) 12%, transparent);min-width:1px;box-shadow:-100vw 0 0 100vw color-mix(in srgb, var(--dsw-alias-bg-layer-1) 58%, transparent), 100vw 0 0 100vw color-mix(in srgb, var(--dsw-alias-bg-layer-1) 58%, transparent);pointer-events:none;position:absolute}.rtj1p9O6q_selectionEdges{z-index:4;top:0;bottom:0;left:var(--trajectory-selection-left);width:var(--trajectory-selection-width);pointer-events:none;min-width:1px;position:absolute}.rtj1p9O6q_hoverLine{z-index:4;top:0;bottom:0;left:clamp(0px, calc(var(--trajectory-hover-left) - 1px), calc(100% - 2px));background:var(--dsw-alias-state-business-primary);pointer-events:none;width:2px;position:absolute}.rtj1p9O6q_selectionEdges:before,.rtj1p9O6q_selectionEdges:after{background:var(--dsw-alias-state-business-primary);content:\"\";width:3px;position:absolute;top:0;bottom:0}.rtj1p9O6q_selectionEdges:before{left:0}.rtj1p9O6q_selectionEdges:after{right:0}.rtj1p9O6q_plot{height:78px}.rtj1p9O6q_labels span:nth-child(4){top:49px}.rtj1p9O6q_labels span:nth-child(5){top:63px}.rtj1p9O6q_selectionEdges[data-dragging=true]:before,.rtj1p9O6q_selectionEdges[data-dragging=true]:after{width:2px}.rtj1p9O6q_selection[data-dragging=true]{background:color-mix(in srgb, var(--dsw-alias-state-business-primary) 18%, transparent)}";
		const tagId$1 = "@rth/dsh-rth-trajectory/TrajectoryTimeline.module.css";
		if (typeof document !== "undefined" && document.querySelector("style[data-plugin-css=" + JSON.stringify(tagId$1) + "]") === null) {
			const tag = document.createElement("style");
			tag.dataset.plugin = "@rth/dsh-rth-trajectory";
			tag.dataset.pluginCss = tagId$1;
			tag.textContent = css$1;
			document.head.appendChild(tag);
		}
		var TrajectoryTimeline_module_css_default = {
			"earlierHistory": "rtj1p9O6q_earlierHistory",
			"empty": "rtj1p9O6q_empty",
			"hoverLine": "rtj1p9O6q_hoverLine",
			"labels": "rtj1p9O6q_labels",
			"lanes": "rtj1p9O6q_lanes",
			"plot": "rtj1p9O6q_plot",
			"root": "rtj1p9O6q_root",
			"selection": "rtj1p9O6q_selection",
			"selectionEdges": "rtj1p9O6q_selectionEdges",
			"span": "rtj1p9O6q_span",
			"track": "rtj1p9O6q_track",
			"turnBoundaries": "rtj1p9O6q_turnBoundaries",
			"turnBoundary": "rtj1p9O6q_turnBoundary"
		};
		//#endregion
		//#region lib/types/client/TrajectoryTimeline.js
		/** Chrome-Network-style overview timeline for focusing the trajectory ledger. */
		const MINIMUM_DRAG_PX = 3;
		const MINIMUM_ZOOM_OPERATIONS = 4;
		const EDGE_PAN_ZONE_FRACTION = .08;
		const EDGE_PAN_STEP_FRACTION = .025;
		const MAXIMUM_EDGE_PAN_PX = 32;
		const TIMELINE_TOOLTIP_DELAY_MS = 500;
		function assistantTimingDetail(metrics) {
			const start = metrics?.stepStartTime;
			const first = metrics?.firstTokenTime;
			const completed = metrics?.completedTime;
			if (metrics?.timingRecorded !== true || typeof start !== "number" || typeof first !== "number" || typeof completed !== "number" || !Number.isFinite(start) || !Number.isFinite(first) || !Number.isFinite(completed) || first < start || completed < first) return {};
			return {
				ttftMs: first - start,
				decodingMs: completed - first
			};
		}
		function timelineRecordDetail(cell) {
			if (cell.rthKind !== void 0) return RTH.timelineDetail(cell);
			const durationMs = cell.timeSeconds === null || !Number.isFinite(cell.timeSeconds) ? void 0 : Math.max(0, cell.timeSeconds * 1e3);
			const startedAt = cell.startedAt === null || !Number.isFinite(cell.startedAt) ? void 0 : cell.startedAt;
			return {
				...durationMs === void 0 ? {} : { durationMs },
				...startedAt === void 0 ? {} : { startedAt },
				...assistantTimingDetail(cell.assistantMetrics)
			};
		}
		function timelineKindLabel(kind, t) {
			switch (kind) {
				case "system": return t("kind.system");
				case "user": return t("kind.user");
				case "context": return t("kind.context");
				case "compacted": return t("kind.compacted");
				case "message": return t("kind.assistant");
				case "tool": return t("kind.tool");
				case "subtool": return t("kind.subtool");
				default: return RTH.kindLabel(kind) ?? kind;
			}
		}
		function formatRecordedTime(timestamp) {
			return new Date(timestamp).toLocaleTimeString(void 0, {
				hour: "2-digit",
				minute: "2-digit",
				second: "2-digit",
				fractionalSecondDigits: 3
			});
		}
		function timelineTooltipLabel(kind, detail, t) {
			const heading = timelineKindLabel(kind, t);
			if (detail === void 0) return heading;
			const duration = detail.durationMs === void 0 ? null : t("timeline.total", { duration: formatTimelineOffset(detail.durationMs, t) });
			return [
				heading,
				detail.startedAt === void 0 ? null : detail.durationMs === void 0 ? t("timeline.started", { time: formatRecordedTime(detail.startedAt) }) : `${formatRecordedTime(detail.startedAt)} → ${formatRecordedTime(detail.startedAt + detail.durationMs)}`,
				[duration, detail.ttftMs === void 0 || detail.decodingMs === void 0 ? null : t("timeline.ttftDecoding", {
					ttft: formatTimelineOffset(detail.ttftMs, t),
					decoding: formatTimelineOffset(detail.decodingMs, t)
				})].filter((value) => value !== null).join(" · ")
			].filter((value) => value !== null && value !== "").join("\n");
		}
		function orderedRange(left, right) {
			return left <= right ? {
				start: left,
				end: right
			} : {
				start: right,
				end: left
			};
		}
		function clampFraction(value) {
			return Math.min(1, Math.max(0, value));
		}
		function centeredRange(center, width, minimum, maximum) {
			const clampedWidth = Math.min(maximum - minimum, Math.max(0, width));
			const start = Math.min(Math.max(center - clampedWidth / 2, minimum), maximum - clampedWidth);
			return {
				start,
				end: start + clampedWidth
			};
		}
		function rangeFraction(range, start, duration, minimum, maximum) {
			const bounded = orderedRange(Math.min(maximum, Math.max(minimum, range.start)), Math.min(maximum, Math.max(minimum, range.end)));
			return {
				start: (bounded.start - start) / duration,
				end: (bounded.end - start) / duration
			};
		}
		function LaneLabels({ t }) {
			return (0, react_jsx_runtime.jsxs)("div", {
				className: TrajectoryTimeline_module_css_default.labels,
				"aria-hidden": "true",
				children: [
					...RTH.LANES.map((lane) => (0, react_jsx_runtime.jsx)("span", { children: lane }, lane))
				]
			});
		}
		function EarlierHistoryBoundary({ loading, onHover, onLoad, t }) {
			return (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.Tooltip, {
				label: loading ? t("history.loadingEarlier") : t("history.clickToLoadEarlier"),
				side: "right",
				delayMs: TIMELINE_TOOLTIP_DELAY_MS,
				children: (0, react_jsx_runtime.jsx)("button", {
					type: "button",
					className: TrajectoryTimeline_module_css_default.earlierHistory,
					"data-earlier-history": true,
					"data-loading": loading || void 0,
					"aria-label": loading ? t("history.loadingEarlierAria") : t("history.loadEarlier"),
					"aria-disabled": loading || onLoad === void 0,
					onClick: onLoad,
					onPointerEnter: (event) => {
						event.stopPropagation();
						onHover();
					},
					onPointerMove: (event) => {
						event.stopPropagation();
					},
					onPointerDown: (event) => {
						event.stopPropagation();
					},
					children: "…"
				})
			});
		}
		/** Overview renderer with drag ranges, click-sized focus, and Escape reset. */
		const TrajectoryTimeline = (0, react.memo)(function TrajectoryTimeline({ t, turns, mode, range, hasEarlierRecords = false, onLoadEarlier, selectedIndex = null, searchMatchIndexes = null, onRangeChange, onRecordSelect, onRecordFocus }) {
			const model = (0, react.useMemo)(() => deriveTrajectoryTimeline(turns, mode), [mode, turns]);
			const detailByIndex = (0, react.useMemo)(() => new Map(turns.flatMap((turn) => turn.groups.flatMap((group) => group.cells.map((cell) => [cell.index, timelineRecordDetail(cell)])))), [turns]);
			const dragRef = (0, react.useRef)(null);
			const panRef = (0, react.useRef)(null);
			const rootRef = (0, react.useRef)(null);
			const trackRef = (0, react.useRef)(null);
			const [draft, setDraft] = (0, react.useState)(null);
			const [hover, setHover] = (0, react.useState)(null);
			const [loadingEarlier, setLoadingEarlier] = (0, react.useState)(false);
			const [panning, setPanning] = (0, react.useState)(false);
			const [viewport, setViewport] = (0, react.useState)(null);
			const [animateViewport, setAnimateViewport] = (0, react.useState)(false);
			(0, react.useEffect)(() => {
				if (model !== null && range !== null && (range.end < model.start || range.start > model.end)) onRangeChange(null);
			}, [
				model,
				onRangeChange,
				range
			]);
			(0, react.useEffect)(() => {
				if (model === null) return;
				setAnimateViewport(false);
				setViewport((current) => current !== null && (current.end < model.start || current.start > model.end) ? null : current);
			}, [model]);
			(0, react.useEffect)(() => {
				if (model === null || selectedIndex === null) return;
				const selectedSpan = model.spans.find((span) => span.index === selectedIndex);
				if (selectedSpan === void 0) return;
				setAnimateViewport(true);
				setViewport((current) => {
					if (current === null) return current;
					if (selectedSpan.end > current.start && selectedSpan.start < current.end) return current;
					const duration = Math.max(1, current.end - current.start);
					const desiredStart = selectedSpan.end <= current.start ? selectedSpan.start : selectedSpan.end - duration;
					const nextStart = Math.min(Math.max(desiredStart, model.start), Math.max(model.start, model.end - duration));
					if (nextStart === current.start) return current;
					return {
						start: nextStart,
						end: nextStart + duration
					};
				});
			}, [model, selectedIndex]);
			const fullDuration = Math.max(1, (model?.end ?? 0) - (model?.start ?? 0));
			const viewportDuration = Math.min(fullDuration, Math.max(1, (viewport?.end ?? 0) - (viewport?.start ?? 0)));
			const viewportStart = model === null || viewport === null ? model?.start ?? 0 : Math.min(Math.max(viewport.start, model.start), model.end - viewportDuration);
			const domainDuration = viewport === null ? fullDuration : viewportDuration;
			const domainStart = viewport === null ? model?.start ?? 0 : viewportStart;
			const showsEarlierBoundary = hasEarlierRecords && model !== null && domainStart === model.start;
			const loadEarlier = onLoadEarlier === void 0 || loadingEarlier ? void 0 : () => {
				setLoadingEarlier(true);
				onLoadEarlier().finally(() => {
					setLoadingEarlier(false);
				});
			};
			const projectedDomainStyle = model === null ? void 0 : {
				"--trajectory-domain-left": `${-(domainStart - model.start) / domainDuration * 100}%`,
				"--trajectory-domain-width": `${fullDuration / domainDuration * 100}%`
			};
			const committed = model === null || range === null ? null : rangeFraction(range, domainStart, domainDuration, model.start, model.end);
			const visibleRange = (model === null || draft === null ? null : rangeFraction(draft, domainStart, domainDuration, model.start, model.end)) ?? committed;
			const activeRange = draft ?? range;
			(0, react.useEffect)(() => {
				const root = rootRef.current;
				if (root === null) return;
				const onWheel = (event) => {
					event.preventDefault();
					const track = trackRef.current;
					if (track === null || model === null) return;
					setAnimateViewport(false);
					const rect = track.getBoundingClientRect();
					const anchorFraction = clampFraction((event.clientX - rect.left) / Math.max(1, rect.width));
					const nextDuration = Math.min(fullDuration, Math.max(Math.min(mode === "sequence" ? MINIMUM_ZOOM_OPERATIONS : 20, fullDuration), domainDuration * Math.exp(event.deltaY * .0015)));
					if (nextDuration >= fullDuration * .999) {
						setViewport(null);
						return;
					}
					const anchorTime = domainStart + anchorFraction * domainDuration;
					const nextStart = Math.min(Math.max(anchorTime - anchorFraction * nextDuration, model.start), model.end - nextDuration);
					setViewport({
						start: nextStart,
						end: nextStart + nextDuration
					});
				};
				root.addEventListener("wheel", onWheel, { passive: false });
				return () => {
					root.removeEventListener("wheel", onWheel);
				};
			}, [
				domainDuration,
				domainStart,
				fullDuration,
				mode,
				model
			]);
			if (model === null) return (0, react_jsx_runtime.jsx)("section", {
				ref: rootRef,
				className: TrajectoryTimeline_module_css_default.root,
				"aria-label": t("timeline.aria"),
				children: (0, react_jsx_runtime.jsxs)("div", {
					className: TrajectoryTimeline_module_css_default.plot,
					children: [(0, react_jsx_runtime.jsx)(LaneLabels, { t }), (0, react_jsx_runtime.jsxs)("div", {
						className: TrajectoryTimeline_module_css_default.track,
						children: [(0, react_jsx_runtime.jsx)("span", {
							className: TrajectoryTimeline_module_css_default.empty,
							children: t("timeline.noTimingData")
						}), hasEarlierRecords && (0, react_jsx_runtime.jsx)(EarlierHistoryBoundary, {
							loading: loadingEarlier,
							onHover: () => {
								setHover(null);
							},
							onLoad: loadEarlier,
							t
						})]
					})]
				})
			});
			const minimumSelectionDuration = Math.min(domainDuration, fullDuration / model.spans.length);
			const fractionAt = (event) => {
				const rect = event.currentTarget.getBoundingClientRect();
				return clampFraction((event.clientX - rect.left) / Math.max(1, rect.width));
			};
			const recordIndexAt = (event) => {
				const value = (event.target instanceof HTMLElement ? event.target : null)?.closest("[data-timeline-record-index]")?.dataset.timelineRecordIndex;
				if (value === void 0) return null;
				const index = Number(value);
				return Number.isFinite(index) ? index : null;
			};
			const commit = (nextRange) => {
				onRangeChange(nextRange);
			};
			const onPointerDown = (event) => {
				if (event.button === 2) {
					panRef.current = {
						anchorClientX: event.clientX,
						anchorStart: domainStart,
						moved: false,
						pannable: viewport !== null,
						pointerId: event.pointerId
					};
					if (viewport !== null) setAnimateViewport(false);
					setPanning(true);
					if (typeof event.currentTarget.setPointerCapture === "function") event.currentTarget.setPointerCapture(event.pointerId);
					return;
				}
				if (event.button !== 0) return;
				const anchor = fractionAt(event);
				const anchorTime = domainStart + anchor * domainDuration;
				const recordIndex = recordIndexAt(event);
				setHover({
					fraction: anchor,
					recordIndex
				});
				dragRef.current = {
					pointerId: event.pointerId,
					anchorTime,
					anchorClientX: event.clientX,
					recordIndex
				};
				if (typeof event.currentTarget.setPointerCapture === "function") event.currentTarget.setPointerCapture(event.pointerId);
				setDraft({
					start: anchorTime,
					end: anchorTime
				});
			};
			const onPointerMove = (event) => {
				const rect = event.currentTarget.getBoundingClientRect();
				const fraction = fractionAt(event);
				setHover({
					fraction,
					recordIndex: recordIndexAt(event)
				});
				const pan = panRef.current;
				if (pan !== null && pan.pointerId === event.pointerId) {
					if (Math.abs(event.clientX - pan.anchorClientX) >= MINIMUM_DRAG_PX) pan.moved = true;
					if (!pan.pannable) return;
					const delta = (event.clientX - pan.anchorClientX) / Math.max(1, rect.width);
					const nextStart = Math.min(Math.max(pan.anchorStart - delta * domainDuration, model.start), model.end - domainDuration);
					setViewport({
						start: nextStart,
						end: nextStart + domainDuration
					});
					return;
				}
				const drag = dragRef.current;
				if (drag === null || drag.pointerId !== event.pointerId) return;
				let nextDomainStart = domainStart;
				if (viewport !== null) {
					const localX = event.clientX - rect.left;
					const edgeWidth = Math.min(MAXIMUM_EDGE_PAN_PX, Math.max(1, rect.width * EDGE_PAN_ZONE_FRACTION));
					const direction = localX < edgeWidth ? -1 : localX > rect.width - edgeWidth ? 1 : 0;
					if (direction !== 0) {
						const strength = clampFraction((direction < 0 ? edgeWidth - localX : localX - (rect.width - edgeWidth)) / edgeWidth);
						const desiredStart = domainStart + direction * domainDuration * EDGE_PAN_STEP_FRACTION * Math.max(.2, strength);
						nextDomainStart = Math.min(Math.max(desiredStart, model.start), model.end - domainDuration);
						if (nextDomainStart !== domainStart) {
							setAnimateViewport(false);
							setViewport({
								start: nextDomainStart,
								end: nextDomainStart + domainDuration
							});
						}
					}
				}
				const pointTime = nextDomainStart + fraction * domainDuration;
				setDraft(orderedRange(drag.anchorTime, pointTime));
			};
			const onPointerEnd = (event) => {
				const pan = panRef.current;
				if (pan !== null && pan.pointerId === event.pointerId) {
					const moved = pan.moved || Math.abs(event.clientX - pan.anchorClientX) >= MINIMUM_DRAG_PX;
					panRef.current = null;
					setPanning(false);
					if (!moved) onRangeChange(null);
					return;
				}
				const drag = dragRef.current;
				if (drag === null || drag.pointerId !== event.pointerId) return;
				const pointFraction = fractionAt(event);
				const pointTime = domainStart + pointFraction * domainDuration;
				const selected = orderedRange(drag.anchorTime, pointTime);
				setHover({
					fraction: pointFraction,
					recordIndex: recordIndexAt(event)
				});
				dragRef.current = null;
				setDraft(null);
				const click = Math.abs(event.clientX - drag.anchorClientX) < MINIMUM_DRAG_PX;
				const clickedSpan = click && drag.recordIndex !== null ? model.spans.find((span) => span.index === drag.recordIndex) : void 0;
				if (clickedSpan !== void 0) {
					onRangeChange(null);
					onRecordSelect?.(clickedSpan.index);
					return;
				}
				commit(selected.end - selected.start < minimumSelectionDuration ? centeredRange(click ? selected.start : (selected.start + selected.end) / 2, minimumSelectionDuration, model.start, model.end) : selected);
				if (click) {
					const timelinePoint = selected.start;
					const nearest = model.spans.reduce((candidate, span) => {
						const candidateDistance = timelinePoint < candidate.start ? candidate.start - timelinePoint : timelinePoint > candidate.end ? timelinePoint - candidate.end : 0;
						return (timelinePoint < span.start ? span.start - timelinePoint : timelinePoint > span.end ? timelinePoint - span.end : 0) < candidateDistance ? span : candidate;
					});
					onRecordFocus?.(nearest.index);
				}
			};
			const onKeyDown = (event) => {
				if (event.key !== "Escape" || range === null) return;
				event.preventDefault();
				onRangeChange(null);
			};
			const onPointerCancel = () => {
				dragRef.current = null;
				panRef.current = null;
				setDraft(null);
				setHover(null);
				setPanning(false);
			};
			return (0, react_jsx_runtime.jsx)("section", {
				ref: rootRef,
				className: TrajectoryTimeline_module_css_default.root,
				"aria-label": t("timeline.aria"),
				children: (0, react_jsx_runtime.jsxs)("div", {
					className: TrajectoryTimeline_module_css_default.plot,
					children: [(0, react_jsx_runtime.jsx)(LaneLabels, { t }), (0, react_jsx_runtime.jsxs)("div", {
						ref: trackRef,
						className: TrajectoryTimeline_module_css_default.track,
						"data-panning": panning || void 0,
						"aria-label": t("timeline.overviewAria"),
						tabIndex: 0,
						onKeyDown,
						onPointerDown,
						onPointerMove,
						onPointerUp: onPointerEnd,
						onPointerCancel,
						onPointerLeave: () => {
							if (dragRef.current === null && panRef.current === null) setHover(null);
						},
						onDoubleClick: (event) => {
							event.preventDefault();
							onRangeChange(null);
						},
						onContextMenu: (event) => {
							event.preventDefault();
						},
						children: [
							showsEarlierBoundary && (0, react_jsx_runtime.jsx)(EarlierHistoryBoundary, {
								loading: loadingEarlier,
								onHover: () => {
									setHover(null);
								},
								onLoad: loadEarlier,
								t
							}),
							hover !== null && hover.recordIndex === null && draft === null && (0, react_jsx_runtime.jsx)("div", {
								className: TrajectoryTimeline_module_css_default.hoverLine,
								"data-timeline-hover-line": true,
								"aria-hidden": "true",
								style: { "--trajectory-hover-left": `${hover.fraction * 100}%` }
							}),
							visibleRange !== null && (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [(0, react_jsx_runtime.jsx)("div", {
								className: TrajectoryTimeline_module_css_default.selection,
								"data-dragging": draft === null ? void 0 : "true",
								"aria-hidden": "true",
								style: {
									"--trajectory-selection-left": `${visibleRange.start * 100}%`,
									"--trajectory-selection-width": `${(visibleRange.end - visibleRange.start) * 100}%`
								}
							}), (0, react_jsx_runtime.jsx)("div", {
								className: TrajectoryTimeline_module_css_default.selectionEdges,
								"data-dragging": draft === null ? void 0 : "true",
								"aria-hidden": "true",
								style: {
									"--trajectory-selection-left": `${visibleRange.start * 100}%`,
									"--trajectory-selection-width": `${(visibleRange.end - visibleRange.start) * 100}%`
								}
							})] }),
							(0, react_jsx_runtime.jsx)("div", {
								className: TrajectoryTimeline_module_css_default.turnBoundaries,
								"data-animate-viewport": animateViewport || void 0,
								"aria-hidden": "true",
								style: projectedDomainStyle,
								children: model.turnBoundaries.filter((boundary) => boundary.time > model.start && boundary.time >= domainStart && boundary.time <= domainStart + domainDuration).map((boundary) => (0, react_jsx_runtime.jsx)("span", {
									className: TrajectoryTimeline_module_css_default.turnBoundary,
									"data-turn": boundary.turn,
									style: { "--trajectory-turn-left": `${(boundary.time - model.start) / fullDuration * 100}%` }
								}, boundary.turn))
							}),
							(0, react_jsx_runtime.jsx)("div", {
								className: TrajectoryTimeline_module_css_default.lanes,
								"data-animate-viewport": animateViewport || void 0,
								"data-timeline-domain": true,
								style: projectedDomainStyle,
								children: model.spans.filter((span) => span.index === selectedIndex || span.end >= domainStart && span.start <= domainStart + domainDuration).map((span) => {
									const left = (span.start - model.start) / fullDuration;
									const widthPercent = (span.end - span.start) / fullDuration * 100;
									const detail = detailByIndex.get(span.index);
									const ttftMs = detail?.ttftMs;
									const decodingMs = detail?.decodingMs;
									const ttftFraction = ttftMs === void 0 || decodingMs === void 0 || ttftMs + decodingMs <= 0 ? null : ttftMs / (ttftMs + decodingMs);
									return (0, react_jsx_runtime.jsx)(_deepseek_ai_dsh_client_ui_primitives.Tooltip, {
										label: () => timelineTooltipLabel(span.kind, detail, t),
										side: "bottom",
										delayMs: TIMELINE_TOOLTIP_DELAY_MS,
										children: (0, react_jsx_runtime.jsx)("span", {
											"aria-hidden": "true",
											className: TrajectoryTimeline_module_css_default.span,
											"data-timeline-span": span.kind,
											"data-rth-span": mode === "sequence" ? span.rthSpan === "unknown" ? "unknown-seq" : void 0 : span.rthSpan,
											"data-timeline-record-index": span.index,
											"data-assistant-timing": ttftFraction === null ? void 0 : "true",
											"data-error": span.isError || void 0,
											"data-equal-duration": mode === "time" || void 0,
											"data-current": span.index === selectedIndex || void 0,
											"data-hovered": hover?.recordIndex === span.index || void 0,
											"data-search-match": searchMatchIndexes === null ? void 0 : searchMatchIndexes.has(span.index) ? "true" : "false",
											"data-selected": activeRange === null ? void 0 : span.start <= activeRange.end && span.end >= activeRange.start ? "true" : "false",
											style: {
												"--trajectory-span-left": `${left * 100}%`,
												"--trajectory-span-width": `${widthPercent}%`,
												"--trajectory-span-gap": `min(${widthPercent * .08}%, 1px)`,
												"--trajectory-span-lane": span.lane,
												...ttftFraction === null ? {} : { "--trajectory-assistant-ttft": `${ttftFraction * 100}%` }
											}
										})
									}, span.index);
								})
							})
						]
					})]
				})
			});
		});
		//#endregion
		//#region lib/types/client/layout.js
		function layoutEntryOrder(entry) {
			return entry.kind === "system" && entry.change.kind === "initial" ? Number.NEGATIVE_INFINITY : entry.seq;
		}
		function inputCellDetail(node, t) {
			if (node.kind === "context" && node.content.length > 0 && node.content.every((block) => block.type === "tool-addition" || block.type === "tool-removal")) {
				const added = node.content.flatMap((block) => block.type === "tool-addition" ? [block.toolName] : []);
				const removed = node.content.flatMap((block) => block.type === "tool-removal" ? [block.toolName] : []);
				const single = node.content.length === 1 ? node.content[0] : void 0;
				const summary = added.length > 0 && removed.length > 0 ? t("layout.toolsChanged", {
					added: added.length,
					removed: removed.length
				}) : added.length > 0 ? t("layout.toolsAddedCount", { count: added.length }) : t("layout.toolsRemovedCount", { count: removed.length });
				return {
					text: single !== void 0 ? t(single.type === "tool-addition" ? "layout.toolAdded" : "layout.toolRemoved", { name: single.toolName }) : `${t("layout.toolUpdateNotice")} · ${summary}`,
					sourceSeq: node.seq,
					sourceBlocks: node.content.map((block) => ({
						type: block.type,
						content: block.toolName
					})),
					...single !== void 0 ? {} : { inputDetail: [...added.length > 0 ? [t("layout.toolsAdded", { names: added.join(", ") })] : [], ...removed.length > 0 ? [t("layout.toolsRemoved", { names: removed.join(", ") })] : []].join("\n") },
					timeSeconds: 0,
					startedAt: finiteTime(node.time)
				};
			}
			const preview = previewContent(node.content);
			const previewMarkdown = preview === "" ? void 0 : preview;
			const images = imageBlockCount(node.content);
			const files = fileBlockCount(node.content);
			return {
				text: [images > 0 ? t("layout.imageCount", { count: images }) : void 0, files > 0 ? t("layout.fileAttachments", { count: files }) : void 0].filter((value) => value !== void 0).join(" · "),
				...previewMarkdown === void 0 ? {} : { previewMarkdown },
				sourceSeq: node.seq,
				messageSource: node.source,
				inputDetail: detailContent(node.content),
				sourceBlocks: node.content.map((block) => sourceBlock(block)),
				timeSeconds: 0,
				startedAt: finiteTime(node.time)
			};
		}
		/**
		* Fold a snapshot into turn → Message/Step groups with expanded cells.
		* @param input - nodes plus in-flight partial/runningCalls.
		* @param t - Trajectory locale translator.
		* @returns turns ordered by first appearance.
		*/
		function deriveTrajectoryLayout(input, t) {
			const { nodes, eventLocations, partial, runningCalls, requests = [], callSchemas } = input;
			const resultByCall = indexResults(nodes);
			const callById = new Map(resultByCall);
			for (const call of runningCalls) callById.set(call.callId, call);
			const emittedCallIds = indexAssistantCallIds(nodes);
			const followingAssistants = indexFollowingAssistants(nodes);
			const callStartById = /* @__PURE__ */ new Map();
			for (const result of resultByCall.values()) {
				const startedAt = finiteTime(result.callTime);
				if (startedAt !== null) callStartById.set(result.callId, startedAt);
			}
			for (const call of runningCalls) {
				if (call.phase === "preparing") continue;
				const startedAt = finiteTime(call.time);
				if (startedAt !== null) callStartById.set(call.callId, startedAt);
			}
			const turns = /* @__PURE__ */ new Map();
			const standaloneCompactions = [];
			let index = 0;
			let prevAbsTime = null;
			let lastAssistantTurn = null;
			const bucket = (turn) => {
				let entry = turns.get(turn);
				if (entry === void 0) {
					entry = { groups: [] };
					turns.set(turn, entry);
				}
				return entry;
			};
			const pushMessage = (turn, laid) => {
				const groups = bucket(turn).groups;
				const last = groups.at(-1);
				if (last?.title === t("group.message")) {
					last.laid.push(laid);
					return;
				}
				groups.push({
					title: t("group.message"),
					laid: [laid]
				});
			};
			const pushStep = (turn, step, laid) => {
				if (laid.length === 0) return;
				const groups = bucket(turn).groups;
				const title = t("group.step", { step });
				const existing = groups.find((group) => group.title === title);
				if (existing !== void 0) {
					existing.laid.push(...laid);
					return;
				}
				groups.push({
					title,
					laid: [...laid]
				});
			};
			const pushStepInput = (turn, step, laid) => {
				if (laid.length === 0) return;
				const groups = bucket(turn).groups;
				const title = t("group.step", { step });
				const existing = groups.find((group) => group.title === title);
				if (existing === void 0) {
					groups.push({
						title,
						laid: [...laid]
					});
					return;
				}
				const request = existing.laid.findIndex((entry) => entry.cell.requestOnly === true);
				if (request === -1) existing.laid.push(...laid);
				else existing.laid.splice(request, 0, ...laid);
			};
			const representedRequests = /* @__PURE__ */ new Set();
			for (const node of nodes) if (node.kind === "assistant" && node.step > 0) representedRequests.add(`${node.turn}\u0000${node.step}`);
			if (partial !== null && partial.step > 0) representedRequests.add(`${partial.turn}\u0000${partial.step}`);
			for (const call of runningCalls) if (call.step > 0) representedRequests.add(`${call.turn}\u0000${call.step}`);
			const entries = [
				...(input.systemPrompts ?? []).map((prompt) => ({
					kind: "system",
					seq: prompt.seq,
					systemPrompt: prompt.text,
					change: {
						seq: prompt.seq,
						time: prompt.time,
						kind: prompt.update ? "system" : "initial"
					}
				})),
				...nodes.map((node, nodeIndex) => ({
					kind: "node",
					seq: node.seq,
					node,
					nodeIndex
				})),
				...requests.filter((request) => request.purpose === "compaction").map((request) => ({
					kind: "compaction",
					seq: request.startSeq,
					request
				})),
				...requests.flatMap((request) => request.purpose !== "assistant" || request.promptChange === void 0 || request.prompt === void 0 ? [] : [{
					kind: "system",
					seq: request.promptChange.seq,
					request,
					change: request.promptChange
				}]),
				...requests.filter((request) => request.purpose === "assistant").filter((request) => !representedRequests.has(`${request.turn}\u0000${request.step}`)).map((request) => ({
					kind: "request",
					seq: request.startSeq,
					request
				}))
			].sort((left, right) => layoutEntryOrder(left) - layoutEntryOrder(right));
			for (const entry of entries) {
				if (entry.kind === "request") {
					const { request } = entry;
					pushStep(request.turn, request.step, [{
						absTime: finiteTime(request.startedAt),
						cell: {
							index: ++index,
							kind: "message",
							text: "",
							sourceSeq: request.startSeq,
							requestOnly: true,
							timeSeconds: request.completedAt === null ? null : durationSeconds(request.completedAt, request.startedAt),
							startedAt: finiteTime(request.startedAt),
							...request.status === "error" ? { isError: true } : {}
						}
					}]);
					prevAbsTime = finiteTime(request.completedAt) ?? finiteTime(request.startedAt) ?? prevAbsTime;
					continue;
				}
				if (entry.kind === "system") {
					const { change, request } = entry;
					pushMessage(change.kind === "initial" ? firstVisibleTurn(nodes, partial) : enclosingPromptTurn(nodes, change.seq, partial), {
						absTime: finiteTime(change.time),
						cell: {
							index: ++index,
							kind: "system",
							text: promptChangeLabel(change, t),
							sourceSeq: change.seq,
							...request?.prompt === void 0 ? {} : { promptDetail: request.prompt },
							...entry.systemPrompt === void 0 ? {} : { systemPromptDetail: entry.systemPrompt },
							...change.previous === void 0 ? {} : { previousPromptDetail: change.previous },
							timeSeconds: 0,
							startedAt: finiteTime(change.time)
						}
					});
					prevAbsTime = finiteTime(change.time) ?? prevAbsTime;
					continue;
				}
				if (entry.kind === "compaction") {
					const request = entry.request;
					const rawOutput = request.rawOutput ?? request.summary;
					const thinkingDetail = rawOutput === void 0 ? "" : detailReasoning(rawOutput);
					const cell = {
						index: ++index,
						kind: "compacted",
						text: request.status === "running" ? t("layout.compacting") : request.status === "error" ? request.error === "trajectory.compaction-interrupted" ? t("layout.compactionInterrupted") : request.error ?? t("layout.compactionFailed") : request.summary === void 0 ? t("layout.compacted") : "",
						...request.status === "complete" && request.summary !== void 0 ? previewContentProperty(request.summary) : {},
						sourceSeq: request.startSeq,
						...request.summary === void 0 ? {} : {
							outputDetail: detailContent(request.summary),
							outputBlocks: request.summary.map((block) => sourceBlock(block))
						},
						...thinkingDetail === "" ? {} : { thinkingDetail },
						...rawOutput === void 0 ? {} : { sourceBlocks: rawOutput.map((block) => sourceBlock(block)) },
						...request.status === "error" ? { isError: true } : {},
						timeSeconds: request.completedAt === null ? null : durationSeconds(request.completedAt, request.startedAt),
						startedAt: finiteTime(request.startedAt)
					};
					attachUsage(cell, request.usage);
					const compaction = { groups: [{
						title: t("group.compaction", { seq: request.startSeq }),
						laid: [{
							absTime: finiteTime(request.startedAt),
							cell
						}]
					}] };
					if (request.turn === null) standaloneCompactions.push(compaction);
					else bucket(request.turn).groups.push(...compaction.groups);
					prevAbsTime = finiteTime(request.completedAt) ?? finiteTime(request.startedAt) ?? prevAbsTime;
					continue;
				}
				const { node, nodeIndex: i } = entry;
				if (node.kind === "user") {
					pushMessage(enclosingUserTurn(followingAssistants[i], partial, lastAssistantTurn), {
						absTime: finiteTime(node.time),
						cell: {
							index: ++index,
							kind: "user",
							...inputCellDetail(node, t),
							opensTurn: true
						}
					});
					prevAbsTime = finiteTime(node.time) ?? prevAbsTime;
					continue;
				}
				if (node.kind === "steering") {
					const placement = steeringPlacement(followingAssistants[i], partial, lastAssistantTurn, eventLocations?.get(node.seq));
					const laid = {
						absTime: finiteTime(node.time),
						cell: {
							index: ++index,
							kind: "user",
							...inputCellDetail(node, t)
						}
					};
					if (placement.step === void 0) pushMessage(placement.turn, laid);
					else pushStepInput(placement.turn, placement.step, [laid]);
					prevAbsTime = finiteTime(node.time) ?? prevAbsTime;
					continue;
				}
				if (node.kind === "assistant") {
					const laidList = withSubCalls(expandAssistant(node, index + 1, prevAbsTime, resultByCall, callStartById, callById, t), t);
					if (node.step > 0) pushStep(node.turn, node.step, laidList);
					else for (const laid of laidList) pushMessage(node.turn, laid);
					const last = laidList[laidList.length - 1];
					if (last !== void 0) index = last.cell.index;
					prevAbsTime = finiteTime(node.time) ?? prevAbsTime;
					lastAssistantTurn = node.turn;
					continue;
				}
				if (node.kind === "context") {
					pushMessage(enclosingUserTurn(followingAssistants[i], partial, lastAssistantTurn), {
						absTime: finiteTime(node.time),
						cell: {
							index: ++index,
							kind: "context",
							...inputCellDetail(node, t)
						}
					});
					prevAbsTime = finiteTime(node.time) ?? prevAbsTime;
					continue;
				}
				if (node.kind === "compaction") {
					prevAbsTime = finiteTime(node.time) ?? prevAbsTime;
					continue;
				}
				if (node.kind === "tool-result") {
					if (!emittedCallIds.has(node.callId)) {
						const toolName = node.call?.name;
						const resultPreview = summarizeResult(node, t);
						const laidList = [{
							absTime: finiteTime(node.callTime ?? node.time),
							...toolName !== void 0 ? { toolName } : {},
							callId: node.callId,
							subCalls: node.subCalls,
							cell: {
								index: ++index,
								kind: "tool",
								sourceSeq: node.seq,
								...node.call !== null ? summarizeCall(node.call.name, node.call.argsRaw) : resultAsText(resultPreview),
								...node.call !== null ? { inputDetail: node.call.argsRaw } : {},
								outputDetail: detailResult(node, t),
								outputBlocks: node.content.map((block) => sourceBlock(block)),
								...resultPreview,
								callId: node.callId,
								isError: node.isError,
								timeSeconds: durationSeconds(node.time, node.callTime),
								startedAt: finiteTime(node.callTime)
							}
						}];
						for (const laid of expandSubCalls(node.subCalls, index, t)) {
							laidList.push(laid);
							index = laid.cell.index;
						}
						pushStep(0, 1, laidList);
					}
					prevAbsTime = finiteTime(node.time) ?? prevAbsTime;
				}
			}
			if (partial !== null) {
				const laidList = withSubCalls(expandAssistant({
					kind: "assistant",
					seq: Number.MAX_SAFE_INTEGER,
					time: 0,
					turn: partial.turn,
					step: partial.step,
					blocks: partial.blocks
				}, index + 1, prevAbsTime, resultByCall, callStartById, callById, t, {
					streaming: true,
					timing: partial.timing
				}), t);
				if (partial.step > 0) pushStep(partial.turn, partial.step, laidList);
				else for (const laid of laidList) pushMessage(partial.turn, laid);
				const last = laidList[laidList.length - 1];
				if (last !== void 0) index = last.cell.index;
			}
			const seenCalls = collectCallIds(turns);
			for (const call of runningCalls) {
				if (call.phase === "preparing" || seenCalls.has(call.callId)) continue;
				const laidList = [{
					absTime: null,
					toolName: call.name,
					callId: call.callId,
					subCalls: call.subCalls,
					cell: {
						index: ++index,
						kind: "tool",
						...summarizeCall(call.name, call.argsRaw),
						inputDetail: call.argsRaw,
						callId: call.callId,
						timeSeconds: null,
						startedAt: finiteTime(call.time)
					}
				}];
				for (const laid of expandSubCalls(call.subCalls, index, t)) {
					laidList.push(laid);
					index = laid.cell.index;
				}
				if (call.step > 0) pushStep(call.turn, call.step, laidList);
				else for (const laid of laidList) pushMessage(call.turn, laid);
			}
			const prologue = turns.get(0);
			if (prologue !== void 0) {
				turns.delete(0);
				const emptyTurn = () => ({ groups: [] });
				const first = turns.get(1) ?? emptyTurn();
				first.groups = [...prologue.groups, ...first.groups];
				turns.set(1, first);
			}
			for (const entry of [...turns.values(), ...standaloneCompactions]) for (const group of entry.groups) for (const laid of group.laid) attachToolSchema(laid, callSchemas);
			return [...[...turns.entries()].map(([turn, entry]) => toTurnModel(turn, entry, t)), ...standaloneCompactions.map((entry) => toTurnModel(null, entry, t))].sort((left, right) => firstCellIndex(left) - firstCellIndex(right));
		}
		/**
		* Append the changing in-flight assistant cells to a stable finalized layout.
		* @param turns - Finalized layout derived with an empty-block partial anchor.
		* @param partial - Current in-flight assistant projection.
		* @param lastIndex - Highest cell index in the finalized layout.
		* @param t - Trajectory locale translator.
		* @returns The original layout without a partial, otherwise a layout sharing every unaffected turn.
		*/
		function appendTrajectoryPartialLayout(turns, partial, lastIndex, t) {
			if (partial === null) return turns;
			const partialTurn = deriveTrajectoryLayout({
				nodes: [],
				partial,
				runningCalls: []
			}, t).at(0);
			if (partialTurn === void 0) return turns;
			const streamed = {
				...partialTurn,
				groups: partialTurn.groups.map((group) => ({
					...group,
					cells: group.cells.map((cell) => ({
						...cell,
						index: cell.index + lastIndex
					}))
				}))
			};
			const turnIndex = turns.findIndex((turn) => turn.turn === streamed.turn);
			if (turnIndex === -1) return [...turns, streamed];
			const current = turns[turnIndex];
			/* v8 ignore next -- findIndex proved the dense array position exists. */
			if (current === void 0) return turns;
			const groups = [...current.groups];
			for (const streamedGroup of streamed.groups) {
				const groupIndex = groups.findIndex((group) => group.title === streamedGroup.title);
				if (groupIndex === -1) {
					groups.push(streamedGroup);
					continue;
				}
				const group = groups[groupIndex];
				/* v8 ignore next -- findIndex proved the dense array position exists. */
				if (group === void 0) continue;
				const streamedCallIds = new Set(streamedGroup.cells.flatMap((cell) => cell.callId === void 0 ? [] : [cell.callId]));
				groups[groupIndex] = {
					...streamedGroup,
					cells: [...group.cells.filter((cell) => cell.requestOnly !== true && (cell.callId === void 0 || !streamedCallIds.has(cell.callId))), ...streamedGroup.cells]
				};
			}
			const updated = [...turns];
			updated[turnIndex] = {
				...current,
				groups
			};
			return updated;
		}
		function attachToolSchema(laid, callSchemas) {
			if (laid.callId === void 0 || callSchemas === void 0) return;
			const schema = callSchemas.get(laid.callId);
			if (schema === void 0) return;
			laid.cell.schemaDetail = JSON.stringify(schema, null, 2);
		}
		function toTurnModel(turn, entry, t) {
			return {
				turn,
				groups: entry.groups.map(({ title, laid }) => {
					const description = groupDescription(laid, t);
					return {
						title,
						...description !== void 0 ? { description } : {},
						cells: laid.map((l) => l.cell)
					};
				})
			};
		}
		/** Chronological section position from the fold's monotonically assigned cell indexes. */
		function firstCellIndex(turn) {
			return Math.min(...turn.groups.flatMap((group) => group.cells.map((cell) => cell.index)), Number.POSITIVE_INFINITY);
		}
		/** Wall-span duration + tool histogram, e.g. `1.5 s bash×6`. */
		function groupDescription(laid, t) {
			const parts = [];
			const times = [];
			for (const l of laid) {
				if (l.absTime === null || !Number.isFinite(l.absTime)) continue;
				times.push(l.absTime);
				if (l.cell.kind === "tool" && l.cell.timeSeconds !== null && Number.isFinite(l.cell.timeSeconds)) times.push(l.absTime + l.cell.timeSeconds * 1e3);
			}
			if (times.length >= 2) {
				const span = formatGroupDuration((Math.max(...times) - Math.min(...times)) / 1e3, t);
				if (span !== void 0) parts.push(span);
			} else if (times.length === 1) {
				const own = laid.find((l) => l.absTime === times[0])?.cell.timeSeconds;
				const span = own !== null && own !== void 0 ? formatGroupDuration(own, t) : void 0;
				if (span !== void 0) parts.push(span);
			}
			const tools = /* @__PURE__ */ new Map();
			for (const l of laid) {
				if (l.toolName === void 0 || l.cell.kind !== "tool") continue;
				tools.set(l.toolName, (tools.get(l.toolName) ?? 0) + 1);
			}
			for (const [name, count] of tools) parts.push(count > 1 ? `${name}×${count}` : name);
			return parts.length === 0 ? void 0 : parts.join(" ");
		}
		function formatGroupDuration(seconds, t) {
			if (!Number.isFinite(seconds)) return void 0;
			return formatElapsedSeconds(seconds, t);
		}
		/** Own-duration seconds from two epoch-ms stamps; null when either is unusable. */
		function durationSeconds(later, earlier) {
			if (earlier === null || !Number.isFinite(later) || !Number.isFinite(earlier)) return null;
			return Math.max(0, (later - earlier) / 1e3);
		}
		/** Epoch-ms usable as an absolute time, else null. */
		function finiteTime(time) {
			return typeof time === "number" && Number.isFinite(time) ? time : null;
		}
		function expandAssistant(node, startIndex, prevAbsTime, results, callStarts, calls, t, opts) {
			if (opts?.streaming === true && node.blocks.length === 0) return [];
			const out = [];
			let index = startIndex - 1;
			const usage = node.usage;
			const streaming = opts?.streaming === true;
			const timing = opts?.timing ?? node.timing;
			const recordedStart = finiteTime(timing?.stepStartTime);
			const messageDuration = streaming ? null : durationSeconds(node.time, recordedStart ?? prevAbsTime);
			const nodeAbs = streaming ? null : finiteTime(node.time);
			const messageText = node.blocks.filter((block) => block.kind === "text" && (!streaming || block.text !== "")).map((block) => block.kind === "text" ? block.text : "").join("\n\n");
			const thinkingText = node.blocks.filter((block) => block.kind === "reasoning" && (!streaming || block.text !== "")).map((block) => block.kind === "reasoning" ? block.text : "").join("\n\n");
			const message = {
				index: ++index,
				recordId: `assistant\u0000${node.turn}\u0000${node.step}`,
				kind: "message",
				sourceSeq: node.seq,
				text: messageText !== "" || thinkingText !== "" ? "" : summarizeAssistantActivity(node.blocks, t),
				...messageText !== "" ? { previewMarkdown: messageText } : thinkingText !== "" ? { previewMarkdown: thinkingText } : {},
				...messageText !== "" ? { outputDetail: messageText } : {},
				...thinkingText !== "" ? { thinkingDetail: thinkingText } : {},
				sourceBlocks: node.blocks.map((block) => assistantSourceBlock(block)),
				...node.interrupted === true ? { isError: true } : {},
				timeSeconds: messageDuration,
				startedAt: recordedStart
			};
			attachUsage(message, usage);
			message.assistantMetrics = {
				timingRecorded: timing !== void 0,
				stepStartTime: timing?.stepStartTime ?? null,
				firstTokenTime: timing?.firstTokenTime ?? null,
				completedTime: streaming ? null : finiteTime(node.time),
				usageProvided: usage !== void 0,
				outputTokens: Number.isFinite(usage?.outputTokens) ? usage?.outputTokens ?? null : null
			};
			out.push({
				absTime: nodeAbs,
				cell: message
			});
			for (const block of node.blocks) {
				if (block.kind !== "tool-call") continue;
				const result = results.get(block.callId);
				const toolDuration = streaming || result === void 0 ? null : durationSeconds(result.time, result.callTime);
				const callAbs = finiteTime(callStarts.get(block.callId));
				const call = calls.get(block.callId);
				const resultPreview = result === void 0 ? void 0 : summarizeResult(result, t);
				out.push({
					absTime: callAbs,
					toolName: block.name,
					callId: block.callId,
					...call === void 0 ? {} : { subCalls: call.subCalls },
					cell: {
						index: ++index,
						kind: "tool",
						...summarizeCall(block.name, block.argsRaw),
						inputDetail: block.argsRaw,
						callId: block.callId,
						...result !== void 0 ? {
							outputDetail: detailResult(result, t),
							outputBlocks: result.content.map((block) => sourceBlock(block)),
							...resultPreview,
							isError: result.isError
						} : {},
						timeSeconds: toolDuration,
						startedAt: callAbs
					}
				});
			}
			return out;
		}
		function summarizeAssistantActivity(blocks, t) {
			const tools = /* @__PURE__ */ new Map();
			for (const block of blocks) {
				if (block.kind !== "tool-call") continue;
				tools.set(block.name, (tools.get(block.name) ?? 0) + 1);
			}
			if (tools.size > 0) return t("layout.toolCallOnly");
			const images = blocks.filter((block) => block.kind === "image").length;
			if (images > 0) return t("layout.imageCount", { count: images });
			return "";
		}
		function promptChangeLabel(change, t) {
			if (change.kind === "initial") return t("layout.initialSystemPrompt");
			if (change.kind === "system") return t("layout.systemPromptUpdated");
			if (change.kind === "tools") return t("layout.toolsUpdated");
			return t("layout.systemPromptAndToolsUpdated");
		}
		function assistantSourceBlock(block) {
			switch (block.kind) {
				case "text": return {
					type: "text",
					content: block.text
				};
				case "reasoning": return {
					type: "thinking",
					content: block.text
				};
				case "tool-call": return {
					type: "tool-call",
					content: block.argsRaw,
					callId: block.callId,
					toolName: block.name
				};
				case "image": return sourceBlock({
					type: "image",
					attachment: block.attachment
				});
				case "other": return sourceBlock(block.block);
			}
		}
		function sourceBlock(value) {
			if (typeof value !== "object" || value === null) return {
				type: "unknown",
				content: stringifySourceValue(value)
			};
			const block = value;
			const type = typeof block.type === "string" ? block.type : "unknown";
			if (typeof block.text === "string") return {
				type: type === "reasoning" ? "thinking" : type,
				content: block.text
			};
			if ((type === "image" || type === "file") && typeof block.attachment === "object" && block.attachment !== null && typeof block.attachment.attachmentId === "string") return {
				type,
				content: stringifySourceValue(value),
				...type === "image" ? { attachment: block.attachment } : { file: block.attachment }
			};
			return {
				type,
				content: stringifySourceValue(value)
			};
		}
		function imageBlockCount(content) {
			return content.filter((block) => block.type === "image").length;
		}
		function fileBlockCount(content) {
			return content.filter((block) => block.type === "file").length;
		}
		function stringifySourceValue(value) {
			return JSON.stringify(value, null, 2) || String(value);
		}
		/**
		* Turn that encloses a user/message: next assistant turn, else the
		* in-flight partial, else the turn after the last finalized assistant (or 1).
		*/
		function enclosingUserTurn(followingAssistant, partial, lastAssistantTurn) {
			if (followingAssistant !== void 0) return followingAssistant.turn;
			if (partial !== null) return partial.turn;
			if (lastAssistantTurn !== null) return lastAssistantTurn + 1;
			return 1;
		}
		function steeringPlacement(followingAssistant, partial, lastAssistantTurn, location) {
			if (location?.kind === "step") return {
				turn: location.turn.turn,
				step: location.step.step
			};
			const locatedTurn = location?.kind === "turn" ? location.turn.turn : void 0;
			if (followingAssistant !== void 0 && (locatedTurn === void 0 || followingAssistant.turn === locatedTurn)) return {
				turn: followingAssistant.turn,
				...followingAssistant.step > 0 ? { step: followingAssistant.step } : {}
			};
			if (partial !== null && (locatedTurn === void 0 || partial.turn === locatedTurn)) return {
				turn: partial.turn,
				...partial.step > 0 ? { step: partial.step } : {}
			};
			if (locatedTurn !== void 0) return { turn: locatedTurn };
			return { turn: lastAssistantTurn ?? 1 };
		}
		function indexFollowingAssistants(nodes) {
			const following = new Array(nodes.length);
			let assistant;
			for (let index = nodes.length - 1; index >= 0; index--) {
				following[index] = assistant;
				const node = nodes[index];
				if (node?.kind === "assistant") assistant = node;
			}
			return following;
		}
		function enclosingPromptTurn(nodes, seq, partial) {
			const next = nodes.find((node) => node.seq > seq && node.kind === "assistant" && node.step > 0);
			if (next?.kind === "assistant") return next.turn;
			return partial?.turn ?? 1;
		}
		/** Earliest raw turn represented by the selected trajectory branch. */
		function firstVisibleTurn(nodes, partial) {
			const turns = nodes.flatMap((node) => node.kind === "assistant" && node.turn > 0 ? [node.turn] : []);
			if (partial !== null && partial.turn > 0) turns.push(partial.turn);
			return turns.length === 0 ? 1 : Math.min(...turns);
		}
		/** Copy provider usage onto a Message cell when present. */
		function attachUsage(cell, usage) {
			if (usage === void 0) return;
			if (usage.inputTokens !== void 0) cell.input = usage.inputTokens;
			if (usage.cacheReadTokens !== void 0) cell.cacheRead = usage.cacheReadTokens;
			if (usage.cacheWriteTokens !== void 0) cell.cacheWrite = usage.cacheWriteTokens;
			if (usage.outputTokens !== void 0) cell.output = usage.outputTokens;
			if (usage.reasoningTokens !== void 0) cell.think = usage.reasoningTokens;
		}
		function indexResults(nodes) {
			const map = /* @__PURE__ */ new Map();
			for (const node of nodes) if (node.kind === "tool-result") map.set(node.callId, node);
			return map;
		}
		function indexAssistantCallIds(nodes) {
			const ids = /* @__PURE__ */ new Set();
			for (const node of nodes) {
				if (node.kind !== "assistant") continue;
				for (const block of node.blocks) if (block.kind === "tool-call") ids.add(block.callId);
			}
			return ids;
		}
		function collectCallIds(turns) {
			const ids = /* @__PURE__ */ new Set();
			for (const entry of turns.values()) for (const group of entry.groups) for (const laid of group.laid) if (laid.callId !== void 0) ids.add(laid.callId);
			return ids;
		}
		/** Interleave each tool cell's nested child calls right after it, reindexing followers. */
		function withSubCalls(laidList, t) {
			if (!laidList.some((laid) => laid.subCalls !== void 0 && laid.subCalls.length > 0)) return laidList;
			const out = [];
			let index = laidList[0] !== void 0 ? laidList[0].cell.index - 1 : 0;
			for (const laid of laidList) {
				out.push({
					...laid,
					cell: {
						...laid.cell,
						index: ++index
					}
				});
				for (const sub of expandSubCalls(laid.subCalls, index, t)) {
					out.push(sub);
					index = sub.cell.index;
				}
			}
			return out;
		}
		/** Sub-dispatch cells for one run_code parent, in start order (running = null duration). */
		function expandSubCalls(subs, startIndex, t) {
			if (subs === void 0 || subs.length === 0) return [];
			const out = [];
			let index = startIndex;
			for (const sub of subs) {
				if (!("kind" in sub) && sub.phase === "preparing") continue;
				const settled = "kind" in sub;
				const resultPreview = settled ? summarizeResult(sub, t) : void 0;
				const laid = {
					absTime: settled ? finiteTime(sub.callTime ?? sub.time) : finiteTime(sub.time),
					toolName: settled ? sub.call?.name ?? sub.callId : sub.name,
					callId: sub.callId,
					cell: {
						index: ++index,
						kind: "subtool",
						callId: sub.callId,
						...settled ? sub.call !== null ? summarizeCall(sub.call.name, sub.call.argsRaw) : resultAsText(resultPreview) : summarizeCall(sub.name, sub.argsRaw),
						...settled ? sub.call !== null ? { inputDetail: sub.call.argsRaw } : {} : { inputDetail: sub.argsRaw },
						...settled ? {
							outputDetail: detailResult(sub, t),
							outputBlocks: sub.content.map((block) => sourceBlock(block)),
							...resultPreview,
							isError: sub.isError
						} : {},
						timeSeconds: settled ? durationSeconds(sub.time, sub.callTime) : null,
						startedAt: settled ? finiteTime(sub.callTime) : finiteTime(sub.time)
					}
				};
				out.push(laid);
				for (const child of expandSubCalls(sub.subCalls, index, t)) {
					out.push(child);
					index = child.cell.index;
				}
			}
			return out;
		}
		function summarizeCall(name, argsRaw) {
			return {
				toolName: name,
				text: name,
				...argsRaw === "" ? {} : { previewMarkdown: argsRaw }
			};
		}
		function summarizeResult(node, t) {
			if (node.isError) return { result: node.error?.code ?? "error" };
			for (const block of node.content) if (block.type === "text" && typeof block.text === "string" && block.text !== "") return {
				result: "",
				resultPreviewMarkdown: block.text
			};
			const images = imageBlockCount(node.content);
			if (images > 0) return { result: t("layout.imageCount", { count: images }) };
			return { result: t("record.noOutput") };
		}
		function resultAsText(result) {
			return {
				text: result?.result ?? "",
				...result?.resultPreviewMarkdown === void 0 ? {} : { previewMarkdown: result.resultPreviewMarkdown }
			};
		}
		function detailResult(node, t) {
			if (node.isError) return node.error === void 0 ? "error" : `${node.error.name}: ${node.error.code}`;
			const text = node.content.filter((block) => block.type === "text" && typeof block.text === "string").map((block) => block.type === "text" ? block.text : "").join("\n");
			if (text !== "") return text;
			const images = imageBlockCount(node.content);
			if (images > 0) return t("layout.imageCount", { count: images });
			if (node.content.length === 0 || node.content.every((block) => block.type === "text" && (typeof block.text !== "string" || block.text === ""))) return t("record.noOutput");
			return JSON.stringify(node.content, null, 2);
		}
		function detailContent(content) {
			return content.filter((block) => block.type === "text" && typeof block.text === "string").map((block) => block.text ?? "").join("\n");
		}
		function detailReasoning(content) {
			return content.filter((block) => block.type === "reasoning" && typeof block.text === "string").map((block) => block.text ?? "").join("\n");
		}
		function previewContent(content) {
			for (const block of content) if (block.type === "text" && typeof block.text === "string") return block.text;
		}
		function previewContentProperty(content) {
			const previewMarkdown = previewContent(content);
			return previewMarkdown === void 0 ? {} : { previewMarkdown };
		}
		//#endregion
		//#region lib/types/client/trajectory-search-index.js
		/** Incremental full-text index for the trajectory ledger. */
		function searchableJson(value) {
			if (value === void 0) return "";
			try {
				return JSON.stringify(value);
			} catch {
				return "";
			}
		}
		function sameSources(left, right) {
			return left.length === right.length && left.every((value, index) => value === right[index]);
		}
		function markdownPreview(cell) {
			if (cell.previewMarkdown === void 0) return "";
			const preview = trajectoryPreviewText(cell.previewMarkdown);
			if (cell.text === "") return preview;
			return preview === "" ? cell.text : `${cell.text} · ${preview}`;
		}
		function resultPreview(cell) {
			return cell.resultPreviewMarkdown === void 0 ? cell.result ?? "" : trajectoryPreviewText(cell.resultPreviewMarkdown);
		}
		function recordSources(turn, group, cell) {
			const blocks = [...cell.sourceBlocks ?? [], ...cell.outputBlocks ?? []];
			return [
				turn === null ? "between turns" : `turn ${turn}`,
				group,
				cell.kind,
				cell.kind === "message" ? "assistant" : "",
				cell.text,
				cell.previewMarkdown ?? "",
				cell.inputDetail ?? "",
				cell.outputDetail ?? "",
				cell.thinkingDetail ?? "",
				cell.schemaDetail ?? "",
				cell.result ?? "",
				cell.resultPreviewMarkdown ?? "",
				cell.callId ?? "",
				...blocks.flatMap((block) => [
					block.type,
					block.content,
					block.callId ?? "",
					block.toolName ?? "",
					block.attachment?.name ?? ""
				]),
				searchableJson(cell.messageSource),
				searchableJson(cell.promptDetail),
				searchableJson(cell.previousPromptDetail)
			];
		}
		/** Session-view-local index that reparses Markdown only when one record's source changes. */
		var TrajectorySearchIndex = class {
			entries = /* @__PURE__ */ new Map();
			layouts;
			/**
			* Incrementally synchronize one or more current trajectory layout slices.
			* @param layouts - Finalized and optional streaming layouts from the same view.
			* @returns Whether the indexed layout version changed.
			*/
			update(layouts) {
				if (this.layouts === layouts) return false;
				this.layouts = layouts;
				const seen = /* @__PURE__ */ new Set();
				for (const turns of layouts) for (const turn of turns) for (const group of turn.groups) for (const cell of group.cells) {
					if (cell.requestOnly === true) continue;
					const id = trajectoryRecordId(cell);
					const sources = recordSources(turn.turn, group.title, cell);
					const previous = this.entries.get(id);
					const entry = previous !== void 0 && sameSources(previous.sources, sources) ? previous : {
						sources,
						text: [
							...sources,
							markdownPreview(cell),
							resultPreview(cell)
						].join("\n").toLocaleLowerCase()
					};
					this.entries.set(id, entry);
					seen.add(id);
				}
				for (const id of this.entries.keys()) if (!seen.has(id)) this.entries.delete(id);
				return true;
			}
			/**
			* Match a query against the latest committed index version.
			* @param query - Space-separated case-insensitive search terms.
			* @returns Matching stable record identities, or `null` without a query.
			*/
			search(query) {
				const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
				if (terms.length === 0) return null;
				const matches = /* @__PURE__ */ new Set();
				for (const [id, entry] of this.entries) if (terms.every((term) => entry.text.includes(term))) matches.add(id);
				return matches;
			}
		};
		//#endregion
		//#region \0dsh-css:/home/runner/work/deepseek-harness/deepseek-harness/packages/client/ui-trajectory/src/client/views.module.css.mjs
		const css = ".rtjqBU-ya_root{--dsh-trajectory-toolbar-height:32px;box-sizing:border-box;width:100%;height:100%;min-height:0;color:var(--dsw-alias-label-primary);background:var(--dsw-alias-bg-layer-1);flex-direction:column;display:flex;overflow:hidden}.rtjqBU-ya_ledger{z-index:0;isolation:isolate;--dsh-trajectory-bottom-clearance:calc(var(--dsh-composer-height,152px) + 16px);flex:1;min-width:0;min-height:0;display:flex;position:relative;overflow:hidden}";
		const tagId = "@rth/dsh-rth-trajectory/views.module.css";
		if (typeof document !== "undefined" && document.querySelector("style[data-plugin-css=" + JSON.stringify(tagId) + "]") === null) {
			const tag = document.createElement("style");
			tag.dataset.plugin = "@rth/dsh-rth-trajectory";
			tag.dataset.pluginCss = tagId;
			tag.textContent = css;
			document.head.appendChild(tag);
		}
		var views_module_css_default = {
			"ledger": "rtjqBU-ya_ledger",
			"root": "rtjqBU-ya_root"
		};
		//#endregion
		//#region lib/types/client/TrajectoryView.js
		/** Trajectory view: compact summary over a turn-aware event ledger. */
		const EMPTY_TURN_IDS = /* @__PURE__ */ new Set();
		const EMPTY_RECORD_IDS = /* @__PURE__ */ new Set();
		const SEARCH_INDEX_THROTTLE_MS = 3e3;
		const HISTORY_PAGE_NODES = 50;
		function containsCall(calls, callId) {
			for (const call of calls) if (call.callId === callId || containsCall(call.subCalls, callId)) return true;
			return false;
		}
		function lastCellIndex(turns) {
			let last = 0;
			for (const turn of turns) for (const group of turn.groups) for (const cell of group.cells) last = Math.max(last, cell.index);
			return last;
		}
		function timelineBlock(block) {
			switch (block.kind) {
				case "text": return {
					kind: "text",
					text: ""
				};
				case "reasoning": return {
					kind: "reasoning",
					text: ""
				};
				case "image": return block;
				case "tool-call": return {
					kind: "tool-call",
					callId: block.callId,
					name: block.name,
					argsRaw: ""
				};
				case "other": return {
					kind: "other",
					block: null
				};
			}
		}
		function partialStructureSignature(partial) {
			if (partial === null) return "";
			return partial.blocks.map((block) => block.kind === "tool-call" ? `${block.kind}:${block.callId}:${block.name}` : block.kind).join("\0");
		}
		function requestUsage(value) {
			const usage = value;
			if (usage === void 0) return void 0;
			return {
				...usage.inputTokens === void 0 ? {} : { input: usage.inputTokens },
				...usage.cacheReadTokens === void 0 ? {} : { cacheRead: usage.cacheReadTokens },
				...usage.cacheWriteTokens === void 0 ? {} : { cacheWrite: usage.cacheWriteTokens },
				...usage.outputTokens === void 0 ? {} : { output: usage.outputTokens },
				...usage.reasoningTokens === void 0 ? {} : { reasoning: usage.reasoningTokens }
			};
		}
		function addUsage(total, usage) {
			if (usage === void 0) return total;
			return {
				...total?.input === void 0 && usage.input === void 0 ? {} : { input: (total?.input ?? 0) + (usage.input ?? 0) },
				...total?.cacheRead === void 0 && usage.cacheRead === void 0 ? {} : { cacheRead: (total?.cacheRead ?? 0) + (usage.cacheRead ?? 0) },
				...total?.cacheWrite === void 0 && usage.cacheWrite === void 0 ? {} : { cacheWrite: (total?.cacheWrite ?? 0) + (usage.cacheWrite ?? 0) },
				...total?.output === void 0 && usage.output === void 0 ? {} : { output: (total?.output ?? 0) + (usage.output ?? 0) },
				...total?.reasoning === void 0 && usage.reasoning === void 0 ? {} : { reasoning: (total?.reasoning ?? 0) + (usage.reasoning ?? 0) }
			};
		}
		function TrajectoryView({ useSession, useTrajectory, useDuration, loadOlder, loadImage, setActualDuration, viewRequest, completeViewRequest, renderSlot, t, jsonStringWrapping }) {
			const [collapsedTurns, setCollapsedTurns] = (0, react.useState)(EMPTY_TURN_IDS);
			const [collapsedAssistants, setCollapsedAssistants] = (0, react.useState)(EMPTY_RECORD_IDS);
			const [timelineSelection, setTimelineSelection] = (0, react.useState)(null);
			const actualDuration = useDuration((value) => value);
			const [actualTime, setActualTime] = (0, react.useState)(false);
			const [searchQuery, setSearchQuery] = (0, react.useState)("");
			const [searchIndex] = (0, react.useState)(() => new TrajectorySearchIndex());
			const [searchIndexRevision, setSearchIndexRevision] = (0, react.useState)(0);
			const searchIndexTimer = (0, react.useRef)(null);
			const searchIndexInitialized = (0, react.useRef)(false);
			const [selectedTimelineIndex, setSelectedTimelineIndex] = (0, react.useState)(null);
			const [timelineRecordSelection, setTimelineRecordSelection] = (0, react.useState)(null);
			const [timelineRecordFocus, setTimelineRecordFocus] = (0, react.useState)(null);
			const completeInspection = useTrajectory((snapshot) => snapshot);
			const latestNodeSeq = completeInspection.eventNodes.at(-1)?.seq;
			const [historyTailSeq, setHistoryTailSeq] = (0, react.useState)(latestNodeSeq);
			const [historyNodeLimit, setHistoryNodeLimit] = (0, react.useState)(HISTORY_PAGE_NODES);
			const fixedTailSeq = historyTailSeq ?? latestNodeSeq;
			const historyTailIndex = fixedTailSeq === void 0 ? -1 : completeInspection.eventNodes.findLastIndex((node) => node.seq <= fixedTailSeq);
			const historyEndIndex = historyTailIndex < 0 && latestNodeSeq !== void 0 ? completeInspection.eventNodes.length : historyTailIndex + 1;
			const historyStartIndex = Math.max(0, historyEndIndex - historyNodeLimit);
			(0, react.useEffect)(() => {
				if (latestNodeSeq !== void 0 && (historyTailSeq === void 0 || historyTailIndex < 0)) setHistoryTailSeq(latestNodeSeq);
			}, [
				historyTailIndex,
				historyTailSeq,
				latestNodeSeq
			]);
			const inspection = (0, react.useMemo)(() => {
				if (historyStartIndex === 0) return completeInspection;
				const eventNodes = completeInspection.eventNodes.slice(historyStartIndex);
				const firstSeq = eventNodes[0]?.seq ?? 0;
				return {
					...completeInspection,
					eventNodes,
					requests: completeInspection.requests.filter((request) => request.startSeq >= firstSeq || (request.resultSeq ?? -1) >= firstSeq)
				};
			}, [completeInspection, historyStartIndex]);
			const historyLoading = useSession((snapshot) => snapshot.openState === "loading");
			const olderHistoryLoading = useSession((snapshot) => snapshot.loadingOlder);
			const sessionHasOlderHistory = useSession((snapshot) => snapshot.hasMore);
			const hasResidentOlderHistory = historyStartIndex > 0;
			const hasOlderHistory = hasResidentOlderHistory || sessionHasOlderHistory;
			const nodes = inspection.eventNodes;
			const eventLocations = inspection.eventLocations;
			const historyBaseSeq = nodes[0]?.seq ?? 0;
			const partial = inspection.partial;
			const runningCalls = inspection.runningCalls;
			const requests = inspection.requests;
			const callSchemas = inspection.callSchemas;
			const inspectCallId = viewRequest?.view === "trajectory" ? viewRequest.focus : null;
			const inspectNodeIndex = (0, react.useMemo)(() => inspectCallId === null ? -1 : completeInspection.eventNodes.findIndex((node) => node.kind === "assistant" ? node.blocks.some((block) => block.kind === "tool-call" && block.callId === inspectCallId) : node.kind === "tool-result" && containsCall([node], inspectCallId)), [completeInspection.eventNodes, inspectCallId]);
			(0, react.useEffect)(() => {
				if (inspectNodeIndex < 0 || inspectNodeIndex >= historyStartIndex) return;
				setHistoryNodeLimit((limit) => limit + historyStartIndex - inspectNodeIndex);
			}, [historyStartIndex, inspectNodeIndex]);
			const requestNumbers = (0, react.useMemo)(() => {
				const assistantsByStep = /* @__PURE__ */ new Map();
				for (const node of completeInspection.eventNodes) {
					if (node.kind !== "assistant" || node.step <= 0) continue;
					assistantsByStep.set(`${node.turn}\u0000${node.step}`, node);
				}
				const requestsByStep = new Map(completeInspection.requests.filter((request) => request.purpose === "assistant").map((request) => [`${request.turn}\u0000${request.step}`, request]));
				const orderedRequests = [...completeInspection.requests.map((request) => ({
					seq: request.startSeq,
					request,
					node: request.purpose === "assistant" ? assistantsByStep.get(`${request.turn}\u0000${request.step}`) : void 0
				})), ...[...assistantsByStep.entries()].flatMap(([key, node]) => requestsByStep.has(key) ? [] : [{
					seq: node.seq,
					request: void 0,
					node
				}])].sort((left, right) => left.seq - right.seq);
				const numbered = [];
				let cumulativeUsage;
				for (const [index, entry] of orderedRequests.entries()) {
					const usage = requestUsage(entry.request?.usage ?? entry.node?.usage);
					cumulativeUsage = addUsage(cumulativeUsage, usage);
					if (entry.request?.purpose !== "compaction") {
						const request = entry.request;
						const node = entry.node;
						const turn = request?.turn ?? node?.turn;
						const step = request?.step ?? node?.step;
						if (turn === void 0 || step === void 0) continue;
						const provider = request?.providerMetadata?.provider ?? node?.providerMetadata?.provider;
						const model = request?.providerMetadata?.model ?? node?.providerMetadata?.model;
						const requestConfig = request?.requestConfig ?? node?.requestConfig;
						const startedAt = request?.startedAt;
						const completedAt = request === void 0 ? node?.time : request.completedAt;
						const resultSeq = request?.resultSeq ?? node?.seq;
						numbered.push({
							seq: entry.seq,
							turn,
							step,
							group: t("group.step", { step }),
							number: index + 1,
							status: request?.status ?? (node?.interrupted === true ? "error" : "complete"),
							...startedAt === void 0 ? {} : { startedAt },
							...completedAt === void 0 ? {} : { completedAt },
							...request?.error === void 0 ? {} : { error: request.error },
							...request?.errorCode === void 0 ? {} : { errorCode: request.errorCode },
							...resultSeq === void 0 ? {} : { resultSeq },
							...request?.retry === void 0 ? {} : { retry: request.retry },
							...request?.maxRetries === void 0 ? {} : { maxRetries: request.maxRetries },
							...request?.retryDelayMs === void 0 ? {} : { retryDelayMs: request.retryDelayMs },
							...provider === void 0 ? {} : { provider },
							...model === void 0 ? {} : { model },
							...requestConfig === void 0 ? {} : { requestConfig },
							...usage === void 0 ? {} : { usage },
							...cumulativeUsage === void 0 ? {} : { cumulativeUsage }
						});
						continue;
					}
					const request = entry.request;
					numbered.push({
						seq: request.startSeq,
						turn: request.turn,
						step: 0,
						group: t("group.compaction", { seq: request.startSeq }),
						number: index + 1,
						purpose: "compaction",
						status: request.status,
						startedAt: request.startedAt,
						completedAt: request.completedAt,
						...request.error === void 0 ? {} : { error: request.error },
						...request.errorCode === void 0 ? {} : { errorCode: request.errorCode },
						...request.status === "running" ? {} : { resultSeq: request.startSeq },
						...request.providerMetadata?.provider === void 0 ? {} : { provider: request.providerMetadata.provider },
						...request.providerMetadata?.model === void 0 ? {} : { model: request.providerMetadata.model },
						...request.requestConfig === void 0 ? {} : { requestConfig: request.requestConfig },
						...usage === void 0 ? {} : { usage },
						...cumulativeUsage === void 0 ? {} : { cumulativeUsage }
					});
				}
				return numbered;
			}, [
				completeInspection.eventNodes,
				completeInspection.requests,
				t
			]);
			const partialTurn = partial?.turn ?? null;
			const partialStep = partial?.step ?? null;
			const finalized = (0, react.useMemo)(() => {
				const turns = deriveTrajectoryLayout({
					nodes,
					eventLocations,
					partial: partialTurn === null || partialStep === null ? null : {
						turn: partialTurn,
						step: partialStep,
						blocks: []
					},
					runningCalls,
					requests,
					systemPrompts: inspection.systemPrompts,
					callSchemas
				}, t);
				return {
					turns,
					lastIndex: lastCellIndex(turns)
				};
			}, [
				nodes,
				eventLocations,
				partialTurn,
				partialStep,
				runningCalls,
				requests,
				inspection.systemPrompts,
				callSchemas,
				t
			]);
			const timelinePartialSignature = partialStructureSignature(partial);
			const partialStartTime = partial?.timing?.stepStartTime;
			const partialFirstTokenTime = partial?.timing?.firstTokenTime;
			const timelinePartial = (0, react.useMemo)(() => partial === null ? null : {
				turn: partial.turn,
				step: partial.step,
				blocks: partial.blocks.map((block) => timelineBlock(block)),
				...partial.timing === void 0 ? {} : { timing: partial.timing }
			}, [
				partialStep,
				partialTurn,
				timelinePartialSignature,
				partialStartTime,
				partialFirstTokenTime
			]);
			const timelineTurns = (0, react.useMemo)(() => appendTrajectoryPartialLayout(finalized.turns, timelinePartial, finalized.lastIndex, t), [
				finalized,
				timelinePartial,
				t
			]);
			const timelineMode = actualDuration ? actualTime ? "actual" : "duration" : actualTime ? "time" : "sequence";
			const partialSearchTurns = (0, react.useMemo)(() => appendTrajectoryPartialLayout([], partial, finalized.lastIndex, t), [
				finalized.lastIndex,
				partial,
				t
			]);
			const searchLayouts = (0, react.useMemo)(() => [finalized.turns, partialSearchTurns], [finalized, partialSearchTurns]);
			const latestSearchLayouts = (0, react.useRef)(searchLayouts);
			latestSearchLayouts.current = searchLayouts;
			(0, react.useEffect)(() => {
				if (!searchIndexInitialized.current) {
					searchIndexInitialized.current = true;
					if (searchIndex.update(searchLayouts)) setSearchIndexRevision((revision) => revision + 1);
					return;
				}
				if (searchIndexTimer.current !== null) return;
				searchIndexTimer.current = setTimeout(() => {
					searchIndexTimer.current = null;
					if (searchIndex.update(latestSearchLayouts.current)) setSearchIndexRevision((revision) => revision + 1);
				}, SEARCH_INDEX_THROTTLE_MS);
			}, [searchIndex, searchLayouts]);
			(0, react.useEffect)(() => () => {
				if (searchIndexTimer.current !== null) clearTimeout(searchIndexTimer.current);
			}, []);
			const streamingCells = (0, react.useMemo)(() => partialSearchTurns.flatMap((turn) => turn.groups.flatMap((group) => group.cells)), [partialSearchTurns]);
			const searchMatchRecordIds = (0, react.useMemo)(() => searchIndex.search(searchQuery), [
				searchIndex,
				searchIndexRevision,
				searchQuery
			]);
			const searchMatchIndexes = (0, react.useMemo)(() => {
				if (searchMatchRecordIds === null) return null;
				const indexes = /* @__PURE__ */ new Set();
				for (const turns of searchLayouts) for (const turn of turns) for (const group of turn.groups) for (const cell of group.cells) if (searchMatchRecordIds.has(trajectoryRecordId(cell))) indexes.add(cell.index);
				return indexes;
			}, [searchLayouts, searchMatchRecordIds]);
			const timelineRange = timelineSelection;
			const timelineFocusIndexes = (0, react.useMemo)(() => timelineRange === null ? null : trajectoryTimelineFocusIndexes(timelineTurns, timelineRange, timelineMode), [
				timelineMode,
				timelineRange,
				timelineTurns
			]);
			const handleRecordSelect = (0, react.useCallback)((index) => {
				if (timelineFocusIndexes !== null && !timelineFocusIndexes.has(index)) setTimelineSelection(null);
			}, [timelineFocusIndexes]);
			const handleTimelineRangeChange = (0, react.useCallback)((range) => {
				setTimelineSelection(range);
			}, []);
			const handleTimelineRecordSelect = (0, react.useCallback)((index) => {
				setTimelineSelection(null);
				setTimelineRecordSelection({ index });
				setSelectedTimelineIndex(index);
			}, []);
			const handleTimelineRecordFocus = (0, react.useCallback)((index) => {
				setTimelineRecordFocus({ index });
			}, []);
			const collapsibleTurnIds = (0, react.useMemo)(() => timelineTurns.filter((turn) => turn.turn !== null && turn.groups.reduce((count, group) => count + group.cells.filter((cell) => cell.requestOnly !== true && cell.kind !== "system").length, 0) > 1).flatMap((turn) => turn.turn === null ? [] : [turn.turn]), [timelineTurns]);
			const allTurnsCollapsed = collapsibleTurnIds.length > 0 && collapsibleTurnIds.every((turn) => collapsedTurns.has(turn));
			const collapsibleAssistantIds = (0, react.useMemo)(() => {
				const ids = [];
				for (const turn of timelineTurns) {
					const cells = turn.groups.flatMap((group) => group.cells);
					for (let i = 0; i < cells.length; i++) {
						const cell = cells[i];
						if (cell?.kind !== "message") continue;
						const next = cells[i + 1];
						if (next?.kind === "tool" || next?.kind === "subtool") ids.push(trajectoryRecordId(cell));
					}
				}
				return ids;
			}, [timelineTurns]);
			const allAssistantsCollapsed = collapsibleAssistantIds.length > 0 && collapsibleAssistantIds.every((index) => collapsedAssistants.has(index));
			const toggleTurn = (turn) => {
				setCollapsedTurns((current) => {
					const collapsed = new Set(current);
					if (collapsed.has(turn)) collapsed.delete(turn);
					else collapsed.add(turn);
					return collapsed;
				});
			};
			const toggleAllTurns = () => {
				setCollapsedTurns((current) => {
					const collapsed = new Set(current);
					if (allTurnsCollapsed) for (const turn of collapsibleTurnIds) collapsed.delete(turn);
					else for (const turn of collapsibleTurnIds) collapsed.add(turn);
					return collapsed;
				});
			};
			const toggleAssistant = (id) => {
				setCollapsedAssistants((current) => {
					const collapsed = new Set(current);
					if (collapsed.has(id)) collapsed.delete(id);
					else collapsed.add(id);
					return collapsed;
				});
			};
			const toggleAllAssistants = () => {
				setCollapsedAssistants((current) => {
					const collapsed = new Set(current);
					if (allAssistantsCollapsed) for (const index of collapsibleAssistantIds) collapsed.delete(index);
					else for (const index of collapsibleAssistantIds) collapsed.add(index);
					return collapsed;
				});
			};
			const loadEarlierHistory = (0, react.useCallback)(async () => {
				if (!hasResidentOlderHistory && !await loadOlder()) return false;
				setHistoryNodeLimit((limit) => limit + HISTORY_PAGE_NODES);
				return true;
			}, [hasResidentOlderHistory, loadOlder]);
			return (0, react_jsx_runtime.jsxs)("div", {
				className: views_module_css_default.root,
				"data-conversation-composer-overlay": "",
				children: [
					(0, react_jsx_runtime.jsx)(TrajectoryToolbar, {
						actualDuration,
						onActualDurationChange: (nextActualDuration) => {
							setActualDuration(nextActualDuration);
							setTimelineSelection(null);
						},
						actualTime,
						onActualTimeChange: (nextActualTime) => {
							setActualTime(nextActualTime);
							setTimelineSelection(null);
						},
						allTurnsCollapsed,
						onToggleAllTurns: toggleAllTurns,
						allAssistantsCollapsed,
						onToggleAllAssistants: toggleAllAssistants,
						searchQuery,
						onSearchQueryChange: setSearchQuery,
						t
					}),
					(0, react_jsx_runtime.jsx)(TrajectoryTimeline, {
						t,
						turns: timelineTurns,
						mode: timelineMode,
						range: timelineRange,
						hasEarlierRecords: hasOlderHistory,
						onLoadEarlier: loadEarlierHistory,
						selectedIndex: selectedTimelineIndex,
						searchMatchIndexes,
						onRangeChange: handleTimelineRangeChange,
						onRecordSelect: handleTimelineRecordSelect,
						onRecordFocus: handleTimelineRecordFocus
					}),
					(0, react_jsx_runtime.jsx)("div", {
						className: views_module_css_default.ledger,
						children: (0, react_jsx_runtime.jsx)(TrajectoryTable, {
							t,
							stringWrapping: jsonStringWrapping === void 0 ? void 0 : {
								...jsonStringWrapping,
								label: t("record.wrapLines")
							},
							renderSlot,
							loadImage,
							requestNumbers,
							turns: timelineTurns,
							streamingCells,
							timelineFocusIndexes,
							searchMatchIndexes,
							onSelectedIndexChange: setSelectedTimelineIndex,
							onRecordSelect: handleRecordSelect,
							recordSelection: timelineRecordSelection,
							recordFocus: timelineRecordFocus,
							historyLoading,
							olderHistoryLoading,
							historyStartSeq: historyBaseSeq,
							hasOlderRecords: hasOlderHistory,
							onLoadOlder: loadEarlierHistory,
							onClearSelection: () => {
								setTimelineSelection(null);
							},
							collapsedTurns,
							onToggleTurn: toggleTurn,
							collapsedAssistants,
							onToggleAssistant: toggleAssistant,
							inspectCallId,
							onInspectApplied: completeViewRequest
						})
					})
				]
			});
		}
		//#endregion
		//#region rth/RthTrajectoryView.js (RTH addition, see NOTICE.md)
		/**
		* The RTH trajectory view: upstream's Toolbar, Overview, Table and inspector over RTH's
		* Mind projection (src/adapter.mjs), with kind filter chips and a session strip.
		* Upstream TrajectoryView above is kept unchanged and unused.
		*/
		const RTH_DEFAULT_GROUPS = () => new Set(RTH.DEFAULT_GROUPS);
		function RthTrajectoryView({ useRth, useDuration, loadImage, setActualDuration, viewRequest, completeViewRequest, renderSlot, t, jsonStringWrapping }) {
			RTH.ensureStyle();
			const data = useRth((snapshot) => snapshot);
			const [enabled, setEnabled] = (0, react.useState)(RTH_DEFAULT_GROUPS);
			const [collapsedTurns, setCollapsedTurns] = (0, react.useState)(EMPTY_TURN_IDS);
			const [collapsedAssistants, setCollapsedAssistants] = (0, react.useState)(EMPTY_RECORD_IDS);
			const [timelineSelection, setTimelineSelection] = (0, react.useState)(null);
			const [searchQuery, setSearchQuery] = (0, react.useState)("");
			const [searchIndex] = (0, react.useState)(() => new TrajectorySearchIndex());
			const [searchIndexRevision, setSearchIndexRevision] = (0, react.useState)(0);
			// Selection is held by record identity: display indexes are renumbered on every rebuild.
			const [selectedRecordId, setSelectedRecordId] = (0, react.useState)(null);
			const [timelineRecordSelection, setTimelineRecordSelection] = (0, react.useState)(null);
			const [timelineRecordFocus, setTimelineRecordFocus] = (0, react.useState)(null);
			const [live, setLive] = (0, react.useState)(true);
			RTH.setCards(data.cards);
			const timelineTurns = (0, react.useMemo)(() => RTH.filterTurns(data.turns, enabled), [data.turns, enabled]);
			const searchLayouts = (0, react.useMemo)(() => [timelineTurns], [timelineTurns]);
			const recordAt = (0, react.useMemo)(() => {
				const byIndex = /* @__PURE__ */ new Map();
				const byId = /* @__PURE__ */ new Map();
				for (const turn of timelineTurns) for (const group of turn.groups) for (const cell of group.cells) {
					byIndex.set(cell.index, cell);
					byId.set(trajectoryRecordId(cell), cell);
				}
				return { byIndex, byId };
			}, [timelineTurns]);
			const selectedTimelineIndex = selectedRecordId === null ? null : recordAt.byId.get(selectedRecordId)?.index ?? null;
			const setSelectedTimelineIndex = (0, react.useCallback)((index) => {
				const cell = index === null ? void 0 : recordAt.byIndex.get(index);
				setSelectedRecordId(cell === void 0 ? null : trajectoryRecordId(cell));
			}, [recordAt]);
			// Live: follow the newest ARRIVAL (the stream's arrival numbers, not display
			// positions) until the reader selects, moves or scrolls away; "Paused · N new" counts the source
			// rows that arrived since, by identity, so eviction, folding and filtering do not distort it.
			const latestArrival = data.latestArrival ?? 0;
			(0, react.useEffect)(() => {
				if (!live) return;
				let newest = null;
				for (const turn of timelineTurns) for (const group of turn.groups) for (const cell of group.cells) if (newest === null || (cell.rthArrival ?? -1) > (newest.rthArrival ?? -1) || (cell.rthArrival ?? -1) === (newest.rthArrival ?? -1) && cell.index > newest.index) newest = cell;
				if (newest !== null) setTimelineRecordFocus({ index: newest.index });
			}, [live, latestArrival, enabled]);
			const pausedAt = (0, react.useRef)(null);
			if (live) pausedAt.current = null;
			else if (pausedAt.current === null) pausedAt.current = latestArrival;
			const newSincePause = live ? 0 : RTH.newSince(timelineTurns, pausedAt.current, data.arrivals);
			// Scrolling away from the tail pauses Live; scrolling back to it resumes. Only scrolls the reader
			// caused (wheel, touch, keys, scrollbar drag) count, so the view's own focus scrolls never do.
			const rootRef = (0, react.useRef)(null);
			(0, react.useEffect)(() => {
				const root = rootRef.current;
				if (root === null) return;
				let intent = 0;
				const mark = () => {
					intent = performance.now();
				};
				const onKey = (event) => {
					if ([
						"PageUp",
						"PageDown",
						"ArrowUp",
						"ArrowDown",
						"Home",
						"End",
						" "
					].includes(event.key)) mark();
				};
				const onScroll = (event) => {
					const el = event.target;
					if (!(el instanceof Element) || el.closest("[data-rth=trajectory]") !== root || el.scrollHeight <= el.clientHeight) return;
					if (performance.now() - intent > 800) return;
					const gap = el.scrollHeight - el.scrollTop - el.clientHeight;
					if (gap > 48) setLive(false);
					else if (gap <= 4) setLive(true);
				};
				root.addEventListener("wheel", mark, { passive: true });
				root.addEventListener("touchmove", mark, { passive: true });
				root.addEventListener("pointerdown", mark);
				root.addEventListener("keydown", onKey);
				root.addEventListener("scroll", onScroll, true);
				return () => {
					root.removeEventListener("wheel", mark);
					root.removeEventListener("touchmove", mark);
					root.removeEventListener("pointerdown", mark);
					root.removeEventListener("keydown", onKey);
					root.removeEventListener("scroll", onScroll, true);
				};
			}, [data.notRth]);
			// Decorative motion stops while the page is hidden.
			const [hidden, setHidden] = (0, react.useState)(() => typeof document !== "undefined" && document.visibilityState === "hidden");
			(0, react.useEffect)(() => {
				const onVisibility = () => setHidden(document.visibilityState === "hidden");
				document.addEventListener("visibilitychange", onVisibility);
				return () => document.removeEventListener("visibilitychange", onVisibility);
			}, []);
			// Keys: j/k move between records, Enter inspects (upstream row), / search, f filters, l live.
			const onRthKeyDown = (event) => {
				if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
				const target = event.target;
				if (target instanceof Element && target.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])") !== null) return;
				const root = event.currentTarget;
				switch (event.key) {
					case "j":
					case "k": {
						const rows = [...root.querySelectorAll("tr[data-record-index]")];
						if (rows.length === 0) return;
						const at = rows.indexOf(document.activeElement?.closest?.("tr[data-record-index]") ?? null);
						const step = event.key === "j" ? 1 : -1;
						const next = rows[at < 0 ? (step > 0 ? 0 : rows.length - 1) : Math.max(0, Math.min(rows.length - 1, at + step))];
						next.focus();
						next.scrollIntoView({ block: "nearest" });
						setLive(false);
						break;
					}
					case "/":
						root.querySelector("input[type=search]")?.focus();
						break;
					case "f":
						root.querySelector("[data-rth=trajectory-strip] .rth-chip")?.focus();
						break;
					case "l":
						setLive((current) => !current);
						break;
					default: return;
				}
				event.preventDefault();
			};
			(0, react.useEffect)(() => {
				if (searchIndex.update(searchLayouts)) setSearchIndexRevision((revision) => revision + 1);
			}, [searchIndex, searchLayouts]);
			// One time base per axis. Two overviews: Sequence (upstream's timeline, observation ORDER
			// only) and Duration (RTH.ClockOverview, producer-clock lane groups, one axis per clock
			// and lifetime). Upstream's timed modes are never used: they would place records at
			// observed_at + a producer duration.
			const timelineMode = "sequence";
			const actualDuration = useDuration((value) => value) === true;
			const clockLayout = (0, react.useMemo)(() => actualDuration ? RTH.layoutClockOverview(timelineTurns) : null, [actualDuration, timelineTurns]);
			const selectFromOverview = (index) => {
				setLive(false);
				setTimelineSelection(null);
				setTimelineRecordSelection({ index });
				setSelectedTimelineIndex(index);
			};
			const searchMatchRecordIds = (0, react.useMemo)(() => searchIndex.search(searchQuery), [searchIndex, searchIndexRevision, searchQuery]);
			const searchMatchIndexes = (0, react.useMemo)(() => {
				if (searchMatchRecordIds === null) return null;
				const indexes = /* @__PURE__ */ new Set();
				for (const turn of timelineTurns) for (const group of turn.groups) for (const cell of group.cells) if (searchMatchRecordIds.has(trajectoryRecordId(cell))) indexes.add(cell.index);
				return indexes;
			}, [timelineTurns, searchMatchRecordIds]);
			const timelineFocusIndexes = (0, react.useMemo)(() => timelineSelection === null ? null : trajectoryTimelineFocusIndexes(timelineTurns, timelineSelection, timelineMode), [timelineMode, timelineSelection, timelineTurns]);
			const handleRecordSelect = (0, react.useCallback)((index) => {
				setLive(false);
				if (timelineFocusIndexes !== null && !timelineFocusIndexes.has(index)) setTimelineSelection(null);
			}, [timelineFocusIndexes]);
			const collapsibleTurnIds = (0, react.useMemo)(() => timelineTurns.filter((turn) => turn.turn !== null && turn.groups.reduce((count, group) => count + group.cells.length, 0) > 1).flatMap((turn) => turn.turn === null ? [] : [turn.turn]), [timelineTurns]);
			const allTurnsCollapsed = collapsibleTurnIds.length > 0 && collapsibleTurnIds.every((turn) => collapsedTurns.has(turn));
			const toggleTurn = (turn) => {
				setCollapsedTurns((current) => {
					const collapsed = new Set(current);
					if (collapsed.has(turn)) collapsed.delete(turn);
					else collapsed.add(turn);
					return collapsed;
				});
			};
			const toggleAllTurns = () => {
				setCollapsedTurns((current) => {
					const collapsed = new Set(current);
					if (allTurnsCollapsed) for (const turn of collapsibleTurnIds) collapsed.delete(turn);
					else for (const turn of collapsibleTurnIds) collapsed.add(turn);
					return collapsed;
				});
			};
			const toggleGroup = (id) => {
				setEnabled((current) => {
					const next = new Set(current);
					if (next.has(id)) next.delete(id);
					else next.add(id);
					return next.size === 0 ? RTH_DEFAULT_GROUPS() : next;
				});
				setTimelineSelection(null);
			};
			if (data.notRth === true) return (0, react_jsx_runtime.jsx)("div", {
				className: `${views_module_css_default.root} rth-trajectory`,
				"data-rth": "trajectory",
				children: (0, react_jsx_runtime.jsx)("div", {
					className: "rth-notrth",
					children: t("rth.notRthSession")
				})
			});
			return (0, react_jsx_runtime.jsxs)("div", {
				className: `${views_module_css_default.root} rth-trajectory`,
				"data-rth": "trajectory",
				"data-rth-live": live ? "on" : "off",
				"data-hidden": hidden ? "" : void 0,
				ref: rootRef,
				"data-conversation-composer-overlay": "",
				tabIndex: -1,
				onKeyDown: onRthKeyDown,
				children: [
					(0, react_jsx_runtime.jsx)(TrajectoryToolbar, {
						rthSequenceOnly: false,
						actualDuration,
						onActualDurationChange: (next) => {
							setActualDuration(next);
							setTimelineSelection(null);
						},
						actualTime: false,
						onActualTimeChange: () => {},
						allTurnsCollapsed,
						onToggleAllTurns: toggleAllTurns,
						allAssistantsCollapsed: false,
						onToggleAllAssistants: () => {},
						searchQuery,
						onSearchQueryChange: setSearchQuery,
						t
					}),
					(0, react_jsx_runtime.jsx)(RTH.StripRow, {
						data,
						enabled,
						onToggle: toggleGroup,
						live,
						newSincePause,
						onLive: () => setLive((current) => !current)
					}),
					actualDuration ? (0, react_jsx_runtime.jsx)(RTH.ClockOverview, {
						layout: clockLayout,
						selectedIndex: selectedTimelineIndex,
						searchMatchIndexes,
						onSelect: selectFromOverview
					}) : (0, react_jsx_runtime.jsx)(TrajectoryTimeline, {
						t,
						turns: timelineTurns,
						mode: timelineMode,
						range: timelineSelection,
						hasEarlierRecords: false,
						onLoadEarlier: void 0,
						selectedIndex: selectedTimelineIndex,
						searchMatchIndexes,
						onRangeChange: setTimelineSelection,
						onRecordSelect: selectFromOverview,
						onRecordFocus: (index) => {
							setTimelineRecordFocus({ index });
						}
					}),
					(0, react_jsx_runtime.jsx)("div", {
						className: views_module_css_default.ledger,
						children: (0, react_jsx_runtime.jsx)(TrajectoryTable, {
							t,
							stringWrapping: jsonStringWrapping === void 0 ? void 0 : {
								...jsonStringWrapping,
								label: t("record.wrapLines")
							},
							renderSlot,
							loadImage,
							requestNumbers: [],
							turns: timelineTurns,
							streamingCells: [],
							timelineFocusIndexes,
							searchMatchIndexes,
							onSelectedIndexChange: setSelectedTimelineIndex,
							onRecordSelect: handleRecordSelect,
							recordSelection: timelineRecordSelection,
							recordFocus: timelineRecordFocus,
							historyLoading: data.stream === "opening",
							olderHistoryLoading: false,
							historyStartSeq: 0,
							hasOlderRecords: false,
							onLoadOlder: async () => false,
							onClearSelection: () => {
								setTimelineSelection(null);
							},
							collapsedTurns,
							onToggleTurn: toggleTurn,
							collapsedAssistants,
							onToggleAssistant: (id) => {
								setCollapsedAssistants((current) => {
									const collapsed = new Set(current);
									if (collapsed.has(id)) collapsed.delete(id);
									else collapsed.add(id);
									return collapsed;
								});
							},
							inspectCallId: null,
							onInspectApplied: completeViewRequest
						})
					})
				]
			});
		}
		//#endregion

		//#region rth/index.js (RTH replacement of lib/types/client/index.js, see NOTICE.md)
		/** Required services: the conversation slot and the locale service. No upstream Definitions, no Session paging. */
		const inject = [
			"slots",
			"uiConversation",
			"sidebarRightTabs",
			"sidebarRight",
			"locale"
		];
		const RTH_TAB_ID = "rth-trajectory-tab";
		const RTH_TAB_KIND = "rth.trajectory";
		/**
		* Client plugin body: register the RTH trajectory as its own conversation view ("rth-trajectory",
		* labelled "RTH"), next to Chat and upstream Trajectory, which stays untouched. RTH's feed is
		* shown only in the RTH session (rth-root); any other session gets an explanatory empty state.
		* The registration rides the slot service's effect wrapper, so plugin unload removes the tab.
		* @param ctx - client root context.
		*/
		function apply(ctx) {
			ctx.effect(() => ctx.locale.register(NS, {
				zh,
				en
			}), "rth-trajectory: dictionaries");
			const t = ctx.locale.bind(NS);
			const duration = createTrajectoryDurationStore();
			const stringWrapping = createTrajectoryStringWrappingStore();
			const viewInject = (sessionId) => ({
				hooks: {
					duration,
					rth: RTH.sourceFor(sessionId)
				},
				jsonStringWrapping: {
					getDefault: () => stringWrapping.getSnapshot(),
					setDefault: (value) => {
						stringWrapping.set(value);
					}
				},
				loadImage: Object.assign(async () => null, { peek: () => void 0 }),
				setActualDuration: (value) => {
					duration.set(value);
				}
			});
			// Placement 2 (the Bot profile): DSH's right sidebar, where the Bot keeps its tab group;
			// its Fullscreen control gives the ledger the full width. Same view, same shared stream.
			ctx.effect(() => ctx.sidebarRightTabs.register({
				id: RTH_TAB_ID,
				kind: RTH_TAB_KIND,
				priority: "extension",
				title: () => t("rth.tab"),
				guide: [{
					id: "rth-trajectory",
					order: 91,
					title: () => t("rth.tab"),
					description: () => t("rth.tabGuide")
				}]
			}), "rth-trajectory: sidebar tab type");
			ctx.effect(() => () => {
				for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
					if (tab.kind !== RTH_TAB_KIND) continue;
					try {
						ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId);
					} catch {}
				}
			}, "rth-trajectory: close open tabs on unload");
			ctx.slots.inject("sidebar.right.pane.tab", () => ctx.slots.register({
				name: "sidebar.right.pane.tab",
				key: RTH_TAB_ID,
				locale: NS,
				inject: viewInject
			}, RthTrajectoryView));
			ctx.slots.inject("sidebar.right.pane.tab.title", () => ctx.slots.register({
				name: "sidebar.right.pane.tab.title",
				key: RTH_TAB_ID,
				locale: NS
			}, () => t("rth.tab")));
			// Placement 1 (plain DSH): its own conversation view, next to Chat and upstream Trajectory.
			ctx.slots.inject("conversation.view", () => ctx.slots.register({
				name: "conversation.view",
				id: "rth-trajectory",
				order: 5,
				locale: NS,
				label: () => t("rth.view"),
				inject: viewInject
			}, RthTrajectoryView));
			// The Bot profile hides DSH's view tabs; its chat header takes a button instead (RTH Main Bot only).
			let omni = null;
			try {
				omni = require("@automagik/omni-dsh/client").omni ?? null;
			} catch {}
			if (omni?.headerButtons !== void 0) ctx.effect(() => omni.headerButtons.register({
				id: "rth-trajectory",
				order: 50,
				Component: RTH.makeHeaderButton(() => {
					ctx.sidebarRight.openTabIn(RTH.RTH_SESSION, RTH_TAB_KIND);
					if (ctx.sidebarRight.isExpanded?.() === false) ctx.sidebarRight.toggleExpanded?.();
				})
			}), "rth-trajectory: Bot header button");
		}
		//#endregion

		exports.apply = apply;
		exports.inject = inject;
		return module.exports;
	}
});

//# sourceMappingURL=client.js.map