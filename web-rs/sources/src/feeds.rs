//! Composition-scoped inputs. Standalone adapters keep their document-lifetime behavior.

use crate::{
    nemesis::{opponent_name, record_against, Grudge},
    recordflash::FLASH_MS,
};
use leptos::{prelude::*, task::spawn_local};
use std::{future::Future, time::Duration};
use storm_web_core::{
    aggregates::{PatchItem, PatchList},
    goal::Progress,
    heroes::{Dim, DimHero},
    insights::{new_personal_record, Records},
    nemesis::{enemy_toons, is_grudge, lobby_signature, Lobby},
    net,
    overlay_layout::{WidgetInstance, WidgetSettings},
    review::Review,
    session::MatchSummary,
};

#[derive(Clone, Copy)]
pub struct Feeds {
    pub names: ReadSignal<Vec<String>>,
    pub voice: ReadSignal<storm_web_core::session::BriefVoice>,
    pub recent: ReadSignal<Vec<MatchSummary>>,
    pub newest: ReadSignal<Vec<MatchSummary>>,
    pub review: ReadSignal<Option<Review>>,
    pub recap: ReadSignal<Option<Review>>,
    pub progress: ReadSignal<Progress>,
    pub patch: ReadSignal<Option<PatchItem>>,
    pub flash: ReadSignal<Option<(String, f64)>>,
    pub grudges: ReadSignal<Vec<Grudge>>,
}

const DIM: u16 = 1;
const NAMES: u16 = 2;
const RECENT: u16 = 4;
const NEWEST: u16 = 8;
const REVIEW: u16 = 16;
const GOAL: u16 = 128;
const PATCH: u16 = 256;
const LOBBY: u16 = 512;
const RECORD: u16 = 1024;
const RECAP: u16 = 32;
const EVENTS: u16 = RECENT | NEWEST | REVIEW | RECAP | PATCH | LOBBY;

impl Feeds {
    pub fn new(widgets: ReadSignal<Vec<WidgetInstance>>) -> Self {
        let demand = Memo::new(move |_| {
            widgets.with(|widgets| {
                widgets.iter().fold(0u16, |bits, w| {
                    bits | match &w.placement.settings {
                        WidgetSettings::Ticker {} => DIM | NAMES | RECENT,
                        WidgetSettings::Widget(_) => DIM | NAMES | NEWEST,
                        WidgetSettings::Queue {} => DIM | NAMES | RECENT,
                        WidgetSettings::Recap {} => DIM | RECAP,
                        WidgetSettings::StreamPool {} => DIM | REVIEW,
                        WidgetSettings::PatchBanner {} => PATCH,
                        WidgetSettings::RecordFlash {} => NEWEST | RECORD,
                        WidgetSettings::Nemesis {} => LOBBY,
                        WidgetSettings::Goal {} => GOAL,
                    }
                })
            })
        });
        let dim = RwSignal::new(Dim::default());
        provide_context(dim);
        let names = RwSignal::new(Vec::new());
        let voice = RwSignal::new(storm_web_core::session::BriefVoice::default());
        let recent = RwSignal::new(Vec::new());
        let newest = RwSignal::new(Vec::new());
        let review = RwSignal::new(None);
        let recap = RwSignal::new(None);
        let progress = RwSignal::new(Progress::default());
        let patch = RwSignal::new(None);
        let lobby = RwSignal::new(None);

        let load_dim = refresh(dim, async || {
            net::get_json::<std::collections::HashMap<String, DimHero>>("/api/dim/heroes")
                .await
                .ok()
                .map(Dim::new)
        });
        let load_names = refresh(names, async || {
            net::fetch_settings().await.ok().map(|s| s.operator_names)
        });
        let load_voice = refresh(voice, async || {
            net::fetch_settings().await.ok().map(|s| s.brief_voice)
        });
        let load_recent = refresh(recent, async || net::fetch_matches(200).await.ok());
        let load_newest = refresh(newest, async || net::fetch_matches(1).await.ok());
        let load_review = refresh(review, || crate::frame::fetch_review(true));
        let load_recap = refresh(recap, || crate::frame::fetch_review(false));
        let load_progress = refresh(progress, async || {
            net::get_json("/api/goal/progress").await.ok()
        });
        let load_patch = refresh(patch, async || {
            net::get_json::<PatchList>("/api/patches")
                .await
                .ok()
                .map(|p| p.items.into_iter().next())
        });
        // Clear badges on an absent or unreadable lobby rather than showing the wrong opponent.
        let load_lobby = refresh(lobby, async || {
            Some(
                net::get_json_opt::<Lobby>("/api/lobby")
                    .await
                    .ok()
                    .flatten(),
            )
        });
        let request = move |bits| {
            if bits & DIM != 0 {
                load_dim()
            }
            if bits & NAMES != 0 {
                load_names();
                load_voice();
            }
            if bits & RECENT != 0 {
                load_recent()
            }
            if bits & NEWEST != 0 {
                load_newest()
            }
            if bits & REVIEW != 0 {
                load_review()
            }
            if bits & RECAP != 0 {
                load_recap()
            }
            if bits & GOAL != 0 {
                load_progress()
            }
            if bits & PATCH != 0 {
                load_patch()
            }
            if bits & LOBBY != 0 {
                load_lobby()
            }
        };
        Effect::new(move |prior: Option<u16>| {
            let bits = demand.get();
            request(bits & !prior.unwrap_or(0));
            bits
        });
        let live = Memo::new(move |_| demand.get() & EVENTS != 0);
        Effect::new(move |_| {
            if live.get() {
                let stop = net::scoped_live_events(move |event| {
                    let bits = match event {
                        "match.parsed" => RECENT | NEWEST | REVIEW | RECAP,
                        "patch.new" => PATCH,
                        "lobby.detected" | "lobby.updated" => LOBBY,
                        "open" => EVENTS,
                        _ => 0,
                    };
                    request(demand.get_untracked() & bits);
                });
                on_cleanup(stop);
            }
        });
        poll(demand, EVENTS | GOAL, |_| 10_000, request);

        Self {
            names: names.read_only(),
            voice: voice.read_only(),
            recent: recent.read_only(),
            newest: newest.read_only(),
            review: review.read_only(),
            recap: recap.read_only(),
            progress: progress.read_only(),
            patch: patch.read_only(),
            flash: watch_record(demand, newest.read_only()),
            grudges: watch_lobby(demand, lobby.read_only()),
        }
    }
}

fn poll(
    demand: Memo<u16>,
    mask: u16,
    cadence: fn(u16) -> u64,
    request: impl Fn(u16) + Copy + 'static,
) {
    let active = Memo::new(move |_| demand.get() & mask);
    Effect::new(move |_| {
        let bits = active.get();
        if bits != 0 {
            let tick = set_interval_with_handle(
                move || request(bits),
                Duration::from_millis(cadence(bits)),
            );
            on_cleanup(move || {
                if let Ok(tick) = tick {
                    tick.clear()
                }
            });
        }
    });
}

/// One in-flight read and at most one trailing refresh per feed. Issuing a newer request
/// invalidates the older result; a disposed owner makes every pending completion a no-op.
fn refresh<T: Send + Sync + 'static, F: Future<Output = Option<T>> + 'static>(
    output: RwSignal<T>,
    fetch: impl Fn() -> F + Copy + 'static,
) -> impl Fn() + Copy {
    let gate = RwSignal::new((false, 0u64));
    move || {
        let Some((running, start)) = gate.try_update(|(running, generation)| {
            *generation += 1;
            (std::mem::replace(running, true), *generation)
        }) else {
            return;
        };
        if running {
            return;
        }
        spawn_local(async move {
            let mut current = start;
            loop {
                let data = fetch().await;
                let Some((_, latest)) = gate.try_get_untracked() else {
                    return;
                };
                if current == latest {
                    gate.set((false, latest));
                    if let Some(data) = data {
                        output.set(data);
                    }
                    return;
                }
                current = latest;
            }
        });
    }
}

fn watch_record(
    demand: Memo<u16>,
    newest: ReadSignal<Vec<MatchSummary>>,
) -> ReadSignal<Option<(String, f64)>> {
    let flash = RwSignal::new(None);
    let id = Memo::new(move |_| {
        (demand.get() & RECORD != 0)
            .then(|| newest.with(|m| m.first().map(|m| m.id)))
            .flatten()
    });
    // The expiry lives with the effect that observed this record, not with any placement.
    let record = RwSignal::new(None);
    let load = refresh(record, move || async move {
        let id = id.try_get_untracked().flatten()?;
        let r = net::get_json::<Records>("/api/records?mine=true&limit=1")
            .await
            .ok()?;
        Some(Some((id, new_personal_record(&r.stats, id))))
    });
    Effect::new(move |_| {
        flash.set(None);
        if id.get().is_some() {
            load();
        }
    });
    Effect::new(move |_| {
        let Some((match_id, data)) = record.get() else {
            return;
        };
        if id.get_untracked() != Some(match_id) {
            return;
        }
        flash.set(data);
        let timer = set_timeout_with_handle(
            move || {
                flash.try_set(None);
            },
            Duration::from_millis(u64::from(FLASH_MS)),
        );
        on_cleanup(move || {
            if let Ok(timer) = timer {
                timer.clear()
            }
        });
    });
    flash.read_only()
}

fn watch_lobby(demand: Memo<u16>, lobby: ReadSignal<Option<Lobby>>) -> ReadSignal<Vec<Grudge>> {
    let grudges = RwSignal::new(Vec::new());
    let roster = Memo::new(move |_| {
        if demand.get() & LOBBY == 0 {
            return None;
        }
        lobby.with(|l| l.as_ref().map(|l| (lobby_signature(l), enemy_toons(l))))
    });
    let resolved = RwSignal::new(None);
    // Lobby-state reads do not wait for opponent lookups. A new roster immediately clears the
    // old badges and invalidates any still-in-flight lookup, even if the server replies late.
    let load = refresh(resolved, move || async move {
        let (sig, toons) = roster.try_get_untracked().flatten()?;
        let mut found = Vec::new();
        for toon in toons {
            if roster.try_with_untracked(|r| r.as_ref().is_some_and(|(s, _)| *s == sig))
                != Some(true)
            {
                return None;
            }
            if let Some((rows, wins, games)) = record_against(&toon).await {
                if wins <= games && is_grudge(wins, games) {
                    found.push(Grudge {
                        name: opponent_name(&rows, &toon),
                        wins,
                        losses: games - wins,
                    });
                }
            }
        }
        Some(Some((sig, found)))
    });
    Effect::new(move |_| {
        grudges.set(Vec::new());
        if roster.with(|r| r.as_ref().is_some_and(|(_, toons)| !toons.is_empty())) {
            load();
        }
    });
    Effect::new(move |_| {
        if let Some((sig, found)) = resolved.get() {
            if roster.with_untracked(|r| r.as_ref().is_some_and(|(s, _)| *s == sig)) {
                grudges.set(found);
            }
        }
    });
    grudges.read_only()
}
