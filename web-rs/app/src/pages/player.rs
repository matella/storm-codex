//! Player: cover with the portrait of the most played hero, hero pool, recent games, chat habits. The Identity tab
//! (links between accounts, written with the admin token) comes with the interactive pages.
//! Port of `web/src/pages/Player.tsx` and `PlayerChat.tsx`.

use leptos::prelude::*;
use leptos_router::{
    hooks::{use_location, use_navigate, use_params_map},
    NavigateOptions,
};
use storm_web_core::{
    aggregates::{has_map_art, tone},
    net,
    players::{
        kda_ratio, likely_alias, num_or_blank, num_or_dash, pct, pct_share, ChatHabits, Identity,
        PlayerSummary,
    },
    route::{query_value, uri_component},
    session::award_label,
};

use crate::{
    matchui::ModeTag,
    pages::player_identity::PlayerIdentity,
    ui::{fetch, map_art, use_dim, Link, Portrait, SecHead, Tone},
};

/// Chat habits of an account: measures only, never the text. Nothing when the account is not in the corpus.
#[component]
fn PlayerChat(toon: Signal<String>) -> impl IntoView {
    let data =
        fetch::<ChatHabits>(move || format!("/api/players/{}/chat", uri_component(&toon.get())));
    move || {
        let d = data.get()?.ok()?;
        if !d.enough {
            return Some(view! { <div class="ds-empty">{format!("Not enough games ({}) for chat habits.", d.games)}</div> }.into_any());
        }
        let t = d.timing.clone();
        Some(view! {
            <div class="ds-panel">
                <div class="ds-statline">
                    <div class="ds-stat"><span class="ds-label">"Messages"</span><strong>{d.messages}</strong><small>{format!("{} per game", storm_web_core::session::to_fixed1(d.per_game))}</small></div>
                    <div class="ds-stat"><span class="ds-label">"Team chat"</span><strong>{pct_share(d.team_share)}</strong><small>{format!("all chat {}", pct_share(d.all_share))}</small></div>
                    <div class="ds-stat"><span class="ds-label">"When"</span><strong>{pct_share(t.early)}<span>"/"</span>{pct_share(t.mid)}<span>"/"</span>{pct_share(t.late)}</strong><small>"early / mid / late"</small></div>
                </div>
                {(!d.top_words.is_empty()).then(|| view! {
                    <div class="ds-chips" style="margin-top: 12px">
                        {d.top_words.iter().map(|w| view! { <span class="ds-chip text">{w.word.clone()}" "<em>{format!("×{}", w.count)}</em></span> }).collect_view()}
                    </div>
                })}
            </div>
        }.into_any())
    }
}

#[component]
pub fn Player() -> impl IntoView {
    let params = use_params_map();
    let toon = Signal::derive(move || params.read().get("toon").unwrap_or_default());
    let loc = use_location();
    let nav = StoredValue::new_local(use_navigate());
    let dim = use_dim();
    let data =
        fetch::<PlayerSummary>(move || format!("/api/players/{}", uri_component(&toon.get())));
    // loaded apart from the summary (linked accounts are slower)
    let identity =
        fetch::<Identity>(move || format!("/api/players/{}/identity", uri_component(&toon.get())));
    let all_heroes = RwSignal::new(false);
    let identity_tab = move || query_value(&loc.search.get(), "tab").as_deref() == Some("identity");
    let go = move |to: String, replace: bool| {
        nav.with_value(|nav| {
            nav(
                &to,
                NavigateOptions {
                    replace,
                    ..Default::default()
                },
            )
        })
    };
    view! {
        <div class="ds-page">
            {move || match data.get() {
                None => view! { <div class="ds-empty" style="margin-top: 24px">"loading…"</div> }.into_any(),
                Some(Err(_)) => view! {
                    <div class="ds-empty" style="margin-top: 24px">"Player not found. "<Link to="/matches">"Back to matches"</Link></div>
                }.into_any(),
                Some(Ok(d)) => {
                    let tk = toon.get();
                    let alias = identity.get().and_then(Result::ok).and_then(|i| likely_alias(&i));
                    let main = d.heroes.first().map(|h| h.hero.clone());
                    let wr = pct(d.wins, d.matches);
                    let ratio = kda_ratio(d.avg_takedowns, d.avg_deaths);
                    let max_g = d.heroes.first().map_or(1, |h| h.games);
                    let others: Vec<String> = d.names.iter().filter(|n| Some(*n) != d.name.as_ref()).cloned().collect();
                    let ring = dim.with(|dm| dm.ring(main.as_deref()));
                    let ident_link = format!("/player/{}?tab=identity", uri_component(&tk));
                    let base_path = format!("/player/{}", uri_component(&tk));
                    let (bp, ip) = (base_path.clone(), ident_link.clone());
                    view! {
                        <header class="ds-hhead" style=format!("--ring: {ring}")>
                            {main.clone().map(|h| view! { <Portrait hero=h size=160 class="xl" /> })}
                            <div class="ds-hhead-txt">
                                <div class="ds-kicker">{format!("Player · {}", d.toon)}</div>
                                <h1 class="ds-title" style="cursor: default">{d.name.clone().unwrap_or_else(|| d.toon.clone())}</h1>
                                {(!others.is_empty()).then(|| view! {
                                    <div class="ds-subtitle" style="cursor: default">
                                        {format!("also seen as {}", others.iter().take(3).cloned().collect::<Vec<_>>().join(", "))}
                                        {(others.len() > 3).then(|| view! { " "<Link to=ident_link.clone()>{format!("+{} more", others.len() - 3)}</Link> })}
                                    </div>
                                })}
                                {alias.map(|a| view! {
                                    <div class="ds-subtitle" style="cursor: default"><Link to=ident_link.clone()>{format!("Also plays as {} →", a.name.clone().unwrap_or(a.toon.clone()))}</Link></div>
                                })}
                                <div class="ds-statline" style="margin-top: 16px">
                                    <div class="ds-stat"><span class="ds-label">"Win rate"</span><strong>{wr}<span>"%"</span></strong><small>{format!("{}–{} · {} games", d.wins, d.matches - d.wins, d.matches)}</small></div>
                                    <div class="ds-stat"><span class="ds-label">"Avg K / D / T"</span><strong>{num_or_dash(d.avg_kills)}<span>"/"</span>{num_or_dash(d.avg_deaths)}<span>"/"</span>{num_or_dash(d.avg_takedowns)}</strong><small>{format!("KDA ratio {ratio}")}</small></div>
                                    <div class="ds-stat"><span class="ds-label">"Avg APM"</span><strong>{num_or_dash(d.avg_apm)}</strong><small>{if d.apm_games > 0 { format!("{} games measured", d.apm_games) } else { "re-process pending".into() }}</small></div>
                                    <div class="ds-stat"><span class="ds-label">"Heroes"</span><strong>{d.heroes.len()}</strong><small>{main.clone().map(|m| format!("main: {m}")).unwrap_or_default()}</small></div>
                                </div>
                            </div>
                        </header>

                        <nav class="ds-nav" aria-label="Player sections">
                            <a class=move || if identity_tab() { None } else { Some("map") } href="?" on:click=move |e| { e.prevent_default(); go(bp.clone(), false) }>"Stats"</a>
                            <a class=move || if identity_tab() { Some("map") } else { None } href="?tab=identity" on:click=move |e| { e.prevent_default(); go(ip.clone(), false) }>"Identity"</a>
                        </nav>
                        {move || if identity_tab() {
                            view! { <PlayerIdentity toon=toon data=identity /> }.into_any()
                        } else {
                            let heroes = d.heroes.clone();
                            let n_heroes = heroes.len();
                            let shown: Vec<_> = if all_heroes.get() { heroes } else { heroes.into_iter().take(18).collect() };
                            let recent = d.recent.clone();
                            view! {
                                <section class="ds-sec">
                                    <SecHead num=1 title="Hero pool">
                                        {move || if n_heroes > 18 {
                                            view! { <span class="ds-pill" on:click=move |_| all_heroes.update(|a| *a = !*a)>
                                                {move || if all_heroes.get() { "top 18".to_string() } else { format!("show all {n_heroes}") }}
                                            </span> }.into_any()
                                        } else {
                                            "games · win rate · avg K/D/T".into_any()
                                        }}
                                    </SecHead>
                                    <div class="ds-panel">
                                        <div class="ds-pool">
                                            {shown.into_iter().map(|h| {
                                                let p = pct(h.wins, h.games);
                                                view! {
                                                    <Link to=format!("/hero/{}", uri_component(&h.hero)) class="ds-poolrow">
                                                        <Portrait hero=h.hero.clone() size=34 />
                                                        <span>{h.hero.clone()}</span>
                                                        <i class="ds-bar"><b style=format!("width: {}%", ((h.games * 100) as f64 / max_g as f64).round())></b></i>
                                                        <span class=format!("wr {}", tone(p))>{format!("{p}% · {}g", h.games)}</span>
                                                        <em>{format!("{}/{}/{}", num_or_blank(h.avg_kills), num_or_blank(h.avg_deaths), num_or_blank(h.avg_takedowns))}</em>
                                                    </Link>
                                                }
                                            }).collect_view()}
                                        </div>
                                    </div>
                                </section>

                                <section class="ds-sec">
                                    <SecHead num=2 title="Recent games" />
                                    <div class="ds-mlist">
                                        {recent.into_iter().map(|g| {
                                            let aw = award_label(g.award.as_deref());
                                            let a = g.takedowns.unwrap_or(0).saturating_sub(g.kills.unwrap_or(0));
                                            let map = g.map.clone().unwrap_or_default();
                                            let style = if has_map_art(&map) { map_art(&map) } else { String::new() };
                                            let won = g.win == Some(true);
                                            let id = g.match_id;
                                            let go_match = move || go(format!("/match/{id}"), false);
                                            let go_match2 = go_match;
                                            view! {
                                                <div class=format!("ds-mrow {}", if won { "w" } else { "l" }) style=style on:click=move |_| go_match()
                                                    role="link" tabindex="0" on:keydown=move |e| if e.key() == "Enter" { go_match2() }>
                                                    <span class="when">{net::fmt_time(g.played_at.as_deref())}</span>
                                                    <span><ModeTag mode=g.mode short=true /></span>
                                                    {match g.hero.clone() { Some(h) => view! { <Portrait hero=h size=40 tone=if won { Tone::Pick } else { Tone::Loss } /> }.into_any(), None => view! { <span></span> }.into_any() }}
                                                    <span class="what"><span class="map">{if map.is_empty() { "—".to_string() } else { map }}</span>
                                                        <span class="hero">{g.hero.clone()}" · "<span class="ds-kda">{g.kills}<i>"/"</i>{g.deaths}<i>"/"</i>{a}</span></span></span>
                                                    <span style="display: flex; gap: 6px">
                                                        {g.apm.map(|apm| view! { <span class="ds-tag" title="actions per minute">{format!("{apm} APM")}</span> })}
                                                        <span class=format!("ds-tag {}", if won { "win" } else { "loss" })>{if won { "WIN" } else { "LOSS" }}</span>
                                                        {aw.filter(|a| a.mvp).map(|_| view! { <span class="ds-tag mvp">"👑 MVP"</span> })}
                                                    </span>
                                                    <span class="len">"›"</span>
                                                </div>
                                            }
                                        }).collect_view()}
                                    </div>
                                </section>

                                <section class="ds-sec">
                                    <SecHead num=3 title="Chat" sub="habits only — message text is never shown" />
                                    <PlayerChat toon=toon />
                                </section>
                            }.into_any()
                        }}
                    }.into_any()
                }
            }}
        </div>
    }
}
