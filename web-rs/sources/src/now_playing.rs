//! Music sources: `/now-playing` (card, `?mini`, `?reveal`) and the music card of `/queue`. Live Spotify playback
//! through the storm-codex proxy (`/api/now-playing` -> Orpheus). Port of `NowPlaying.tsx`, `RevealCard.tsx`,
//! `useTrackReveal.ts` and `MusicCard` of `Queue.tsx`; the parsing and the reveal state machine live in
//! `storm_web_core::music` (tested natively).

use crate::frame::OverlayFrame;
use leptos::{prelude::*, task::spawn_local};
use std::time::Duration;
use storm_web_core::{
    music::{
        fmt_ms, hold_expired, next_reveal_state, parse_track, progress_pct, track_key, RevealPhase,
        RevealState, Track, HOLD_MS,
    },
    net,
    route::{query_has, Anchor},
};

const CARD_BG: &str = "linear-gradient(135deg, rgba(18,20,28,.96), rgba(12,13,18,.96))";

/// The proxy answer, read now and every `every_ms`. A failed read keeps the last answer.
pub fn use_now_playing(every_ms: u64) -> RwSignal<serde_json::Value> {
    let np = RwSignal::new(serde_json::Value::Null);
    let fetch = move || {
        spawn_local(async move {
            if let Ok(v) = net::fetch_now_playing().await {
                np.set(v);
            }
        })
    };
    fetch();
    set_interval(fetch, Duration::from_millis(every_ms));
    np
}

pub fn mount() {
    mount_to_body(NowPlaying);
}

/// Spotify widget, top right. Default: full card (cover, title/artist/album, progress, equaliser); `?mini`:
/// cover + title + artist; `?reveal`: a large announcement at each start of playback, then the `?mini` badge.
/// Hidden when nothing plays or on pause.
#[component]
fn NowPlaying() -> impl IntoView {
    let search = net::search();
    let mini = query_has(&search, "mini");
    let reveal = query_has(&search, "reveal");
    // 2 s for reveal (quick reaction to a track change); 5 s otherwise so the live OBS source is not affected
    let np = use_now_playing(if reveal { 2000 } else { 5000 });
    let track = Memo::new(move |_| np.with(parse_track));

    if reveal {
        let phase = use_track_reveal(track);
        return view! {
            <OverlayFrame anchor=Anchor::TopRight pad=36.0>
                <RevealCard track phase />
            </OverlayFrame>
        }
        .into_any();
    }

    // smooth progress: snapped to the server value at each poll, then advanced locally every 500 ms
    let prog = RwSignal::new(0.0_f64);
    let server = Memo::new(move |_| track.with(|t| (t.progress_ms, t.title.clone())));
    Effect::new(move |_| prog.set(server.get().0.unwrap_or(0.0)));
    let tick = set_interval_with_handle(
        move || {
            let (playing, dur) = track.with_untracked(|t| (t.playing, t.duration_ms));
            if let (true, Some(d)) = (playing, dur.filter(|d| *d > 0.0)) {
                prog.update(|p| *p = (*p + 500.0).min(d));
            }
        },
        Duration::from_millis(500),
    );
    on_cleanup(move || {
        if let Ok(h) = tick {
            h.clear()
        }
    });
    let playing = Memo::new(move |_| track.with(|t| t.playing));

    view! {
        <OverlayFrame anchor=Anchor::TopRight pad=36.0>
            {move || if !playing.get() {
                // hidden when nothing plays or on pause: empty frame (invisible OBS source)
                view! { <span></span> }.into_any()
            } else if mini {
                mini_card(track).into_any()
            } else {
                full_card(track, prog).into_any()
            }}
        </OverlayFrame>
    }
    .into_any()
}

/// Cover art, or a note placeholder when the source gives none.
fn cover(art: Option<String>, size: f64, radius: f64) -> impl IntoView {
    match art {
        Some(src) => view! {
            <img src=src alt="" style=format!("width:{size}px;height:{size}px;border-radius:{radius}px;object-fit:cover;flex-shrink:0;box-shadow:0 4px 14px rgba(0,0,0,.5)") />
        }
        .into_any(),
        None => view! {
            <div style=format!("width:{size}px;height:{size}px;border-radius:{radius}px;flex-shrink:0;display:flex;align-items:center;justify-content:center;background:var(--surface-2);font-size:{}px;color:var(--text-2)", size * 0.38)>"♫"</div>
        }
        .into_any(),
    }
}

const ELLIPSIS: &str = "white-space:nowrap;overflow:hidden;text-overflow:ellipsis";

fn mini_card(track: Memo<Track>) -> impl IntoView {
    view! {
        <div style=format!("display:flex;gap:12px;align-items:center;width:290px;background:{CARD_BG};border:1px solid var(--hairline-strong);border-radius:12px;padding:10px;box-shadow:0 10px 30px rgba(0,0,0,.5)")>
            {move || cover(track.with(|t| t.art.clone()), 48.0, 8.0)}
            <div style="flex:1;min-width:0">
                <div style=format!("font-size:14px;font-weight:700;{ELLIPSIS};line-height:1.25")>{move || track.with(|t| t.title.clone())}</div>
                {move || track.with(|t| t.artist.clone()).filter(|a| !a.is_empty()).map(|a| view! {
                    <div style=format!("font-size:12px;color:var(--text-2);{ELLIPSIS};margin-top:1px")>{a}</div>
                })}
            </div>
        </div>
    }
}

fn full_card(track: Memo<Track>, prog: RwSignal<f64>) -> impl IntoView {
    let eq = (0..4)
        .map(|i| {
            view! {
                <span style=format!("width:3px;background:var(--accent);border-radius:1px;animation:eq .9s ease-in-out {}s infinite", i as f64 * 0.18)></span>
            }
        })
        .collect_view();
    view! {
        <div style=format!("width:400px;background:{CARD_BG};border:1px solid var(--hairline-strong);border-radius:16px;padding:16px;display:flex;gap:16px;align-items:center;box-shadow:0 14px 40px rgba(0,0,0,.55)")>
            {move || cover(track.with(|t| t.art.clone()), 80.0, 12.0)}
            <div style="flex:1;min-width:0">
                <div style="display:flex;align-items:center;gap:8px;margin-bottom:6px">
                    <span class="kick" style="font-size:10px;color:var(--accent)">"NOW PLAYING"</span>
                    <span style="display:inline-flex;align-items:flex-end;gap:2px;height:13px">{eq}</span>
                    <span class="kick" style="font-size:9px;margin-left:auto">"ORPHEUS"</span>
                </div>
                <div style=format!("font-size:17px;font-weight:700;{ELLIPSIS};line-height:1.25")>{move || track.with(|t| t.title.clone())}</div>
                {move || track.with(|t| t.artist.clone()).filter(|a| !a.is_empty()).map(|a| view! {
                    <div style=format!("font-size:13.5px;color:var(--text-2);{ELLIPSIS};margin-top:1px")>{a}</div>
                })}
                {move || track.with(|t| t.album.clone()).filter(|a| !a.is_empty()).map(|a| view! {
                    <div style=format!("font-size:11px;color:var(--text-3, #7c8194);{ELLIPSIS};margin-top:1px")>{a}</div>
                })}
                {move || track.with(|t| t.duration_ms).filter(|d| *d > 0.0).map(|d| view! {
                    <div style="margin-top:10px">
                        <div style="height:4px;border-radius:2px;background:rgba(255,255,255,.12);overflow:hidden">
                            <div style=move || format!("width:{}%;height:100%;background:var(--accent);border-radius:2px;transition:width .5s linear", progress_pct(prog.get(), Some(d)))></div>
                        </div>
                        <div class="mono" style="display:flex;justify-content:space-between;font-size:10px;color:var(--text-2);margin-top:4px">
                            <span>{move || fmt_ms(prog.get())}</span>
                            <span>{fmt_ms(d)}</span>
                        </div>
                    </div>
                })}
            </div>
        </div>
    }
}

/// The reveal state machine driven by the polled track: applies the pure reducer on every change of
/// `(playing, key)`, and arms the hold timer whenever a big card is (re)targeted.
fn use_track_reveal(track: Memo<Track>) -> Memo<RevealPhase> {
    let state = RwSignal::new(RevealState::default());
    let input = Memo::new(move |_| track.with(|t| (t.playing, track_key(t))));
    Effect::new(move |_| {
        let (playing, key) = input.get();
        let cur = state.get_untracked();
        let next = next_reveal_state(&cur, playing, key.as_deref());
        if next != cur {
            state.set(next);
        }
    });
    let hold = Memo::new(move |_| state.with(|s| (s.phase == RevealPhase::Big, s.hold_token)));
    Effect::new(move |_| {
        if !hold.get().0 {
            return;
        }
        let h = set_timeout_with_handle(
            move || {
                let cur = state.get_untracked();
                let next = hold_expired(&cur);
                if next != cur {
                    state.set(next);
                }
            },
            Duration::from_millis(HOLD_MS as u64),
        );
        on_cleanup(move || {
            if let Ok(h) = h {
                h.clear()
            }
        });
    });
    Memo::new(move |_| state.with(|s| s.phase))
}

/// One box that animates between the large announcement and the compact badge, the two content layers
/// cross-fading. Both layers are always mounted (that is what allows the fade); the state class on the root
/// decides which one shows.
#[component]
fn RevealCard(track: Memo<Track>, phase: Memo<RevealPhase>) -> impl IntoView {
    let art = move || {
        track.with(|t| t.art.clone()).map_or_else(
            || view! { <div class="npr-art npr-none">"♫"</div> }.into_any(),
            |a| {
                view! { <div class="npr-art" style=format!("background-image:url({a})")></div> }
                    .into_any()
            },
        )
    };
    let title = move || track.with(|t| t.title.clone());
    let artist = move || track.with(|t| t.artist.clone());
    view! {
        <div class=move || format!("npr {}", phase.get().class())>
            <div class="npr-card">
                <div class="npr-layer npr-big">
                    <div class="npr-krow">
                        <span class="npr-kick">"Now playing"</span>
                        <span class="npr-eq"><i></i><i></i><i></i><i></i></span>
                    </div>
                    {art}
                    <div class="npr-title">{title}</div>
                    <div class="npr-artist">{artist}</div>
                </div>
                <div class="npr-layer npr-mini">
                    {art}
                    <div class="npr-txt">
                        <div class="npr-title">{title}</div>
                        <div class="npr-artist">{artist}</div>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Built-in "now playing" of the queue panel, so that scene is a single self-contained source.
#[component]
pub fn MusicCard(t: Track, authenticated: bool) -> impl IntoView {
    let icon = match (t.playing, t.art.clone()) {
        (true, Some(src)) => view! { <img src=src alt="" style="width:36px;height:36px;border-radius:6px;object-fit:cover" /> }.into_any(),
        (playing, _) => view! { <span style="font-size:18px">{if playing { "♪" } else { "♫" }}</span> }.into_any(),
    };
    let body = if t.playing {
        view! {
            <div style=format!("font-size:14px;font-weight:600;{ELLIPSIS}")>{t.title.clone()}</div>
            {t.artist.clone().filter(|a| !a.is_empty()).map(|a| view! { <div style=format!("font-size:12px;color:var(--text-2);{ELLIPSIS}")>{a}</div> })}
        }
        .into_any()
    } else {
        view! { <div style="font-size:12.5px;color:var(--text-2)">{if authenticated { "Nothing playing" } else { "Music — off" }}</div> }.into_any()
    };
    view! {
        <div style="background:rgba(14,16,22,.92);border:1px solid var(--hairline-strong);border-radius:12px;padding:10px 14px;display:flex;align-items:center;gap:11px">
            {icon}
            <div style="flex:1;min-width:0">{body}</div>
            <span class="kick" style="font-size:9px">"ORPHEUS"</span>
        </div>
    }
}
