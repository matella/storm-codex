//! Welcome / import progress of the desktop app (`/welcome`, standalone mode only): progress bar, counts,
//! rejected and failed replays, and the states that need the user (no folder, not enough disk space).

use leptos::prelude::*;
use leptos::task::spawn_local;
use storm_web_core::{
    net,
    standalone::{
        human_bytes, names_text, parse_names, phase_message, progress_percent, reprocess_message,
        Phase, Status,
    },
};

use crate::{
    standalone::{rescan, use_standalone, use_status_poll, FolderEditor, CSS},
    ui::{use_operator, Link},
};

const PAGE_CSS: &str = r#"
.welcome .wbar{height:12px;border-radius:999px;background:var(--panel-2);border:1px solid var(--line);overflow:hidden;margin:6px 0 4px}
.welcome .wbar i{display:block;height:100%;background:var(--accent);transition:width .4s ease}
.welcome .wpct{font:12px var(--mono);color:var(--muted-2)}
.welcome .wtiles{display:flex;flex-wrap:wrap;gap:10px;margin-top:16px}
.welcome .wtile{min-width:120px;padding:10px 14px;border-radius:12px;background:var(--panel-2);border:1px solid var(--line)}
.welcome .wtile b{display:block;font:900 30px/1 var(--display)}
.welcome .wtile span{font:600 10px var(--mono);letter-spacing:.12em;text-transform:uppercase;color:var(--muted-2)}
.welcome .wmsg{font-size:15px;line-height:1.5;margin:0 0 12px}
.welcome .wmsg.bad{color:var(--loss-soft)}
.welcome details{margin-top:14px} .welcome summary{cursor:pointer;font:600 12px var(--mono)}
.welcome ul{margin:8px 0 0;padding-left:18px;font:12px var(--mono);line-height:1.7;overflow-wrap:anywhere}
"#;

fn tile(n: impl Fn() -> String + Send + Sync + 'static, label: &'static str) -> impl IntoView {
    view! { <div class="wtile"><b>{n}</b><span>{label}</span></div> }
}

/// The page body. Only the values are reactive: the elements stay mounted across the polls, so an opened list
/// stays open and the progress bar animates.
#[component]
fn Body() -> impl IntoView {
    let status = use_standalone().status;
    // reads one value of the current status (the default before the first answer)
    fn read<T: Default>(status: RwSignal<Option<Status>>, f: impl Fn(&Status) -> T) -> T {
        status.with(|s| s.as_ref().map(f).unwrap_or_default())
    }
    let phase = Memo::new(move |_| status.with(|s| s.as_ref().map(|s| s.phase)));
    let pct = move || read(status, |s| progress_percent(s.done, s.total));
    let rejected = Memo::new(move |_| read(status, |s| s.rejected.clone()));
    let failed = Memo::new(move |_| read(status, |s| s.failed.clone()));
    let space = move || {
        read(status, |s| match (s.phase, s.free_bytes, s.needed_bytes) {
            (Phase::DiskFull, Some(free), Some(needed)) => Some(format!(
                "Free: {} · Needed: {}",
                human_bytes(free),
                human_bytes(needed)
            )),
            _ => None,
        })
    };
    let msg = RwSignal::new(None::<String>);
    view! {
        <div class="ds-panel">
            <p class=move || if phase.get() == Some(Phase::Error) { "wmsg bad" } else { "wmsg" }>
                {move || read(status, phase_message)}
            </p>
            {move || status.get().as_ref().and_then(reprocess_message).map(|m| view! { <p class="wmsg">{m}</p> })}
            <div style:display=move || if matches!(phase.get(), Some(Phase::NoFolders | Phase::Error)) { "none" } else { "block" }>
                <div class="wbar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow=move || pct().to_string()>
                    <i style:width=move || format!("{}%", pct())></i>
                </div>
                <div class="wpct">{move || read(status, |s| format!("{}% · {} of {} replays", progress_percent(s.done, s.total), s.done, s.total))}</div>
            </div>
            <div class="wtiles">
                {tile(move || read(status, |s| format!("{}/{}", s.done, s.total)), "imported")}
                {tile(move || read(status, |s| s.duplicates.to_string()), "already known")}
                {tile(move || rejected.with(Vec::len).to_string(), "rejected")}
                {tile(move || failed.with(Vec::len).to_string(), "failed")}
            </div>
            <details style:display=move || if rejected.with(Vec::is_empty) { "none" } else { "block" }>
                <summary>{move || format!("Rejected replays, not valid games ({})", rejected.with(Vec::len))}</summary>
                <ul>{move || rejected.get().into_iter().map(|r| view! { <li>{format!("{} — {}", r.file, r.reason)}</li> }).collect_view()}</ul>
            </details>
            <details style:display=move || if failed.with(Vec::is_empty) { "none" } else { "block" }>
                <summary>{move || format!("Failed uploads, retried at the next start ({})", failed.with(Vec::len))}</summary>
                <ul>{move || failed.get().into_iter().map(|f| view! { <li>{f}</li> }).collect_view()}</ul>
            </details>
            <Show when=move || space().is_some()>
                <div class="sa-row" style="margin-top: 16px">
                    <span class="mono">{move || space().unwrap_or_default()}</span>
                    <button class="ds-btn" on:click=move |_| rescan(msg)>"Rescan now"</button>
                    {move || msg.get().map(|m| view! { <span class="sa-ok">{m}</span> })}
                </div>
            </Show>
        </div>
    }
}

/// "Your in-game names": the names that make "me" on every stats page (`app_settings.operator_names`). The server
/// fills them once with the most frequent player when the first import is done; the field re-reads them at each phase
/// change until the user edits it, then saves through `PUT /api/admin/settings` (open in the desktop app).
#[component]
fn Identity() -> impl IntoView {
    let operator = use_operator();
    let status = use_standalone().status;
    let typed = RwSignal::new(None::<String>);
    let msg = RwSignal::new(None::<String>);
    let phase = Memo::new(move |_| status.with(|s| s.as_ref().map(|s| s.phase)));
    Effect::new(move |_| {
        phase.track();
        spawn_local(async move {
            if let Ok(s) = net::fetch_settings().await {
                operator.set(s.operator_names);
            }
        });
    });
    let value = move || typed.get().unwrap_or_else(|| names_text(&operator.get()));
    let save = move || {
        let names = parse_names(&value());
        let body = serde_json::json!({ "operator_names": names }).to_string();
        spawn_local(async move {
            let m = match net::write("PUT", "/api/admin/settings", Some(body), None).await {
                Ok((s, _)) if (200..300).contains(&s) => {
                    operator.set(names);
                    typed.set(None);
                    "✓ Saved.".to_string()
                }
                Ok((s, _)) => format!("Could not save (HTTP {s})."),
                Err(_) => "Could not save: the local server did not answer.".to_string(),
            };
            msg.set(Some(m));
        });
    };
    view! {
        <div class="ds-panel" style="margin-top: 14px">
            <div class="ds-label">"Your in-game names"</div>
            <p class="sa-none" style="margin: 0 0 10px">"Every stats page is shown from your side. Filled in from your games once they are imported; add your other accounts, separated by commas."</p>
            <div class="sa-row">
                <input class="ds-input" style="min-width: 260px" placeholder="Name, other account"
                    prop:value=value on:input=move |ev| typed.set(Some(event_target_value(&ev))) />
                <button class="ds-btn" on:click=move |_| save()>"Save"</button>
                {move || msg.get().map(|m| {
                    let class = if m.starts_with('✓') { "sa-ok" } else { "sa-err" };
                    view! { <span class=class>{m}</span> }
                })}
            </div>
        </div>
    }
}

#[component]
pub fn Welcome() -> impl IntoView {
    let ctx = use_standalone();
    use_status_poll(ctx);
    // the folder editor stays mounted while the status refreshes every second (keeps the typed path)
    let no_folders = Memo::new(move |_| {
        ctx.status
            .with(|s| s.as_ref().is_some_and(|s| s.phase == Phase::NoFolders))
    });
    let present = Memo::new(move |_| ctx.status.with(Option::is_some));
    view! {
        <div class="ds-page welcome">
            <style>{CSS}{PAGE_CSS}</style>
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Storm Codex · this PC"</div>
                <h1 class="ds-title" style="cursor: default; font-size: clamp(44px, 7vw, 88px)">"Welcome"</h1>
                <div class="ds-subtitle" style="cursor: default">"Your replays are imported from this PC. You can browse the app while it runs."</div>
            </header>
            <section class="ds-sec" style="margin-top: 34px">
                {move || match (ctx.known.get(), present.get()) {
                    (false, _) => view! { <div class="sa-none">"Loading…"</div> }.into_any(),
                    (true, false) => view! { <div class="sa-none">"The import screen is only available in the Storm Codex desktop app."</div> }.into_any(),
                    (true, true) => view! { <Body /> }.into_any(),
                }}
                <Show when=move || present.get()>
                    <Identity />
                </Show>
                <Show when=move || no_folders.get()>
                    <div class="ds-panel" style="margin-top: 14px">
                        <div class="ds-label">"Add a replay folder"</div>
                        <p class="sa-none" style="margin: 0 0 10px">"Usually Documents\\Heroes of the Storm\\Accounts\\…\\Replays\\Multiplayer. Any folder above it works too."</p>
                        <FolderEditor />
                    </div>
                </Show>
            </section>
            <div class="sa-row" style="margin-top: 22px">
                <Link to="/" class="pill on">"Continue to the app ›"</Link>
                <Link to="/admin" class="pill">"Settings"</Link>
            </div>
        </div>
    }
}
