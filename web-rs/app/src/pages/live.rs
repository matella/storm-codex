//! Live: the game in progress, read as it goes from the files HotS writes while playing (stats about 10 s behind,
//! ability presses more). Port of `web/src/pages/Live.tsx` and `components/LiveGame.tsx`.
//! Spec: docs/specs/2026-10-24-feed-live-en-partie-design.md.

use leptos::prelude::*;
use storm_web_core::{
    herodetail::decamelize,
    livegame::{
        ability_label, ability_lag_sec, current_talents, feed_text, game_clock, is_stale, me_json,
        me_key, parse_me, parse_scope, recent_bins, scope_key, seconds_since, sort_abilities,
        split_teams, team_name, Ability, LivePlayer, LiveView, Scope, BINS, SCOPES,
    },
    net,
};

use crate::{
    layout::LiveBus,
    ui::{use_interval, use_talents, Avatar},
};

fn player_name(p: &LivePlayer) -> String {
    p.hero
        .clone()
        .unwrap_or_else(|| format!("Player {}", p.player_id))
}

#[component]
pub fn Live() -> impl IntoView {
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Live game"</div>
                <h1 class="ds-title" style="cursor: default">"Live"</h1>
                <div class="ds-subtitle" style="cursor: default">
                    "The game in progress, read from the replay files HotS writes while you play — stats about 10 s behind, ability presses up to a minute or more."
                </div>
            </header>
            <section class="ds-sec" style="margin-top: 24px">
                <LiveGame />
            </section>
        </div>
    }
}

#[component]
fn LiveGame() -> impl IntoView {
    let bus = expect_context::<LiveBus>();
    let scope = RwSignal::new(parse_scope(net::storage_get(scope_key()).as_deref()));
    let manual_me = RwSignal::new(None::<i64>);
    let tick = RwSignal::new(0u32);
    // a WebSocket cut is caught up by a slow poll
    use_interval(15_000, move || tick.update(|t| *t += 1));
    let now = RwSignal::new(net::now_ms() as f64);
    use_interval(1_000, move || now.set(net::now_ms() as f64));
    let data = LocalResource::new(move || {
        bus.live.track();
        tick.track();
        let mut url = format!("/api/live?scope={}", scope.get().id());
        if let Some(me) = manual_me.get() {
            url.push_str(&format!("&me={me}"));
        }
        async move { net::get_json_opt::<LiveView>(&url).await }
    });
    // "I am" belongs to one game: reloaded when the session changes
    let session = Memo::new(move |_| data.get().and_then(Result::ok).flatten().map(|v| v.session));
    Effect::new(move |_| {
        let me = session
            .get()
            .and_then(|s| parse_me(net::storage_get(me_key()).as_deref(), &s));
        manual_me.set(me);
    });
    let set_scope = move |s: Scope| {
        scope.set(s);
        net::storage_set(scope_key(), s.id());
    };
    let set_me = move |me: Option<i64>| {
        manual_me.set(me);
        if let Some(s) = session.get_untracked() {
            match me {
                Some(m) => net::storage_set(me_key(), &me_json(&s, m)),
                None => net::storage_remove(me_key()),
            }
        }
    };
    move || {
        match data.get() {
        None => view! { <div class="ds-empty">"loading…"</div> }.into_any(),
        Some(Err(_)) => view! { <div class="ds-empty">"The live feed is unreachable — is the server running?"</div> }.into_any(),
        Some(Ok(None)) => view! {
            <div class="ds-empty ds-lv-idle">
                <b>"Waiting for a game."</b>
                <span>"The live view fills in once the uploader sends the first block of an in-progress game."</span>
            </div>
        }.into_any(),
        Some(Ok(Some(v))) => view! { <LiveBoard v=v now=now scope=scope set_scope=set_scope set_me=set_me /> }.into_any(),
    }
    }
}

#[component]
fn LiveBoard(
    v: LiveView,
    now: RwSignal<f64>,
    scope: RwSignal<Scope>,
    set_scope: impl Fn(Scope) + Copy + Send + Sync + 'static,
    set_me: impl Fn(Option<i64>) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let talents = use_talents();
    let talent_name = move |id: &str| {
        talents
            .with(|m| m.get(id).map(|t| t.name.clone()))
            .unwrap_or_else(|| decamelize(id))
    };
    let hero_of = {
        let players = v.players.clone();
        move |id: i64| {
            players
                .iter()
                .find(|p| p.player_id == id)
                .map(player_name)
                .unwrap_or_else(|| format!("Player {id}"))
        }
    };
    let (team0, team1) = split_teams(&v.players);
    let last_chunk = net::ts(&v.last_chunk_at);
    let since = move || seconds_since(last_chunk, now.get());
    let status = v.status.clone();
    let stale = {
        let status = status.clone();
        move || is_stale(&status, since())
    };
    let lag = ability_lag_sec(&v);
    let clock = game_clock(v.tracker_loop, v.gates_loop);
    let my_team = v
        .players
        .iter()
        .find(|p| Some(p.player_id) == v.me)
        .map(|p| p.team);
    let with_abilities: Vec<LivePlayer> = v
        .players
        .iter()
        .filter(|p| p.abilities.is_some())
        .cloned()
        .collect();
    let me = v.me;
    let me_source = v.me_source.clone();
    let all_players: Vec<LivePlayer> = team0.iter().chain(team1.iter()).cloned().collect();
    let feed: Vec<_> = v
        .feed
        .iter()
        .enumerate()
        .map(|(i, f)| (i, f.clone()))
        .collect();
    let has_game_bytes = v.bytes.game > 0.0;
    let game_loop = v.game_loop;
    let gates = v.gates_loop;

    let status_tag = {
        let status = status.clone();
        let stale = stale.clone();
        move || match status.as_str() {
            "ended" => view! { <span class="ds-status ready">"game over"</span> }.into_any(),
            "unreadable" => view! { <span class="ds-status stale">"unreadable"</span> }.into_any(),
            _ if stale() => view! { <span class="ds-status stale">"interrupted"</span> }.into_any(),
            _ => view! { <span class="ds-status analyzed ds-lv-pulse">"live"</span> }.into_any(),
        }
    };
    let lag_line = {
        let clock = clock.clone();
        let status = status.clone();
        move || {
            if status == "ended" {
                view! { <span>{format!("final data — game length {clock}")}</span> }.into_any()
            } else {
                view! {
                <span>{format!("stats up to {clock}")}</span>
                <span>{format!("abilities {}", lag.map_or("waiting for data".to_string(), |l| format!("{l}s behind the stats")))}</span>
                <span>{move || format!("last data {}s ago", since())}</span>
            }.into_any()
            }
        }
    };
    let stale_warn = {
        let stale = stale.clone();
        move || {
            stale().then(|| view! { <div class="ds-lv-warn">{format!("No new data for {}s — the game may be paused or disconnected.", since())}</div> })
        }
    };
    let team_level = |team: i64, level: i64| {
        let mine = my_team == Some(team);
        view! {
            <div class=format!("ds-lv-tl t{team}{}", if mine { " mine" } else { "" })>
                <span class="ds-label">{format!("{}{}", team_name(Some(team)), if mine { " · you" } else { "" })}</span>
                <strong>{if level == 0 { "—".to_string() } else { level.to_string() }}</strong>
            </div>
        }
    };
    let team_board = {
        let hero_of = hero_of.clone();
        move |label: &'static str, players: Vec<LivePlayer>| {
            let red = players.first().is_some_and(|p| p.team == 1);
            let _ = &hero_of;
            view! {
                <div class=format!("ds-dside {}", if red { "red" } else { "blue" })>
                    <div class="ds-dside-hd"><strong>{label}</strong><small class="ds-lv-sub">"takedowns / deaths"</small></div>
                    {players.into_iter().map(|p| {
                        let mine = Some(p.player_id) == me;
                        let ts = p.talents.clone();
                        let talent_view = move || {
                            if ts.is_empty() {
                                return view! { <span class="ds-none">"no talent yet"</span> }.into_any();
                            }
                            let cur = talents.with(|m| current_talents(&ts, |id| m.get(id).map(|t| t.tier)));
                            cur.into_iter().enumerate().map(|(i, (t, tier))| {
                                let title = tier.filter(|n| *n != 0).map(|n| format!("Level {n}"));
                                view! {
                                    <span class="ds-talent" title=title><b>{tier.unwrap_or(i as i64 + 1)}</b>{talent_name(&t.tree_id)}</span>
                                }
                            }).collect_view().into_any()
                        };
                        view! {
                            <div class=format!("ds-lv-prow{}", if mine { " me" } else { "" })>
                                <Avatar hero=p.hero.clone() size=34 />
                                <div class="ds-lv-pmain">
                                    <div class="ds-lv-pname">
                                        {player_name(&p)}
                                        {mine.then(|| view! { <span class="ds-tag you">"you"</span> })}
                                    </div>
                                    <div class="ds-talents">{talent_view}</div>
                                </div>
                                <div class="ds-lv-kd"><b>{p.takedowns}</b>"/"<span>{p.deaths}</span></div>
                            </div>
                        }
                    }).collect_view()}
                </div>
            }
        }
    };
    let ability_block = move |p: LivePlayer| {
        let list = sort_abilities(p.abilities.as_deref().unwrap_or(&[]));
        let total: i64 = list.iter().map(|a| a.count).sum();
        let rows = if list.is_empty() {
            view! { <div class="ds-lv-ab-empty">{if has_game_bytes { "no ability press yet" } else { "waiting for the first ability data (it lags behind the stats)" }}</div> }.into_any()
        } else {
            list.iter().map(|a: &Ability| {
                let bins = recent_bins(&a.recent, game_loop, BINS);
                let peak = bins.iter().copied().max().unwrap_or(0).max(1) as f64;
                view! {
                    <div class="ds-lv-ab-row">
                        <code>{ability_label(a)}</code>
                        <span class="ds-lv-spark" aria-label=format!("{} presses in the last 3 minutes", a.recent.len())>
                            {bins.into_iter().map(|b| view! { <i style=format!("height: {}%", if b > 0 { 20.0 + 80.0 * b as f64 / peak } else { 0.0 })></i> }).collect_view()}
                        </span>
                        <b>{a.count}</b>
                    </div>
                }
            }).collect_view().into_any()
        };
        view! {
            <div class="ds-lv-ab">
                <div class="ds-lv-ab-hd">
                    <Avatar hero=p.hero.clone() size=22 />
                    <strong>{player_name(&p)}</strong>
                    {(Some(p.player_id) == me).then(|| view! { <span class="ds-tag you">"you"</span> })}
                    <small>{format!("{total} presses")}</small>
                </div>
                {rows}
            </div>
        }
    };
    let hero_of2 = hero_of.clone();
    view! {
        <div class="ds-lv">
            <section class="ds-panel ds-lv-head">
                <div class="ds-lv-clock"><span class="ds-label">"Game clock"</span><strong>{clock.clone()}</strong></div>
                <div class="ds-lv-meta">
                    <div class="ds-lv-line">
                        {status_tag}
                        {v.map.clone().filter(|m| !m.is_empty()).map(|m| view! { <span class="ds-lv-map">{m}</span> })}
                    </div>
                    <div class="ds-lv-line ds-lv-lag">{lag_line}</div>
                    {stale_warn}
                    {(v.status == "unreadable").then(|| view! { <div class="ds-lv-warn">{format!("Live data can't be decoded ({}). A game patch may have changed the format.", v.reason.clone().unwrap_or_default())}</div> })}
                    {v.protocol.assumed_latest.then(|| view! { <div class="ds-lv-note">"Decoded with the newest known protocol (the game build isn't readable mid-game)."</div> })}
                </div>
                <div class="ds-lv-levels">
                    {team_level(0, v.team_levels[0])}
                    {team_level(1, v.team_levels[1])}
                </div>
            </section>

            <section class="ds-lv-controls">
                <div class="ds-pills" role="radiogroup" aria-label="Visibility">
                    <span class="ds-label">"Show"</span>
                    {SCOPES.iter().map(|(id, label, hint)| {
                        let id = *id;
                        view! {
                            <span role="radio" aria-checked=move || (scope.get() == id).to_string() tabindex="0" title=*hint
                                class=move || if scope.get() == id { "ds-pill on" } else { "ds-pill" }
                                on:click=move |_| set_scope(id)
                                on:keydown=move |e| if e.key() == "Enter" || e.key() == " " { set_scope(id) }>{*label}</span>
                        }
                    }).collect_view()}
                </div>
                <label class="ds-lv-me">
                    <span class="ds-label">"I am"</span>
                    <select class="ds-input" on:change=move |e| set_me(event_target_value(&e).parse::<i64>().ok())>
                        <option value="" selected=me.is_none()>{if me.is_none() { "not detected — pick" } else { "auto" }}</option>
                        {all_players.iter().map(|p| view! {
                            <option value=p.player_id.to_string() selected=Some(p.player_id) == me>{format!("{} · {}", team_name(Some(p.team)), player_name(p))}</option>
                        }).collect_view()}
                    </select>
                    {me_source.map(|s| view! { <small>{match s.as_str() { "explicit" => "picked", "lobby" => "from lobby", _ => "only human player" }}</small> })}
                </label>
            </section>
            {move || (scope.get() == Scope::All).then(|| view! {
                <div class="ds-lv-warn">"Everything includes enemy actions you can't see in game. That is a live information advantage, and how Blizzard views it is unknown."</div>
            })}

            <section class="ds-grid2 ds-lv-teams">
                {team_board(team_name(Some(0)), team0.clone())}
                {team_board(team_name(Some(1)), team1.clone())}
            </section>

            <section class="ds-grid2 ds-lv-lower">
                <div class="ds-panel flush">
                    <div class="ds-panel-hd">
                        <strong>"Ability presses"</strong>
                        <small class="ds-lv-sub">"key presses, not confirmed casts · IDs not named yet · last 3 min"</small>
                    </div>
                    {move || if with_abilities.is_empty() {
                        let msg = if me.is_none() && scope.get() != Scope::All { "Pick who you are to see your abilities." } else { "Hidden at this visibility level." };
                        view! { <div class="ds-empty-row">{msg}</div> }.into_any()
                    } else {
                        with_abilities.iter().cloned().map(ability_block).collect_view().into_any()
                    }}
                </div>
                <div class="ds-panel flush">
                    <div class="ds-panel-hd"><strong>"Game feed"</strong><small class="ds-lv-sub">"newest first"</small></div>
                    {if feed.is_empty() {
                        view! { <div class="ds-empty-row">"Nothing yet."</div> }.into_any()
                    } else {
                        view! {
                            <ol class="ds-lv-feed">
                                {feed.iter().map(|(_, f)| {
                                    let class = format!("k-{}{}", f.kind, match f.team { Some(0) => " t0", Some(1) => " t1", _ => "" });
                                    let text = {
                                        let h = hero_of2.clone();
                                        let f = f.clone();
                                        move || feed_text(&f, &h, |id| talent_name(id))
                                    };
                                    view! { <li class=class><time>{game_clock(f.at_loop, gates)}</time><span>{text}</span></li> }
                                }).collect_view()}
                            </ol>
                        }.into_any()
                    }}
                </div>
            </section>
        </div>
    }
}
