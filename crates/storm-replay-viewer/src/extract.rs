use crate::model::*;
use crate::Error;
use std::collections::{HashMap, HashSet};
use storm_replay::{Replay, Value};

const EPS: f64 = 0.004;

pub(crate) fn loop_to_sec(gameloop: i64) -> f64 {
    (gameloop as f64 - LOOP_OFFSET as f64) / LOOPS_PER_SEC
}

pub(crate) fn build(replay: &Replay) -> Result<ViewerModel, Error> {
    let tracker = replay.tracker_events()?;
    let details = replay.details()?;

    // 1) Map extent via SStatGameEvent{GameStart}.MapSizeX/Y (×4096)
    let (msx, msy) = map_size(&tracker).ok_or(Error::Missing("MapSize"))?;
    let (map_w, map_h) = (msx / FIXED, msy / FIXED); // in tiles
    let norm_tile = |x: i64, y: i64| (x as f64 / map_w, y as f64 / map_h);
    // Game coords (TargetPoint) = tiles ×4096, msx/msy also ×4096 → the 4096 factor cancels out.
    let norm_world = |x: i64, y: i64| (x as f64 / msx, y as f64 / msy);

    // 2) unit(tagIndex) → (player, isHero) from SUnitBornEvent.
    //    Key = m_unitTagIndex ALONE (not (index, recycle)): SUnitPositionsEvent references the
    //    units by index only, so we resolve the CURRENT occupant of that index — "last
    //    Born wins" gives exactly that. Safe for heroes: a hero unit never frees
    //    its index during the game (Died→Revived keeps it), so the current occupant is
    //    always the right hero.
    //    ⚠️ NOT safe as is for structures/minions (fast-follow): created/destroyed in bulk, an
    //    index can be recycled while a sibling is alive → ambiguous resolution. Before
    //    extending this pipeline to non-hero units, key by (index, recycle) with per-event
    //    resolution (via m_unitTagRecycle).
    let mut unit_player: HashMap<i64, (i64, bool)> = HashMap::new();
    for e in &tracker {
        if event_name(e) != "SUnitBornEvent" {
            continue;
        }
        let p = field_int(e, "m_controlPlayerId").unwrap_or(0);
        let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
        let is_hero = e
            .field("m_unitTypeName")
            .and_then(Value::as_str_lossy)
            .map(|n| n.starts_with("Hero"))
            .unwrap_or(false);
        // US-26: lane minions/camps are controlled by the team AI (11 = blue, 12 = red,
        // same convention as the structures) — widened beyond 1..=10 so that the resolution of
        // minions/camps (non-heroes) works in THIS SAME table (see the index recycling
        // warning above, accepted here since the downstream dedup mitigates it).
        if (1..=12).contains(&p) {
            unit_player.insert(idx, (p, is_hero));
        }
    }

    // 3) exact samples from SUnitPositionsEvent (m_items = [idxDelta, x, y]*). US-26: the SAME
    //    pass also feeds the minion/camp samples (resolved non-hero units) — no
    //    second tracker pass for this volume, already the largest event of the file.
    //    Aggressive dedup per grid cell (~1/64 tile) × time bucket (~2s): we keep only
    //    one point per (cell, bucket, team), and a hard cap `MINION_CAP` protects the payload.
    const MINION_CAP: usize = 15000;
    const MINION_TIME_BUCKET_SEC: f64 = 2.0;
    let mut samples: HashMap<i64, Vec<Sample>> = HashMap::new();
    let mut minions: Vec<MinionSample> = Vec::new();
    let mut minion_seen: HashSet<(i64, i64, i64, i64)> = HashSet::new();
    let mut minion_seen_count: u64 = 0; // non-hero units met (before dedup), for the truncation warning
    let mut minion_truncated = false;
    for e in &tracker {
        if event_name(e) != "SUnitPositionsEvent" {
            continue;
        }
        let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
        let items: Vec<i64> = e
            .field("m_items")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_int).collect())
            .unwrap_or_default();
        // The first index is not 0: m_firstUnitIndex gives the starting point of the
        // cumulative sum of the deltas (the following triplets are relative to the previous one).
        let mut idx = field_int(e, "m_firstUnitIndex").unwrap_or(0);
        for tri in items.as_chunks::<3>().0 {
            idx += tri[0]; // index cumulatif
            match unit_player.get(&idx) {
                Some(&(p, true)) => {
                    let (x, y) = norm_tile(tri[1], tri[2]);
                    samples.entry(p).or_default().push(Sample {
                        t,
                        x: q3(x),
                        y: q3(y),
                        exact: true,
                    });
                }
                Some(&(p, false)) => {
                    minion_seen_count += 1;
                    if minions.len() >= MINION_CAP {
                        minion_truncated = true;
                        continue;
                    }
                    let team = match p {
                        11 => 0,
                        12 => 1,
                        _ => -1,
                    };
                    let (x, y) = norm_tile(tri[1], tri[2]);
                    // Clamp [0,1] before q3 like heroes/deaths/structures: norm_tile can
                    // slightly overflow at the map edges (jungle units close to the limits).
                    let (qx, qy) = (q3(x.clamp(0.0, 1.0)), q3(y.clamp(0.0, 1.0)));
                    let gx = (qx * 64.0).floor() as i64;
                    let gy = (qy * 64.0).floor() as i64;
                    let tbucket = (t / MINION_TIME_BUCKET_SEC).floor() as i64;
                    if minion_seen.insert((gx, gy, team, tbucket)) {
                        minions.push(MinionSample {
                            t,
                            x: qx,
                            y: qy,
                            team,
                        });
                    }
                }
                None => {}
            }
        }
    }
    if minion_truncated {
        eprintln!("minions truncated: {}/{minion_seen_count}", minions.len());
    }
    minions.sort_by(|a, b| a.t.total_cmp(&b.t));

    // 4) densification via SCmd*/TargetPoint game events keyed by _userid → playerId.
    //    AUTHORITATIVE mapping via SPlayerSetupEvent (m_userId → m_playerId) — NOT the fragile
    //    "slot+1" assumption. `user_to_player` built once from the tracker.
    let user_to_player = user_to_player(&tracker); // m_userId → m_playerId (1..=10)
    let mut cmd_samples: HashMap<i64, Vec<Sample>> = HashMap::new();
    // US-18: ability casts — SCmdEvent with a non-zero m_abil. We do NOT identify which
    // ability (m_abilLink) — just "a cast happened"; that is why this collection lives
    // in the SAME pass as the command densification rather than a dedicated pass.
    let mut cast_times: HashMap<i64, Vec<f64>> = HashMap::new();
    // US-19: talent picks — SHeroTalentTreeSelectedEvent, one per pick, arrival order =
    // tier order (1, 2, 3…). Folded into THIS pass (no dedicated game-events pass);
    // we just store `t`, the tier (index+1) is assigned after sorting.
    let mut talent_times: HashMap<i64, Vec<f64>> = HashMap::new();
    replay.visit_game_events(|ev: Value| {
        let uid = ev
            .field("_userid")
            .and_then(|u| u.field("m_userId"))
            .and_then(Value::as_int);
        let p = uid.and_then(|u| user_to_player.get(&u).copied());

        if event_name(&ev) == "SHeroTalentTreeSelectedEvent" {
            if let Some(p) = p {
                let t = loop_to_sec(field_int(&ev, "_gameloop").unwrap_or(0));
                if t >= 0.0 {
                    talent_times.entry(p).or_default().push(t);
                }
            }
        }

        // Ability cast: m_abil present and non-zero. Checked BEFORE the early return on
        // TargetPoint below — the same SCmdEvent can carry m_abil AND m_data.TargetPoint,
        // both must be extractable from the same event.
        let is_cast = !matches!(ev.field("m_abil"), None | Some(Value::Null));
        if is_cast {
            if let Some(p) = p {
                let t = loop_to_sec(field_int(&ev, "_gameloop").unwrap_or(0));
                if t >= 0.0 {
                    cast_times.entry(p).or_default().push(t);
                }
            }
        }

        let Some(tp) = ev.field("m_data").and_then(|d| d.field("TargetPoint")) else {
            return;
        };
        let (Some(x), Some(y)) = (
            tp.field("x").and_then(Value::as_int),
            tp.field("y").and_then(Value::as_int),
        ) else {
            return;
        };
        let Some(p) = p else {
            return;
        };
        let t = loop_to_sec(field_int(&ev, "_gameloop").unwrap_or(0));
        let (nx, ny) = norm_world(x, y);
        if (0.0..=1.0).contains(&nx) && (0.0..=1.0).contains(&ny) {
            cmd_samples.entry(p).or_default().push(Sample {
                t,
                x: q3(nx),
                y: q3(ny),
                exact: false,
            });
        }
    })?;

    // Sort + time dedup of the casts: a cast within the CAST_DEDUP_SEC following the
    // previous kept cast (same player) is dropped — avoids a near-continuous flash when
    // several consecutive SCmdEvents carry m_abil for the same key press.
    const CAST_DEDUP_SEC: f64 = 0.3;
    for v in cast_times.values_mut() {
        v.sort_by(|a, b| a.total_cmp(b));
        let mut kept: Vec<f64> = Vec::with_capacity(v.len());
        for &t in v.iter() {
            if kept.last().is_none_or(|&last| t - last >= CAST_DEDUP_SEC) {
                kept.push(t);
            }
        }
        *v = kept;
    }

    // Talents: sorted by increasing t, tier = pick order (1-based). talent_id stays None (V1 —
    // reference-data-free, see the field's doc in model.rs).
    let mut talents: HashMap<i64, Vec<TalentPick>> = HashMap::new();
    for (p, mut ts) in talent_times {
        ts.sort_by(|a, b| a.total_cmp(b));
        talents.insert(
            p,
            ts.into_iter()
                .enumerate()
                .map(|(i, t)| TalentPick {
                    t,
                    tier: i as i64 + 1,
                    talent_id: None,
                })
                .collect(),
        );
    }

    // 5) merge samples (exact + command), sort by increasing t, dedup of the
    //    near-stationary points (delta < EPS since the last kept one, even exact), clamp [0,1].
    let mut player_ids: Vec<i64> = unit_player
        .values()
        .filter(|(_, is_hero)| *is_hero)
        .map(|(p, _)| *p)
        .collect();
    player_ids.sort_unstable();
    player_ids.dedup();

    let mut heroes: Vec<HeroTrack> = Vec::new();
    for p in &player_ids {
        let mut merged: Vec<Sample> = samples.remove(p).unwrap_or_default();
        merged.extend(cmd_samples.remove(p).unwrap_or_default());
        merged.sort_by(|a, b| a.t.total_cmp(&b.t));

        let mut deduped: Vec<Sample> = Vec::with_capacity(merged.len());
        for s in merged {
            let clamped = Sample {
                t: s.t,
                x: s.x.clamp(0.0, 1.0),
                y: s.y.clamp(0.0, 1.0),
                exact: s.exact,
            };
            if let Some(last) = deduped.last() {
                if last.exact == clamped.exact
                    && (last.x - clamped.x).abs() < EPS
                    && (last.y - clamped.y).abs() < EPS
                {
                    continue;
                }
            }
            deduped.push(clamped);
        }

        heroes.push(HeroTrack {
            player_id: *p,
            samples: deduped,
            life: Vec::new(), // rempli ci-dessous
            casts: cast_times.remove(p).unwrap_or_default(),
            talents: talents.remove(p).unwrap_or_default(),
        });
    }

    // 6) life intervals from Born(spawn)/Died/Revived of the player's hero unit
    let duration_sec = loop_to_sec(replay.header.elapsed_game_loops as i64);
    let mut life: HashMap<i64, Vec<Interval>> = HashMap::new();
    let mut open_from: HashMap<i64, f64> = HashMap::new();

    // We replay the tracker in order to build born→died→revived per hero unit.
    for e in &tracker {
        let name = event_name(e);
        match name.as_str() {
            "SUnitBornEvent" => {
                let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
                if let Some(&(p, true)) = unit_player.get(&idx) {
                    let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
                    open_from.entry(p).or_insert(t);
                }
            }
            "SUnitDiedEvent" => {
                let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
                if let Some(&(p, true)) = unit_player.get(&idx) {
                    let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
                    if let Some(from) = open_from.remove(&p) {
                        life.entry(p).or_default().push(Interval { from, to: t });
                    }
                }
            }
            "SUnitRevivedEvent" => {
                let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
                if let Some(&(p, true)) = unit_player.get(&idx) {
                    let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
                    open_from.entry(p).or_insert(t);
                }
            }
            _ => {}
        }
    }
    // Close each hero's last open interval at the end of the game.
    for (p, from) in open_from {
        life.entry(p).or_default().push(Interval {
            from,
            to: duration_sec,
        });
    }
    for v in life.values_mut() {
        v.sort_by(|a, b| a.from.total_cmp(&b.from));
    }

    for h in &mut heroes {
        h.life = life.remove(&h.player_id).unwrap_or_default();
    }

    // 7) deaths from the SUnitDiedEvents of the tracked hero units
    let mut deaths: Vec<Death> = Vec::new();
    for e in &tracker {
        if event_name(e) != "SUnitDiedEvent" {
            continue;
        }
        let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
        let Some(&(victim_player_id, true)) = unit_player.get(&idx) else {
            continue;
        };
        let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
        let x = field_int(e, "m_x").unwrap_or(0);
        let y = field_int(e, "m_y").unwrap_or(0);
        let (nx, ny) = norm_tile(x, y);
        let killer = field_int(e, "m_killerPlayerId");
        let killer_player_id = killer.filter(|k| (1..=10).contains(k));
        deaths.push(Death {
            t,
            x: q3(nx.clamp(0.0, 1.0)),
            y: q3(ny.clamp(0.0, 1.0)),
            victim_player_id,
            killer_player_id,
        });
    }
    deaths.sort_by(|a, b| a.t.total_cmp(&b.t));

    // 8) heroes already built in player_ids order (sorted + deduplicated) — defensive sort to
    //    guarantee the invariant on the consumer side; warnings = vec![] (US-7 later).
    heroes.sort_by_key(|h| h.player_id);

    // 9) structures: SUnitBornEvents controlled by 11 (team 0) / 12 (team 1), types
    //    Town*/KingsCore/HallOfStorms, excluding the post-destruction visual ghost
    //    ("...Destroyed"). Destruction = SUnitDiedEvent matched by (tagIndex, tagRecycle) — NOT
    //    tagIndex alone: structures can recycle an index while a sibling is alive.
    let mut structures: Vec<Structure> = Vec::new();
    let mut tag_to_structure: HashMap<(i64, i64), usize> = HashMap::new();
    for e in &tracker {
        if event_name(e) != "SUnitBornEvent" {
            continue;
        }
        let control = field_int(e, "m_controlPlayerId").unwrap_or(0);
        if control != 11 && control != 12 {
            continue;
        }
        let Some(type_name) = e.field("m_unitTypeName").and_then(Value::as_str_lossy) else {
            continue;
        };
        let is_structure = (type_name.starts_with("Town")
            || type_name.starts_with("KingsCore")
            || type_name.starts_with("HallOfStorms"))
            && !type_name.contains("Destroyed");
        if !is_structure {
            continue;
        }
        let team = if control == 11 { 0 } else { 1 };
        let kind = if type_name.contains("Core") {
            "core"
        } else if type_name.starts_with("TownTownHall") {
            "fort"
        } else if type_name.contains("Tower") {
            "tower"
        } else if type_name.starts_with("TownWall") {
            "wall"
        } else if type_name.starts_with("TownGate") {
            "gate"
        } else if type_name.starts_with("TownMoonwell") {
            "well"
        } else {
            "other"
        }
        .to_string();
        let x = field_int(e, "m_x").unwrap_or(0);
        let y = field_int(e, "m_y").unwrap_or(0);
        let (nx, ny) = norm_tile(x, y);
        let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
        let recycle = field_int(e, "m_unitTagRecycle").unwrap_or(-1);

        tag_to_structure.insert((idx, recycle), structures.len());
        structures.push(Structure {
            team,
            kind,
            x: q3(nx.clamp(0.0, 1.0)),
            y: q3(ny.clamp(0.0, 1.0)),
            destroyed_at: None,
        });
    }
    for e in &tracker {
        if event_name(e) != "SUnitDiedEvent" {
            continue;
        }
        let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
        let recycle = field_int(e, "m_unitTagRecycle").unwrap_or(-1);
        if let Some(&i) = tag_to_structure.get(&(idx, recycle)) {
            let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
            structures[i].destroyed_at = Some(t);
        }
    }
    structures.sort_by(|a, b| (a.team, &a.kind).cmp(&(b.team, &b.kind)));

    // 10) unified feed: reuses the already computed deaths/structures (no extra heavy tracker
    //     pass) + a light scan of the SStatGameEvent{JungleCampCapture} for the camps. `team` stays
    //     `None` for the takedowns — the FRONT derives it from `players[]` (keeps this crate
    //     independent of the playerId→team mapping).
    let mut events: Vec<FeedEvent> = Vec::new();
    for d in &deaths {
        events.push(FeedEvent {
            t: d.t,
            kind: "takedown".to_string(),
            team: None,
            victim_player_id: Some(d.victim_player_id),
            killer_player_id: d.killer_player_id,
            structure_kind: None,
        });
    }
    for s in &structures {
        if let Some(t) = s.destroyed_at {
            events.push(FeedEvent {
                t,
                kind: "structure".to_string(),
                team: Some(s.team),
                victim_player_id: None,
                killer_player_id: None,
                structure_kind: Some(s.kind.clone()),
            });
        }
    }
    for e in &tracker {
        if event_name(e) != "SStatGameEvent" {
            continue;
        }
        if e.field("m_eventName")
            .and_then(Value::as_str_lossy)
            .as_deref()
            != Some("JungleCampCapture")
        {
            continue;
        }
        let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
        // Capturing team = m_fixedData["TeamID"] in fixed-point (÷4096): 1 = blue, 2 = red
        // (see docs/research/hots-replay-data-reference.md). Our internal convention: 0 = blue,
        // 1 = red → team = resolved − 1. None if the field is missing.
        let team = fixed_data_int(e, "TeamID")
            .map(|v| (v as f64 / FIXED).round() as i64 - 1)
            .filter(|t| (0..=1).contains(t));
        events.push(FeedEvent {
            t,
            kind: "camp".to_string(),
            team,
            victim_player_id: None,
            killer_player_id: None,
            structure_kind: None,
        });
    }
    events.retain(|e| e.t >= 0.0);
    events.sort_by(|a, b| a.t.total_cmp(&b.t));

    // 11) levels: SStatGameEvent{LevelUp}.m_intData = [{PlayerID}, {Level}] → playerId→team
    // via the same cross-reference as player_toons (SPlayerSetupEvent.m_slotId × details.players
    // .working_set_slot_id → team_id), NOT the "1..5/6..10" assumption (true on this corpus but
    // not guaranteed by the format). ARAM = shared team XP: ~5 near-simultaneous SStatGameEvents
    // per team level-up → deduplicated into a single LevelTick (team, level) at the
    // earliest t.
    let mut slot_to_team: HashMap<i64, i64> = HashMap::new();
    for p in &details.players {
        if let Some(slot) = p.working_set_slot_id {
            slot_to_team.insert(slot, p.team_id);
        }
    }
    let mut player_team: HashMap<i64, i64> = HashMap::new();
    for e in &tracker {
        if event_name(e) != "SPlayerSetupEvent" {
            continue;
        }
        let (Some(pid), Some(slot)) = (field_int(e, "m_playerId"), field_int(e, "m_slotId")) else {
            continue;
        };
        if let Some(&team) = slot_to_team.get(&slot) {
            player_team.insert(pid, team);
        }
    }

    let mut level_first_t: HashMap<(i64, i64), f64> = HashMap::new(); // (team, level) → earliest t
    for e in &tracker {
        if event_name(e) != "SStatGameEvent" {
            continue;
        }
        if e.field("m_eventName")
            .and_then(Value::as_str_lossy)
            .as_deref()
            != Some("LevelUp")
        {
            continue;
        }
        let (Some(pid), Some(level)) = (int_data_int(e, "PlayerID"), int_data_int(e, "Level"))
        else {
            continue;
        };
        let Some(&team) = player_team.get(&pid) else {
            continue;
        };
        let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
        level_first_t
            .entry((team, level))
            .and_modify(|cur| {
                if t < *cur {
                    *cur = t;
                }
            })
            .or_insert(t);
    }
    let mut levels: Vec<LevelTick> = level_first_t
        .into_iter()
        .filter(|&((_, _), t)| t >= 0.0)
        .map(|((team, level), t)| LevelTick { t, team, level })
        .collect();
    // Deterministic sort: `level_first_t` comes from a HashMap (iteration order randomised per
    // process) — two entries at strictly equal t (same gameloop, different teams/levels)
    // must have a STABLE order from one run to the next (the golden JSON compares array order).
    // Tie-break by (team, level) after t.
    levels.sort_by(|a, b| {
        a.t.total_cmp(&b.t)
            .then(a.team.cmp(&b.team))
            .then(a.level.cmp(&b.level))
    });

    // 12) per-map objectives (US-21..24): routing + extraction isolated in `crate::maps` — this
    //     `build` function knows NO map-specific logic. Known "gap" maps →
    //     an explicit warning rather than a misleading silence.
    let map_name = details.title.clone();
    let (mut objectives, obj_warnings) = crate::maps::objectives(&map_name, &tracker);
    objectives.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(obj_warnings);

    Ok(ViewerModel {
        meta: Meta {
            map_name,
            map_size: [map_w, map_h],
            duration_sec,
            loop_offset: LOOP_OFFSET,
            viewer_version: VIEWER_VERSION,
        },
        heroes,
        deaths,
        structures,
        events,
        levels,
        objectives,
        warnings,
        minions,
    })
}

pub(crate) fn event_name(e: &Value) -> String {
    e.field("_event")
        .and_then(Value::as_str_lossy)
        .and_then(|s| s.rsplit('.').next().map(str::to_string))
        .unwrap_or_default()
}
pub(crate) fn field_int(e: &Value, k: &str) -> Option<i64> {
    e.field(k).and_then(Value::as_int)
}

// Value of an SStatGameEvent.m_fixedData by key (same key/value schema as map_size).
fn fixed_data_int(e: &Value, key: &str) -> Option<i64> {
    let fixed = e.field("m_fixedData").and_then(Value::as_array)?;
    for kv in fixed {
        if kv.field("m_key").and_then(Value::as_str_lossy).as_deref() == Some(key) {
            return kv.field("m_value").and_then(Value::as_int);
        }
    }
    None
}

// Value of an SStatGameEvent.m_intData by key (same key/value schema as fixed_data_int, but
// m_value is a raw integer — not fixed-point). Used for LevelUp (PlayerID/Level).
fn int_data_int(e: &Value, key: &str) -> Option<i64> {
    let data = e.field("m_intData").and_then(Value::as_array)?;
    for kv in data {
        if kv.field("m_key").and_then(Value::as_str_lossy).as_deref() == Some(key) {
            return kv.field("m_value").and_then(Value::as_int);
        }
    }
    None
}

fn map_size(tracker: &[Value]) -> Option<(f64, f64)> {
    for e in tracker {
        if event_name(e) != "SStatGameEvent" {
            continue;
        }
        if e.field("m_eventName")
            .and_then(Value::as_str_lossy)
            .as_deref()
            != Some("GameStart")
        {
            continue;
        }
        let fixed = e.field("m_fixedData").and_then(Value::as_array)?;
        let mut sx = None;
        let mut sy = None;
        for kv in fixed {
            let k = kv.field("m_key").and_then(Value::as_str_lossy);
            let v = kv.field("m_value").and_then(Value::as_int);
            match k.as_deref() {
                Some("MapSizeX") => sx = v,
                Some("MapSizeY") => sy = v,
                _ => {}
            }
        }
        return Some((sx? as f64, sy? as f64));
    }
    None
}

// Quantisation to 3 decimals (stabilises the golden + reduces the payload).
fn q3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

// AUTHORITATIVE: m_userId (from the game events' `_userid`) → tracker m_playerId (1..=10),
// from SPlayerSetupEvent (m_type == 1 = player). Replaces the "slot+1" assumption.
fn user_to_player(tracker: &[Value]) -> HashMap<i64, i64> {
    let mut m = HashMap::new();
    for e in tracker {
        if event_name(e) != "SPlayerSetupEvent" {
            continue;
        }
        let (Some(uid), Some(pid)) = (field_int(e, "m_userId"), field_int(e, "m_playerId")) else {
            continue;
        };
        if (1..=10).contains(&pid) {
            m.insert(uid, pid);
        }
    }
    m
}

// playerId (1..=10) → toon_handle: SPlayerSetupEvent (m_playerId → m_slotId) cross-referenced with
// details().players[*].working_set_slot_id → toon_handle. Used by the server (Chunk 2).
pub(crate) fn player_toons(replay: &Replay) -> Result<Vec<(i64, String)>, crate::Error> {
    let tracker = replay.tracker_events()?;
    let details = replay.details()?;
    let mut slot_to_toon: HashMap<i64, String> = HashMap::new();
    for p in &details.players {
        if let Some(slot) = p.working_set_slot_id {
            slot_to_toon.insert(slot, p.toon_handle.clone());
        }
    }
    let mut out = Vec::new();
    for e in &tracker {
        if event_name(e) != "SPlayerSetupEvent" {
            continue;
        }
        let (Some(pid), Some(slot)) = (field_int(e, "m_playerId"), field_int(e, "m_slotId")) else {
            continue;
        };
        if let Some(toon) = slot_to_toon.get(&slot) {
            out.push((pid, toon.clone()));
        }
    }
    out.sort_by_key(|(p, _)| *p);
    Ok(out)
}
