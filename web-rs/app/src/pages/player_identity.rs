//! Identity tab of a player: dated name history, guessed other accounts (an estimate) and confirmed links. Port of
//! `web/src/components/PlayerIdentity.tsx`.

use leptos::{prelude::*, task::spawn_local};
use storm_web_core::{
    net,
    players::{recent_names, seen_range, Identity, Suggestion},
    route::uri_component,
};

use crate::ui::{Link, SecHead};

const SHOWN_SUGGESTIONS: usize = 5;

/// `PUT /api/players/{toon}/links/{other}`: confirm ("same person", a note — no merge) or dismiss a suggestion.
async fn put_link(toon: &str, other: &str, status: &str) -> Result<(), String> {
    let url = format!(
        "/api/players/{}/links/{}",
        uri_component(toon),
        uri_component(other)
    );
    let body = serde_json::json!({ "status": status }).to_string();
    match net::write("PUT", &url, Some(body), None).await {
        Ok((s, _)) if (200..300).contains(&s) => Ok(()),
        Ok((401, _)) => Err("admin token required".to_string()),
        Ok((s, _)) => Err(format!("error {s}")),
        Err(e) => Err(e),
    }
}

#[component]
fn SuggestionCard(
    s: Suggestion,
    busy: ReadSignal<bool>,
    on_decide: Callback<(String, &'static str)>,
) -> impl IntoView {
    let high = s.level == "high";
    let (a, b) = (s.toon.clone(), s.toon.clone());
    view! {
        <div class="ds-panel" style="margin-top: 10px">
            <div style="display: flex; align-items: center; gap: 12px; flex-wrap: wrap">
                <span class=format!("ds-status {}", if high { "analyzed" } else { "stale" })>{format!("{} {}", if high { "HIGH" } else { "LOW" }, s.score)}</span>
                <Link to=format!("/player/{}", uri_component(&s.toon)) style="font: 800 20px var(--display); text-transform: uppercase; color: var(--text); text-decoration: none">
                    {s.name.clone().unwrap_or_else(|| s.toon.clone())}
                </Link>
                <span class="ds-actions">
                    <button class="ds-btn small primary" disabled=move || busy.get() on:click=move |_| on_decide.run((a.clone(), "confirmed"))>"Confirm"</button>
                    <button class="ds-btn small" disabled=move || busy.get() on:click=move |_| on_decide.run((b.clone(), "dismissed"))>"Not the same person"</button>
                </span>
            </div>
            <ul style="margin: 8px 0 0; padding-left: 18px; color: var(--muted-2); font: 12px var(--mono)">
                {s.reasons.iter().map(|r| view! { <li>{r.text.clone()}</li> }).collect_view()}
            </ul>
        </div>
    }
}

#[component]
pub fn PlayerIdentity(
    toon: Signal<String>,
    data: LocalResource<Result<Identity, String>>,
) -> impl IntoView {
    let all_names = RwSignal::new(false);
    let all_sugg = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let err = RwSignal::new(None::<String>);
    let decide = Callback::new(move |(other, status): (String, &'static str)| {
        busy.set(true);
        err.set(None);
        let t = toon.get_untracked();
        spawn_local(async move {
            match put_link(&t, &other, status).await {
                // symmetric decision: every identity read may change
                Ok(()) => data.refetch(),
                Err(e) => err.set(Some(e)),
            }
            busy.set(false);
        });
    });
    let busy_read = busy.read_only();

    move || {
        match data.get() {
        None => view! { <div class="ds-empty" style="margin-top: 24px">"looking for linked accounts…"</div> }.into_any(),
        Some(Err(_)) => view! { <div class="ds-empty" style="margin-top: 24px">"Could not load identity."</div> }.into_any(),
        Some(Ok(d)) => {
            let (shown, hidden) = recent_names(&d.names, 3);
            let n_names = d.names.len();
            let has_profiles = !d.profiles.is_empty();
            let has_confirmed = !d.confirmed.is_empty();
            let n_sugg = d.suggestions.len();
            let names_sub = move || {
                if hidden > 0 {
                    view! {
                        <span class="ds-pill" on:click=move |_| all_names.update(|a| *a = !*a)>
                            {move || if all_names.get() { "recent only".to_string() } else { format!("show all {n_names}") }}
                        </span>
                    }
                    .into_any()
                } else {
                    format!("{n_names} seen").into_any()
                }
            };
            let names_all = d.names.clone();
            let name_chips = move || {
                let all = all_names.get();
                let list = if all { names_all.clone() } else { shown.clone() };
                let empty = list.is_empty();
                let chips = list
                    .into_iter()
                    .enumerate()
                    .map(|(i, n)| {
                        let current = if i == 0 && !all { "current · " } else { "" };
                        view! { <span class="ds-chip text">{format!("{} ", n.name)}<em>{format!("{current}{} · {}g", seen_range(&n), n.games)}</em></span> }
                    })
                    .collect_view();
                (chips, empty)
            };
            let sugg = d.suggestions.clone();
            let confirmed = d.confirmed.clone();
            let profiles = d.profiles.clone();
            view! {
                <section class="ds-sec">
                    <SecHead num=1 title="Names">{names_sub()}</SecHead>
                    <div class="ds-panel">
                        <div class="ds-chips" style=move || if all_names.get() { "max-height: 280px; overflow-y: auto" } else { "" }>
                            {move || { let (chips, empty) = name_chips(); view! { {chips}{empty.then(|| view! { <span class="ds-none">"No dated names yet."</span> })} } }}
                        </div>
                    </div>
                </section>

                {has_profiles.then(|| view! {
                    <section class="ds-sec">
                        <SecHead num=2 title="Hotkey profiles" sub="custom names, read from replays" />
                        <div class="ds-panel">
                            <div class="ds-chips">
                                {profiles.into_iter().map(|p| view! { <span class="ds-chip text">{format!("{} ", p.name)}<em>{format!("{} · {}g", seen_range(&p), p.games)}</em></span> }).collect_view()}
                            </div>
                        </div>
                    </section>
                })}

                <section class="ds-sec">
                    <SecHead num=if has_profiles { 3 } else { 2 } title="Other accounts" sub="estimate" />
                    <div class="ds-banner">"Guesses from shared hotkey profile names, teammates, hero pool, talent habits and play hours — never proof. Confirming only adds a note; the two profiles stay separate."</div>
                    {move || err.get().map(|e| {
                        let need_token = e == "admin token required";
                        view! { <div class="ds-toast bad">{e}{need_token.then(|| view! { " — set it in "<Link to="/admin">"Admin"</Link> })}</div> }
                    })}
                    {move || {
                        let list: Vec<Suggestion> = if all_sugg.get() { sugg.clone() } else { sugg.iter().take(SHOWN_SUGGESTIONS).cloned().collect() };
                        list.into_iter().map(|s| view! { <SuggestionCard s=s busy=busy_read on_decide=decide /> }).collect_view()
                    }}
                    {(n_sugg == 0).then(|| view! { <div class="ds-empty" style="margin-top: 10px">"No likely other accounts."</div> })}
                    {(n_sugg > SHOWN_SUGGESTIONS).then(|| view! {
                        <p style="margin-top: 10px">
                            <span class="ds-pill" on:click=move |_| all_sugg.update(|a| *a = !*a)>
                                {move || if all_sugg.get() { "top 5 only".to_string() } else { format!("show all {n_sugg}") }}
                            </span>
                        </p>
                    })}
                </section>

                {has_confirmed.then(|| view! {
                    <section class="ds-sec">
                        <SecHead num=if has_profiles { 4 } else { 3 } title="Confirmed" sub="marked as the same person" />
                        <div class="ds-chips">
                            {confirmed.into_iter().map(|c| view! {
                                <Link to=format!("/player/{}", uri_component(&c.toon)) class="ds-chip text" style="text-decoration: none; color: inherit">{c.name.clone().unwrap_or_else(|| c.toon.clone())}</Link>
                            }).collect_view()}
                        </div>
                    </section>
                })}
            }
            .into_any()
        }
    }
    }
}
