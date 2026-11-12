//! Leagues: teams grouped by league. Read-only; teams are created and put in a league from Admin.
//! Port of `web/src/pages/Leagues.tsx`.

use leptos::prelude::*;
use storm_web_core::aggregates::{group_leagues, plural, Team};

use crate::ui::{fetch, Link, SecHead};

#[component]
pub fn Leagues() -> impl IntoView {
    let teams = fetch::<Vec<Team>>(|| "/api/teams".to_string());
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Leagues"</div>
                <h1 class="ds-title" style="cursor: default">"Leagues"</h1>
                <div class="ds-subtitle" style="cursor: default">
                    {move || match teams.get() {
                        Some(Ok(t)) => {
                            format!("{} in {}", plural(t.len(), "team", "teams"), plural(group_leagues(&t).len(), "league", "leagues"))
                        }
                        _ => "loading…".to_string(),
                    }}
                    " — create teams and set their league in Admin."
                </div>
                <div class="ds-cover-foot"><Link class="ds-btn" to="/admin">"Manage teams in Admin ›"</Link></div>
            </header>
            {move || match teams.get() {
                None => view! { <div class="ds-empty" style="margin-top: 20px">"loading…"</div> }.into_any(),
                Some(Err(_)) => ().into_any(),
                Some(Ok(t)) if t.is_empty() => {
                    view! { <div class="ds-empty" style="margin-top: 20px">"No teams yet — create them in Admin."</div> }.into_any()
                }
                Some(Ok(t)) => group_leagues(&t)
                    .into_iter()
                    .enumerate()
                    .map(|(i, (league, ts))| {
                        let sub = plural(ts.len(), "team", "teams");
                        view! {
                            <section class="ds-sec">
                                <SecHead num=(i + 1) as u32 title=league sub=sub />
                                <div class="ds-teams">
                                    {ts.into_iter().map(|t| {
                                        let roster = t.roster.clone().unwrap_or_default();
                                        view! {
                                            <div class="ds-team">
                                                <strong>{t.name.clone()}</strong>
                                                <div class="ds-chips">
                                                    {if roster.is_empty() {
                                                        view! { <span class="ds-none">"no roster"</span> }.into_any()
                                                    } else {
                                                        roster.into_iter().map(|r| view! { <span class="ds-chip text"><span>{r}</span></span> }).collect_view().into_any()
                                                    }}
                                                </div>
                                            </div>
                                        }
                                    }).collect_view()}
                                </div>
                            </section>
                        }
                    })
                    .collect_view()
                    .into_any(),
            }}
        </div>
    }
}
