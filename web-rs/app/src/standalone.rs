//! Standalone (desktop) mode in the app: the import status shared by the layout, the welcome page and the
//! settings, its poller, the "This PC" settings section (with the desktop shell's backup export/import) and the
//! desktop shell's update banner. On the box
//! deployment `GET /api/standalone/status` answers 404 once at startup and nothing here shows or polls.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use leptos::{prelude::*, task::spawn_local, wasm_bindgen::JsCast};
use storm_web_core::{
    net,
    route::app_href,
    standalone::{
        backup_outcome, error_message, extras_with, extras_without, parse_update_status, poll_ms,
        redirects_to_welcome, reloads_after, update_failed_message, BackupAction, BackupOutcome,
        Status, UpdateState, UpdateStatus,
    },
};

use crate::ui::use_interval;

/// `sessionStorage` key: the welcome redirect is decided once per session (tab).
const WELCOME_DECIDED_KEY: &str = "standalone_welcome_decided";

/// Import status shared by the pages. `known` turns true after the startup request; `status` stays `None` on the
/// box deployment (404), so nothing standalone is shown there.
#[derive(Clone, Copy)]
pub struct Standalone {
    pub status: RwSignal<Option<Status>>,
    pub known: RwSignal<bool>,
}

pub fn use_standalone() -> Standalone {
    expect_context::<Standalone>()
}

/// Reads the status once (the only standalone request made on the box) and, on the first load of the session,
/// sends the user to `/welcome` while the import is not finished. Called by the layout, inside the router.
pub fn provide_standalone() -> Standalone {
    let ctx = Standalone {
        status: RwSignal::new(None),
        known: RwSignal::new(false),
    };
    provide_context(ctx);
    let nav = leptos_router::hooks::use_navigate();
    spawn_local(async move {
        let status = net::fetch_standalone_status().await.ok().flatten();
        if let Some(s) = &status {
            if net::session_get(WELCOME_DECIDED_KEY).is_none() {
                // one decision per session: redirected now, or ready -> never redirected again
                net::session_set(WELCOME_DECIDED_KEY, "1");
                let here = net::pathname();
                if redirects_to_welcome(s.phase)
                    && storm_web_core::route::app_path(&here) != "/welcome"
                {
                    nav("/welcome", Default::default());
                }
            }
        }
        ctx.status.set(status);
        ctx.known.set(true);
    });
    ctx
}

/// Polls the status while the calling page is mounted: every second during the import, every 10 s otherwise.
/// Waits for the startup request; stops for good on the box (no status) and when the page is left.
pub fn use_status_poll(ctx: Standalone) {
    fn step(ctx: Standalone, alive: Arc<AtomicBool>) {
        if !alive.load(Ordering::Relaxed) {
            return;
        }
        if !ctx.known.get_untracked() {
            net::after_ms(300, move || step(ctx, alive));
            return;
        }
        if ctx.status.get_untracked().is_none() {
            return; // box deployment: nothing to poll
        }
        spawn_local(async move {
            let next = match net::fetch_standalone_status().await {
                Ok(Some(s)) => {
                    let ms = poll_ms(s.phase);
                    if alive.load(Ordering::Relaxed) {
                        ctx.status.set(Some(s));
                    }
                    Some(ms)
                }
                Ok(None) => {
                    ctx.status.set(None);
                    None
                }
                Err(_) => Some(10_000),
            };
            if let Some(ms) = next {
                net::after_ms(ms, move || step(ctx, alive));
            }
        });
    }
    let alive = Arc::new(AtomicBool::new(true));
    let flag = alive.clone();
    on_cleanup(move || flag.store(false, Ordering::Relaxed));
    step(ctx, alive);
}

/// `PUT /api/standalone/folders`; the new status, or the server's message (400 for a non-directory).
async fn put_folders(folders: Vec<String>) -> Result<Status, String> {
    let body = serde_json::json!({ "folders": folders }).to_string();
    match net::write("PUT", "/api/standalone/folders", Some(body), None).await {
        Ok((s, t)) if (200..300).contains(&s) => {
            serde_json::from_str::<Status>(&t).map_err(|e| e.to_string())
        }
        Ok((s, t)) => Err(error_message(s, &t)),
        Err(_) => Err("network error — server unreachable".into()),
    }
}

/// `POST /api/standalone/rescan` (202).
pub fn rescan(msg: RwSignal<Option<String>>) {
    spawn_local(async move {
        let m = match net::write("POST", "/api/standalone/rescan", None, None).await {
            Ok((s, _)) if (200..300).contains(&s) => "✓ rescan started".to_string(),
            Ok((s, t)) => format!("✗ {}", error_message(s, &t)),
            Err(_) => "✗ network error — server unreachable".to_string(),
        };
        msg.set(Some(m));
        net::after_ms(5000, move || msg.set(None));
    });
}

/// Replay folders: the scanned ones (detected, or found under an extra folder) read-only, and the extra folders
/// the user saved, with add (path typed in) and remove. Only the extras are sent back (`PUT` body); the server's
/// refusal is shown inline.
#[component]
pub fn FolderEditor() -> impl IntoView {
    let ctx = use_standalone();
    let path = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    // memos: the rows are rebuilt only when a list changes, not at every poll
    let scanned = Memo::new(move |_| {
        ctx.status
            .with(|s| s.as_ref().map(|s| s.folders.clone()).unwrap_or_default())
    });
    let extras = Memo::new(move |_| {
        ctx.status.with(|s| {
            s.as_ref()
                .map(|s| s.extra_folders.clone())
                .unwrap_or_default()
        })
    });
    let save = move |next: Vec<String>, clear_input: bool| {
        busy.set(true);
        spawn_local(async move {
            match put_folders(next).await {
                Ok(s) => {
                    ctx.status.set(Some(s));
                    error.set(None);
                    if clear_input {
                        path.set(String::new());
                    }
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };
    let add = move || {
        let next = ctx
            .status
            .with_untracked(|s| s.as_ref().map(|s| extras_with(s, &path.get_untracked())));
        match next {
            Some(Ok(next)) => save(next, true),
            Some(Err(e)) => error.set(Some(e)),
            None => {}
        }
    };
    let remove = move |target: &str| {
        if let Some(next) = ctx
            .status
            .with_untracked(|s| s.as_ref().map(|s| extras_without(s, target)))
        {
            save(next, false);
        }
    };
    view! {
        <div class="sa-folders">
            <div class="sa-sub">"Scanned folders — found automatically or under your extra folders"</div>
            {move || {
                let list = scanned.get();
                if list.is_empty() {
                    view! { <div class="sa-none">"No replay folder found yet."</div> }.into_any()
                } else {
                    list.into_iter()
                        .map(|f| view! { <div class="sa-folder"><span class="mono">{f}</span></div> })
                        .collect_view()
                        .into_any()
                }
            }}
            <div class="sa-sub" style="margin-top: 10px">"Your extra folders — scanned with everything under them"</div>
            {move || {
                let list = extras.get();
                if list.is_empty() {
                    view! { <div class="sa-none">"None added."</div> }.into_any()
                } else {
                    list.into_iter()
                        .map(|f| {
                            let target = f.clone();
                            view! {
                                <div class="sa-folder">
                                    <span class="mono">{f}</span>
                                    <button class="ds-btn small danger" disabled=move || busy.get()
                                        on:click=move |_| remove(&target)>"Remove"</button>
                                </div>
                            }
                        })
                        .collect_view()
                        .into_any()
                }
            }}
            <div class="sa-row">
                <input class="ds-input" style="flex: 1; min-width: 240px" placeholder="C:\\path\\to\\a\\replays\\folder"
                    prop:value=move || path.get() on:input=move |e| path.set(event_target_value(&e))
                    on:keydown=move |e| if e.key() == "Enter" { add() } />
                <button class="ds-btn primary" disabled=move || busy.get() on:click=move |_| add()>"Add folder"</button>
            </div>
            {move || error.get().map(|e| view! { <div class="sa-err">{format!("✗ {e}")}</div> })}
        </div>
    }
}

/// Styles shared by the welcome page and the "This PC" section.
pub const CSS: &str = r#"
.sa-folders{display:flex;flex-direction:column;gap:8px}
.sa-folder{display:flex;align-items:center;gap:10px;padding:8px 0;border-bottom:1px solid var(--line)}
.sa-folder .mono{flex:1;min-width:0;overflow-wrap:anywhere;font-size:12px}
.sa-row{display:flex;flex-wrap:wrap;gap:8px;align-items:center}
.sa-err{font:12px var(--mono);color:var(--loss-soft)}
.sa-ok{font:12px var(--mono);color:var(--win)}
.sa-none{font:12px var(--mono);color:var(--kicker)}
.sa-sub{font:600 11px var(--mono);color:var(--muted-2)}
.sa-kv{display:grid;grid-template-columns:max-content 1fr;gap:6px 16px;font-size:13px}
.sa-kv .mono{font-size:12px;overflow-wrap:anywhere}
.sa-privacy{margin:14px 0 0;font-weight:600}
.sa-update{display:flex;align-items:center;gap:12px;flex-wrap:wrap;padding:8px 16px;background:rgba(93,202,165,.1);border-bottom:1px solid rgba(93,202,165,.35);font-size:13px}
"#;

/// OBS browser-source URLs, built from the running origin (never a fixed port).
fn obs_urls() -> Vec<(&'static str, String)> {
    let o = net::origin();
    [
        ("Cast overlay (1920×1080, transparent)", "/cast/overlay"),
        ("Queue", "/queue"),
        ("Ticker", "/ticker"),
        ("Widget (add ?me=<name>)", "/widget"),
        ("Now playing", "/now-playing"),
    ]
    .into_iter()
    .map(|(label, path)| (label, format!("{o}{}", app_href(path))))
    .collect()
}

/// Settings section "This PC" (Admin page), shown only in standalone mode.
#[component]
pub fn ThisPc(num: Signal<u32>) -> impl IntoView {
    let ctx = use_standalone();
    use_status_poll(ctx);
    let msg = RwSignal::new(None::<String>);
    view! {
        <Show when=move || ctx.status.with(Option::is_some)>
            <style>{CSS}</style>
            <crate::pages::admin::Sec num=num title="This PC" note=ViewFn::from(|| "desktop app")>
                <div class="ds-panel">
                    <div class="ds-label">"Replay folders"</div>
                    <FolderEditor />
                    <div class="sa-row" style="margin-top: 14px">
                        <button class="ds-btn" on:click=move |_| rescan(msg)>"Rescan now"</button>
                        {move || msg.get().map(|m| {
                            let class = if m.starts_with('✓') { "sa-ok" } else { "sa-err" };
                            view! { <span class=class>{m}</span> }
                        })}
                    </div>
                    <Backup />
                    <div class="ds-label" style="margin-top: 18px">"About"</div>
                    <div class="sa-kv">
                        <span>"Data location"</span>
                        <span class="mono">{move || ctx.status.with(|s| s.as_ref().map(|s| s.data_dir.clone()).unwrap_or_default())}</span>
                        <span>"App version"</span>
                        <span class="mono">{move || ctx.status.with(|s| s.as_ref().map(|s| s.version.clone()).unwrap_or_default())}</span>
                    </div>
                    <p class="sa-privacy">"Nothing leaves this PC except the update check."</p>
                    <div class="ds-label" style="margin-top: 18px">"OBS browser sources"</div>
                    <div class="sa-kv">
                        {obs_urls().into_iter().map(|(label, url)| view! {
                            <span>{label}</span>
                            <span class="mono" style="user-select: all">{url}</span>
                        }).collect_view()}
                    </div>
                </div>
            </crate::pages::admin::Sec>
        </Show>
    }
}

/// `window.__TAURI__.core` and its `invoke`, read defensively (absent in a browser and on the box).
fn tauri_invoke() -> Option<(leptos::wasm_bindgen::JsValue, js_sys::Function)> {
    let tauri = js_sys::Reflect::get(&js_sys::global(), &"__TAURI__".into()).ok()?;
    if !tauri.is_object() {
        return None;
    }
    let core = js_sys::Reflect::get(&tauri, &"core".into()).ok()?;
    if !core.is_object() {
        return None;
    }
    let invoke = js_sys::Reflect::get(&core, &"invoke".into())
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    Some((core, invoke))
}

/// Readable reason of a rejected Tauri call: the string itself, an `Error`'s `message`, or the value as JSON.
fn rejection_reason(e: &leptos::wasm_bindgen::JsValue) -> String {
    if let Some(text) = e.as_string() {
        return text;
    }
    if e.is_object() {
        if let Some(m) = js_sys::Reflect::get(e, &"message".into())
            .ok()
            .and_then(|m| m.as_string())
        {
            return m;
        }
    }
    js_sys::JSON::stringify(e)
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or_default()
}

/// Calls a Tauri command. `Ok` with the answer as JSON (`Null` when it resolved with `undefined` or a value JSON
/// cannot hold), `Err` with the reason when the shell is missing, the call throws or the promise rejects. Never
/// panics whatever `window.__TAURI__` looks like.
async fn invoke(cmd: &str) -> Result<serde_json::Value, String> {
    let (core, f) = tauri_invoke().ok_or_else(|| "desktop shell unavailable".to_string())?;
    let ret = f
        .call1(&core, &cmd.into())
        .map_err(|e| rejection_reason(&e))?;
    let value = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&ret))
        .await
        .map_err(|e| rejection_reason(&e))?;
    Ok(js_sys::JSON::stringify(&value)
        .ok()
        .and_then(|t| t.as_string())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null))
}

/// "Backup" block of the "This PC" section: export and import through the desktop shell's `export_data` /
/// `import_data` commands, called with **no argument** (the shell opens the native dialogs and asks for the import
/// confirmation itself). Rendered only inside the shell (`window.__TAURI__`); one command at a time, both buttons
/// disabled while it runs; the outcome (or the shell's rejection text) is shown inline; a finished import reloads
/// the page, whose data it replaced.
#[component]
fn Backup() -> impl IntoView {
    let busy = RwSignal::new(None::<BackupAction>);
    let outcome = RwSignal::new(None::<BackupOutcome>);
    let run = move |action: BackupAction| {
        if busy.get_untracked().is_some() {
            return;
        }
        busy.set(Some(action));
        outcome.set(None);
        spawn_local(async move {
            let result = invoke(action.command()).await;
            let out = backup_outcome(action, result.as_ref().map(|_| ()).map_err(String::as_str));
            if reloads_after(action, &out) {
                net::after_ms(1500, net::reload);
            }
            outcome.set(Some(out));
            busy.set(None);
        });
    };
    let label = move |action: BackupAction, idle: &'static str| {
        move || {
            if busy.get() == Some(action) {
                action.busy_label()
            } else {
                idle
            }
        }
    };
    tauri_invoke().is_some().then(|| {
        view! {
            <div class="ds-label" style="margin-top: 18px">"Backup"</div>
            <div class="sa-row">
                <button class="ds-btn" disabled=move || busy.get().is_some()
                    on:click=move |_| run(BackupAction::Export)>{label(BackupAction::Export, "Export my data")}</button>
                <button class="ds-btn" disabled=move || busy.get().is_some()
                    on:click=move |_| run(BackupAction::Import)>{label(BackupAction::Import, "Import a backup")}</button>
                {move || outcome.get().map(|o| {
                    let class = match o {
                        BackupOutcome::Done(_) => "sa-ok",
                        BackupOutcome::Cancelled(_) => "sa-none",
                        BackupOutcome::Failed(_) => "sa-err",
                    };
                    view! { <span class=class>{o.text().to_string()}</span> }
                })}
            </div>
            <div class="sa-none" style="margin-top: 6px">
                "A backup holds your database and your replays. Importing one replaces all Storm Codex data on this PC."
            </div>
        }
    })
}

/// "Restart to update" banner of the desktop shell: polls `update_status` every 60 s when `window.__TAURI__`
/// exists; outside the shell it renders nothing and never polls. The button is disabled while `install_update`
/// runs (one call per click sequence); a failure is shown inline and the button comes back.
#[component]
pub fn UpdateBanner() -> impl IntoView {
    let update = RwSignal::new(UpdateStatus::default());
    let busy = RwSignal::new(false);
    let failure = RwSignal::new(None::<String>);
    if tauri_invoke().is_some() {
        let check = move || {
            spawn_local(async move {
                if let Ok(v) = invoke("update_status").await {
                    update.set(parse_update_status(&v));
                }
            })
        };
        check();
        use_interval(60_000, check);
    }
    let install = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        failure.set(None);
        spawn_local(async move {
            // on success the shell restarts the app: the button stays disabled meanwhile
            if let Err(reason) = invoke("install_update").await {
                failure.set(Some(update_failed_message(&reason)));
                busy.set(false);
            }
        })
    };
    view! {
        <Show when=move || update.with(|u| u.state == UpdateState::Ready)>
            <style>{CSS}</style>
            <div class="sa-update">
                <span>{move || match update.with(|u| u.version.clone()) {
                    Some(v) => format!("Storm Codex {v} is ready to install."),
                    None => "A new version of Storm Codex is ready to install.".to_string(),
                }}</span>
                <button class="ds-btn primary small" disabled=move || busy.get() on:click=install>
                    {move || if busy.get() { "Restarting…" } else { "Restart to update" }}
                </button>
                {move || failure.get().map(|m| view! { <span class="sa-err">{m}</span> })}
            </div>
        </Show>
    }
}
