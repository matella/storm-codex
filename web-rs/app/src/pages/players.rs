//! Players: search by current name, old name or fragment (`?q=` in the URL).
//! Port of `web/src/pages/Players.tsx`.

use leptos::{prelude::*, task::spawn_local};
use leptos_router::{
    hooks::{use_location, use_navigate},
    NavigateOptions,
};
use storm_web_core::{
    net,
    players::{highlight_segments, name_initials, pct, PlayerHit, PlayerSearch},
    route::{app_path, query_value, uri_component},
};

use crate::ui::Link;

const MIN_CHARS: usize = 2;

#[component]
fn Marked(text: String, q: String) -> impl IntoView {
    highlight_segments(&text, &q)
        .into_iter()
        .map(|s| {
            if s.hit {
                view! { <mark>{s.text}</mark> }.into_any()
            } else {
                view! { <span>{s.text}</span> }.into_any()
            }
        })
        .collect_view()
}

#[component]
fn HitRow(h: PlayerHit, q: String) -> impl IntoView {
    let shown = if h.matched_old {
        h.name.clone().unwrap_or_else(|| h.matched_name.clone())
    } else {
        h.matched_name.clone()
    };
    let wr = pct(h.wins, h.games);
    let wr_style = if wr >= 55 {
        "color: var(--win)"
    } else if wr < 45 {
        "color: var(--loss)"
    } else {
        ""
    };
    let roles = if h.roles.is_empty() {
        "—".to_string()
    } else {
        h.roles
            .iter()
            .map(|(r, _)| r.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let last = h
        .last_seen
        .as_deref()
        .map_or("—".to_string(), |l| net::fmt_time(Some(l)));
    view! {
        <Link to=format!("/player/{}", uri_component(&h.toon)) class="ds-rm" style="margin-top: 10px; text-decoration: none; color: inherit">
            <span class="ds-pt" style="--s: 44px; --ring: var(--accent)"><span class="ds-ini">{name_initials(&shown)}</span></span>
            <div style="flex: 1; min-width: 0">
                <strong>
                    {if h.matched_old { shown.clone().into_any() } else { view! { <Marked text=shown.clone() q=q.clone() /> }.into_any() }}
                    {h.matched_old.then(|| view! {
                        <span class="ds-pill" style="margin-left: 8px">"was “"<Marked text=h.matched_name.clone() q=q.clone() />"”"</span>
                    })}
                </strong>
                <span>{roles}</span>
                <em>{format!("{} games · last seen {last}", h.games)}</em>
            </div>
            <div style="text-align: right">
                <strong style=wr_style>{format!("{wr}%")}</strong>
                <em>"win rate"</em>
            </div>
        </Link>
    }
}

#[component]
pub fn Players() -> impl IntoView {
    let loc = use_location();
    let nav = StoredValue::new_local(use_navigate());
    let q = Memo::new(move |_| query_value(&loc.search.get(), "q").unwrap_or_default());
    let text = RwSignal::new(q.get_untracked());
    // `autoFocus`: a node inserted by the router does not take the attribute, so focus it by hand
    let input = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
        }
    });
    // debounce: the URL (so the request) follows the typing after a short pause
    let generation = RwSignal::new(0u32);
    let on_input = move |v: String| {
        text.set(v);
        generation.update(|g| *g += 1);
        let mine = generation.get_untracked();
        net::after_ms(250, move || {
            if generation.get_untracked() != mine {
                return;
            }
            let t = text.get_untracked();
            if t == q.get_untracked() {
                return;
            }
            let path = app_path(&loc.pathname.get_untracked()).to_string();
            let to = if t.is_empty() {
                path
            } else {
                format!("{path}?q={}", storm_web_core::aggregates::form_encode(&t))
            };
            nav.with_value(|nav| {
                nav(
                    &to,
                    NavigateOptions {
                        replace: true,
                        ..Default::default()
                    },
                )
            });
        });
    };
    let term = Memo::new(move |_| q.get().trim().to_string());
    let enabled = move || term.with(|t| t.chars().count() >= MIN_CHARS);
    // the last answer stays on screen while the next one loads
    let hits = RwSignal::new(None::<Vec<PlayerHit>>);
    let failed = RwSignal::new(false);
    Effect::new(move |_| {
        let t = term.get();
        if t.chars().count() < MIN_CHARS {
            return;
        }
        spawn_local(async move {
            match net::get_json::<PlayerSearch>(&format!(
                "/api/players/search?q={}",
                uri_component(&t)
            ))
            .await
            {
                Ok(r) if term.get_untracked() == t => {
                    failed.set(false);
                    hits.set(Some(r.hits));
                }
                Err(_) if term.get_untracked() == t => failed.set(true),
                _ => {}
            }
        });
    });
    view! {
        <div class="ds-page">
            <div class="ds-crumb">"Players"</div>
            <div class="ds-kicker">"Search"</div>
            <h1 class="ds-title" style="cursor: default; font-size: clamp(40px, 6vw, 72px)">"Players"</h1>
            <div class="ds-subtitle" style="cursor: default">"Current names, old names, or part of a name — only players seen in your replays."</div>
            <input class="ds-input" style="width: 100%; margin-top: 18px; font-size: 16px" node_ref=input autofocus placeholder="Search a player…"
                prop:value=move || text.get() on:input=move |e| on_input(event_target_value(&e)) aria-label="Search a player" />
            {move || {
                if !enabled() {
                    return view! { <div class="ds-empty" style="margin-top: 18px">{format!("Type at least {MIN_CHARS} characters.")}</div> }.into_any();
                }
                if failed.get() {
                    return view! { <div class="ds-empty" style="margin-top: 18px">"Search failed. Try again."</div> }.into_any();
                }
                let data = hits.get();
                let list = data.clone().unwrap_or_default();
                let t = term.get();
                view! {
                    <span class="ds-label" style="margin-top: 16px">
                        {if data.is_none() { "searching…".to_string() } else { format!("{} player{} found", list.len(), if list.len() == 1 { "" } else { "s" }) }}
                    </span>
                    {list.iter().cloned().map(|h| view! { <HitRow h=h q=t.clone() /> }).collect_view()}
                    {(data.is_some() && list.is_empty()).then(|| view! { <div class="ds-empty" style="margin-top: 10px">{format!("No player matches “{t}”.")}</div> })}
                }.into_any()
            }}
        </div>
    }
}
