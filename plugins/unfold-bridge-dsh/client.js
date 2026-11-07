// Generated from src/client by `npm run build` (scripts/build-client.mjs). Do not edit by hand.
window.__ModuleLoader__.load({
  id: '@automagik/unfold-bridge-dsh',
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

// src/client/index.js
var index_exports = {};
__export(index_exports, {
  INSTALL_SLOT: () => INSTALL_SLOT,
  SLOTS: () => SLOTS,
  apply: () => apply,
  inject: () => inject
});
module.exports = __toCommonJS(index_exports);

// src/client/api.js
var ROUTE = "api/unfold-bridge";
async function post(path, body, { remove = false } = {}) {
  let response;
  try {
    response = await fetch(`${ROUTE}/${path}`, {
      method: remove ? "DELETE" : "POST",
      credentials: "same-origin",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body)
    });
  } catch {
    throw Object.assign(new Error("network"), { code: "network" });
  }
  if (response.ok) return;
  const answer = await response.json().catch(() => null);
  const code = answer?.error?.code ?? (response.status === 401 ? "unauthorized" : `http_${response.status}`);
  throw Object.assign(new Error(code), { code });
}
var query = (name, value) => `${name}=${encodeURIComponent(value)}`;
var sendDraft = (draft) => post(`drafts/send?${query("effect_id", draft.effect_id)}`, { token: draft.token });
var cancelDraft = (draft) => post(`drafts/cancel?${query("effect_id", draft.effect_id)}`, { token: draft.token });
var watchSession = (sessionId) => post("subscriptions", { session_id: sessionId });
var dropSubscription = (id) => post(`subscriptions?${query("id", id)}`, {}, { remove: true });
var undoChange = (changeId) => post(`undo?${query("change_id", changeId)}`, {});
var RETRY_MS = { first: 2e3, most: 3e4 };
var CLOSED = 2;
function openView(onSnapshot, { EventSourceImpl = globalThis.EventSource, later = setTimeout, cancel = clearTimeout } = {}) {
  if (typeof EventSourceImpl !== "function") return () => {
  };
  let source = null;
  let timer = null;
  let pause = RETRY_MS.first;
  let closed = false;
  const open = () => {
    timer = null;
    const current = source = new EventSourceImpl(`${ROUTE}/view`);
    current.addEventListener("view", (event) => {
      pause = RETRY_MS.first;
      try {
        onSnapshot(JSON.parse(event.data));
      } catch {
      }
    });
    current.addEventListener("error", () => {
      onSnapshot(null);
      if (closed || current !== source || current.readyState !== CLOSED || timer !== null) return;
      current.close();
      timer = later(open, pause);
      pause = Math.min(pause * 2, RETRY_MS.most);
    });
  };
  open();
  return () => {
    closed = true;
    if (timer !== null) cancel(timer);
    source.close();
  };
}

// src/client/locales/en.json
var en_default = {
  "board.open": "Unfold",
  "board.title": "Unfold",
  "board.close": "Close",
  "board.agents": "Agents",
  "board.noAgents": "No sessions yet.",
  "board.watched": "Watched",
  "board.approval": "Waiting for your approval on DSH's card",
  "board.drafts": "Drafts",
  "board.draftWaiting": "Draft for {name} · {workspace}. Open that session to read and send it.",
  "board.notices": "Changes by Unfold",
  "suggest.title": "Start with {name} · {workspace}?",
  "suggest.body": "This session had the most recent turn. Unfold follows only the sessions you pick.",
  "suggest.watch": "Watch",
  "link.on": "Unfold connected",
  "link.off": "Unfold not connected",
  "link.offHint": "Your clicks reach Unfold when the link is back.",
  "link.waiting": "Waiting for the Unfold bridge…",
  "status.idle": "Idle",
  "status.running": "Working",
  "status.error": "Error",
  "status.approval_decided": "Approval answered",
  "status.other": "{status}",
  "row.watch": "Watch with Unfold",
  "row.unwatch": "Stop watching with Unfold",
  "row.watchedByRule": "Unfold also follows this session through another subscription",
  "dock.adviceLabel": "Unfold's read of this agent's messages",
  "dock.marked": "This agent's text contains instructions",
  "dock.adviceHint": "Answer on DSH's own approval card. This read sends nothing.",
  "dock.draftBy": "Draft by Unfold",
  "dock.draftTo": "To: {name} · {workspace}",
  "dock.draftHint": "Send passes exactly this text to this session.",
  "dock.send": "Send",
  "dock.cancel": "Cancel",
  "dock.sending": "Sending…",
  "settings.title": "Unfold watch",
  "settings.intro": "Unfold sees your sessions' names and status, and what your agents say only in the sessions it watches. A draft is sent only when you click Send.",
  "settings.link": "Link",
  "settings.subscriptions": "Subscriptions",
  "settings.noSubscriptions": "Unfold watches nothing yet.",
  "settings.byUnfold": "by Unfold",
  "settings.byYou": "by you",
  "settings.remove": "Remove",
  "settings.log": "Change log",
  "settings.noLog": "No changes yet.",
  "settings.undo": "Undo",
  "match.sessions": "session {list}",
  "match.names": "agent {list}",
  "match.workspaces": "workspace {list}",
  "match.kinds": "({list})",
  "kind.turn_done": "turns",
  "kind.error": "errors",
  "kind.approval_waiting": "approvals",
  "kind.status": "status",
  "kind.created": "new agents",
  "log.byUnfold": "Unfold",
  "log.byYou": "You",
  "log.added": "added {what}",
  "log.removed": "removed {what}",
  "log.line": "{who}: {change}",
  "error.bad_token": "This draft changed. Read the new one before sending.",
  "error.not_found": "Already gone.",
  "error.send_failed": "DSH did not take the message. The draft is spent; it is never sent again.",
  "error.send_unknown": "No answer in time, so DSH may have sent it. The draft is never sent again.",
  "error.agentd_unavailable": "Unfold is not connected and too many clicks are waiting.",
  "error.unauthorized": "DSH did not admit this page. Reload it.",
  "error.other": "That did not work ({code}).",
  "board.openWaiting": "Unfold, {count} waiting",
  "hidden.one": "1 hidden character: {list}",
  "hidden.many": "{count} hidden characters: {list}",
  "hidden.hold": "Send stays off while a draft holds characters you can't see. Cancel it, or ask Unfold for a new one.",
  "log.byUnfoldAsked": "Unfold, at your request",
  "row.watchedMark": "Unfold watches this session",
  "install.open": "Install as app",
  "install.title": "DSH as an app",
  "install.hint": "DSH opens in a window of its own, like any app.",
  "install.button": "Install",
  "install.safari": "In Safari, choose File › Add to Dock."
};

// src/client/locales/pt-BR.json
var pt_BR_default = {
  "board.open": "Unfold",
  "board.title": "Unfold",
  "board.close": "Fechar",
  "board.agents": "Agentes",
  "board.noAgents": "Nenhuma sessão ainda.",
  "board.watched": "Em acompanhamento",
  "board.approval": "Aguardando sua aprovação no cartão do DSH",
  "board.drafts": "Rascunhos",
  "board.draftWaiting": "Rascunho para {name} · {workspace}. Abra essa sessão para ler e enviar.",
  "board.notices": "Mudanças feitas por Unfold",
  "suggest.title": "Começar por {name} · {workspace}?",
  "suggest.body": "Esta sessão teve o turno mais recente. Unfold acompanha só as sessões que você escolher.",
  "suggest.watch": "Acompanhar",
  "link.on": "Conexão com Unfold: ativa",
  "link.off": "Conexão com Unfold: inativa",
  "link.offHint": "Seus cliques são entregues quando a conexão com Unfold voltar.",
  "link.waiting": "Aguardando a ponte com Unfold…",
  "status.idle": "Sem atividade",
  "status.running": "Em execução",
  "status.error": "Erro",
  "status.approval_decided": "Aprovação respondida",
  "status.other": "{status}",
  "row.watch": "Acompanhar com Unfold",
  "row.unwatch": "Parar de acompanhar com Unfold",
  "row.watchedByRule": "Unfold também acompanha esta sessão por outra inscrição",
  "dock.adviceLabel": "Leitura de Unfold sobre as mensagens deste agente",
  "dock.marked": "O texto deste agente contém instruções",
  "dock.adviceHint": "Responda no próprio cartão de aprovação do DSH. Esta leitura não envia nada.",
  "dock.draftBy": "Rascunho de Unfold",
  "dock.draftTo": "Para: {name} · {workspace}",
  "dock.draftHint": "Enviar passa exatamente este texto para esta sessão.",
  "dock.send": "Enviar",
  "dock.cancel": "Cancelar",
  "dock.sending": "Enviando…",
  "settings.title": "Acompanhamento de Unfold",
  "settings.intro": "Unfold vê os nomes e o estado das suas sessões, e o que seus agentes dizem só nas sessões acompanhadas. Um rascunho só é enviado quando você clica em Enviar.",
  "settings.link": "Conexão",
  "settings.subscriptions": "Inscrições",
  "settings.noSubscriptions": "Unfold ainda não acompanha nada.",
  "settings.byUnfold": "por Unfold",
  "settings.byYou": "por você",
  "settings.remove": "Remover",
  "settings.log": "Histórico de mudanças",
  "settings.noLog": "Nenhuma mudança ainda.",
  "settings.undo": "Desfazer",
  "match.sessions": "sessão {list}",
  "match.names": "agente {list}",
  "match.workspaces": "espaço de trabalho {list}",
  "match.kinds": "({list})",
  "kind.turn_done": "turnos",
  "kind.error": "erros",
  "kind.approval_waiting": "aprovações",
  "kind.status": "estado",
  "kind.created": "novos agentes",
  "log.byUnfold": "Unfold",
  "log.byYou": "Você",
  "log.added": "adicionou {what}",
  "log.removed": "removeu {what}",
  "log.line": "{who}: {change}",
  "error.bad_token": "Este rascunho mudou. Leia o novo antes de enviar.",
  "error.not_found": "Isso já não existe.",
  "error.send_failed": "O DSH não aceitou a mensagem. O rascunho foi descartado e nunca é enviado de novo.",
  "error.send_unknown": "Sem resposta a tempo, então o DSH pode ter enviado. O rascunho nunca é enviado de novo.",
  "error.agentd_unavailable": "Conexão com Unfold inativa, e há cliques demais na fila.",
  "error.unauthorized": "O DSH não autorizou esta página. Recarregue a página.",
  "error.other": "Não funcionou ({code}).",
  "board.openWaiting": "Unfold, {count} aguardando",
  "hidden.one": "1 caractere oculto: {list}",
  "hidden.many": "{count} caracteres ocultos: {list}",
  "hidden.hold": "Enviar fica desativado enquanto um rascunho tiver caracteres que você não vê. Cancele ou peça um novo rascunho.",
  "log.byUnfoldAsked": "Unfold, a seu pedido",
  "row.watchedMark": "Unfold acompanha esta sessão",
  "install.open": "Instalar como app",
  "install.title": "DSH como app",
  "install.hint": "O DSH abre em uma janela própria, como qualquer app.",
  "install.button": "Instalar",
  "install.safari": "No Safari, escolha Arquivo › Adicionar ao Dock."
};

// src/client/i18n.js
var DICTIONARIES = { en: en_default, "pt-BR": pt_BR_default };
var languageOf = (active) => typeof active === "string" && active.toLowerCase().startsWith("pt") ? "pt-BR" : "en";
var fill = (text, params) => params ? text.replace(/\{(\w+)\}/g, (whole, name) => Object.hasOwn(params, name) ? String(params[name]) : whole) : text;
function translator(language) {
  const dictionary = DICTIONARIES[language] ?? en_default;
  const t = (key, params) => fill(dictionary[key] ?? en_default[key] ?? key, params);
  t.language = DICTIONARIES[language] ? language : "en";
  return t;
}
var when = (at, t) => Number.isFinite(at) ? new Intl.DateTimeFormat(t.language === "pt-BR" ? "pt-BR" : "en-US", { dateStyle: "short", timeStyle: "short" }).format(at) : "";

// src/client/install.js
var INSTALL_HASH = "#install";
var DISPLAY_MODES = ["(display-mode: standalone)", "(display-mode: fullscreen)"];
var isInstalled = (win) => win?.navigator?.standalone === true || DISPLAY_MODES.some((query2) => win?.matchMedia?.(query2)?.matches === true);
function createInstall(win) {
  let held = null;
  let snap = { canPrompt: false, installed: isInstalled(win), opened: win?.location?.hash === INSTALL_HASH, prompts: win !== void 0 && win !== null && "onbeforeinstallprompt" in win };
  const listeners = /* @__PURE__ */ new Set();
  const set = (next) => {
    snap = { ...snap, ...next };
    for (const fn of Array.from(listeners)) fn();
  };
  const onPrompt = (event) => {
    event.preventDefault?.();
    held = event;
    set({ canPrompt: true });
  };
  const onInstalled = () => {
    held = null;
    set({ canPrompt: false, installed: true });
  };
  let watching = false;
  return {
    get: () => snap,
    subscribe(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    // Listens until the returned stop; watching again while watching listens once, and any stop
    // stops all (the bundle's own watch at load and apply's effect share the listeners).
    watch() {
      if (!watching && win?.addEventListener) {
        win.addEventListener("beforeinstallprompt", onPrompt);
        win.addEventListener("appinstalled", onInstalled);
        watching = true;
      }
      return () => {
        if (!watching) return;
        win.removeEventListener?.("beforeinstallprompt", onPrompt);
        win.removeEventListener?.("appinstalled", onInstalled);
        watching = false;
      };
    },
    // The person's click. The event is spent at once, so a second click prompts nothing.
    prompt() {
      const event = held;
      if (!event) return void 0;
      held = null;
      set({ canPrompt: false });
      try {
        return Promise.resolve(event.prompt()).catch(() => void 0);
      } catch {
        return void 0;
      }
    }
  };
}
var offerShown = (offer) => !offer.installed && (offer.canPrompt || offer.opened && !offer.prompts);

// src/client/hidden.js
var RANGES = [
  [0, 8],
  [11, 12],
  [14, 31],
  [127, 159],
  [173, 173],
  [847, 847],
  [1564, 1564],
  [4447, 4448],
  [6068, 6069],
  [6155, 6159],
  [8203, 8207],
  [8234, 8238],
  [8288, 8303],
  [12644, 12644],
  [65024, 65039],
  [65279, 65279],
  [65440, 65440],
  [65520, 65528],
  [113824, 113827],
  [119155, 119162],
  [917504, 921599]
];
var ZWJ = 8205;
var VS16 = 65039;
var EMOJI = /\p{Extended_Pictographic}/u;
var SKIN_TONE = (cp) => cp >= 127995 && cp <= 127999;
var listed = (cp) => RANGES.some(([low, high]) => cp >= low && cp <= high);
var emoji = (cp) => cp !== void 0 && EMOJI.test(String.fromCodePoint(cp));
var presentation = (points, i) => points[i] === VS16 && emoji(points[i - 1]);
function visibleJoin(points, i) {
  if (presentation(points, i)) return true;
  if (points[i] !== ZWJ || !emoji(points[i + 1])) return false;
  return emoji(points[i - 1]) || presentation(points, i - 1) || SKIN_TONE(points[i - 1]) && emoji(points[i - 2]);
}
function hiddenIn(text) {
  if (typeof text !== "string") return [];
  const points = Array.from(text, (char) => char.codePointAt(0));
  return points.filter((cp, i) => listed(cp) && !visibleJoin(points, i));
}
var codeName = (cp) => `U+${cp.toString(16).toUpperCase().padStart(4, "0")}`;
function hiddenWarning(points, t) {
  const names = [...new Set(points)].map(codeName);
  const list = names.length > 12 ? `${names.slice(0, 12).join(", ")}, …` : names.join(", ");
  return points.length === 1 ? t("hidden.one", { list }) : t("hidden.many", { count: points.length, list });
}

// src/client/view.js
var subscriptionsOf = (snap) => snap?.view?.subscriptions ?? [];
var matchOf = (sub) => sub?.match ?? {};
var listIn = (value) => Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
var has = (list, value) => listIn(list).includes(value);
var matchesAgent = (sub, agent) => {
  const match = matchOf(sub);
  return has(match.session_ids, agent.session_id) || has(match.names, agent.name) || has(match.workspaces, agent.workspace);
};
var watchedAgent = (snap, agent) => subscriptionsOf(snap).some((sub) => matchesAgent(sub, agent));
function rowSubscription(snap, sessionId) {
  const own = subscriptionsOf(snap).filter((sub) => has(matchOf(sub).session_ids, sessionId));
  const alone = (sub) => listIn(matchOf(sub).session_ids).length === 1 && listIn(matchOf(sub).names).length === 0 && listIn(matchOf(sub).workspaces).length === 0;
  return own.find(alone) ?? own[0] ?? null;
}
var agentOf = (snap, sessionId) => snap?.agents?.find((agent) => agent.session_id === sessionId) ?? null;
function draftFor(snap, sessionId) {
  const own = (snap?.drafts ?? []).filter((draft) => draft.session_id === sessionId);
  return own.reduce((newest, draft) => newest === null || draft.at >= newest.at ? draft : newest, null);
}
var pendingFor = (snap, sessionId) => (snap?.approvals ?? []).filter((approval) => approval.session_id === sessionId);
function adviceFor(snap, approval) {
  const notes = (snap?.view?.advice ?? []).filter((advice) => advice.call_id === approval.call_id && advice.session_id === approval.session_id);
  return notes.reduce((newest, advice) => newest === null || advice.at >= newest.at ? advice : newest, null);
}
function suggestion(snap) {
  if (!snap || subscriptionsOf(snap).length > 0) return null;
  return (snap.agents ?? []).reduce((best, agent) => Number.isFinite(agent.last_turn_at) && (best === null || agent.last_turn_at > best.last_turn_at) ? agent : best, null);
}
var listOf = (value) => Array.isArray(value) ? value : value ? [value] : [];
var changesOf = (snap) => listOf(snap?.view?.log_tail).filter((entry) => entry && typeof entry.change_id === "string").slice().reverse();
var noticesOf = (snap) => changesOf(snap).filter((entry) => entry.by === "unfold");
function describeSubscription(sub, snap, t) {
  const match = matchOf(sub);
  const label = (id) => {
    const agent = agentOf(snap, id);
    return agent ? `${agent.name} · ${agent.workspace}` : id;
  };
  const parts = [];
  const sessions = listIn(match.session_ids);
  const names = listIn(match.names);
  const workspaces = listIn(match.workspaces);
  if (sessions.length > 0) parts.push(t("match.sessions", { list: sessions.map(label).join(", ") }));
  if (names.length > 0) parts.push(t("match.names", { list: names.join(", ") }));
  if (workspaces.length > 0) parts.push(t("match.workspaces", { list: workspaces.join(", ") }));
  const kinds = listIn(sub?.kinds);
  const what = parts.join(", ");
  return kinds.length > 0 ? `${what} ${t("match.kinds", { list: kinds.map((kind) => t(`kind.${kind}`)).join(", ") })}` : what;
}
var changedBy = (entry) => entry.by !== "unfold" ? "log.byYou" : entry.cause === "Person" ? "log.byUnfoldAsked" : "log.byUnfold";
function describeChange(entry, snap, t) {
  const what = (list) => listOf(list).map((sub) => describeSubscription(sub, snap, t)).filter(Boolean).join(" | ");
  const changes = [];
  const added = what(entry.added);
  const removed = what(entry.removed);
  if (added) changes.push(t("log.added", { what: added }));
  if (removed) changes.push(t("log.removed", { what: removed }));
  return t("log.line", { who: t(changedBy(entry)), change: changes.join("; ") });
}
var statusText = (status, t) => ["idle", "running", "error", "approval_decided"].includes(status) ? t(`status.${status}`) : t("status.other", { status: String(status ?? "") });

// src/client/slots.js
var import_react = require("react");
var KNOWN_ERRORS = /* @__PURE__ */ new Set(["bad_token", "not_found", "send_failed", "send_unknown", "agentd_unavailable", "unauthorized"]);
var errorText = (code, t) => KNOWN_ERRORS.has(code) ? t(`error.${code}`) : t("error.other", { code });
function useTap() {
  const [state, setState] = (0, import_react.useState)({ busy: false, error: null });
  const run = (action) => {
    if (state.busy) return;
    setState({ busy: true, error: null });
    Promise.resolve().then(action).then(
      () => setState({ busy: false, error: null }),
      (error) => setState({ busy: false, error: error?.code ?? "other" })
    );
  };
  return [run, state];
}
var TapError = ({ code, t }) => code ? (0, import_react.createElement)("p", { className: "ub-error", role: "alert" }, errorText(code, t)) : null;
var dotKind = (status) => status === "running" ? "running" : status === "error" ? "error" : "idle";
var Dot = ({ status, watched }) => (0, import_react.createElement)("span", { className: `ub-dot ub-dot-${dotKind(status)}${watched ? " ub-dot-watched" : ""}`, "aria-hidden": "true" });
var Eye = ({ on }) => (0, import_react.createElement)(
  "svg",
  { className: "ub-eye", viewBox: "0 0 16 16", width: 14, height: 14, "aria-hidden": "true" },
  (0, import_react.createElement)("path", { d: "M1.5 8s2.4-4.5 6.5-4.5S14.5 8 14.5 8s-2.4 4.5-6.5 4.5S1.5 8 1.5 8z", fill: "none", stroke: "currentColor", strokeWidth: 1.3 }),
  (0, import_react.createElement)("circle", { cx: 8, cy: 8, r: 2, fill: on ? "currentColor" : "none", stroke: "currentColor", strokeWidth: 1.3 })
);
function LinkState({ snap, t }) {
  if (!snap) return (0, import_react.createElement)("div", { className: "ub-link ub-link-off", "data-unfold-link": "down" }, (0, import_react.createElement)("p", null, t("link.off")), (0, import_react.createElement)("p", { className: "ub-hint" }, t("link.waiting")));
  if (snap.connected) return (0, import_react.createElement)("p", { className: "ub-link ub-link-on", "data-unfold-link": "on" }, t("link.on"));
  return (0, import_react.createElement)("div", { className: "ub-link ub-link-off", "data-unfold-link": "off" }, (0, import_react.createElement)("p", null, t("link.off")), (0, import_react.createElement)("p", { className: "ub-hint" }, t("link.offHint")));
}
var Hidden = ({ points, t }) => points.length > 0 ? (0, import_react.createElement)("p", { className: "ub-marked", role: "alert", "data-unfold-hidden": String(points.length) }, hiddenWarning(points, t)) : null;
function Advice({ advice, t }) {
  return (0, import_react.createElement)(
    "section",
    { className: "ub-advice", "data-unfold-advice": advice.call_id },
    (0, import_react.createElement)("p", { className: "ub-label" }, t("dock.adviceLabel")),
    advice.marked ? (0, import_react.createElement)("p", { className: "ub-marked", "data-unfold-marked": "true" }, t("dock.marked")) : null,
    (0, import_react.createElement)(Hidden, { points: hiddenIn(advice.note), t }),
    (0, import_react.createElement)("p", { className: "ub-note" }, advice.note),
    (0, import_react.createElement)("p", { className: "ub-hint" }, t("dock.adviceHint"))
  );
}
function RowAction({ snap, t, actions, sessionId }) {
  const [run, tap] = useTap();
  if (!snap) return null;
  const agent = agentOf(snap, sessionId);
  const own = rowSubscription(snap, sessionId);
  const watched = own !== null || agent !== null && watchedAgent(snap, agent);
  const label = [own ? t("row.unwatch") : t("row.watch"), agent ? statusText(agent.status, t) : null].filter(Boolean).join(" · ");
  const title = [label, !own && watched ? t("row.watchedByRule") : null, tap.error ? errorText(tap.error, t) : null].filter(Boolean).join(" · ");
  return (0, import_react.createElement)("button", {
    type: "button",
    className: `ub-row${own ? " ub-row-on" : ""}`,
    "data-unfold-row": sessionId,
    "aria-pressed": own ? "true" : "false",
    "aria-label": label,
    title,
    disabled: tap.busy,
    onClick: () => run(() => own ? actions.drop(own.id) : actions.watch(sessionId))
  }, (0, import_react.createElement)(Eye, { on: own !== null }), (0, import_react.createElement)(Dot, { status: agent?.status, watched }));
}
function WatchedMark({ snap, t, sessionId }) {
  const agent = agentOf(snap, sessionId);
  if (!snap || !(rowSubscription(snap, sessionId) !== null || agent !== null && watchedAgent(snap, agent))) return null;
  return (0, import_react.createElement)("span", { className: "ub-watched", role: "img", "aria-label": t("row.watchedMark"), title: t("row.watchedMark"), "data-unfold-watched": sessionId }, (0, import_react.createElement)(Eye, { on: true }));
}
function Suggestion({ snap, t, actions }) {
  const [run, tap] = useTap();
  const agent = suggestion(snap);
  if (!agent) return null;
  return (0, import_react.createElement)(
    "section",
    { className: "ub-suggest", "data-unfold-suggest": agent.session_id },
    (0, import_react.createElement)("p", { className: "ub-label" }, t("suggest.title", { name: agent.name, workspace: agent.workspace })),
    (0, import_react.createElement)("p", { className: "ub-hint" }, t("suggest.body")),
    (0, import_react.createElement)("button", { type: "button", className: "ub-button", disabled: tap.busy, onClick: () => run(() => actions.watch(agent.session_id)) }, t("suggest.watch")),
    (0, import_react.createElement)(TapError, { code: tap.error, t })
  );
}
function BoardPanel({ snap, t, actions, onClose }) {
  const panel = (0, import_react.useRef)(null);
  (0, import_react.useEffect)(() => {
    panel.current?.focus();
  }, []);
  const agents = snap?.agents ?? [];
  const notices = noticesOf(snap);
  const drafts = snap?.drafts ?? [];
  return (0, import_react.createElement)(
    "div",
    {
      ref: panel,
      className: "ub-board",
      role: "dialog",
      tabIndex: -1,
      "aria-label": t("board.title"),
      "data-unfold-board": "",
      onKeyDown: (event) => {
        if (event.key === "Escape" && onClose) onClose();
      }
    },
    (0, import_react.createElement)(
      "header",
      { className: "ub-board-head" },
      (0, import_react.createElement)("h2", null, t("board.title")),
      onClose ? (0, import_react.createElement)("button", { type: "button", className: "ub-button ub-quiet", onClick: onClose }, t("board.close")) : null
    ),
    (0, import_react.createElement)(LinkState, { snap, t }),
    snap ? (0, import_react.createElement)(Suggestion, { snap, t, actions }) : null,
    snap ? (0, import_react.createElement)("h3", null, t("board.agents")) : null,
    !snap ? null : agents.length === 0 ? (0, import_react.createElement)("p", { className: "ub-hint" }, t("board.noAgents")) : (0, import_react.createElement)("ul", { className: "ub-agents" }, agents.map((agent) => {
      const watched = watchedAgent(snap, agent);
      const pending = pendingFor(snap, agent.session_id);
      return (0, import_react.createElement)(
        "li",
        { key: agent.session_id, className: "ub-agent", "data-unfold-agent": agent.session_id },
        (0, import_react.createElement)(
          "div",
          { className: "ub-agent-line" },
          (0, import_react.createElement)(Dot, { status: agent.status, watched }),
          (0, import_react.createElement)("span", { className: "ub-agent-name" }, `${agent.name} · ${agent.workspace}`),
          (0, import_react.createElement)("span", { className: "ub-hint" }, statusText(agent.status, t)),
          watched ? (0, import_react.createElement)("span", { className: "ub-badge" }, t("board.watched")) : null
        ),
        pending.map((approval) => {
          const advice = adviceFor(snap, approval);
          return (0, import_react.createElement)(
            "div",
            { key: approval.call_id, className: "ub-approval", "data-unfold-approval": approval.call_id },
            (0, import_react.createElement)("p", { className: "ub-hint" }, t("board.approval")),
            advice ? (0, import_react.createElement)(Advice, { advice, t }) : null
          );
        })
      );
    })),
    drafts.length > 0 ? (0, import_react.createElement)("h3", null, t("board.drafts")) : null,
    drafts.length > 0 ? (0, import_react.createElement)("ul", { className: "ub-drafts" }, drafts.map((draft) => (0, import_react.createElement)("li", { key: draft.effect_id, "data-unfold-draft-for": draft.session_id }, t("board.draftWaiting", { name: draft.name, workspace: draft.workspace })))) : null,
    notices.length > 0 ? (0, import_react.createElement)("h3", null, t("board.notices")) : null,
    notices.length > 0 ? (0, import_react.createElement)("ul", { className: "ub-notices" }, notices.map((entry) => (0, import_react.createElement)(ChangeRow, { key: entry.change_id, entry, snap, t, actions }))) : null
  );
}
function BoardButton({ snap, t, actions, wide }) {
  const [open, setOpen] = (0, import_react.useState)(false);
  const waiting = (snap?.approvals?.length ?? 0) + (snap?.drafts?.length ?? 0);
  return (0, import_react.createElement)(
    "div",
    { className: "ub-footer" },
    (0, import_react.createElement)("button", {
      type: "button",
      className: "ub-footer-button",
      "aria-expanded": open ? "true" : "false",
      "aria-label": waiting > 0 ? t("board.openWaiting", { count: waiting }) : t("board.open"),
      "data-unfold-board-button": "",
      onClick: () => setOpen(!open)
    }, (0, import_react.createElement)(Eye, { on: Boolean(snap?.connected) }), wide ? (0, import_react.createElement)("span", null, t("board.open")) : null, waiting > 0 ? (0, import_react.createElement)("span", { className: "ub-count", "aria-hidden": "true" }, String(waiting)) : null),
    open ? (0, import_react.createElement)(BoardPanel, { snap, t, actions, onClose: () => setOpen(false) }) : null
  );
}
var SectionIcon = ({ size = 16 }) => (0, import_react.createElement)(
  "svg",
  { className: "ub-icon", viewBox: "0 0 16 16", width: size, height: size, "aria-hidden": "true" },
  (0, import_react.createElement)("path", { d: "M2 9.5c2.2-3.4 4-3.4 6 0s3.8 3.4 6 0", fill: "none", stroke: "currentColor", strokeWidth: 1.3, strokeLinecap: "round" }),
  (0, import_react.createElement)("path", { d: "M2 5.5c2.2-3.4 4-3.4 6 0s3.8 3.4 6 0", fill: "none", stroke: "currentColor", strokeWidth: 1.3, strokeLinecap: "round", opacity: 0.55 })
);
var AppIcon = () => (0, import_react.createElement)(
  "svg",
  { className: "ub-icon", viewBox: "0 0 16 16", width: 14, height: 14, "aria-hidden": "true" },
  (0, import_react.createElement)("rect", { x: 2, y: 2.5, width: 12, height: 11, rx: 2, fill: "none", stroke: "currentColor", strokeWidth: 1.3 }),
  (0, import_react.createElement)("path", { d: "M8 5v5M5.8 7.8 8 10l2.2-2.2", fill: "none", stroke: "currentColor", strokeWidth: 1.3 })
);
function InstallPanel({ offer, t, onInstall, onClose }) {
  const panel = (0, import_react.useRef)(null);
  (0, import_react.useEffect)(() => {
    panel.current?.focus();
  }, []);
  return (0, import_react.createElement)(
    "div",
    {
      ref: panel,
      className: "ub-board",
      role: "dialog",
      tabIndex: -1,
      "aria-label": t("install.title"),
      "data-unfold-install-panel": "",
      onKeyDown: (event) => {
        if (event.key === "Escape") onClose();
      }
    },
    (0, import_react.createElement)(
      "header",
      { className: "ub-board-head" },
      (0, import_react.createElement)("h2", null, t("install.title")),
      (0, import_react.createElement)("button", { type: "button", className: "ub-button ub-quiet", onClick: onClose }, t("board.close"))
    ),
    offer.canPrompt ? [
      (0, import_react.createElement)("p", { key: "hint", className: "ub-hint" }, t("install.hint")),
      (0, import_react.createElement)("button", { key: "install", type: "button", className: "ub-button ub-primary", "data-unfold-install": "", onClick: onInstall }, t("install.button"))
    ] : (0, import_react.createElement)("p", { className: "ub-hint", "data-unfold-install-safari": "" }, t("install.safari"))
  );
}
function InstallAction({ offer, t, onInstall, wide }) {
  const [open, setOpen] = (0, import_react.useState)(offer.opened);
  if (!offerShown(offer)) return null;
  return (0, import_react.createElement)(
    "div",
    { className: "ub-footer" },
    (0, import_react.createElement)("button", {
      type: "button",
      className: "ub-footer-button",
      "aria-expanded": open ? "true" : "false",
      "aria-label": t("install.open"),
      "data-unfold-install-button": "",
      onClick: () => offer.canPrompt ? onInstall() : setOpen(!open)
    }, (0, import_react.createElement)(AppIcon), wide ? (0, import_react.createElement)("span", null, t("install.open")) : null),
    open ? (0, import_react.createElement)(InstallPanel, { offer, t, onInstall, onClose: () => setOpen(false) }) : null
  );
}
function DraftCard({ draft, t, actions }) {
  const [run, tap] = useTap();
  const to = t("dock.draftTo", { name: draft.name, workspace: draft.workspace });
  const hidden = [...hiddenIn(to), ...hiddenIn(draft.text)];
  return (0, import_react.createElement)(
    "section",
    { className: "ub-draft", "data-unfold-draft": draft.effect_id },
    (0, import_react.createElement)("p", { className: "ub-label" }, t("dock.draftBy")),
    (0, import_react.createElement)("p", { className: "ub-to", "data-unfold-draft-to": "" }, to),
    (0, import_react.createElement)(Hidden, { points: hidden, t }),
    (0, import_react.createElement)("div", { className: "ub-draft-text", "data-unfold-draft-text": "" }, draft.text),
    (0, import_react.createElement)("p", { className: "ub-hint" }, hidden.length > 0 ? t("hidden.hold") : t("dock.draftHint")),
    (0, import_react.createElement)(
      "div",
      { className: "ub-actions" },
      (0, import_react.createElement)("button", { type: "button", className: "ub-button ub-primary", "data-unfold-send": draft.effect_id, disabled: tap.busy || hidden.length > 0, onClick: () => {
        if (hidden.length === 0) run(() => actions.send(draft));
      } }, tap.busy ? t("dock.sending") : t("dock.send")),
      (0, import_react.createElement)("button", { type: "button", className: "ub-button ub-quiet", "data-unfold-cancel": draft.effect_id, disabled: tap.busy, onClick: () => run(() => actions.cancel(draft)) }, t("dock.cancel"))
    ),
    (0, import_react.createElement)(TapError, { code: tap.error, t })
  );
}
function Dock({ snap, t, actions, sessionId }) {
  const advice = pendingFor(snap, sessionId).map((approval) => adviceFor(snap, approval)).filter(Boolean);
  const draft = draftFor(snap, sessionId);
  if (advice.length === 0 && !draft) return null;
  return (0, import_react.createElement)(
    "div",
    { className: "ub-dock", "data-unfold-dock": sessionId },
    advice.map((note) => (0, import_react.createElement)(Advice, { key: note.call_id, advice: note, t })),
    draft ? (0, import_react.createElement)(DraftCard, { key: draft.effect_id, draft, t, actions }) : null
  );
}
function SubscriptionRow({ sub, snap, t, actions }) {
  const [run, tap] = useTap();
  const badge = sub.added_by === "unfold" ? t("settings.byUnfold") : sub.added_by === "person" ? t("settings.byYou") : null;
  return (0, import_react.createElement)(
    "li",
    { className: "ub-sub", "data-unfold-sub": sub.id },
    (0, import_react.createElement)("span", null, describeSubscription(sub, snap, t)),
    badge ? (0, import_react.createElement)("span", { className: "ub-badge", "data-unfold-by": sub.added_by }, badge) : null,
    (0, import_react.createElement)("button", { type: "button", className: "ub-button ub-quiet", disabled: tap.busy, onClick: () => run(() => actions.drop(sub.id)) }, t("settings.remove")),
    (0, import_react.createElement)(TapError, { code: tap.error, t })
  );
}
function ChangeRow({ entry, snap, t, actions }) {
  const [run, tap] = useTap();
  const at = when(entry.at, t);
  return (0, import_react.createElement)(
    "li",
    { className: "ub-change", "data-unfold-change": entry.change_id },
    at ? (0, import_react.createElement)("time", { className: "ub-hint", dateTime: new Date(entry.at).toISOString() }, at) : null,
    (0, import_react.createElement)("span", null, describeChange(entry, snap, t)),
    (0, import_react.createElement)("button", { type: "button", className: "ub-button ub-quiet", disabled: tap.busy, onClick: () => run(() => actions.undo(entry.change_id)) }, t("settings.undo")),
    (0, import_react.createElement)(TapError, { code: tap.error, t })
  );
}
function SettingsSection({ snap, t, actions }) {
  const head = [
    (0, import_react.createElement)("h2", { key: "title" }, t("settings.title")),
    (0, import_react.createElement)("p", { key: "intro", className: "ub-hint" }, t("settings.intro")),
    (0, import_react.createElement)("h3", { key: "link" }, t("settings.link")),
    (0, import_react.createElement)(LinkState, { key: "state", snap, t })
  ];
  if (!snap?.view) return (0, import_react.createElement)("div", { className: "ub-settings", "data-unfold-settings": "" }, head);
  const subs = snap.view.subscriptions ?? [];
  const changes = changesOf(snap);
  return (0, import_react.createElement)(
    "div",
    { className: "ub-settings", "data-unfold-settings": "" },
    head,
    (0, import_react.createElement)("h3", null, t("settings.subscriptions")),
    subs.length === 0 ? (0, import_react.createElement)("p", { className: "ub-hint" }, t("settings.noSubscriptions")) : (0, import_react.createElement)("ul", { className: "ub-subs" }, subs.map((sub) => (0, import_react.createElement)(SubscriptionRow, { key: sub.id, sub, snap, t, actions }))),
    (0, import_react.createElement)("h3", null, t("settings.log")),
    changes.length === 0 ? (0, import_react.createElement)("p", { className: "ub-hint" }, t("settings.noLog")) : (0, import_react.createElement)("ul", { className: "ub-changes" }, changes.map((entry) => (0, import_react.createElement)(ChangeRow, { key: entry.change_id, entry, snap, t, actions })))
  );
}

// src/client/styles.js
var STYLES = `
.ub-row { display: inline-flex; align-items: center; gap: 2px; padding: 2px 4px; border: 0; border-radius: var(--dsw-radius-sm, 4px); background: none; color: var(--dsw-alias-label-tertiary, inherit); cursor: pointer; }
.ub-row:hover, .ub-footer-button:hover { background: var(--dsw-alias-interactive-bg-hover, rgba(127, 127, 127, 0.12)); }
.ub-row-on { color: var(--dsw-alias-state-business-primary, inherit); }
.ub-watched { display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 16px; color: var(--dsw-alias-state-business-primary, inherit); }
.ub-watched .ub-eye { width: 12px; height: 12px; }
.ub-row:focus-visible, .ub-footer-button:focus-visible, .ub-button:focus-visible { outline: var(--dsw-focus-ring-width, 2px) solid var(--dsw-focus-ring-color, Highlight); }
.ub-dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--dsw-alias-label-caption, gray); opacity: 0.5; }
.ub-dot-watched { opacity: 1; }
.ub-dot-running { background: var(--dsw-alias-state-business-primary, royalblue); }
.ub-dot-error { background: var(--dsw-alias-state-error-primary, crimson); }
.ub-footer { position: relative; }
.ub-footer-button { display: inline-flex; align-items: center; gap: 6px; padding: 6px 8px; border: 0; border-radius: var(--dsw-radius-md, 6px); background: none; color: var(--dsw-alias-label-secondary, inherit); font: inherit; cursor: pointer; }
.ub-count { min-width: 16px; padding: 0 4px; border-radius: 8px; background: var(--dsw-alias-state-business-primary, royalblue); color: var(--dsw-alias-bg-base, white); font-size: 11px; text-align: center; }
.ub-board { position: fixed; left: 8px; bottom: 56px; z-index: 50; width: min(380px, calc(100vw - 16px)); max-height: 70vh; overflow: auto; padding: 12px; border: 1px solid var(--dsw-alias-border-l2, rgba(127, 127, 127, 0.3)); border-radius: var(--dsw-radius-lg, 8px); background: var(--dsw-alias-bg-layer-1, Canvas); color: var(--dsw-alias-label-primary, CanvasText); box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18); }
.ub-board-head { display: flex; align-items: center; justify-content: space-between; }
.ub-board h2, .ub-settings h2 { margin: 0 0 8px; font-size: 15px; }
.ub-board h3, .ub-settings h3 { margin: 12px 0 4px; font-size: 13px; color: var(--dsw-alias-label-secondary, inherit); }
.ub-board ul, .ub-settings ul { margin: 0; padding: 0; list-style: none; }
.ub-board li, .ub-settings li { padding: 4px 0; }
.ub-agent-line { display: flex; align-items: center; gap: 6px; }
.ub-agent-name { font-weight: 500; }
.ub-badge { padding: 0 6px; border: 1px solid var(--dsw-alias-border-l3, rgba(127, 127, 127, 0.4)); border-radius: 8px; font-size: 11px; color: var(--dsw-alias-label-secondary, inherit); }
.ub-hint, .ub-link { margin: 2px 0; font-size: 12px; color: var(--dsw-alias-label-tertiary, inherit); }
.ub-link-on { color: var(--dsw-alias-state-business-primary, inherit); }
.ub-label { margin: 0 0 2px; font-size: 12px; font-weight: 600; color: var(--dsw-alias-label-secondary, inherit); }
.ub-marked { margin: 0 0 4px; font-size: 12px; font-weight: 600; color: var(--dsw-alias-state-error-primary, crimson); }
.ub-note { margin: 0 0 4px; white-space: pre-wrap; overflow-wrap: anywhere; unicode-bidi: plaintext; }
.ub-error { margin: 4px 0 0; font-size: 12px; color: var(--dsw-alias-state-error-primary, crimson); }
.ub-dock { display: flex; flex-direction: column; gap: 8px; margin: 0 0 8px; }
.ub-advice, .ub-draft, .ub-suggest { padding: 8px 10px; border: 1px solid var(--dsw-alias-border-l2, rgba(127, 127, 127, 0.3)); border-radius: var(--dsw-radius-md, 6px); background: var(--dsw-alias-bg-layer-1, transparent); }
/* Text from agents keeps its own direction: a stray right-to-left mark can't reorder the line around it. */
.ub-to { margin: 0 0 6px; font-weight: 600; unicode-bidi: plaintext; }
.ub-draft-text { white-space: pre-wrap; overflow-wrap: anywhere; unicode-bidi: plaintext; font-family: var(--ds-font-family-code, monospace); font-size: 13px; }
.ub-actions { display: flex; gap: 8px; margin-top: 8px; }
.ub-button { padding: 4px 10px; border: 1px solid var(--dsw-alias-border-l3, rgba(127, 127, 127, 0.4)); border-radius: var(--dsw-radius-sm, 4px); background: none; color: inherit; font: inherit; font-size: 12px; cursor: pointer; }
.ub-primary { border-color: var(--dsw-alias-state-business-primary, royalblue); background: var(--dsw-alias-state-business-primary, royalblue); color: var(--dsw-alias-bg-base, white); }
.ub-quiet { border-color: transparent; }
.ub-button:disabled { opacity: 0.5; cursor: not-allowed; }
.ub-sub, .ub-change, .ub-notice { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
`;
function installStyles() {
  const style = document.createElement("style");
  style.dataset.unfoldBridge = "";
  style.textContent = STYLES;
  document.head.append(style);
  return () => style.remove();
}

// src/client/index.js
var import_react2 = require("react");
var SLOTS = [
  ["sidebar.workspaces.session.row.action", { id: "unfold-watch", order: 300 }, RowAction],
  // DSH mounts this seat only on an idle row, in place of the row's own state dot.
  ["sidebar.session.row.leading", { id: "unfold-watched", order: 100 }, WatchedMark],
  ["sidebar.footer.action", { id: "unfold-board", order: 100 }, BoardButton],
  ["conversation.input.dock", { id: "unfold-dock", order: 50 }, Dock],
  // The nav label is "Unfold" in every language, so it never needs registering again on a
  // locale change.
  ["settings.section", { id: "unfold-bridge", order: 900, label: "Unfold" }, SettingsSection],
  // DSH 0.2.2 renders a nav icon for a section whose id matches this seat's registration.
  ["settings.section.icon", { id: "unfold-bridge", order: 900 }, SectionIcon]
];
var INSTALL_SLOT = ["sidebar.footer.action", { id: "unfold-bridge.install", order: 110 }];
var inject = ["slots"];
var install = createInstall(globalThis.window);
install.watch();
function createStore() {
  let snap = null;
  const listeners = /* @__PURE__ */ new Set();
  return {
    get: () => snap,
    set(next) {
      snap = next;
      for (const fn of Array.from(listeners)) fn();
    },
    subscribe(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    }
  };
}
function apply(ctx) {
  const store = createStore();
  const locale = ctx.get?.("locale");
  const language = () => languageOf(locale?.getLocale?.().active);
  const onLocale = (fn) => locale?.subscribe?.(fn) ?? (() => {
  });
  const actions = { send: sendDraft, cancel: cancelDraft, watch: watchSession, drop: dropSubscription, undo: undoChange };
  const bind = (View) => function UnfoldSlot(props) {
    const snap = (0, import_react2.useSyncExternalStore)(store.subscribe, store.get);
    const t = translator((0, import_react2.useSyncExternalStore)(onLocale, language));
    return (0, import_react2.createElement)(View, { ...props, snap, t, actions });
  };
  ctx.effect(installStyles, "unfold-bridge: client styles");
  ctx.effect(() => openView((snap) => store.set(snap)), "unfold-bridge: the view stream");
  ctx.effect(() => install.watch(), "unfold-bridge: the install offer");
  for (const [name, options, View] of SLOTS) ctx.slots.inject(name, () => ctx.slots.register({ name, ...options }, bind(View)));
  const [slot, slotOptions] = INSTALL_SLOT;
  ctx.slots.inject(slot, () => ctx.slots.register({ name: slot, ...slotOptions }, function UnfoldInstall(props) {
    const offer = (0, import_react2.useSyncExternalStore)(install.subscribe, install.get);
    const t = translator((0, import_react2.useSyncExternalStore)(onLocale, language));
    return (0, import_react2.createElement)(InstallAction, { ...props, offer, t, onInstall: install.prompt });
  }));
}
return Object.defineProperty({ ...module.exports }, Symbol.toStringTag, { value: 'Module' })
  },
})
