

use crate::{owned_dir, owned_replace};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::webview::{PermissionResponse, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, WebviewUrl};

pub const APPEARANCES: [&str; 3] = ["system", "day", "night"];
pub const LANGUAGES: [&str; 3] = ["system", "en", "pt-BR"];

pub const DICTATION_LANGUAGES: [&str; 3] = ["auto", "pt", "en"];

pub const HANDOFF_LOCK: &str = "Experimental: locked until the hand-off classifier passes its gate.";
const HANDOFF_LOCK_PT: &str = "Experimental: bloqueado até o classificador de tarefas passar no teste.";

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {

    pub auto_update: bool,

    pub appearance: String,

    pub language: String,

    pub dictation_language: String,

    pub suggest_handoffs: bool,

    pub answer_with: String,

    pub cloud_ack: bool,

    pub use_dsh_watch: bool,

    #[cfg(feature = "menu_bar")]
    pub use_own_dsh: bool,

    pub deepseek_effort: String,

    pub deepseek_note: bool,

    pub reply_max_tokens: Option<u32>,

    pub smart_turn_endpoint: bool,

    pub smart_turn_shadow: bool,

    pub voice_front_templates: bool,
}

pub const REPLY_MAX_TOKENS: std::ops::RangeInclusive<u64> = 1..=1_048_576;

fn reply_max_of(v: &Value) -> Option<u32> {
    v.as_u64().filter(|n| REPLY_MAX_TOKENS.contains(n)).map(|n| n as u32)
}

pub const EFFORTS: [&str; 5] = ["auto", "off", "low", "high", "max"];

pub const ANSWER_GRANITE: &str = "granite";

pub const ANSWER_DEEPSEEK: &str = "deepseek";

pub fn answer_ok(a: &str) -> bool {
    a == ANSWER_GRANITE || a == ANSWER_DEEPSEEK || crate::dsh::model_id_ok(a)
}

pub fn answer_from(v: &Value) -> String {
    if let Some(a) = v["answer_with"].as_str().filter(|a| answer_ok(a)) {

        return if crate::dsh::is_deepseek_model(a) { ANSWER_DEEPSEEK } else { a }.to_string();
    }
    (if v["use_cloud"].as_bool().unwrap_or(true) { ANSWER_DEEPSEEK } else { ANSWER_GRANITE }).to_string()
}

impl Settings {

    pub fn automagik_on(&self) -> bool {
        #[cfg(feature = "menu_bar")]
        return self.use_own_dsh;
        #[cfg(not(feature = "menu_bar"))]
        false
    }

    pub fn answers_direct(&self) -> bool {
        self.answer_with == ANSWER_DEEPSEEK
    }

    pub fn answers_via_bridge(&self) -> bool {
        self.answer_with != ANSWER_GRANITE && !self.answers_direct()
    }
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { auto_update: true, appearance: "system".into(), language: "system".into(), dictation_language: "auto".into(), suggest_handoffs: false, answer_with: ANSWER_DEEPSEEK.into(), cloud_ack: false, use_dsh_watch: false, #[cfg(feature = "menu_bar")] use_own_dsh: false, deepseek_effort: "auto".into(), deepseek_note: false, reply_max_tokens: None, smart_turn_endpoint:false, smart_turn_shadow:true, voice_front_templates:false }
    }
}

pub fn path(data: &Path) -> PathBuf {
    data.join("app").join("settings.json")
}

pub fn from_json(v: &Value) -> Settings {
    let d = Settings::default();
    let pick = |k: &str, allowed: &[&str], def: String| v[k].as_str().filter(|s| allowed.contains(s)).map(str::to_string).unwrap_or(def);
    Settings {
        auto_update: v["auto_update"].as_bool().unwrap_or(d.auto_update),
        appearance: pick("appearance", &APPEARANCES, d.appearance),
        language: pick("language", &LANGUAGES, d.language),
        dictation_language: pick("dictation_language", &DICTATION_LANGUAGES, d.dictation_language),

        suggest_handoffs: false,
        answer_with: answer_from(v),
        cloud_ack: v["cloud_ack"] == true,
        use_dsh_watch: v["use_dsh_watch"] == true,
        #[cfg(feature = "menu_bar")]
        use_own_dsh: v["use_own_dsh"] == true,
        deepseek_effort: pick("deepseek_effort", &EFFORTS, d.deepseek_effort),
        deepseek_note: v["deepseek_note"] == true,
        reply_max_tokens: reply_max_of(&v["reply_max_tokens"]),
        smart_turn_endpoint:v["smart_turn_endpoint"].as_bool().unwrap_or(d.smart_turn_endpoint),
        smart_turn_shadow:v["smart_turn_shadow"].as_bool().unwrap_or(d.smart_turn_shadow),
        voice_front_templates:v["voice_front_templates"] == true,
    }
}

pub fn to_json(s: &Settings) -> Value {
    #[allow(unused_mut)]
    let mut v = json!({"auto_update": s.auto_update, "appearance": s.appearance, "language": s.language, "dictation_language": s.dictation_language, "suggest_handoffs": s.suggest_handoffs, "answer_with": s.answer_with, "cloud_ack": s.cloud_ack, "use_dsh_watch": s.use_dsh_watch, "deepseek_effort": s.deepseek_effort, "deepseek_note": s.deepseek_note});
    #[cfg(feature = "menu_bar")]
    {
        v["use_own_dsh"] = json!(s.use_own_dsh);
    }
    if let Some(r) = s.reply_max_tokens {
        v["reply_max_tokens"] = json!(r);
    }
    v["smart_turn_endpoint"]=json!(s.smart_turn_endpoint);
    v["smart_turn_shadow"]=json!(s.smart_turn_shadow);
    v["voice_front_templates"]=json!(s.voice_front_templates);
    v
}

pub fn load(data: &Path) -> Settings {
    std::fs::read(path(data)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| from_json(&v)).unwrap_or_default()
}

pub fn save(data: &Path, s: &Settings) -> Result<(), String> {
    owned_dir(data, &data.join("app"))?;
    let p = path(data);
    owned_replace(&p, &data.join("app").join("settings.json.tmp"), format!("{:#}\n", to_json(s)).as_bytes()).map_err(|e| format!("{}: {e}", p.display()))?;
    if CTX.get().is_some_and(|c|c.data==data) {sync_turn_mode(s);}
    Ok(())
}

pub fn apply(s: &mut Settings, key: &str, value: &Value) -> Result<(), String> {
    let text = |allowed: &[&str]| value.as_str().filter(|v| allowed.contains(v)).map(str::to_string).ok_or(format!("{key}: one of {}", allowed.join(", ")));
    match key {
        "voice_front_templates" => s.voice_front_templates=value.as_bool().ok_or("voice_front_templates: true or false")?,
        "smart_turn_endpoint" => s.smart_turn_endpoint=value.as_bool().ok_or("smart_turn_endpoint: true or false")?,
        "smart_turn_shadow" => s.smart_turn_shadow=value.as_bool().ok_or("smart_turn_shadow: true or false")?,
        "auto_update" => s.auto_update = value.as_bool().ok_or("auto_update: true or false")?,
        "answer_with" => {
            let a = value.as_str().filter(|a| answer_ok(a)).ok_or("answer_with: granite, deepseek or a model id")?;

            s.answer_with = if crate::dsh::is_deepseek_model(a) { ANSWER_DEEPSEEK } else { a }.to_string();

            s.cloud_ack |= s.answers_direct();
        }
        "cloud_ack" if value == &json!(true) => s.cloud_ack = true,
        "cloud_ack" => return Err("cloud_ack: only true".into()),
        "use_dsh_watch" => s.use_dsh_watch = value.as_bool().ok_or("use_dsh_watch: true or false")?,
        "deepseek_effort" => s.deepseek_effort = text(&EFFORTS)?,

        "reply_max_tokens" if value.is_null() || value == &json!(false) => s.reply_max_tokens = None,
        "reply_max_tokens" => s.reply_max_tokens = Some(reply_max_of(value).ok_or("reply_max_tokens: a whole number from 1 to 1048576, or false (off)")?),
        #[cfg(feature = "menu_bar")]
        "use_own_dsh" => s.use_own_dsh = value.as_bool().ok_or("use_own_dsh: true or false")?,
        "appearance" => s.appearance = text(&APPEARANCES)?,
        "language" => s.language = text(&LANGUAGES)?,
        "dictation_language" => s.dictation_language = text(&DICTATION_LANGUAGES)?,
        "suggest_handoffs" if value == &json!(false) => s.suggest_handoffs = false,
        "suggest_handoffs" => return Err(HANDOFF_LOCK.into()),
        _ => return Err(format!("unknown setting {key:?}")),
    }
    Ok(())
}

pub fn language_tag(s: &Settings, system: Option<String>) -> Option<String> {
    match s.language.as_str() {
        "system" => system,
        l => Some(l.to_string()),
    }
}

#[allow(dead_code)]
pub fn front_reply_locale() -> Option<crate::voice_front::policy::Selection> {
    front_locale_for(&current(), crate::system_language())
}
fn front_locale_for(s: &Settings, system: Option<String>) -> Option<crate::voice_front::policy::Selection> {
    use crate::voice_front::policy::LocaleSource;
    let source = if s.language == "system" { LocaleSource::System } else { LocaleSource::Setting };
    crate::voice_front::policy::settings_locale(language_tag(s, system).as_deref(), source)
}

pub fn is_pt(tag: Option<&str>) -> bool {
    tag.is_some_and(|t| t.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("pt")) && t.get(2..3).is_none_or(|c| c == "-" || c == "_"))
}

pub fn pt() -> bool {
    is_pt(language_tag(&current(), crate::system_language()).as_deref())
}

pub fn t<'a>(en: &'a str, pt: &'a str) -> &'a str {
    if self::pt() {
        pt
    } else {
        en
    }
}

pub fn theme_js(t: &str) -> String {
    let t = json!(if APPEARANCES.contains(&t) { t } else { "system" });
    format!("(()=>{{try{{if(typeof window.__rthTheme==='function'){{window.__rthTheme({t});return}}if({t}==='system')localStorage.removeItem('unfold.theme');else localStorage.setItem('unfold.theme',{t})}}catch(e){{}}}})()")
}

pub fn memory_count(instance: &Path) -> Option<usize> {
    let text = std::fs::read_to_string(instance.join("agentd").join("memory-acts.jsonl")).ok()?;
    let ids: std::collections::BTreeSet<String> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["state"] == "committed")
        .filter_map(|v| v["outcome"]["memory_id"].as_str().map(str::to_string))
        .collect();
    Some(ids.len())
}

pub struct Ctx {
    pub data: PathBuf,

    pub contents: Option<PathBuf>,

    pub instance: Option<PathBuf>,
    pub version: String,
    pub log: crate::Log,
}
pub static CTX: OnceLock<Ctx> = OnceLock::new();

static WRITE: Mutex<()> = Mutex::new(());

static TURN_MODE:std::sync::atomic::AtomicU8=std::sync::atomic::AtomicU8::new(2);
pub(crate) fn sync_turn_mode(s:&Settings){
    TURN_MODE.store(u8::from(s.smart_turn_endpoint)|(u8::from(s.smart_turn_shadow)<<1),std::sync::atomic::Ordering::SeqCst);
}
pub fn turn_mode()->crate::turn_detect::Mode{
    let n=TURN_MODE.load(std::sync::atomic::Ordering::SeqCst);
    crate::turn_detect::Mode{endpoint:n&1!=0,shadow:n&2!=0}
}
pub fn current() -> Settings {
    CTX.get().map(|c| load(&c.data)).unwrap_or_default()
}

pub fn open(app: &AppHandle) {
    open_at(app, None)
}

pub fn open_at(app: &AppHandle, pane: Option<&str>) {
    let _ = APP.set(app.clone());
    let pane = pane.filter(|p| p.chars().all(|c| c.is_ascii_lowercase()));
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(p) = pane {
            let _ = w.eval(format!("window.__settingsGo && window.__settingsGo({p:?})"));
        }
        return;
    }
    let effects = tauri::window::EffectsBuilder::new().effect(tauri::window::Effect::Sidebar).state(tauri::window::EffectState::FollowsWindowActiveState).build();
    let page = pane.map_or("settings.html".to_string(), |p| format!("settings.html#pane={p}"));
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App(page.into()))
        .title(t("Settings", "Ajustes"))
        .inner_size(860.0, 640.0)
        .min_inner_size(680.0, 460.0)

        .visible(false)
        .maximizable(false)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .transparent(true)
        .effects(effects)
        .on_permission_request(|_, _| PermissionResponse::Deny)
        .on_navigation(|u| u.scheme() == "tauri" && u.host_str() == Some("localhost"))
        .build();
    match built {
        Ok(w) => {
            crate::fit_window(&w, (860.0, 640.0));
            let _ = w.show();
            let _ = w.set_focus();
        }
        Err(e) => {
            if let Some(c) = CTX.get() {
                c.log.line(&format!("settings: the window did not open: {e}"));
            }
        }
    }
}

pub fn refresh(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.eval(format!("window.__settings && window.__settings({})", view()));
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn refresh_open() {
    if let Some(app) = APP.get() {
        refresh(app);
    }
}

fn catalog() -> Option<Value> {
    CTX.get()?.contents.as_ref().and_then(|c| crate::models::load_catalog(c).ok())
}

fn model_name(cat: Option<&Value>, id: &str) -> String {
    cat.and_then(|c| c["models"].as_array()?.iter().find(|m| m["id"] == id)?["name"].as_str().map(str::to_string)).unwrap_or_else(|| id.to_string())
}

pub fn answer_after_welcome(current: &str, on: bool) -> &str {
    match (on, current) {
        (false, _) => ANSWER_GRANITE,
        (true, ANSWER_GRANITE) => ANSWER_DEEPSEEK,
        (true, a) => a,
    }
}

pub fn set_answer_from_welcome(on: bool) -> Result<(), String> {
    let c = CTX.get().ok_or("settings are not available in this run")?;
    let _w = WRITE.lock().unwrap();
    let mut s = load(&c.data);
    let a = answer_after_welcome(&s.answer_with, on).to_string();

    let ack = s.cloud_ack || a == ANSWER_DEEPSEEK;
    if s.answer_with != a || s.cloud_ack != ack {
        s.answer_with = a;
        s.cloud_ack = ack;
        save(&c.data, &s)?;
        c.log.line(&format!("settings: answer_with = {} (welcome page)", s.answer_with));
    }
    Ok(())
}

pub fn migrate(data: &Path, log: &crate::Log) {
    let Some(raw) = std::fs::read(path(data)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return };
    let Some(old) = raw["answer_with"].as_str().filter(|a| answer_ok(a) && crate::dsh::is_deepseek_model(a)) else { return };
    let _w = WRITE.lock().unwrap();
    let mut s = from_json(&raw);
    s.deepseek_note = true;
    match save(data, &s) {
        Ok(()) => log.line(&format!("settings: answer_with {old} (DeepSeek through Unfold Automagik) is now deepseek (direct, it sends the recalled memories); a one-time note goes with its first reply")),
        Err(e) => log.line(&format!("settings: answer_with {old} could not be moved to deepseek: {e}")),
    }
}

pub fn take_deepseek_note() -> bool {
    let Some(c) = CTX.get() else { return false };
    let _w = WRITE.lock().unwrap();
    let mut s = load(&c.data);
    if !s.deepseek_note {
        return false;
    }
    s.deepseek_note = false;
    if let Err(e) = save(&c.data, &s) {
        c.log.line(&format!("settings: the DeepSeek note could not be cleared: {e}"));
    }
    true
}

pub fn turn_effort() -> String {
    if crate::flash::direct() {
        current().deepseek_effort
    } else {
        "off".into()
    }
}

pub fn turn_reply_max_tokens() -> Option<u32> {
    current().reply_max_tokens
}

static REV: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub fn rev() -> u64 {
    let _ = REV.compare_exchange(0, now_ms(), std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst);
    REV.load(std::sync::atomic::Ordering::SeqCst)
}
fn bump_rev() -> u64 {
    rev();
    REV.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1).max(1)
}

pub static SWITCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn restart_after_reply() -> bool {
    SWITCHING.store(true, std::sync::atomic::Ordering::SeqCst);
    #[cfg(feature = "menu_bar")]
    {
        let waits = crate::chat::replying();
        std::thread::spawn(|| crate::dsh_child::restart_after_reply(&crate::chat::replying, &crate::setup::restart_chat, std::time::Duration::from_secs(120)));
        waits
    }
    #[cfg(not(feature = "menu_bar"))]
    {
        crate::setup::restart_chat();
        false
    }
}

pub fn choose(choice: Option<&str>, effort: Option<&str>, ack: bool, from: &str) -> Result<(u64, bool), String> {
    let c = CTX.get().ok_or("settings are not available in this run")?;
    let changed = {
        let _w = WRITE.lock().unwrap();
        let mut s = load(&c.data);
        let before = s.answer_with.clone();
        if let Some(a) = choice {
            apply(&mut s, "answer_with", &json!(a))?;
        }
        if let Some(e) = effort {
            apply(&mut s, "deepseek_effort", &json!(e))?;
        }
        s.cloud_ack |= ack && s.answers_direct();
        save(&c.data, &s)?;
        c.log.line(&format!("settings: answer_with = {}, deepseek_effort = {} ({from})", s.answer_with, s.deepseek_effort));
        s.answer_with != before
    };
    let rev = bump_rev();
    refresh_open();
    let after_reply = changed && restart_after_reply();
    Ok((rev, after_reply))
}

pub fn model_name_of(id: &str) -> String {
    crate::dsh::name_of(id).unwrap_or_else(|| model_name(catalog().as_ref(), id))
}

pub fn view() -> Value {
    let Some(c) = CTX.get() else { return json!({"error": t("Settings are not available in this run.", "Os ajustes não estão disponíveis nesta execução.")}) };
    let s = load(&c.data);
    let cat = catalog();
    let chosen = crate::models::load_choice(&c.data);
    let talk = chosen.as_ref().and_then(|ch| ch["talk"].as_str()).map(|id| model_name(cat.as_ref(), id));
    let memory_model = chosen.as_ref().and_then(|ch| ch["memory.decide"].as_str()).map(|id| model_name(cat.as_ref(), id));
    let offer = cat.as_ref().map(|k| crate::models::offer(k, &c.data, &crate::home()));
    let dictation = crate::dictation_models::view(&c.data, pt(), crate::dictation::test_result());
    with_voice_pane(&c.data, with_own_dsh(with_self_pane(json!({
        "settings": to_json(&s),
        "about": {"version": c.version, "build": build_id(), "talk_model": talk, "memory_model": memory_model},
        "update": crate::updater::status(),
        "models": {"chosen": chosen, "parts": offer},
        "memory": {"on": true, "count": c.instance.as_deref().and_then(memory_count)},
        "handoffs": {"on": false, "locked": t(HANDOFF_LOCK, HANDOFF_LOCK_PT)},
        "cloud": crate::flash::view(s.answers_direct(), &c.data),
        "dsh": crate::dsh::view(&s, &crate::dsh::source()),
        "dsh_watch": crate::dsh::watch_view(s.automagik_on() && s.use_dsh_watch, &crate::dsh::source()),
        "dictation": dictation,
        "privacy": {
            "data": c.data,
            "brain": c.instance.as_ref().map(|i| i.join("brain-home")),
            "models_dir": c.data.join("models"),
        },
        "system_language": crate::system_language(),
        "system_clock": crate::system_hour_cycle(),
    }))))
}

fn with_self_pane(mut v: Value) -> Value {
    if cfg!(feature = "self_pane") {
        v["self_pane"] = json!(true);
    }
    v
}

fn with_own_dsh(#[allow(unused_mut)] mut v: Value) -> Value {
    #[cfg(feature = "menu_bar")]
    {
        v["own_dsh"] = crate::dsh_child::view();
    }
    v
}

#[cfg(feature = "voice_pane")]
fn with_voice_pane(data: &Path, mut v: Value) -> Value {
    v["voice_pane"] = json!(true);
    if let Some(shown) = voice(data).shown() {
        v["voice"] = shown;
    }
    v
}
#[cfg(not(feature = "voice_pane"))]
fn with_voice_pane(_: &Path, v: Value) -> Value {
    v
}

fn build_id() -> Option<String> {
    static ID: OnceLock<Option<String>> = OnceLock::new();
    ID.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        crate::models::hash_file(&exe, &|_| {}).ok().map(|h| h[..12].to_string())
    })
    .clone()
}

#[tauri::command(async)]
pub fn rth_settings(webview: tauri::Webview, action: String, key: Option<String>, value: Option<Value>) -> Result<Value, String> {
    run(&webview, &action, key, value).map_err(|f| {
        if let Some(c) = CTX.get() {
            c.log.line(&format!("settings: {action}: {}", f.detail));
        }
        t(f.said.0, f.said.1).to_string()
    })
}

type Said = (&'static str, &'static str);
const FAILED: Said = ("That didn’t work. Try again.", "Não deu certo; tente de novo.");
const NOT_SAVED: Said = ("Couldn’t save that setting.", "Não consegui guardar esse ajuste.");
const NOT_RESUMED: Said = ("Couldn’t resume updates. Try again.", "Não consegui retomar as atualizações; tente de novo.");
const DICTATION_BUSY: Said = ("That model is downloading. Pause or cancel it first.", "Esse modelo está sendo baixado. Pause ou cancele antes.");
const TEST_FAILED: Said = ("The test can’t run while dictation is listening.", "O teste não roda enquanto o ditado está ouvindo.");
const MODELS_LATER: Said = ("Models can be changed once Unfold is on its home screen.", "Dá para trocar os modelos quando Unfold estiver na tela inicial.");

struct Failed {
    said: Said,
    detail: String,
}
impl From<String> for Failed {
    fn from(detail: String) -> Self {
        Failed { said: FAILED, detail }
    }
}
impl From<&str> for Failed {
    fn from(detail: &str) -> Self {
        detail.to_string().into()
    }
}
fn said(said: Said) -> impl FnOnce(String) -> Failed {
    move |detail| Failed { said, detail }
}

fn run(webview: &tauri::Webview, action: &str, key: Option<String>, value: Option<Value>) -> Result<Value, Failed> {
    let url = webview.url().map_err(|e| e.to_string())?;
    if webview.label() != "settings" || url.scheme() != "tauri" || url.host_str() != Some("localhost") {
        return Err("settings are only available from Unfold's Settings window".into());
    }
    let c = CTX.get().ok_or("settings are not available in this run")?;
    if let Some(r) = self_action(action, value.as_ref()) {
        return r;
    }
    if let Some(r) = voice_action(Some(webview.app_handle()), &c.data, action, value.as_ref()) {
        return r;
    }
    match action {

        "get" => {
            crate::dsh::list_now(&crate::dsh::source(), &c.log);
            Ok(view())
        }
        "set" => {
            let key = key.ok_or("set: a key is required")?;
            let value = value.ok_or("set: a value is required")?;
            {
                let _w = WRITE.lock().unwrap();
                let mut s = load(&c.data);
                apply(&mut s, &key, &value)?;
                save(&c.data, &s).map_err(said(NOT_SAVED))?;
                c.log.line(&format!("settings: {key} = {value}"));
                if key == "appearance" {
                    if let Some(main) = webview.app_handle().get_webview_window("main") {
                        let _ = main.eval(theme_js(&s.appearance));
                    }
                }
            }
            #[cfg(feature = "voice_pane")]
            if key=="voice_front_templates" {crate::front_native::flag_changed();}
            if key == "auto_update" {
                crate::updater::wake();
            }
            if key == "answer_with" || key == "deepseek_effort" {
                bump_rev();
            }

            if key == "answer_with" || key == "cloud_ack" || key == "use_dsh_watch" {
                restart_after_reply();
            }
            #[cfg(feature = "menu_bar")]
            if key == "use_own_dsh" {
                crate::dsh_child::set_on(value == json!(true));
            }
            Ok(view())
        }
        "check_now" => {
            crate::updater::check_now();
            Ok(view())
        }

        #[cfg(feature = "menu_bar")]
        "own_dsh_retry" => {
            crate::dsh_child::retry();
            Ok(view())
        }
        #[cfg(feature = "menu_bar")]
        "own_dsh_fetch" => {
            crate::dsh_child::fetch_again();
            Ok(view())
        }
        #[cfg(feature = "menu_bar")]
        "own_dsh_omni_retry" => {
            crate::dsh_child::omni_retry().map_err(|e| e.to_string())?;
            Ok(view())
        }
        #[cfg(feature = "menu_bar")]
        "own_dsh_port" => {
            crate::dsh_child::another_port().map_err(|e| e.to_string())?;
            Ok(view())
        }
        #[cfg(feature = "menu_bar")]
        "own_dsh_install" | "own_dsh_open" => {
            crate::dsh_child::open(action == "own_dsh_install")?;
            Ok(view())
        }
        "resume_updates" => {
            crate::updater::resume_updates().map_err(said(NOT_RESUMED))?;
            c.log.line("settings: updates resumed by the owner");
            Ok(view())
        }
        "models" => {
            let choice = value.filter(Value::is_object).ok_or("models: a {part: id} choice is required")?;
            crate::setup::choose_models(choice).map_err(said(MODELS_LATER))?;
            Ok(view())
        }
        "dictation_install" | "dictation_pause" | "dictation_cancel" | "dictation_remove" | "dictation_default" | "dictation_test" => {
            let id = value.as_ref().and_then(Value::as_str).ok_or("dictation: a model id is required")?.to_string();
            crate::dictation_models::model(crate::dictation_models::catalog(), &id).ok_or("dictation: not a model id")?;
            let busy = crate::dictation_models::jobs().iter().any(|j| j.id == id && !matches!(j.stage, crate::dictation_models::Stage::Paused | crate::dictation_models::Stage::Failed(_)));
            match action {

                "dictation_install" => crate::dictation_models::start_install(c.data.clone(), &id, c.log.clone(), crate::dictation::models_changed).map_err(said(FAILED))?,
                "dictation_pause" => crate::dictation_models::pause_install(&c.data, &id),
                "dictation_cancel" => crate::dictation_models::cancel_install(&c.data, &id),
                "dictation_remove" => {
                    if busy {
                        return Err(Failed { said: DICTATION_BUSY, detail: format!("{id} is downloading") });
                    }
                    crate::dictation_models::cancel_install(&c.data, &id);
                    crate::dictation_models::remove(&c.data, &id)?;
                    c.log.line(&format!("settings: dictation model {id} removed"));
                }
                "dictation_test" => crate::dictation::start_test(c.data.clone(), &id, pt(), c.log.clone()).map_err(said(TEST_FAILED))?,
                _ => {
                    crate::dictation_models::set_default(&c.data, &id).map_err(said(NOT_SAVED))?;
                    c.log.line(&format!("settings: dictation default = {id}"));
                }
            }
            Ok(view())
        }
        _ => Err("unknown settings action".into()),
    }
}

#[cfg(feature = "self_pane")]
fn self_action(action: &str, value: Option<&Value>) -> Option<Result<Value, Failed>> {
    if !crate::self_link::ACTIONS.contains(&action) {
        return None;
    }
    Some(self_reply(action, value.unwrap_or(&Value::Null), &crate::self_link::LINK, crate::self_link::WAIT).map(|r| {
        let mut v = view();
        if let (Some(o), Some(r)) = (v.as_object_mut(), r.as_object()) {
            o.extend(r.clone());
        }
        v
    }))
}
#[cfg(not(feature = "self_pane"))]
fn self_action(_: &str, _: Option<&Value>) -> Option<Result<Value, Failed>> {
    None
}

#[cfg(feature = "voice_pane")]
fn voice_action(app: Option<&AppHandle>, data: &Path, action: &str, value: Option<&Value>) -> Option<Result<Value, Failed>> {
    if !crate::voice::ACTIONS.contains(&action) {
        return None;
    }
    if let Some(app) = app {
        let _ = VOICE_APP.set(app.clone());
    }

    if action == "clone_record_start" && crate::dictation::mic_busy() {
        return Some(Err(Failed { said: VOICE_DICTATING, detail: "clone_record_start: dictation is listening".into() }));
    }
    if action=="voice_stop" {crate::front_native::fence();}
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let v = voice(data);
    let listening_changed = matches!(action, "listening_set" | "voice_on" | "fleet_choose" | "voice_probe" | "say_set" | "voice_key_set" | "voice_key_clear");
    let r = v.act(action, value.unwrap_or(&Value::Null));
    if let (Some(e), Some(c)) = (v.take_probe_error(), CTX.get()) {
        c.log.line(&format!("settings: voice probe: {e}"));
    }
    if let (Some(e), Some(c)) = (v.take_key_error(), CTX.get()) {
        c.log.line(&format!("settings: voice key: {e}"));
    }
    if r.is_ok() && listening_changed { REPLIES.invalidate_listening(); reply_warm(); }
    Some(voice_reply(action, r, view))
}
#[cfg(not(feature = "voice_pane"))]
fn voice_action(_: Option<&AppHandle>, _: &Path, _: &str, _: Option<&Value>) -> Option<Result<Value, Failed>> {
    None
}

#[cfg(feature = "voice_pane")]
fn voice(data: &Path) -> crate::voice::Voice<'static> {
    let env_key = std::env::var("UNFOLD_LOCALFLEET_KEY").ok().filter(|k| !k.is_empty());
    crate::voice::Voice { root: data.join("voice"), backend: &crate::voice::Fleet, env_key, keys: VOICE_KEYS, probed: &crate::voice::PROBED, mic: &AppMic, studio: &crate::voice::STUDIO, speaker: &VOICE_SPEAKER, emit: voice_emit }
}

#[cfg(feature = "voice_pane")]
static VOICE_APP: OnceLock<AppHandle> = OnceLock::new();

#[cfg(feature = "voice_pane")]
static VOICE_SPEAKER: crate::speaker::Speaker = crate::speaker::Speaker::new(&crate::speaker::Cpal);

#[cfg(feature = "voice_pane")]
pub fn voice_window_closed() {
    crate::voice::window_closed(&crate::voice::STUDIO, &VOICE_SPEAKER);
}

#[cfg(feature = "voice_pane")]
fn voice_emit(e: &Value) {
    if let Some(w) = VOICE_APP.get().and_then(|a| a.get_webview_window("settings")) {
        let _ = w.eval(&format!("window.__voiceEvent && window.__voiceEvent({e})"));
    }
}

#[cfg(feature = "voice_pane")]
static REPLIES: crate::voice::Replies = crate::voice::Replies::new();

#[cfg(feature = "voice_pane")]
fn reply_log(lines: impl IntoIterator<Item = String>) {
    if let Some(c) = CTX.get() {
        lines.into_iter().chain(REPLIES.take_timing()).for_each(|l| c.log.line(&l));
    }
}

#[cfg(feature = "voice_pane")]
pub fn conversation_voice_ready() -> bool {
    CTX.get().is_some_and(|c| voice(&c.data).reply_voice(true).is_some())
}
#[cfg(feature = "voice_pane")]
pub fn reply_silence() { reply_log(REPLIES.dictation(&VOICE_SPEAKER)); }
#[cfg(feature = "voice_pane")]
pub fn conversation_output_reset() { VOICE_SPEAKER.reset_output(); }

#[cfg(feature = "voice_pane")]
pub fn conversation_log(line: String) { reply_log([line]); }
#[cfg(feature = "voice_pane")]
pub fn listening_level() -> crate::conversation_overlap::Level { REPLIES.listening_level(pt()) }
#[cfg(feature = "voice_pane")]
pub(crate) fn listening_emit(e: &Value) {
    crate::conversation::listening_clip_event(e);
    voice_emit(e);
}
#[cfg(feature = "voice_pane")]
pub fn listening_sound(clip: usize, pause_ms: u32, epoch: u64, turn: u64) {
    let Some(c) = CTX.get() else { return };
    let due = std::time::Instant::now();
    let _ = std::thread::Builder::new().name("listening-sound".into()).spawn(move || {
        let pt = pt();
        let played=REPLIES.play_listening(pt, clip, &voice(&c.data), || due.elapsed().as_millis() <= 150 && crate::conversation::backchannel_current(epoch, turn));
        conversation_log(format!("listening_sound: clip_index={clip}, played={played}, pause_ms={pause_ms}, dispatch_ms={}",due.elapsed().as_millis()));
        if played {
            let text = if pt { crate::conversation_overlap::PT[clip] } else { crate::conversation_overlap::EN[clip] };
            conversation_log(format!("backchannel: clip={text:?}, pause_ms={pause_ms}"));
        }
    });
}

#[cfg(feature = "voice_pane")]
pub fn reply_turn(dictated: bool) -> Option<Value> {

    crate::front_native::new_turn(|| {
    let mut field = CTX.get().and_then(|c| voice(&c.data).reply_voice(dictated));
    if crate::conversation::speaks_full() && field.is_some() { field = Some(json!({"dictated": dictated, "spoken": "full"})); }
    if field.is_none() {

        reply_warm();
    }
    reply_log(REPLIES.turn(dictated, field.is_some(), &VOICE_SPEAKER));
    field
    })
}

#[cfg(feature = "voice_pane")]
pub fn reply_warm() {
    if let Some(c) = CTX.get() {
        let _ = std::thread::Builder::new().name("voice-warm".into()).spawn(move || {
            let v = voice(&c.data); v.warm(); REPLIES.prepare_listening(&v);
        });
    }
}

#[cfg(feature = "voice_pane")]
pub fn reply_dictation() {
    reply_log(REPLIES.dictation(&VOICE_SPEAKER));

    if let Some(c) = CTX.get() {
        let _ = std::thread::Builder::new().name("voice-warm".into()).spawn(move || {
            let v = voice(&c.data);
            v.warm();
            reply_log([REPLIES.warm_up(&v, pt())]);
        });
    }
}

#[cfg(feature = "voice_pane")]
pub fn reply_unsent() {
    reply_log(REPLIES.unsent());
    VOICE_SPEAKER.stop();
}

#[cfg(feature = "voice_pane")]
pub fn reply_frame(f: &Value) {
    let Some(c) = CTX.get() else { return };
    REPLIES.frame(f, pt(), move || voice(&c.data), reply_emit, |l| reply_log([l]));
    crate::conversation::reply_frame(f);
}

#[cfg(feature = "voice_pane")]
pub fn front_speak(request:crate::voice_front::runtime::SpeechRequest)->crate::voice_front::Result<()> {
    static TURNS:Mutex<()>=Mutex::new(());
    let c=CTX.get().ok_or(crate::voice_front::Error::Closed)?;
    let epoch=request.ticket.epoch();let ticket=request.ticket.clone();
    voice(&c.data).speak_front(request,epoch,&TURNS,&||current().voice_front_templates && !ticket.closed(),crate::front_native::speech_event)
        .map(|_|()).map_err(|_|crate::voice_front::Error::Closed)
}
#[cfg(feature = "voice_pane")]
pub fn front_stop(ticket:&crate::voice_front::runtime::Ticket) {VOICE_SPEAKER.stop_front(ticket);}
#[cfg(feature = "voice_pane")]
pub fn front_terminal(epoch:crate::voice_front::runtime::Epoch) {
    if crate::conversation::front_epoch()==Some(epoch) {reply_log(REPLIES.front_terminal());}
}

#[cfg(feature = "voice_pane")]
fn reply_emit(e: &Value) {
    crate::conversation::rendered_interrupt(e);
    let decision = REPLIES.event(e);
    if matches!(e["state"].as_str(), Some("done" | "failed")) && crate::conversation::active() { reply_warm(); }
    reply_log(decision);
    voice_emit(e);
}

#[cfg(feature = "voice_pane")]
struct AppMic;
#[cfg(feature = "voice_pane")]
impl crate::voice::Mic for AppMic {
    fn state(&self) -> crate::voice::MicState {
        use crate::dictation::{Mic, Permission};
        match crate::capture::Cpal.permission() {
            Permission::Granted => crate::voice::MicState::Granted,
            Permission::NotAsked => crate::voice::MicState::NotAsked,
            _ => crate::voice::MicState::Denied,
        }
    }
    fn ask(&self) {
        crate::dictation::Mic::ask(&crate::capture::Cpal, Box::new(|_| {}));
    }
    fn open(&self, sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn std::any::Any + Send>, u32), String> {
        let open = crate::dictation::Mic::opener(&crate::capture::Cpal).ok_or("no microphone opener")?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("voice-mic".into())
            .spawn(move || {
                let _ = tx.send(open(sink));
            })
            .map_err(|e| e.to_string())?;
        match rx.recv_timeout(crate::dictation::OPEN_WITHIN) {
            Ok(Ok((stream, rate, _device))) => Ok((stream, rate)),
            Ok(Err(e)) => Err(format!("{e:?}")),
            Err(_) => Err("the microphone didn't start in time".into()),
        }
    }
}

#[cfg(feature = "voice_pane")]
const LF_SERVICE: &str = "ai.weunfold.unfold.localfleet";
#[cfg(feature = "voice_pane")]
const LF_ACCOUNT: &str = "unfold-app";
#[cfg(all(feature = "voice_pane", not(feature = "rehearsal_pack")))]
const ERR_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(all(feature = "voice_pane", not(feature = "rehearsal_pack")))]
pub(crate) const VOICE_KEYS: &dyn crate::voice::Keys = &AppKeys;
#[cfg(all(feature = "voice_pane", feature = "rehearsal_pack"))]
pub(crate) const VOICE_KEYS: &dyn crate::voice::Keys = &crate::scratch_keys::Item { service: LF_SERVICE, account: LF_ACCOUNT };

#[cfg(all(feature = "voice_pane", feature = "rehearsal_pack"))]
impl crate::voice::Keys for crate::scratch_keys::Item<'static> {
    fn read(&self) -> Result<Option<String>, String> {
        crate::scratch_keys::Item::read(self)
    }
    fn present(&self) -> bool {
        crate::scratch_keys::Item::present(self).unwrap_or(false)
    }
    fn write(&self, key: &str) -> Result<(), String> {
        crate::scratch_keys::Item::write(self, key)
    }
    fn clear(&self) -> Result<(), String> {
        crate::scratch_keys::Item::clear(self).map(|_| ())
    }
}

#[cfg(all(feature = "voice_pane", not(feature = "rehearsal_pack")))]
struct AppKeys;
#[cfg(all(feature = "voice_pane", not(feature = "rehearsal_pack")))]
impl crate::voice::Keys for AppKeys {
    fn read(&self) -> Result<Option<String>, String> {
        crate::keychain_call()?;
        match security_framework::os::macos::passwords::find_generic_password(None, LF_SERVICE, LF_ACCOUNT) {
            Ok((pw, _)) => String::from_utf8(pw.to_vec()).map(Some).map_err(|_| "the stored key is not UTF-8".to_string()),
            Err(e) if e.code() == ERR_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(format!("Keychain error {}", e.code())),
        }
    }
    fn present(&self) -> bool {
        lf_items().is_ok_and(|i| !i.is_empty())
    }

    fn write(&self, key: &str) -> Result<(), String> {
        crate::keychain_call()?;
        self.clear()?;
        let kc = security_framework::os::macos::keychain::SecKeychain::default().map_err(|e| format!("Keychain error {}", e.code()))?;
        kc.add_generic_password(LF_SERVICE, LF_ACCOUNT, key.as_bytes()).map_err(|e| format!("Keychain error {}", e.code()))
    }
    fn clear(&self) -> Result<(), String> {
        for item in lf_items()? {
            item.delete();
        }
        if lf_items()?.is_empty() { Ok(()) } else { Err("the old item could not be deleted".into()) }
    }
}

#[cfg(all(feature = "voice_pane", not(feature = "rehearsal_pack")))]
fn lf_items() -> Result<Vec<security_framework::os::macos::keychain_item::SecKeychainItem>, String> {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit, Reference, SearchResult};
    crate::keychain_call()?;
    let mut q = ItemSearchOptions::new();
    q.class(ItemClass::generic_password()).service(LF_SERVICE).account(LF_ACCOUNT).load_refs(true).limit(Limit::All);
    match q.search() {
        Ok(found) => Ok(found.into_iter().filter_map(|r| match r {
            SearchResult::Ref(Reference::KeychainItem(i)) => Some(i),
            _ => None,
        }).collect()),
        Err(e) if e.code() == ERR_ITEM_NOT_FOUND => Ok(Vec::new()),
        Err(e) => Err(format!("Keychain error {}", e.code())),
    }
}

#[cfg(feature = "voice_pane")]
const VOICE_FILE_LATER: Said = ("Choosing a file comes in a later version.", "Escolher um arquivo chega numa próxima versão.");
#[cfg(feature = "voice_pane")]
const VOICE_MIC_ASK: Said = ("Allow the microphone, then press Record again.", "Permita o microfone e clique em Gravar de novo.");
#[cfg(feature = "voice_pane")]
const VOICE_MIC_DENIED: Said = ("Unfold can’t use the microphone. Allow it in System Settings › Privacy & Security › Microphone.", "Unfold não pode usar o microfone. Permita em Ajustes do Sistema › Privacidade e Segurança › Microfone.");

#[cfg(feature = "voice_pane")]
const VOICE_DICTATING: Said = ("Dictation is already listening.", "O ditado já está ouvindo.");
#[cfg(feature = "voice_pane")]
const VOICE_MIC_FAILED: Said = ("The microphone didn’t start. Try again in a moment.", "O microfone não ligou. Tente de novo daqui a pouco.");
#[cfg(feature = "voice_pane")]
const VOICE_NOT_A_VOICE: Said = ("That folder isn’t a voice.", "Essa pasta não é uma voz.");
#[cfg(feature = "voice_pane")]
const VOICE_TOO_BIG: Said = ("That folder is too big to import. A voice can have up to 32 files and 20 MB in total.", "Essa pasta é grande demais para importar. Uma voz pode ter até 32 arquivos e 20 MB no total.");
#[cfg(feature = "voice_pane")]
const VOICE_EXISTS: Said = ("That voice is already in your voices.", "Essa voz já está nas suas vozes.");
#[cfg(feature = "voice_pane")]
const VOICE_NO_DEVICE: Said = ("Unfold couldn’t reach a device that can speak.", "Unfold não conseguiu alcançar um dispositivo que possa falar.");
#[cfg(feature = "voice_pane")]
const VOICE_KEY_BAD: Said = ("That doesn’t look like a LocalFleet key.", "Isso não parece uma chave do LocalFleet.");
#[cfg(feature = "voice_pane")]
const VOICE_KEY_NOT_SAVED: Said = ("Couldn’t save the key in the Keychain.", "Não deu para guardar a chave nas Chaves deste Mac.");
#[cfg(feature = "voice_pane")]
const TRANSCRIPT_EMPTY: Said = ("transcript_empty", "transcript_empty");

#[cfg(feature = "voice_pane")]
fn voice_reply(action: &str, r: Result<Value, crate::voice::Refusal>, view: impl FnOnce() -> Value) -> Result<Value, Failed> {
    use crate::voice::Refusal;
    match r {
        Ok(r) => {
            let mut v = view();
            if let (Some(o), Some(r)) = (v.as_object_mut(), r.as_object()) {
                o.extend(r.clone());
            }
            Ok(v)
        }
        Err(Refusal::Later(a)) => Err(Failed { said: VOICE_FILE_LATER, detail: format!("{a}: not in this build") }),
        Err(Refusal::NoDevice) => Err(Failed { said: VOICE_NO_DEVICE, detail: format!("{action}: no device can speak") }),
        Err(Refusal::TranscriptEmpty) => Err(Failed { said: TRANSCRIPT_EMPTY, detail: format!("{action}: the words are blank") }),
        Err(Refusal::MicAsk) => Err(Failed { said: VOICE_MIC_ASK, detail: format!("{action}: asked for the microphone") }),
        Err(Refusal::MicDenied) => Err(Failed { said: VOICE_MIC_DENIED, detail: format!("{action}: the microphone is denied") }),
        Err(Refusal::MicBusy) => Err(format!("{action}: a take is already recording").into()),
        Err(Refusal::MicFailed(d)) => Err(Failed { said: VOICE_MIC_FAILED, detail: format!("{action}: {d}") }),
        Err(Refusal::NotAVoice(d)) => Err(Failed { said: VOICE_NOT_A_VOICE, detail: format!("{action}: {d}") }),
        Err(Refusal::TooBig) => Err(Failed { said: VOICE_TOO_BIG, detail: format!("{action}: over 32 files or 20 MB") }),
        Err(Refusal::VoiceExists) => Err(Failed { said: VOICE_EXISTS, detail: format!("{action}: that id is in the catalogue") }),
        Err(Refusal::KeyBad) => Err(Failed { said: VOICE_KEY_BAD, detail: format!("{action}: not a key") }),
        Err(Refusal::KeyNotSaved(d)) => Err(Failed { said: VOICE_KEY_NOT_SAVED, detail: format!("{action}: {d}") }),
        Err(Refusal::Bad(d)) => Err(format!("{action}: {d}").into()),
    }
}

#[cfg(feature = "self_pane")]
const SELF_DOWN: Said = ("Unfold didn’t answer. Check again in a moment.", "Unfold não respondeu. Verifique de novo em instantes.");

#[cfg(feature = "self_pane")]
fn self_reply(action: &str, value: &Value, link: &crate::self_link::Registry, wait: std::time::Duration) -> Result<Value, Failed> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let fresh = || format!("app-get-{}", N.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1);
    let (kind, body) = crate::self_link::frame(action, value, &fresh)?;
    let down = |e: crate::self_link::Failed| Failed { said: SELF_DOWN, detail: format!("{action}: {e:?}") };
    let reply = link.request(kind, body, wait).map_err(down)?;
    if reply["kind"] == "rejected" {
        return Err(format!("{action}: agentd rejected it: {}", reply["reason"].as_str().unwrap_or("?")).into());
    }
    if kind == "self_get" {
        return Ok(json!({"self": reply["state"]}));
    }
    let state = link.request("self_get", json!({"request_id": fresh()}), wait).map(|f| f["state"].clone()).unwrap_or(Value::Null);
    Ok(json!({"self_result": reply["result"], "self": state}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("unfold-settings-{}-{t}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(not(feature = "self_pane"))]
    #[test]
    fn without_the_flag_the_self_actions_are_unknown_and_the_view_has_no_self() {
        for a in ["self_get", "self_set", "proposal_decide", "horizon_decide", "consent_set"] {
            assert!(self_action(a, Some(&json!({"request_id": "r1"}))).is_none(), "{a}");
        }
        assert!(view().get("self").is_none());
        assert!(with_self_pane(json!({})).get("self_pane").is_none(), "the page hides the Self pane");
    }

    #[cfg(not(feature = "voice_pane"))]
    #[test]
    fn without_the_voice_flag_the_voice_actions_are_unknown_and_the_view_has_no_voice() {
        let d = scratch();
        for a in ["voice_probe", "voice_choose", "say_set", "clone_create", "clone_delete", "voice_preview"] {
            assert!(voice_action(None, &d, a, Some(&json!({}))).is_none(), "{a}");
        }
        let v = with_voice_pane(&d, json!({}));
        assert!(v.get("voice_pane").is_none() && v.get("voice").is_none(), "the page hides the Voice pane");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(feature = "voice_pane")]
    #[test]
    fn with_the_voice_flag_the_view_offers_the_voice_pane_and_merges_the_reply() {
        use crate::voice::Refusal;
        let d = scratch();
        assert_eq!(with_voice_pane(&d, json!({"memory": {}}))["voice_pane"], json!(true));
        assert!(voice_action(None, &d, "self_get", None).is_none(), "not a voice action");
        let _ = std::fs::remove_dir_all(&d);
        let r = voice_reply("clone_create", Ok(json!({"voice": {"say_mode": "full"}, "voice_created": {"voice_id": "v1"}})), || json!({"about": {}, "voice_pane": true})).ok().unwrap();
        assert_eq!(r, json!({"about": {}, "voice_pane": true, "voice": {"say_mode": "full"}, "voice_created": {"voice_id": "v1"}}));
        let said = |r: Result<Value, Failed>| r.err().unwrap().said;
        for a in ["clone_file_pick", "clone_ref_file"] {
            let s = said(voice_reply(a, Err(Refusal::Later(a.into())), || json!({})));
            assert_eq!(s, ("Choosing a file comes in a later version.", "Escolher um arquivo chega numa próxima versão."), "{a}: only the file picks are still to come");
        }
        assert_eq!(said(voice_reply("clone_create", Err(Refusal::NoDevice), || json!({}))), VOICE_NO_DEVICE);
        assert_eq!(said(voice_reply("clone_ref_text", Err(Refusal::TranscriptEmpty), || json!({}))), TRANSCRIPT_EMPTY, "the page's own code");
        assert_eq!(said(voice_reply("say_set", Err(Refusal::Bad("x".into())), || json!({}))), FAILED);
        assert_eq!(said(voice_reply("voice_key_set", Err(Refusal::KeyBad), || json!({}))), VOICE_KEY_BAD);
        let f = voice_reply("voice_key_set", Err(Refusal::KeyNotSaved("Keychain error -25293".into())), || json!({})).err().unwrap();
        assert_eq!((f.said, f.detail.as_str()), (VOICE_KEY_NOT_SAVED, "voice_key_set: Keychain error -25293"), "the log gets the code, never a key");
        for (r, s) in [(Refusal::MicAsk, VOICE_MIC_ASK), (Refusal::MicDenied, VOICE_MIC_DENIED), (Refusal::MicFailed("x".into()), VOICE_MIC_FAILED), (Refusal::MicBusy, FAILED), (Refusal::NotAVoice("x".into()), VOICE_NOT_A_VOICE), (Refusal::TooBig, VOICE_TOO_BIG), (Refusal::VoiceExists, VOICE_EXISTS)] {
            assert_eq!(said(voice_reply("clone_record_start", Err(r), || json!({}))), s);
        }
        assert_eq!(VOICE_TOO_BIG, ("That folder is too big to import. A voice can have up to 32 files and 20 MB in total.", "Essa pasta é grande demais para importar. Uma voz pode ter até 32 arquivos e 20 MB no total."));
    }

    #[cfg(feature = "self_pane")]
    #[test]
    fn with_the_flag_the_view_offers_the_self_pane() {
        assert_eq!(with_self_pane(json!({"memory": {}}))["self_pane"], json!(true));
    }

    #[cfg(feature = "self_pane")]
    #[allow(clippy::type_complexity)]
    fn fake_agent(answer: fn(&Value) -> Option<Value>) -> (std::sync::Arc<crate::self_link::Registry>, std::thread::JoinHandle<Vec<Value>>, std::sync::Arc<Mutex<std::net::TcpStream>>) {
        use std::io::{BufRead, BufReader};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let app = std::sync::Arc::new(Mutex::new(std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap()));
        let (agent, _) = l.accept().unwrap();
        agent.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        let link = std::sync::Arc::new(crate::self_link::Registry::default());
        link.attach(app.clone());
        let l2 = link.clone();
        let t = std::thread::spawn(move || {
            let (mut seen, mut line, mut r) = (vec![], String::new(), BufReader::new(agent));
            while r.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
                let f: Value = serde_json::from_str(line.trim_end()).unwrap();
                line.clear();
                if let Some(reply) = answer(&f) {
                    l2.on_frame(&reply);
                }
                seen.push(f);
            }
            seen
        });
        (link, t, app)
    }

    #[cfg(feature = "self_pane")]
    #[test]
    fn a_write_returns_its_result_and_the_state_read_after_it() {
        let (link, agent, app) = fake_agent(|f| match f["kind"].as_str() {
            Some("self_get") => Some(json!({"kind": "self_state", "request_id": f["request_id"], "state": {"revision": "cd34"}})),
            Some("horizon_decide") => Some(json!({"kind": "self_result", "request_id": f["request_id"], "result": {"ok": {}}})),
            _ => None,
        });
        let wait = std::time::Duration::from_secs(2);
        let r = self_reply("horizon_decide", &json!({"request_id": "h1", "horizon_id": "h-1", "approve": false}), &link, wait).ok().unwrap();
        assert_eq!(r, json!({"self_result": {"ok": {}}, "self": {"revision": "cd34"}}));
        let g = self_reply("self_get", &json!({}), &link, wait).ok().unwrap();
        assert_eq!(g, json!({"self": {"revision": "cd34"}}));
        link.close(&app);
        drop(app);
        let seen = agent.join().unwrap();
        let kinds: Vec<_> = seen.iter().map(|f| (f["kind"].as_str().unwrap().to_string(), f["request_id"].as_str().unwrap().to_string())).collect();
        assert_eq!(kinds[0], ("horizon_decide".to_string(), "h1".to_string()));
        assert_eq!(kinds.len(), 3, "the write, its re-read, the plain get: {kinds:?}");
        assert!(kinds[1].0 == "self_get" && kinds[2].0 == "self_get" && kinds[1].1 != kinds[2].1, "each get gets its own id");
    }

    #[cfg(feature = "self_pane")]
    #[test]
    fn a_refusal_comes_back_as_the_result_and_a_silent_agent_as_the_down_line() {
        let (link, _agent, _app) = fake_agent(|f| match f["kind"].as_str() {
            Some("self_set") => Some(json!({"kind": "self_result", "request_id": f["request_id"], "result": {"refused": {"field": "spec", "code": "stale", "reason": "r"}}})),
            _ => None,
        });
        let wait = std::time::Duration::from_millis(300);
        let r = self_reply("self_set", &json!({"request_id": "s1", "base_revision": "ab", "spec": {}}), &link, wait).ok().unwrap();
        assert_eq!((r["self_result"]["refused"]["code"].as_str(), r["self"].is_null()), (Some("stale"), true));
        let e = self_reply("proposal_decide", &json!({"request_id": "p1", "proposal_id": "x", "approve": true}), &link, wait).err().unwrap();
        assert_eq!(e.said, SELF_DOWN);
        let bad = self_reply("proposal_decide", &json!({"request_id": "p1", "approve": true}), &link, wait).err().unwrap();
        assert_eq!(bad.said, FAILED, "refused before a frame: {}", bad.detail);
    }

    #[cfg(feature = "self_pane")]
    #[test]
    fn an_agent_without_a_saved_self_rejects_and_the_page_hears_it_at_once() {
        let (link, _agent, _app) = fake_agent(|f| Some(json!({"kind": "rejected", "reason": "this agent has no instance session, so no saved Self", "cause": f["id"]})));
        let t0 = std::time::Instant::now();
        let e = self_reply("self_get", &json!({}), &link, std::time::Duration::from_secs(5)).err().unwrap();
        assert_eq!(e.said, FAILED);
        assert!(e.detail.contains("no instance session"), "{}", e.detail);
        assert!(t0.elapsed() < std::time::Duration::from_secs(2));
    }

    #[cfg(feature = "self_pane")]
    #[test]
    fn the_self_down_line_keeps_the_portuguese_rule() {
        assert!(!SELF_DOWN.1.contains("o Unfold") && !SELF_DOWN.1.contains("de Unfold"));
    }

    #[test]
    fn the_dictation_language_is_auto_until_picked_and_the_pick_is_kept() {
        let d = scratch();
        let mut s = load(&d);
        assert_eq!(s.dictation_language, "auto");
        apply(&mut s, "dictation_language", &json!("pt")).unwrap();
        save(&d, &s).unwrap();
        assert_eq!(load(&d).dictation_language, "pt");
        assert!(apply(&mut s, "dictation_language", &json!("pt-PT")).is_err());
        assert_eq!(from_json(&json!({"dictation_language": "fr"})).dictation_language, "auto");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn portuguese_is_any_pt_tag() {
        for tag in ["pt", "pt-BR", "PT-br", "pt_PT"] {
            assert!(is_pt(Some(tag)), "{tag}");
        }
        for tag in [None, Some(""), Some("en-US"), Some("p"), Some("ptx"), Some("es")] {
            assert!(!is_pt(tag), "{tag:?}");
        }
    }

    #[test]
    fn the_sleep_lines_say_the_night_proposes_drive_changes_for_approval() {

        let page = include_str!("../../ui/settings.html");
        for l in ["sleep_on: (m) =>", "sleep_ask: (m) =>"] {
            let lines: Vec<&str> = page.lines().filter(|x| x.contains(l)).collect();
            assert_eq!(lines.len(), 2, "{l}: an EN and a PT line");
            assert!(lines[0].contains("reads the day’s chat in the cloud and proposes changes to Unfold’s drives for you to approve"), "{l} EN");
            assert!(lines[1].contains("lê a conversa do dia na nuvem e propõe mudanças nas vontades de Unfold para você aprovar"), "{l} PT");
        }
        for gone in ["note in the chat", "recado no chat", "day’s text", "texto do dia"] {
            assert!(!page.contains(gone), "{gone}");
        }
    }

    #[test]
    fn the_portuguese_settings_say_chaves_for_the_keychain() {

        let page = include_str!("../../ui/settings.html");

        let pt = &page[page.find("\n    pt: {").expect("the pt table")..];
        let pt = &pt[..pt.find("\n    },\n  }\n").expect("its end")];
        let self_pt = &page[page.find("Object.assign(W.pt, {").expect("the Self PT words")..];
        let self_pt = &self_pt[..self_pt.find("\n  })").expect("their end")];
        let hits: Vec<&str> = pt.lines().chain(self_pt.lines()).filter(|l| l.contains("Keychain") || l.to_lowercase().contains("porta-chaves")).collect();
        assert!(hits.is_empty(), "PT lines naming the Keychain: {hits:?}");
        assert!(pt.contains(" chaves keychain "), "a person searching in PT finds it as chaves");
    }

    #[test]
    fn the_portuguese_words_never_gender_unfold() {

        let page = include_str!("../../ui/settings.html");
        let pt = &page[page.find("\n    pt: {").expect("the pt table")..page.find("\n  let L = W.en").expect("its end")];
        let words: Vec<&str> = pt.split(|c: char| !c.is_alphanumeric()).filter(|w| ["ele", "ela", "dele", "dela", "eles", "elas"].contains(&w.to_lowercase().as_str())).collect();
        assert!(words.is_empty(), "gendered pronouns in the PT table: {words:?}");

        let mut bad = Vec::new();
        for clause in pt.split(|c: char| ".;:!?'\"`\n".contains(c)) {
            let w: Vec<&str> = clause.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
            for k in (0..w.len()).filter(|&k| w[k] == "Unfold") {
                if k > 0 && ["o", "a", "os", "as", "do", "da", "no", "na", "ao", "à", "pelo", "pela", "num", "numa", "dum", "duma", "um", "uma"].contains(&w[k - 1].to_lowercase().as_str()) {
                    bad.push(format!("{} Unfold", w[k - 1]));
                }
                if let Some(g) = w[k + 1..].iter().take(6).find(|x| ["atualizado", "pronto", "parado", "configurado", "sozinho", "instalado"].contains(x)) {
                    bad.push(format!("Unfold … {g}"));
                }
            }
        }
        assert!(bad.is_empty(), "an article before Unfold, or a word that genders it, in the PT table: {bad:?}");
    }

    #[test]
    fn the_rust_portuguese_never_puts_an_article_before_unfold() {

        assert_eq!(MODELS_LATER.1, "Dá para trocar os modelos quando Unfold estiver na tela inicial.");
    }

    #[test]
    fn the_settings_page_says_unfold_automagik_and_none_of_the_old_rows() {

        let page = include_str!("../../ui/settings.html");
        for gone in ["Use my DSH", "Usar meu DSH", "Use DeepSeek (cloud)", "Usar o DeepSeek (nuvem)", "your DSH", "seu DSH", "Watch my DSH agents", "Unfold’s own DSH", "DSH próprio de Unfold", "use_cloud", "cloud_ack", "'use_dsh'", "DeepSeek (via Automagik)"] {
            assert!(!page.contains(gone), "{gone}");
        }
        assert!(!page.to_lowercase().contains("sandbox"), "never sandbox");
        for said in ["title: 'Unfold Automagik'", "answer: 'Answer with'", "a_deepseek: 'DeepSeek (direct)'", "a_deepseek: 'DeepSeek (direto)'", "a_granite: 'Granite (this Mac)'", "watch: 'Watch its agents & suggest next'", "off: 'Off: Granite answers on this Mac, nothing leaves it.'", "off_direct: 'Off: DeepSeek answers directly, with Unfold’s key. Other models and the agents need Unfold Automagik.'", "p_key_v: 'Shared with Unfold Automagik'", "p_key_v: 'Compartilhada com Unfold Automagik'"] {
            assert!(page.contains(said), "{said}");
        }

        let en = &page[page.find("\n    en: {").expect("the en table")..page.find("\n    pt: {").expect("the pt table")];
        let own_en = &page[page.find("  const OWN_WORDS = {").expect("its words")..page.find("\n    pt: {\n      title: 'Unfold Automagik'").expect("its PT")];

        let strings = |t: &str| {
            let (mut out, mut cur, mut open) = (Vec::new(), String::new(), None::<char>);

            let code: String = t.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            for c in code.chars() {
                match open {
                    None if c == '\'' || c == '`' => open = Some(c),
                    None => {}
                    Some(q) if c == q => {
                        out.push(std::mem::take(&mut cur));
                        open = None;
                    }
                    Some(_) => cur.push(c),
                }
            }
            out
        };
        assert!(strings(en).iter().any(|q| q.contains("click Send")), "the strings are read");
        for t in [en, own_en] {
            let said = strings(t);
            let taps: Vec<&String> = said.iter().filter(|q| q.split(|c: char| !c.is_alphanumeric()).any(|w| w.eq_ignore_ascii_case("tap") || w.eq_ignore_ascii_case("taps"))).collect();
            assert!(taps.is_empty(), "EN says click: {taps:?}");
        }
    }

    #[test]
    fn the_unfold_automagik_portuguese_never_genders_unfold() {

        let page = include_str!("../../ui/settings.html");
        let at = page.find("\n    pt: {\n      title: 'Unfold Automagik'").expect("its PT");
        let pt = &page[at..at + page[at..].find("\n    },\n  }\n").expect("their end")];
        assert!(pt.contains("chaves") && !pt.contains("Keychain"), "PT says Chaves");
        let mut bad = Vec::new();
        for clause in pt.split(|c: char| ".;:!?'\"`\n".contains(c)) {
            let w: Vec<&str> = clause.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
            bad.extend(w.iter().filter(|x| ["ele", "ela", "dele", "dela", "eles", "elas", "desligado", "ligado", "parado", "pronto", "instalado", "atualizado", "sozinho"].contains(&x.to_lowercase().as_str())).map(|x| x.to_string()));
            for k in (0..w.len()).filter(|&k| w[k] == "Unfold") {
                if k > 0 && ["o", "a", "os", "as", "do", "da", "no", "na", "ao", "à", "pelo", "pela", "num", "numa", "dum", "duma", "um", "uma"].contains(&w[k - 1].to_lowercase().as_str()) {
                    bad.push(format!("{} Unfold", w[k - 1]));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:?}");
    }

    #[test]
    fn the_settings_page_makes_no_local_first_promise() {

        let page = include_str!("../../ui/settings.html").to_lowercase();
        for p in ["local-first", "local first", "the little that ever leaves", "nothing ever leaves", "never leaves this mac", "o pouco que sai", "nada sai do seu mac", "nunca sai do seu mac", "everything stays on this mac"] {
            assert!(!page.contains(p), "{p}");
        }
    }

    #[test]
    fn answer_with_reads_writes_and_survives_a_bad_value() {
        let mut s = Settings::default();
        assert_eq!(s.answer_with, ANSWER_DEEPSEEK, "DeepSeek through Unfold Automagik by default");
        for a in [ANSWER_GRANITE, "kimi/kimi-k2", ANSWER_DEEPSEEK] {
            apply(&mut s, "answer_with", &json!(a)).unwrap();
            assert_eq!((s.answer_with.as_str(), from_json(&to_json(&s)).answer_with.as_str()), (a, a), "saved and read back");
        }
        for a in ["deepseek/deepseek-v4-flash", "deepseek-official/deepseek-flash"] {
            apply(&mut s, "answer_with", &json!(a)).unwrap();
            assert_eq!(s.answer_with, ANSWER_DEEPSEEK, "{a}: DeepSeek is offered once, direct");
            assert_eq!(from_json(&json!({"answer_with": a})).answer_with, ANSWER_DEEPSEEK, "{a}: a stored bridge DeepSeek id reads as direct");
        }
        for bad in [json!("cloud"), json!("no-slash"), json!("has space/m"), json!(true), json!(null), json!(format!("x/{}", "y".repeat(200)))] {
            assert!(apply(&mut s, "answer_with", &bad).is_err(), "{bad}");
        }
        assert_eq!(s.answer_with, ANSWER_DEEPSEEK, "a refused value changes nothing");
        assert_eq!(from_json(&json!({"answer_with": "neon"})).answer_with, ANSWER_DEEPSEEK, "a bad stored value reads as the default");
        for gone in ["use_cloud", "use_dsh"] {
            assert!(apply(&mut s, gone, &json!(true)).is_err(), "{gone}: no longer a setting");
            assert!(to_json(&s).get(gone).is_none(), "{gone}: no longer written");
        }
        assert!(s.answers_direct() && !s.answers_via_bridge(), "DeepSeek: Unfold calls it directly");
        apply(&mut s, "answer_with", &json!("kimi/kimi-k2")).unwrap();
        assert!(!s.answers_direct() && s.answers_via_bridge());
        apply(&mut s, "answer_with", &json!(ANSWER_GRANITE)).unwrap();
        assert!(!s.answers_direct() && !s.answers_via_bridge());
    }

    #[test]
    fn picking_deepseek_beside_its_privacy_line_is_the_acknowledgment() {
        let mut s = Settings::default();
        assert!(!s.cloud_ack, "DeepSeek by default is not an acknowledgment");
        for a in [ANSWER_GRANITE, "kimi/kimi-k2"] {
            apply(&mut s, "answer_with", &json!(a)).unwrap();
            assert!(!s.cloud_ack, "{a}: not DeepSeek's acknowledgment");
        }
        apply(&mut s, "answer_with", &json!(ANSWER_DEEPSEEK)).unwrap();
        assert!(s.cloud_ack, "picked beside its privacy line");
        apply(&mut s, "answer_with", &json!(ANSWER_GRANITE)).unwrap();
        assert!(s.cloud_ack, "picking another keeps the acknowledgment");
        assert_eq!(from_json(&to_json(&s)), s, "round trip");
        let mut s = Settings::default();
        assert!(apply(&mut s, "cloud_ack", &json!(false)).is_err());
        apply(&mut s, "cloud_ack", &json!(true)).unwrap();
        assert!(s.cloud_ack);
        assert!(!from_json(&json!({"cloud_ack": "yes"})).cloud_ack, "a bad stored value reads as not acknowledged");
    }

    #[test]
    fn settings_from_before_0_5_read_as_the_answer_they_chose() {

        let table = [
            (json!({}), ANSWER_DEEPSEEK),
            (json!({"use_cloud": true}), ANSWER_DEEPSEEK),
            (json!({"use_cloud": true, "cloud_ack": true, "use_dsh": false}), ANSWER_DEEPSEEK),
            (json!({"use_cloud": false}), ANSWER_GRANITE),
            (json!({"use_cloud": false, "use_dsh": false}), ANSWER_GRANITE),
            (json!({"use_cloud": false, "use_dsh": true}), ANSWER_GRANITE),
            (json!({"use_dsh": true}), ANSWER_DEEPSEEK),
            (json!({"use_cloud": true, "cloud_ack": false}), ANSWER_DEEPSEEK),
            (json!({"use_cloud": true, "use_dsh": true}), ANSWER_DEEPSEEK),
            (json!({"use_cloud": "no"}), ANSWER_DEEPSEEK),
            (json!({"use_cloud": false, "use_dsh": "yes"}), ANSWER_GRANITE),

            (json!({"use_cloud": true, "use_dsh": true, "answer_with": "granite"}), ANSWER_GRANITE),
            (json!({"use_cloud": false, "answer_with": "kimi/kimi-k2"}), "kimi/kimi-k2"),

            (json!({"use_cloud": false, "answer_with": 7}), ANSWER_GRANITE),
        ];
        for (stored, want) in table {
            assert_eq!(from_json(&stored).answer_with, want, "{stored}");
        }

        let s = from_json(&json!({"use_cloud": true, "use_dsh": true}));
        assert!(!s.use_dsh_watch && !s.automagik_on());
        #[cfg(feature = "menu_bar")]
        assert!(from_json(&json!({"use_own_dsh": true, "use_dsh": true, "use_dsh_watch": true})).automagik_on());

        assert!(from_json(&json!({"use_cloud": true, "cloud_ack": true})).cloud_ack && !from_json(&json!({"use_cloud": true})).cloud_ack);

        let d = scratch();
        let _ = crate::WRITE_MODE.set(crate::WriteMode::Dev);
        std::fs::create_dir_all(d.join("app")).unwrap();
        std::fs::write(path(&d), r#"{"use_cloud": false, "cloud_ack": true, "use_dsh": true, "use_dsh_watch": true, "use_own_dsh": true}"#).unwrap();
        let s = load(&d);
        assert_eq!((s.answer_with.as_str(), s.use_dsh_watch, s.cloud_ack), (ANSWER_GRANITE, true, true));
        save(&d, &s).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path(&d)).unwrap()).unwrap();
        assert_eq!((v["answer_with"].as_str(), v.get("use_cloud"), v.get("use_dsh"), v["cloud_ack"].as_bool()), (Some(ANSWER_GRANITE), None, None, Some(true)), "{v}");
        assert_eq!(load(&d), s, "the same choice after the save");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_welcome_page_s_deepseek_switch_sets_answer_with() {
        assert_eq!(answer_after_welcome(ANSWER_GRANITE, true), ANSWER_DEEPSEEK);
        assert_eq!(answer_after_welcome(ANSWER_DEEPSEEK, true), ANSWER_DEEPSEEK);
        assert_eq!(answer_after_welcome("kimi/kimi-k2", true), "kimi/kimi-k2", "a picked model stays");
        assert_eq!(answer_after_welcome("kimi/kimi-k2", false), ANSWER_GRANITE);
        assert_eq!(answer_after_welcome(ANSWER_DEEPSEEK, false), ANSWER_GRANITE);
    }

    #[test]
    fn the_watch_switch_is_off_by_default_and_independent_of_answer_with() {
        let mut s = Settings::default();
        assert!(!s.use_dsh_watch, "Watch its agents is off until the person turns it on");
        apply(&mut s, "use_dsh_watch", &json!(true)).unwrap();
        assert!(s.use_dsh_watch && from_json(&to_json(&s)) == s, "saved and read back");
        apply(&mut s, "answer_with", &json!(ANSWER_GRANITE)).unwrap();
        assert!(s.use_dsh_watch, "Granite answering leaves the watch on");
        assert!(apply(&mut s, "use_dsh_watch", &json!("on")).is_err());
        assert!(!from_json(&json!({"use_dsh_watch": "yes"})).use_dsh_watch, "a bad stored value reads as off");
    }

    #[test]
    fn defaults_are_auto_update_on_and_handoffs_off() {
        let s = Settings::default();
        assert!(s.auto_update && !s.suggest_handoffs && s.answer_with == ANSWER_DEEPSEEK && !s.automagik_on(), "DeepSeek is the default answer, Unfold Automagik off");
        assert_eq!((s.appearance.as_str(), s.language.as_str()), ("system", "system"));
        assert_eq!(from_json(&Value::Null), s, "no file reads as the defaults");
    }

    #[test]
    fn unknown_values_read_as_their_default_and_a_stored_handoff_stays_off() {
        let s = from_json(&json!({"auto_update": false, "appearance": "neon", "language": "pt-BR", "suggest_handoffs": true}));
        assert_eq!(s, Settings { auto_update: false, appearance: "system".into(), language: "pt-BR".into(), dictation_language: "auto".into(), suggest_handoffs: false, answer_with: ANSWER_DEEPSEEK.into(), cloud_ack: false, use_dsh_watch: false, #[cfg(feature = "menu_bar")] use_own_dsh: false, deepseek_effort: "auto".into(), deepseek_note: false, reply_max_tokens: None, smart_turn_endpoint:false, smart_turn_shadow:true, voice_front_templates:false });
    }

    #[test]
    fn the_toggle_round_trips_through_the_file() {
        let d = scratch();
        let _ = crate::WRITE_MODE.set(crate::WriteMode::Dev);
        assert_eq!(load(&d), Settings::default());
        let mut s = load(&d);
        apply(&mut s, "auto_update", &json!(false)).unwrap();
        apply(&mut s, "appearance", &json!("night")).unwrap();
        save(&d, &s).unwrap();
        assert_eq!(load(&d), s);
        assert!(!load(&d).auto_update, "off stays off across launches");
        apply(&mut s, "auto_update", &json!(true)).unwrap();
        save(&d, &s).unwrap();
        assert!(load(&d).auto_update);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn changes_are_validated_and_handoffs_are_locked() {
        let mut s = Settings::default();
        assert!(apply(&mut s, "auto_update", &json!("off")).is_err());
        assert!(apply(&mut s, "appearance", &json!("sepia")).is_err());
        assert_eq!(apply(&mut s, "suggest_handoffs", &json!(true)).unwrap_err(), HANDOFF_LOCK);
        assert!(apply(&mut s, "suggest_handoffs", &json!(false)).is_ok());
        assert!(apply(&mut s, "nope", &json!(1)).is_err());
        assert_eq!(s, Settings::default(), "a refused change changes nothing");
    }

    #[test]
    fn the_language_setting_overrides_the_macs() {
        let mut s = Settings::default();
        assert_eq!(language_tag(&s, Some("pt-BR".into())).as_deref(), Some("pt-BR"));
        s.language = "en".into();
        assert_eq!(language_tag(&s, Some("pt-BR".into())).as_deref(), Some("en"));
    }

    #[test]
    fn front_locale_follows_the_language_setting_and_native_preference_not_dictation() {
        use crate::voice_front::policy::{Locale, LocaleSource};
        let mut s = Settings::default();
        let from_os = front_locale_for(&s, Some("en-US".into())).unwrap();
        assert_eq!(from_os.locale, Locale::En);
        assert_eq!(from_os.source, LocaleSource::System);
        assert!(front_locale_for(&s, Some("pt-PT".into())).is_none());
        assert!(front_locale_for(&s, None).is_none());
        s.language = "pt-BR".into(); s.dictation_language = "en".into();
        let explicit = front_locale_for(&s, Some("en-US".into())).unwrap();
        assert_eq!(explicit.locale, Locale::PtBr);
        assert_eq!(explicit.source, LocaleSource::Setting);
    }

    #[test]
    fn the_theme_reaches_the_main_page_hook_or_its_storage() {
        let js = theme_js("night");
        assert!(js.contains(r#"window.__rthTheme("night")"#) && js.contains(r#"localStorage.setItem('unfold.theme',"night")"#));
        assert!(theme_js("</script>").contains(r#"("system")"#), "only the three values are ever evaluated");
    }

    #[test]
    fn the_memory_count_is_distinct_committed_ids_from_the_ledger() {
        let d = scratch();
        assert_eq!(memory_count(&d), None, "no ledger: unknown, not zero");
        std::fs::create_dir_all(d.join("agentd")).unwrap();
        let lines = [
            json!({"state": "accepted", "act": {}}),
            json!({"state": "committed", "outcome": {"memory_id": "rm-1", "revision": 1}}),
            json!({"state": "committed", "outcome": {"memory_id": "rm-1", "revision": 2}}),
            json!({"state": "committed", "outcome": {"memory_id": "rm-2", "revision": 1}}),
            json!({"state": "refused", "outcome": {"reason": "x"}}),
        ];
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect::<String>() + "torn {\n";
        std::fs::write(d.join("agentd/memory-acts.jsonl"), text).unwrap();
        assert_eq!(memory_count(&d), Some(2));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_thinking_level_is_auto_until_picked_and_only_its_five_levels_are_taken() {
        let mut s = Settings::default();
        assert_eq!(s.deepseek_effort, "auto", "Auto by default");
        for e in EFFORTS {
            apply(&mut s, "deepseek_effort", &json!(e)).unwrap();
            assert_eq!(from_json(&to_json(&s)).deepseek_effort, e, "saved and read back");
        }
        for bad in [json!("medium"), json!("xhigh"), json!("none"), json!("High"), json!(2), json!(null)] {
            assert!(apply(&mut s, "deepseek_effort", &bad).is_err(), "{bad}: not a level (no Medium)");
        }
        assert_eq!(s.deepseek_effort, "max", "a refused level changes nothing");
        assert_eq!(from_json(&json!({"deepseek_effort": "medium"})).deepseek_effort, "auto", "a bad stored level reads as Auto");
    }

    #[test]
    fn the_reply_length_limit_is_off_until_set_and_takes_only_what_agentd_takes() {
        let mut s = Settings::default();
        assert_eq!(s.reply_max_tokens, None, "off by default");
        assert!(to_json(&s).get("reply_max_tokens").is_none(), "off is the key absent, never null");
        for r in [1u32, 4096, 1_048_576] {
            apply(&mut s, "reply_max_tokens", &json!(r)).unwrap();
            assert_eq!(to_json(&s)["reply_max_tokens"], json!(r), "saved as a number");
            assert_eq!(from_json(&to_json(&s)).reply_max_tokens, Some(r), "read back");
        }
        for bad in [json!(0), json!(-1), json!(1_048_577), json!(4096.5), json!("4096"), json!(true), json!([4096])] {
            assert!(apply(&mut s, "reply_max_tokens", &bad).is_err(), "{bad}: agentd refuses a turn carrying it");
        }
        assert_eq!(s.reply_max_tokens, Some(1_048_576), "a refused value changes nothing");
        apply(&mut s, "reply_max_tokens", &json!(false)).unwrap();
        assert_eq!((s.reply_max_tokens, to_json(&s).get("reply_max_tokens").is_none()), (None, true), "the switch off (false) turns it off");
        apply(&mut s, "reply_max_tokens", &json!(64)).unwrap();
        apply(&mut s, "reply_max_tokens", &Value::Null).unwrap();
        assert_eq!(s.reply_max_tokens, None, "null too");
        for bad in [json!(0), json!(4096.5), json!("4096"), json!(null), json!(2_000_000)] {
            assert_eq!(from_json(&json!({"reply_max_tokens": bad})).reply_max_tokens, None, "{bad}: a bad stored limit reads as off");
        }
        assert_eq!(from_json(&json!({"reply_max_tokens": 800})).reply_max_tokens, Some(800));
    }

    #[test]
    fn a_stored_bridge_deepseek_pick_moves_to_direct_once_and_owes_one_note() {
        let dir = std::env::temp_dir().join(format!("ufd-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("app")).unwrap();
        let logs = dir.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let log = crate::Log::open_in(None, &logs);
        std::fs::write(path(&dir), r#"{"answer_with": "deepseek-official/deepseek-flash", "cloud_ack": false, "use_own_dsh": true}"#).unwrap();
        migrate(&dir, &log);
        let s = load(&dir);
        assert_eq!((s.answer_with.as_str(), s.deepseek_note, s.cloud_ack), (ANSWER_DEEPSEEK, true, false), "moved, the note owed, no acknowledgment made up");
        let raw: Value = serde_json::from_slice(&std::fs::read(path(&dir)).unwrap()).unwrap();
        assert_eq!(raw["answer_with"], "deepseek", "saved");
        let text = std::fs::read_to_string(logs.join("app.log")).unwrap_or_default();
        assert!(text.contains("deepseek-official/deepseek-flash") && text.contains("is now deepseek"), "logged: {text}");
        migrate(&dir, &log);
        assert_eq!(std::fs::read_to_string(logs.join("app.log")).unwrap_or_default().matches("is now deepseek").count(), 1, "once");
        std::fs::write(path(&dir), r#"{"answer_with": "kimi/kimi-k2"}"#).unwrap();
        migrate(&dir, &log);
        assert_eq!((load(&dir).answer_with.as_str(), load(&dir).deepseek_note), ("kimi/kimi-k2", false), "another bridge model stays");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod turn_mode_tests {
    use super::*;
    #[test]
    fn timer_and_shadow_defaults_validate_and_persist() {
        let mut s=Settings::default();
        assert!(!s.smart_turn_endpoint && s.smart_turn_shadow);
        assert_eq!(from_json(&json!({"smart_turn_endpoint":"yes","smart_turn_shadow":null})),s);
        for key in ["smart_turn_endpoint","smart_turn_shadow"] {
            for value in [true,false] {
                apply(&mut s,key,&json!(value)).unwrap();
                assert_eq!(from_json(&to_json(&s)),s);
            }
            assert!(apply(&mut s,key,&json!("yes")).is_err());
        }
    }
    #[test]
    fn native_front_flag_is_off_and_requires_an_explicit_boolean() {
        assert!(!Settings::default().voice_front_templates);
        for v in [json!({}), json!({"voice_front_templates":null}), json!({"voice_front_templates":"true"}), json!({"voice_front_templates":1})] {
            assert!(!from_json(&v).voice_front_templates);
        }
        let mut s = Settings::default();
        assert!(apply(&mut s, "voice_front_templates", &json!("true")).is_err());
        assert!(!s.voice_front_templates);
        apply(&mut s, "voice_front_templates", &json!(true)).unwrap();
        assert!(from_json(&to_json(&s)).voice_front_templates);
        apply(&mut s, "voice_front_templates", &json!(false)).unwrap();
        assert!(!from_json(&to_json(&s)).voice_front_templates);
    }

}
