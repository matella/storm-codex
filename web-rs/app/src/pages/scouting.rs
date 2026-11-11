//! Scouting: the list of reports and the creation of one. Port of `web/src/pages/Scouting.tsx`.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::hooks::use_navigate;
use serde::Deserialize;
use storm_web_core::{
    net,
    scouting::{day, ScoutingListItem},
};

use crate::ui::{fetch, Link, SecHead};

#[derive(Deserialize)]
struct Created {
    id: i64,
}

#[component]
fn ReportCard(r: ScoutingListItem) -> impl IntoView {
    let rec = match r.record {
        Some(x) if x.n > 0 => view! { {x.k}<i>"–"</i>{x.n - x.k} }.into_any(),
        _ => "—".into_any(),
    };
    let span = r
        .first_date
        .as_deref()
        .map(|f| format!(" · {} → {}", day(Some(f)), day(r.last_date.as_deref())))
        .unwrap_or_default();
    view! {
        <Link to=format!("/scouting/{}", r.id) class="ds-rcard">
            <div class="ds-rcard-main">
                <span class="ds-idx">{format!("REPORT #{} · CREATED {}", r.id, day(Some(&r.created_at)))}</span>
                <strong>{r.target_name.clone().unwrap_or_else(|| "Unnamed team".to_string())}</strong>
                <span class="ds-rcard-title">{r.title.clone()}</span>
            </div>
            <div class="ds-rcard-side">
                <span class="ds-rcard-rec">{rec}</span>
                <span class="ds-meta">{format!("{} replay{}{}", r.games, if r.games == 1 { "" } else { "s" }, span)}</span>
                <span class=format!("ds-status {}", r.status.class())>{r.status.label()}</span>
            </div>
        </Link>
    }
}

#[component]
pub fn Scouting() -> impl IntoView {
    let reports = fetch::<Vec<ScoutingListItem>>(|| "/api/scouting".to_string());
    let nav = use_navigate();
    let title = RwSignal::new(String::new());
    let target = RwSignal::new(String::new());
    let msg = RwSignal::new(None::<String>);
    let create = move || {
        let nav = nav.clone();
        if title.get_untracked().trim().is_empty() {
            msg.set(Some("give the report a title".to_string()));
            return;
        }
        let body = serde_json::json!({ "title": title.get_untracked(), "target_name": target.get_untracked() }).to_string();
        spawn_local(async move {
            match net::write("POST", "/api/scouting", Some(body), None).await {
                Ok((401, _)) => msg.set(Some(
                    "✗ unauthorized — set the admin token in Admin".to_string(),
                )),
                Ok((s, _)) if !(200..300).contains(&s) => msg.set(Some(format!("✗ HTTP {s}"))),
                Ok((_, text)) => match serde_json::from_str::<Created>(&text) {
                    Ok(c) => nav(&format!("/scouting/{}", c.id), Default::default()),
                    Err(e) => msg.set(Some(format!("✗ {e}"))),
                },
                Err(e) => msg.set(Some(format!("✗ {e}"))),
            }
        });
    };
    let on_enter = {
        let create = create.clone();
        move |e: leptos::ev::KeyboardEvent| {
            if e.key() == "Enter" {
                create();
            }
        }
    };
    let on_enter2 = on_enter.clone();
    let count = move || {
        reports
            .get()
            .and_then(Result::ok)
            .map_or(0, |r| r.len() as u32)
    };
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Storm Codex · scouting"</div>
                <h1 class="ds-title" style="cursor: default">"Scouting"</h1>
                <div class="ds-subtitle" style="cursor: default">
                    "Drop an opposing team's replays, get their draft habits as numbers, hand the pack to an LLM, bring back a map-by-map draft plan."
                </div>
                <div class="ds-edit" style="margin-top: 22px">
                    <input class="ds-input big" placeholder="team name" prop:value=move || target.get()
                        on:input=move |e| target.set(event_target_value(&e)) on:keydown=on_enter />
                    <input class="ds-input" style="flex: 2; min-width: 220px" placeholder="report title (e.g. Week 4 — playoffs)"
                        prop:value=move || title.get() on:input=move |e| title.set(event_target_value(&e)) on:keydown=on_enter2 />
                    <span class="ds-btn primary" style="align-self: center" on:click=move |_| create()>"New report"</span>
                </div>
                {move || msg.get().map(|m| view! { <div class="ds-meta" style="margin-top: 8px; color: var(--loss-soft)">{m}</div> })}
            </header>
            <section class="ds-sec">
                {move || view! { <SecHead num=count() title="Reports" sub="newest first" /> }}
                {move || match reports.get() {
                    None => view! { <div class="ds-empty">"loading…"</div> }.into_any(),
                    Some(Err(_)) => ().into_any(),
                    Some(Ok(list)) if list.is_empty() => view! { <div class="ds-empty">"No report yet — name the team above and create one."</div> }.into_any(),
                    Some(Ok(list)) => view! { <div class="ds-rlist">{list.into_iter().map(|r| view! { <ReportCard r=r /> }).collect_view()}</div> }.into_any(),
                }}
            </section>
        </div>
    }
}
