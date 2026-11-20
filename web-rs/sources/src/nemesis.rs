//! `/nemesis` (spec `docs/specs/2026-11-20-streamer-widgets.md` item 5): a badge per resolved
//! opponent the operator has a grudge-match record against, shown during Draft/loading.
//!
//! Opponent toons come from `GET /api/lobby` (not `/api/live`, which never serialises toon
//! handles — see `storm_web_core::nemesis`), on the existing `lobby.detected`/`lobby.updated`
//! events. The per-opponent win/loss record reuses the existing `against` archive filter twice
//! (`GET /api/matches?against=<toon>&limit=1` for the game count via `X-Total-Count`, and the same
//! with `&result=win` for the win count) — no backend change. Looked up once per resolved lobby
//! (`lobby_signature`), not once per event, per the spec's perf acceptance bullet. Shows only the
//! opponent's name and the operator's own W–L against them, nothing more than the app's existing
//! "Played against" picker and Compare page already show to the operator — `/api/lobby` already
//! resolves the whole roster for the loading-screen companion, this badge does not add any new
//! identity exposure, only a public (OBS) rendering of it.

use crate::frame::OverlayFrame;
use leptos::{prelude::*, task::spawn_local};
use storm_web_core::{
    nemesis::{enemy_toons, is_grudge, lobby_signature, Lobby},
    net,
    route::{uri_component, Anchor},
    session::MatchSummary,
};

pub fn mount() {
    mount_to_body(Nemesis);
}

#[derive(Clone, PartialEq)]
struct Grudge {
    name: String,
    wins: u32,
    losses: u32,
}

/// Opponent's display name as seen in the most recent game against them (`players[].name` of the
/// `against` row), else the toon handle.
fn opponent_name(rows: &[MatchSummary], toon: &str) -> String {
    rows.first()
        .and_then(|m| m.players.as_ref())
        .and_then(|ps| ps.iter().find(|p| p.toon.as_deref() == Some(toon)))
        .and_then(|p| p.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| toon.to_string())
}

/// Total games and wins against `toon`, each read off `X-Total-Count` (two small `limit=1`
/// requests: the body is not used for the count, only for `opponent_name` on the first one).
async fn record_against(toon: &str) -> Option<(Vec<MatchSummary>, u32, u32)> {
    let against = uri_component(toon);
    let (rows, total) = net::get_json_counted::<Vec<MatchSummary>>(&format!(
        "/api/matches?against={against}&limit=1"
    ))
    .await
    .ok()?;
    let games = total.unwrap_or(0) as u32;
    let (_, win_total) = net::get_json_counted::<Vec<MatchSummary>>(&format!(
        "/api/matches?against={against}&result=win&limit=1"
    ))
    .await
    .ok()?;
    let wins = win_total.unwrap_or(0) as u32;
    Some((rows, wins, games))
}

#[component]
fn Nemesis() -> impl IntoView {
    let grudges = RwSignal::new(Vec::<Grudge>::new());
    // last lobby signature already looked up: debounces the per-opponent requests to once per
    // lobby instead of once per `lobby.updated`/poll tick
    let checked = RwSignal::new(String::new());

    let check = move || {
        spawn_local(async move {
            let Ok(Some(lobby)) = net::get_json_opt::<Lobby>("/api/lobby").await else {
                grudges.set(Vec::new());
                checked.set(String::new());
                return;
            };
            let sig = lobby_signature(&lobby);
            if sig.is_empty() {
                grudges.set(Vec::new());
                checked.set(String::new());
                return;
            }
            if checked.get_untracked() == sig {
                return;
            }
            checked.set(sig);
            let mut found = Vec::new();
            for toon in enemy_toons(&lobby) {
                if let Some((rows, wins, games)) = record_against(&toon).await {
                    if is_grudge(wins, games) {
                        found.push(Grudge {
                            name: opponent_name(&rows, &toon),
                            wins,
                            losses: games - wins,
                        });
                    }
                }
            }
            grudges.set(found);
        });
    };

    check();
    net::start_live_events(move |ev| {
        if ev == "lobby.detected" || ev == "lobby.updated" {
            check();
        }
    });

    view! {
        <OverlayFrame anchor=Anchor::TopLeft top=28.0>
            <div style="display:flex;flex-direction:column;gap:8px">
                {move || grudges.get().into_iter().map(|g| view! {
                    <div style="display:inline-flex;align-items:center;gap:9px;background:rgba(40,10,10,.92);border:1px solid var(--loss);border-radius:999px;padding:6px 14px;box-shadow:0 6px 20px rgba(0,0,0,.5);font-size:14px">
                        <span style="font-weight:700;color:var(--loss);letter-spacing:.03em">"⚠ NEMESIS"</span>
                        <span style="font-weight:600">{g.name.clone()}</span>
                        <span class="mono" style="font-size:12.5px;color:var(--text-2)">
                            <span style="color:var(--win)">{g.wins}"W"</span>"–"<span style="color:var(--loss)">{g.losses}"L"</span>
                        </span>
                    </div>
                }).collect_view()}
            </div>
        </OverlayFrame>
    }
}
