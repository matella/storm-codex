//! Map objectives: stat events, unit births/deaths/owner changes,
//! final cleanup and first objective.

use super::*;

/// `var` variables hoisted from the per-map objective branches (parser.js:705-784). `None` ≡
/// JS undefined (and the initial `null` of `dragon`: same observable effects) — a property
/// access on it → Throw (catch → Failure); a direct assignment (`dragon = {...}`)
/// works on any map, as in JS (the UnitBorn branches are not guarded per map).
#[derive(Default)]
pub(super) struct ObjState {
    pub(super) moon: Option<J>,
    pub(super) sun: Option<J>,
    pub(super) dragon: Option<J>,
    pub(super) current_terror: Option<J>,
    /// JS array `[null, null]`: object `{"0","1"}` + numeric for..in iteration.
    pub(super) golems: Option<J>,
    pub(super) immortal: Option<J>,
    pub(super) current_spiders: Option<Spiders>,
    pub(super) current_protector: Option<J>,
    pub(super) nukes: Option<J>,
    pub(super) wave_units: Option<J>,
    /// `var waveID`: None ≡ undefined (`waveID += 1` → NaN, index → undefined).
    pub(super) wave_id: Option<f64>,
    pub(super) beacons: Option<J>,
}

/// `currentSpiders` (Crypts, parser.js:746) — never serialised: scalar fields carried in
/// natif, `None` ≡ undefined.
#[derive(Default)]
pub(super) struct Spiders {
    pub(super) units: Map<String, J>,
    pub(super) active: bool,
    pub(super) team: Option<f64>,
    pub(super) event_idx: Option<f64>,
    pub(super) unit_idx: Option<f64>,
    pub(super) max_duration: Option<f64>,
}

/// JS `===` on our values (scalars only; `None` ≡ undefined ≠ null).
pub(super) fn js_strict_eq(a: Option<&J>, b: Option<&J>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(J::Null), Some(J::Null)) => true,
        (Some(J::Number(x)), Some(J::Number(y))) => {
            x.as_f64().unwrap_or(f64::NAN) == y.as_f64().unwrap_or(f64::NAN)
        }
        (Some(J::String(x)), Some(J::String(y))) => x == y,
        (Some(J::Bool(x)), Some(J::Bool(y))) => x == y,
        _ => false,
    }
}

/// JS property key from an optional number (undefined → "undefined").
pub(super) fn js_key(x: Option<f64>) -> String {
    x.map_or_else(|| "undefined".into(), js_num_str)
}

/// JS array index from an optional number (non-integer/negative/NaN → undefined).
pub(super) fn js_idx(x: Option<f64>) -> Option<usize> {
    let x = x?;
    (x.is_finite() && x >= 0.0 && x.fract() == 0.0).then_some(x as usize)
}

/// `match.map === ReplayTypes.MapType.<k>`.
pub(super) fn is_map(match_: &Map<String, J>, k: &str) -> bool {
    match_.get("map").and_then(J::as_str) == Some(rt_str("MapType", k))
}

/// `match.objective` (objet) — absent/non-objet → Throw.
pub(super) fn objective_mut(match_: &mut Map<String, J>) -> R<&mut Map<String, J>> {
    match_
        .get_mut("objective")
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw("match.objective absent".into()))
}

/// `match.objective[team]` object — missing key or non-object (Mines array…) → Throw,
/// like `undefined.events` in JS.
pub(super) fn obj_slot_mut<'a>(
    match_: &'a mut Map<String, J>,
    team: &str,
) -> R<&'a mut Map<String, J>> {
    objective_mut(match_)?
        .get_mut(team)
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw(format!("objective[{team}] absent")))
}

/// `slot.events` tableau — absent → Throw (`undefined.push`).
pub(super) fn slot_events_mut(slot: &mut Map<String, J>) -> R<&mut Vec<J>> {
    slot.get_mut("events")
        .and_then(J::as_array_mut)
        .ok_or_else(|| Abort::Throw("objective.events absent".into()))
}

/// `match.objective.<key>` tableau (tributes, results, warheads, waves…) — absent → Throw.
pub(super) fn obj_arr_mut<'a>(match_: &'a mut Map<String, J>, key: &str) -> R<&'a mut Vec<J>> {
    objective_mut(match_)?
        .get_mut(key)
        .and_then(J::as_array_mut)
        .ok_or_else(|| Abort::Throw(format!("objective.{key} absent")))
}

/// `slot.<key> += by` (missing key → undefined → NaN, as in JS).
pub(super) fn js_incr(slot: &mut Map<String, J>, key: &str, by: f64) {
    let v = js_number(slot.get(key)) + by;
    slot.insert(key.into(), jnum(v));
}

/// `match.objective.waves[waveID]` — out of bounds/NaN/undefined → Throw (`undefined.x`).
pub(super) fn wave_mut(
    match_: &mut Map<String, J>,
    wave_id: Option<f64>,
) -> R<&mut Map<String, J>> {
    let waves = obj_arr_mut(match_, "waves")?;
    js_idx(wave_id)
        .and_then(|i| waves.get_mut(i))
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw("waves[waveID] absent".into()))
}

/// `match.version.m_build` (nombre JS — absent → NaN).
pub(super) fn match_build(match_: &Map<String, J>) -> f64 {
    js_number(match_.get("version").and_then(|v| get(v, &["m_build"])))
}

/// `braxisWaveStrength(units, build)` (parser.js:2594-2657). The keys and values of
/// BraxisUnitType are identical: the init to 0 and the increment by `units[u].type` coincide.
pub(super) fn braxis_wave_strength(units: &Map<String, J>, build: f64) -> f64 {
    let mut types: HashMap<String, f64> = HashMap::new();
    if let Some(o) = replay_types()["BraxisUnitType"].as_object() {
        for k in o.keys() {
            types.insert(k.clone(), 0.0);
        }
    }
    for u in units.values() {
        // type outside the table → undefined + 1 = NaN, as in JS
        *types.entry(js_prop(get(u, &["type"]))).or_insert(f64::NAN) += 1.0;
    }
    let n = |k: &str| {
        types
            .get(rt_str("BraxisUnitType", k))
            .copied()
            .unwrap_or(f64::NAN)
    };
    if build < 66488.0 {
        let score = 0.05 * (n("ZergZergling") - 6.0) + n("ZergBaneling") * 0.05;
        let score = js_max(score, n("ZergHydralisk") * 0.14);
        js_max(score, n("ZergGuardian") * 0.3)
    } else if build < 75589.0 {
        let score = 0.1 * n("ZergBaneling");
        let score = js_max(score, 0.25 * (n("ZergHydralisk") - 2.0));
        js_max(score, 0.35 * (n("ZergGuardian") - 1.0))
    } else {
        // 2.47.0 — "only cause Ultralisks to be ignored" (a NaN build falls here, as in JS)
        let score = 0.1 * n("ZergBaneling");
        let score = js_max(score, 0.24 * n("ZergHydralisk"));
        let score = js_max(score, 0.30 * n("ZergGuardian"));
        js_max(score, 0.45 * (n("ZergUltralisk") - 3.0))
    }
}

/// Strengths of the two Braxis waves (`waveUnits[0]`, `waveUnits[1]`) — undefined waveUnits → Throw.
pub(super) fn braxis_strengths(st: &ObjState, build: f64) -> R<(f64, f64)> {
    let wu = st
        .wave_units
        .as_ref()
        .and_then(J::as_object)
        .ok_or_else(|| Abort::Throw("waveUnits undefined".into()))?;
    let side = |k: &str| {
        wu.get(k)
            .and_then(J::as_object)
            .ok_or_else(|| Abort::Throw(format!("waveUnits[{k}] absent")))
    };
    Ok((
        braxis_wave_strength(side("0")?, build),
        braxis_wave_strength(side("1")?, build),
    ))
}

/// "Per-map objective" stat events (parser.js:974-1189, 1214-1238).
pub(super) fn obj_stat_event(
    name: &str,
    event: &J,
    match_: &mut Map<String, J>,
    st: &mut ObjState,
    loop_game_start: f64,
) -> R<()> {
    if objective_less_map(match_) {
        return Ok(()); // extended ARAM maps: no objective (see EXTRA_MAPS)
    }
    let s = |k: &str| rt_str("StatEventType", k);
    let gameloop_j = get(event, &["_gameloop"]).cloned().unwrap_or(J::Null);
    let gameloop = js_number(get(event, &["_gameloop"]));
    let time = jnum(loops_to_seconds(gameloop - loop_game_start));
    // `event.m_intData[i].m_value`: [i] missing → Throw, m_value missing → undefined
    let idata = |i: &str| -> R<Option<&J>> {
        let d = get(event, &["m_intData", i])
            .ok_or_else(|| Abort::Throw(format!("objectif: m_intData[{i}] absent")))?;
        Ok(get(d, &["m_value"]))
    };
    let fdata = |i: &str| -> R<Option<&J>> {
        let d = get(event, &["m_fixedData", i])
            .ok_or_else(|| Abort::Throw(format!("objectif: m_fixedData[{i}] absent")))?;
        Ok(get(d, &["m_value"]))
    };

    if name == s("SkyTempleShotsFired") {
        // parser.js:974-992
        let team = js_number(idata("2")?) - 1.0;
        let damage = js_number(fdata("0")?) / 4096.0;
        if team == 0.0 || team == 1.0 {
            let slot = obj_slot_mut(match_, &js_num_str(team))?;
            slot_events_mut(slot)?.push(json!({
                "team": jnum(team), "loop": gameloop_j, "damage": jnum(damage), "time": time,
            }));
            js_incr(slot, "damage", damage);
            js_incr(slot, "count", 1.0);
        }
    } else if name == s("AltarCaptured") {
        // parser.js:993-1010
        let team = js_number(idata("0")?) - 1.0;
        let owned = idata("1")?.cloned().unwrap_or(J::Null);
        let damage = js_number(Some(&owned)) + 1.0;
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(json!({
            "team": jnum(team), "loop": gameloop_j, "owned": owned,
            "damage": jnum(damage), "time": time,
        }));
        js_incr(slot, "damage", damage);
        js_incr(slot, "count", 1.0);
    } else if name == s("ImmortalDefeated") {
        // parser.js:1011-1024
        let winner = js_number(idata("1")?) - 1.0;
        let duration = idata("2")?.cloned().unwrap_or(J::Null);
        let power = js_number(fdata("0")?) / 4096.0;
        obj_arr_mut(match_, "results")?.push(json!({
            "winner": jnum(winner), "loop": gameloop_j, "duration": duration,
            "time": time, "power": jnum(power),
        }));
    } else if name == s("TributeCollected") {
        // parser.js:1025-1037
        let team = js_number(fdata("0")?) / 4096.0 - 1.0;
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(json!({
            "team": jnum(team), "loop": gameloop_j, "time": time,
        }));
        js_incr(slot, "count", 1.0);
    } else if name == s("DragonKnightActivated") {
        // parser.js:1038-1053
        let team = js_number(fdata("0")?) / 4096.0 - 1.0;
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(json!({
            "team": jnum(team), "loop": gameloop_j, "time": time,
        }));
        js_incr(slot, "count", 1.0);
        st.dragon
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("dragon null/undefined".into()))?
            .insert("team".into(), jnum(team));
    } else if name == s("GardenTerrorActivated") {
        // parser.js:1054-1069
        let team = js_number(fdata("1")?) / 4096.0 - 1.0;
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(json!({
            "team": jnum(team), "loop": gameloop_j, "time": time,
        }));
        js_incr(slot, "count", 1.0);
        st.current_terror
            .as_mut()
            .and_then(J::as_object_mut)
            .and_then(|ct| ct.get_mut(&js_num_str(team)))
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("currentTerror[team] absent".into()))?
            .insert("active".into(), J::from(true));
    } else if name == s("ShrineCaptured") {
        // parser.js:1070-1089
        let team = js_number(idata("1")?) - 1.0;
        let t0 = if team == 0.0 {
            idata("2")?
        } else {
            idata("3")?
        }
        .cloned()
        .unwrap_or(J::Null);
        let t1 = if team == 1.0 {
            idata("2")?
        } else {
            idata("3")?
        }
        .cloned()
        .unwrap_or(J::Null);
        obj_arr_mut(match_, "shrines")?.push(json!({
            "team": jnum(team), "loop": gameloop_j, "time": time,
            "team0Score": t0, "team1Score": t1,
        }));
    } else if name == s("PunisherKilled") {
        // parser.js:1090-1106
        let team = js_number(idata("1")?) - 1.0;
        let ptype = get(event, &["m_stringData", "0"])
            .ok_or_else(|| Abort::Throw("punisher: m_stringData[0] absent".into()))?;
        let ptype = get(ptype, &["m_value"]).cloned().unwrap_or(J::Null);
        let duration = idata("2")?.cloned().unwrap_or(J::Null);
        let siege = js_number(fdata("0")?) / 4096.0;
        let hero = js_number(fdata("1")?) / 4096.0;
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(json!({
            "team": jnum(team), "loop": gameloop_j, "type": ptype, "time": time,
            "duration": duration, "siegeDamage": jnum(siege), "heroDamage": jnum(hero),
        }));
        js_incr(slot, "count", 1.0);
    } else if name == s("SpidersSpawned") {
        // parser.js:1107-1126
        let team = js_number(fdata("0")?) / 4096.0 - 1.0;
        let score = idata("1")?.cloned().unwrap_or(J::Null);
        {
            let cs = st
                .current_spiders
                .as_mut()
                .ok_or_else(|| Abort::Throw("currentSpiders undefined".into()))?;
            cs.active = true;
            cs.team = Some(team);
        }
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(json!({
            "team": jnum(team), "score": score, "loop": gameloop_j, "time": time,
        }));
        js_incr(slot, "count", 1.0);
        let count = js_number(slot.get("count"));
        if let Some(cs) = st.current_spiders.as_mut() {
            cs.event_idx = Some(count - 1.0);
            cs.unit_idx = Some(0.0);
        }
    } else if name == s("SixTowersStart") || name == s("SixTowersEnd") {
        // parser.js:1159-1180
        let team = js_number(idata("0")?) - 1.0;
        let kind = if name == s("SixTowersStart") {
            "capture"
        } else {
            "end"
        };
        obj_arr_mut(match_, "sixTowerEvents")?.push(json!({
            "loop": gameloop_j, "team": jnum(team), "kind": kind, "time": time,
        }));
    } else if name == s("TowersFortCaptured") {
        // parser.js:1181-1189
        let owned_by = js_number(idata("0")?) - 11.0;
        obj_arr_mut(match_, "structures")?.push(json!({
            "loop": gameloop_j, "ownedBy": jnum(owned_by), "time": time,
        }));
    } else if name == s("BraxisWaveStart") {
        // parser.js:1214-1225
        let s0 = js_number(fdata("0")?) / 4096.0;
        let s1 = js_number(fdata("1")?) / 4096.0;
        let wave = wave_mut(match_, st.wave_id)?;
        wave.insert("startLoop".into(), gameloop_j);
        wave.insert("startTime".into(), time);
        wave.insert("startScore".into(), json!({"0": jnum(s0), "1": jnum(s1)}));
    } else if name == s("GhostShipCaptured") {
        // parser.js:1226-1238
        let team = js_number(fdata("0")?) / 4096.0 - 1.0;
        let team_score = idata("0")?.cloned().unwrap_or(J::Null);
        let other_score = idata("1")?.cloned().unwrap_or(J::Null);
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(json!({
            "loop": gameloop_j, "time": time, "team": jnum(team),
            "teamScore": team_score, "otherScore": other_score,
        }));
        js_incr(slot, "count", 1.0);
    }
    Ok(())
}

/// Per-map branches of UnitBorn (parser.js:1262-1475) — tested by unit type, without
/// a guard on the map (reproduced as is).
pub(super) fn obj_unit_born(
    event: &J,
    match_: &mut Map<String, J>,
    st: &mut ObjState,
    players: &Map<String, J>,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
) -> R<()> {
    if objective_less_map(match_) {
        return Ok(()); // extended ARAM maps: no objective (see EXTRA_MAPS)
    }
    let u = |k: &str| rt_str("UnitType", k);
    let t = get_str(event, &["m_unitTypeName"]).unwrap_or("");
    let gameloop_j = get(event, &["_gameloop"]).cloned().unwrap_or(J::Null);
    let gameloop = js_number(get(event, &["_gameloop"]));
    let secs = loops_to_seconds(gameloop - loop_game_start);
    let tag_j = get(event, &["m_unitTagIndex"]).cloned().unwrap_or(J::Null);
    let rtag_j = get(event, &["m_unitTagRecycle"])
        .cloned()
        .unwrap_or(J::Null);
    let x_j = get(event, &["m_x"]).cloned().unwrap_or(J::Null);
    let y_j = get(event, &["m_y"]).cloned().unwrap_or(J::Null);

    if t == u("MinesBoss") {
        // parser.js:1262-1271
        let team = js_number(get(event, &["m_controlPlayerId"])) - 11.0;
        let spawn = json!({
            "loop": gameloop_j, "team": jnum(team), "time": jnum(secs),
            "unitTagIndex": tag_j, "unitTagRecycle": rtag_j,
        });
        st.golems
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("golems undefined".into()))?
            .insert(js_num_str(team), spawn);
    } else if t == u("RavenLordTribute") {
        // parser.js:1272-1278
        obj_arr_mut(match_, "tributes")?.push(json!({
            "loop": gameloop_j, "x": x_j, "y": y_j, "time": jnum(secs),
        }));
    } else if t == u("MoonShrine") {
        st.moon = Some(json!({"tag": tag_j, "rtag": rtag_j}));
    } else if t == u("SunShrine") {
        st.sun = Some(json!({"tag": tag_j, "rtag": rtag_j}));
    } else if t == u("GardenTerrorVehicle") {
        // parser.js:1283-1291
        let team = js_number(get(event, &["m_upkeepPlayerId"])) - 11.0;
        let spawn = json!({"team": jnum(team), "active": false, "tag": tag_j, "rtag": rtag_j});
        st.current_terror
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("currentTerror undefined".into()))?
            .insert(js_num_str(team), spawn);
    } else if t == u("GardenTerror") {
        // parser.js:1292-1301
        let team = js_number(get(event, &["m_upkeepPlayerId"])) - 11.0;
        let unit = json!({
            "team": jnum(team), "tag": tag_j, "rtag": rtag_j,
            "time": jnum(secs), "loop": gameloop_j,
        });
        obj_slot_mut(match_, &js_num_str(team))?
            .get_mut("units")
            .and_then(J::as_array_mut)
            .ok_or_else(|| Abort::Throw("objective.units absent".into()))?
            .push(unit);
    } else if t == u("DragonVehicle") {
        st.dragon = Some(json!({"tag": tag_j, "rtag": rtag_j}));
    } else if t == u("Webweaver") {
        // parser.js:1305-1317
        let spider = json!({
            "tag": tag_j, "rtag": rtag_j, "x": x_j, "y": y_j,
            "loop": gameloop_j, "time": jnum(secs),
        });
        let cs = st
            .current_spiders
            .as_mut()
            .ok_or_else(|| Abort::Throw("currentSpiders undefined".into()))?;
        cs.units.insert(js_key(cs.unit_idx), spider);
        cs.unit_idx = Some(cs.unit_idx.unwrap_or(f64::NAN) + 1.0);
    } else if t == u("Triglav") {
        // parser.js:1318-1341 — currentProtector is ALSO the object pushed into events:
        // the eventIdx set after the push is visible in the output.
        let team = js_number(get(event, &["m_upkeepPlayerId"])) - 11.0;
        let mut cp = Map::new();
        cp.insert("tag".into(), tag_j);
        cp.insert("rtag".into(), rtag_j);
        cp.insert("team".into(), jnum(team));
        cp.insert("loop".into(), gameloop_j);
        cp.insert("x".into(), x_j);
        cp.insert("y".into(), y_j);
        cp.insert("time".into(), jnum(secs));
        cp.insert("active".into(), J::from(true));
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(J::Object(cp.clone()));
        js_incr(slot, "count", 1.0);
        let event_idx = js_number(slot.get("count")) - 1.0;
        slot_events_mut(slot)?
            .last_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("events vide".into()))?
            .insert("eventIdx".into(), jnum(event_idx));
        cp.insert("eventIdx".into(), jnum(event_idx));
        st.current_protector = Some(J::Object(cp));
    } else if t == u("Nuke") {
        // parser.js:1342-1358
        let key = js_prop(get(event, &["m_controlPlayerId"]));
        let player = player_id_map.get(&key).cloned();
        let team = if js_truthy(player.as_ref()) {
            let h = player.as_ref().and_then(J::as_str).unwrap_or_default();
            get(
                players
                    .get(h)
                    .ok_or_else(|| Abort::Throw(format!("nuke: unknown player {h}")))?,
                &["team"],
            )
            .cloned()
            .unwrap_or(J::Null)
        } else {
            jnum(js_number(get(event, &["m_upkeepPlayerId"])) - 11.0)
        };
        let mut e = Map::new();
        e.insert("tag".into(), tag_j);
        e.insert("rtag".into(), rtag_j);
        e.insert("loop".into(), gameloop_j);
        e.insert("x".into(), x_j);
        e.insert("y".into(), y_j);
        e.insert("time".into(), jnum(secs));
        if let Some(p) = player {
            e.insert("player".into(), p);
        }
        e.insert("team".into(), team);
        st.nukes
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("nukes undefined".into()))?
            .insert(unit_uid(event), J::Object(e));
    } else if replay_types()["BraxisUnitType"]
        .as_object()
        .is_some_and(|o| o.contains_key(t))
    {
        // parser.js:1359-1405
        let team = js_number(get(event, &["m_controlPlayerId"])) - 11.0;
        let e = json!({
            "tag": tag_j, "rtag": rtag_j, "loop": gameloop_j.clone(),
            "x": x_j, "y": y_j, "team": jnum(team), "time": jnum(secs),
            "type": get(event, &["m_unitTypeName"]).cloned().unwrap_or(J::Null),
        });
        let build = match_build(match_);
        let (s0, s1) = braxis_strengths(st, build)?;
        let both_empty = {
            let wu = st
                .wave_units
                .as_ref()
                .and_then(J::as_object)
                .ok_or_else(|| Abort::Throw("waveUnits undefined".into()))?;
            let empty = |k: &str| -> R<bool> {
                Ok(wu
                    .get(k)
                    .and_then(J::as_object)
                    .ok_or_else(|| Abort::Throw(format!("waveUnits[{k}] absent")))?
                    .is_empty())
            };
            empty("0")? && empty("1")?
        };
        if both_empty {
            st.wave_id = Some(st.wave_id.unwrap_or(f64::NAN) + 1.0);
            obj_arr_mut(match_, "waves")?.push(json!({
                "initLoop": gameloop_j.clone(), "initTime": jnum(secs),
                "scores": [{"0": 0, "1": 0, "loop": gameloop_j.clone(), "time": jnum(secs)}],
                "endLoop": {"0": 0, "1": 0}, "endTime": {"0": 0, "1": 0},
            }));
        } else {
            wave_mut(match_, st.wave_id)?
                .get_mut("scores")
                .and_then(J::as_array_mut)
                .ok_or_else(|| Abort::Throw("wave.scores absent".into()))?
                .push(json!({
                    "0": jnum(s0), "1": jnum(s1),
                    "loop": gameloop_j.clone(), "time": jnum(secs),
                }));
        }
        st.wave_units
            .as_mut()
            .and_then(J::as_object_mut)
            .and_then(|wu| wu.get_mut(&js_num_str(team)))
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("waveUnits[team] absent".into()))?
            .insert(unit_uid(event), e);
    } else if t == u("BraxisZergPath") {
        // parser.js:1406-1418
        let start_falsy = !js_truthy(wave_mut(match_, st.wave_id)?.get("startLoop"));
        if start_falsy {
            let build = match_build(match_);
            let (s0, s1) = braxis_strengths(st, build)?;
            let wave = wave_mut(match_, st.wave_id)?;
            wave.insert("startLoop".into(), gameloop_j);
            wave.insert("startTime".into(), jnum(secs));
            wave.insert("startScore".into(), json!({"0": jnum(s0), "1": jnum(s1)}));
        }
    } else if t == u("BraxisControlPoint") {
        // parser.js:1419-1426
        let side = if js_number(get(event, &["m_y"])) > 100.0 {
            "top"
        } else {
            "bottom"
        };
        st.beacons
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("beacons undefined".into()))?
            .insert(
                unit_uid(event),
                json!({"tag": tag_j, "rtag": rtag_j, "side": side}),
            );
    } else if t == u("ImmortalHeaven") || t == u("ImmortalHell") {
        // parser.js:1427-1433
        let im = st
            .immortal
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("immortal undefined".into()))?;
        im.insert("start".into(), gameloop_j);
        im.insert("tag".into(), tag_j);
        im.insert("rtag".into(), rtag_j);
    } else if t == u("WarheadSpawn") || t == u("WarheadDropped") {
        // parser.js:1434-1451
        let kind = if t == u("WarheadSpawn") {
            "spawn"
        } else {
            "dropped"
        };
        obj_arr_mut(match_, "warheads")?.push(json!({
            "loop": gameloop_j, "type": kind, "x": x_j, "y": y_j, "time": jnum(secs),
        }));
    } else if t == u("AllianceCavalry") || t == u("HordeCavalry") {
        // parser.js:1452-1461
        let team = js_number(get(event, &["m_controlPlayerId"])) - 11.0;
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(json!({
            "loop": gameloop_j, "born": jnum(secs), "id": unit_uid(event),
        }));
    } else if t == u("NeutralPayload") {
        // parser.js:1462-1475
        obj_arr_mut(match_, "events")?.push(json!({
            "loop": gameloop_j.clone(), "born": jnum(secs), "id": unit_uid(event),
            "control": [{"team": -1, "loop": gameloop_j, "time": jnum(secs)}],
        }));
    }
    Ok(())
}

/// Per-map branches of UnitDied (parser.js:1655-1896) — guarded by `match.map`.
pub(super) fn obj_unit_died(
    event: &J,
    match_: &mut Map<String, J>,
    st: &mut ObjState,
    loop_game_start: f64,
) -> R<()> {
    if objective_less_map(match_) {
        return Ok(()); // extended ARAM maps: no objective (see EXTRA_MAPS)
    }
    let tag = get(event, &["m_unitTagIndex"]);
    let rtag = get(event, &["m_unitTagRecycle"]);
    let uid = unit_uid(event);
    let gameloop_j = get(event, &["_gameloop"]).cloned().unwrap_or(J::Null);
    let gameloop = js_number(get(event, &["_gameloop"]));
    let secs = loops_to_seconds(gameloop - loop_game_start);

    if is_map(match_, "HauntedMines") {
        // parser.js:1656-1687 — death of a tracked golem
        let golems = st
            .golems
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("golems undefined".into()))?;
        for key in js_for_in_keys(golems) {
            let golem = golems.get(&key).cloned().unwrap_or(J::Null);
            if js_truthy(Some(&golem))
                && js_strict_eq(get(&golem, &["unitTagIndex"]), tag)
                && js_strict_eq(get(&golem, &["unitTagRecycle"]), rtag)
            {
                let start_time = js_number(get(&golem, &["time"]));
                let obj_event = json!({
                    "startLoop": get(&golem, &["loop"]).cloned().unwrap_or(J::Null),
                    "startTime": get(&golem, &["time"]).cloned().unwrap_or(J::Null),
                    "endLoop": gameloop_j.clone(),
                    "endTime": jnum(secs),
                    "duration": jnum(secs - start_time),
                    "team": get(&golem, &["team"]).cloned().unwrap_or(J::Null),
                });
                let team_key = js_prop(get(&golem, &["team"]));
                golems.insert(key, J::Null);
                // match.objective[team] is an ARRAY here (Mines init)
                objective_mut(match_)?
                    .get_mut(&team_key)
                    .and_then(J::as_array_mut)
                    .ok_or_else(|| Abort::Throw(format!("objective[{team_key}] absent")))?
                    .push(obj_event);
            }
        }
    } else if is_map(match_, "HauntedWoods") {
        // parser.js:1688-1720 — death of the activated plant
        let ct = st
            .current_terror
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("currentTerror undefined".into()))?;
        for tkey in js_for_in_keys(ct) {
            let terror = ct.get(&tkey).cloned().unwrap_or(J::Null);
            if js_truthy(get(&terror, &["active"]))
                && js_strict_eq(get(&terror, &["tag"]), tag)
                && js_strict_eq(get(&terror, &["rtag"]), rtag)
            {
                // team = parseInt(t) → property key String(number)
                let team_key = js_parse_int(&tkey).map_or_else(|| "NaN".into(), |v| v.to_string());
                let slot = obj_slot_mut(match_, &team_key)?;
                let last = slot_events_mut(slot)?
                    .last_mut()
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw("events vide".into()))?;
                let duration = gameloop - js_number(last.get("loop"));
                last.insert("loopDuration".into(), jnum(duration));
                last.insert("duration".into(), jnum(loops_to_seconds(duration)));
                match get(&terror, &["player"]) {
                    Some(p) => last.insert("player".into(), p.clone()),
                    None => last.remove("player"), // = undefined → missing on stringify
                };
                ct.get_mut(&tkey)
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw("currentTerror[t] absent".into()))?
                    .insert("active".into(), J::from(false));
            }
        }
        // parser.js:1722-1734 — death of a terror (units); iterates ALL the keys of
        // objective, including "type" (string → .units undefined → empty loop)
        let obj = objective_mut(match_)?;
        for t in js_for_in_keys(obj) {
            let Some(units) = obj
                .get_mut(&t)
                .and_then(J::as_object_mut)
                .and_then(|o| o.get_mut("units"))
                .and_then(J::as_array_mut)
            else {
                continue;
            };
            for unit in units.iter_mut() {
                let tid = format!(
                    "{}-{}",
                    js_prop(get(unit, &["tag"])),
                    js_prop(get(unit, &["rtag"]))
                );
                if tid == uid {
                    let uo = unit
                        .as_object_mut()
                        .ok_or_else(|| Abort::Throw("unit non-objet".into()))?;
                    uo.insert("end".into(), jnum(secs));
                    // BUG parser.js:1731 reproduced: u.start does not exist → NaN → null
                    let start = js_number(uo.get("start"));
                    uo.insert("duration".into(), jnum(secs - start));
                }
            }
        }
    } else if is_map(match_, "DragonShire") {
        // parser.js:1735-1750
        let matched = st.dragon.as_ref().is_some_and(|d| {
            js_strict_eq(get(d, &["tag"]), tag) && js_strict_eq(get(d, &["rtag"]), rtag)
        });
        if matched {
            let d = st.dragon.clone().unwrap_or(J::Null);
            // dragon never activated → team undefined → objective[undefined] → Throw, as in JS
            let team_key = js_prop(get(&d, &["team"]));
            let slot = obj_slot_mut(match_, &team_key)?;
            let last = slot_events_mut(slot)?
                .last_mut()
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("events vide".into()))?;
            let loop_duration = gameloop - js_number(last.get("loop"));
            last.insert("loopDuration".into(), jnum(loop_duration));
            last.insert("duration".into(), jnum(loops_to_seconds(loop_duration)));
            match get(&d, &["player"]) {
                Some(p) => last.insert("player".into(), p.clone()),
                None => last.remove("player"),
            };
            st.dragon = None;
        }
    } else if is_map(match_, "Crypts") {
        // parser.js:1751-1782
        let cs = st
            .current_spiders
            .as_mut()
            .ok_or_else(|| Abort::Throw("currentSpiders undefined".into()))?;
        if cs.active {
            for skey in js_for_in_keys(&cs.units) {
                let spider = cs.units.get(&skey).cloned().unwrap_or(J::Null);
                if js_strict_eq(tag, get(&spider, &["tag"]))
                    && js_strict_eq(rtag, get(&spider, &["rtag"]))
                {
                    cs.max_duration = Some(loops_to_seconds(
                        gameloop - js_number(get(&spider, &["loop"])),
                    ));
                    cs.units.remove(&skey);
                    if cs.units.is_empty() {
                        let team_key = js_key(cs.team);
                        let slot = obj_slot_mut(match_, &team_key)?;
                        let events = slot_events_mut(slot)?;
                        let ev = js_idx(cs.event_idx)
                            .and_then(|i| events.get_mut(i))
                            .and_then(J::as_object_mut)
                            .ok_or_else(|| Abort::Throw("events[eventIdx] absent".into()))?;
                        ev.insert("duration".into(), cs.max_duration.map_or(J::Null, jnum));
                        ev.insert("endLoop".into(), gameloop_j.clone());
                        ev.insert("end".into(), jnum(secs));
                        cs.active = false;
                        cs.units = Map::new();
                        break;
                    }
                }
            }
        }
    } else if is_map(match_, "Volskaya") {
        // parser.js:1783-1800
        let cp = st
            .current_protector
            .as_ref()
            .ok_or_else(|| Abort::Throw("currentProtector undefined".into()))?
            .clone();
        if js_truthy(get(&cp, &["active"]))
            && js_strict_eq(get(&cp, &["tag"]), tag)
            && js_strict_eq(get(&cp, &["rtag"]), rtag)
        {
            let duration = loops_to_seconds(gameloop - js_number(get(&cp, &["loop"])));
            let team_key = js_prop(get(&cp, &["team"]));
            let slot = obj_slot_mut(match_, &team_key)?;
            let events = slot_events_mut(slot)?;
            js_idx(Some(js_number(get(&cp, &["eventIdx"]))))
                .and_then(|i| events.get_mut(i))
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("events[eventIdx] absent".into()))?
                .insert("duration".into(), jnum(duration));
            st.current_protector = Some(json!({"active": false}));
        }
    } else if is_map(match_, "Warhead Junction") {
        // parser.js:1801-1808
        let nukes = st
            .nukes
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("nukes undefined".into()))?;
        if let Some(nuke) = nukes.get_mut(&uid).and_then(J::as_object_mut) {
            let success = gameloop - js_number(nuke.get("loop")) > 16.0 * 2.0;
            nuke.insert("success".into(), J::from(success));
        }
    } else if is_map(match_, "BraxisHoldout") {
        // parser.js:1809-1859
        let side = {
            let wu = st
                .wave_units
                .as_ref()
                .and_then(J::as_object)
                .ok_or_else(|| Abort::Throw("waveUnits undefined".into()))?;
            let has = |k: &str| -> R<bool> {
                Ok(wu
                    .get(k)
                    .and_then(J::as_object)
                    .ok_or_else(|| Abort::Throw(format!("waveUnits[{k}] absent")))?
                    .contains_key(&uid))
            };
            if has("0")? {
                Some(0usize)
            } else if has("1")? {
                Some(1usize)
            } else {
                None
            }
        };
        if let Some(team) = side {
            let team_key = team.to_string();
            let other_key = (1 - team).to_string();
            let is_ultra = st
                .wave_units
                .as_ref()
                .and_then(|wu| get(wu, &[&team_key, &uid, "type"]))
                .and_then(J::as_str)
                == Some(rt_str("BraxisUnitType", "ZergUltralisk"));
            if is_ultra && get(event, &["m_killerPlayerId"]) == Some(&J::Null) {
                // stillborn ultralisk: the opposing wave starts at 100 (parser.js:1820-1828)
                let build = match_build(match_);
                let (s0, s1) = braxis_strengths(st, build)?;
                let wave = wave_mut(match_, st.wave_id)?;
                if !wave.contains_key("startScore") {
                    wave.insert("startScore".into(), json!({"0": jnum(s0), "1": jnum(s1)}));
                }
                wave.get_mut("startScore")
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw("startScore non-objet".into()))?
                    .insert(other_key.clone(), J::from(100));
            }
            let wave = wave_mut(match_, st.wave_id)?;
            wave.get_mut("endLoop")
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("wave.endLoop absent".into()))?
                .insert(team_key.clone(), gameloop_j.clone());
            wave.get_mut("endTime")
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("wave.endTime absent".into()))?
                .insert(team_key.clone(), jnum(secs));
            st.wave_units
                .as_mut()
                .and_then(J::as_object_mut)
                .and_then(|wu| wu.get_mut(&team_key))
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("waveUnits[team] absent".into()))?
                .remove(&uid);
        }
    } else if is_map(match_, "BattlefieldOfEternity") {
        // parser.js:1860-1870
        let im = st
            .immortal
            .as_ref()
            .and_then(J::as_object)
            .ok_or_else(|| Abort::Throw("immortal undefined".into()))?;
        if im.contains_key("tag")
            && js_strict_eq(tag, im.get("tag"))
            && js_strict_eq(rtag, im.get("rtag"))
        {
            let start = js_number(im.get("start"));
            obj_arr_mut(match_, "results")?
                .last_mut()
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("results vide".into()))?
                .insert(
                    "immortalDuration".into(),
                    jnum(loops_to_seconds(gameloop - start)),
                );
            st.immortal = Some(json!({}));
        }
    } else if is_map(match_, "AlteracPass") {
        // parser.js:1871-1882
        for i in ["0", "1"] {
            let slot = obj_slot_mut(match_, i)?;
            for unit in slot_events_mut(slot)?.iter_mut() {
                if js_strict_eq(get(unit, &["id"]), Some(&J::from(uid.as_str()))) {
                    unit.as_object_mut()
                        .ok_or_else(|| Abort::Throw("event non-objet".into()))?
                        .insert("died".into(), jnum(secs));
                }
            }
        }
    } else if is_map(match_, "Hanamura") {
        // parser.js:1883-1895
        let events = obj_arr_mut(match_, "events")?;
        if let Some(payload) = events.last_mut() {
            if js_strict_eq(get(payload, &["id"]), Some(&J::from(uid.as_str()))) {
                let po = payload
                    .as_object_mut()
                    .ok_or_else(|| Abort::Throw("payload non-objet".into()))?;
                po.insert("died".into(), jnum(secs));
                let winner = po
                    .get("control")
                    .and_then(J::as_array)
                    .and_then(|c| c.last())
                    .ok_or_else(|| Abort::Throw("payload.control vide".into()))?;
                let winner = get(winner, &["team"]).cloned().unwrap_or(J::Null);
                po.insert("winner".into(), winner);
            }
        }
    }
    Ok(())
}

/// Per-map UnitOwnerChange (parser.js:1897-1979) — four independent `if`s (not else-if).
pub(super) fn obj_unit_owner_change(
    event: &J,
    match_: &mut Map<String, J>,
    st: &mut ObjState,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
) -> R<()> {
    if objective_less_map(match_) {
        return Ok(()); // extended ARAM maps: no objective (see EXTRA_MAPS)
    }
    let tag = get(event, &["m_unitTagIndex"]);
    let rtag = get(event, &["m_unitTagRecycle"]);
    let gameloop_j = get(event, &["_gameloop"]).cloned().unwrap_or(J::Null);
    let gameloop = js_number(get(event, &["_gameloop"]));
    let secs = loops_to_seconds(gameloop - loop_game_start);
    let control = js_number(get(event, &["m_controlPlayerId"]));
    let control_key = js_prop(get(event, &["m_controlPlayerId"]));

    if is_map(match_, "DragonShire") {
        // parser.js:1898-1931
        let eq = |o: &Option<J>| {
            o.as_ref().is_some_and(|v| {
                js_strict_eq(get(v, &["tag"]), tag) && js_strict_eq(get(v, &["rtag"]), rtag)
            })
        };
        let shrine = if eq(&st.moon) {
            Some("moon")
        } else if eq(&st.sun) {
            Some("sun")
        } else {
            None
        };
        if let Some(which) = shrine {
            // team !== 0 → -11; otherwise -1 ("our blue team is already 0")
            let team = if control == 0.0 { -1.0 } else { control - 11.0 };
            objective_mut(match_)?
                .get_mut("shrines")
                .and_then(J::as_object_mut)
                .and_then(|s| s.get_mut(which))
                .and_then(J::as_array_mut)
                .ok_or_else(|| Abort::Throw("shrines absent".into()))?
                .push(json!({
                    "loop": gameloop_j.clone(), "team": jnum(team), "time": jnum(secs),
                }));
        } else if eq(&st.dragon) && control > 0.0 && control != 11.0 && control != 12.0 {
            let d = st
                .dragon
                .as_mut()
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("dragon non-objet".into()))?;
            match player_id_map.get(&control_key) {
                Some(p) => d.insert("player".into(), p.clone()),
                None => d.remove("player"),
            };
        }
    }
    if is_map(match_, "HauntedWoods") {
        // parser.js:1932-1943
        let ct = st
            .current_terror
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("currentTerror undefined".into()))?;
        for tkey in js_for_in_keys(ct) {
            let matched = ct.get(&tkey).is_some_and(|terror| {
                js_strict_eq(get(terror, &["tag"]), tag)
                    && js_strict_eq(get(terror, &["rtag"]), rtag)
            });
            if matched {
                let terror = ct
                    .get_mut(&tkey)
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw("currentTerror[t] non-objet".into()))?;
                match player_id_map.get(&control_key) {
                    Some(p) => terror.insert("player".into(), p.clone()),
                    None => terror.remove("player"),
                };
            }
        }
    }
    if is_map(match_, "BraxisHoldout") {
        // parser.js:1944-1963
        let uid = unit_uid(event);
        let side = {
            let beacons = st
                .beacons
                .as_ref()
                .and_then(J::as_object)
                .ok_or_else(|| Abort::Throw("beacons undefined".into()))?;
            beacons
                .get(&uid)
                .map(|b| get(b, &["side"]).cloned().unwrap_or(J::Null))
        };
        if let Some(side) = side {
            let team = if control == 0.0 { -1.0 } else { control - 11.0 };
            obj_arr_mut(match_, "beacons")?.push(json!({
                "team": jnum(team), "loop": gameloop_j.clone(), "side": side, "time": jnum(secs),
            }));
        }
    }
    if is_map(match_, "Hanamura") {
        // parser.js:1964-1978
        let uid = unit_uid(event);
        let events = obj_arr_mut(match_, "events")?;
        if let Some(payload) = events.last_mut() {
            if js_strict_eq(get(payload, &["id"]), Some(&J::from(uid.as_str()))) {
                let team = if control == 0.0 { -1.0 } else { control - 11.0 };
                payload
                    .as_object_mut()
                    .and_then(|p| p.get_mut("control"))
                    .and_then(J::as_array_mut)
                    .ok_or_else(|| Abort::Throw("payload.control absent".into()))?
                    .push(json!({
                        "team": jnum(team), "loop": gameloop_j.clone(), "time": jnum(secs),
                    }));
            }
        }
    }
    Ok(())
}

/// Post-loop cleanups of the per-map objectives (parser.js:1982-2109).
pub(super) fn obj_cleanup(
    match_: &mut Map<String, J>,
    st: &mut ObjState,
    loop_game_start: f64,
) -> R<()> {
    if objective_less_map(match_) {
        return Ok(()); // extended ARAM maps: no objective (see EXTRA_MAPS)
    }
    let loop_length = js_number(match_.get("loopLength"));
    let loop_length_j = match_.get("loopLength").cloned().unwrap_or(J::Null);
    let end_secs = loops_to_seconds(loop_length - loop_game_start);

    if is_map(match_, "HauntedMines") {
        // parser.js:1983-2011 — golems still alive at the end
        let golems = st
            .golems
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("golems undefined".into()))?;
        for key in js_for_in_keys(golems) {
            let golem = golems.get(&key).cloned().unwrap_or(J::Null);
            if js_truthy(Some(&golem)) {
                let start_time = js_number(get(&golem, &["time"]));
                let obj_event = json!({
                    "startLoop": get(&golem, &["loop"]).cloned().unwrap_or(J::Null),
                    "startTime": get(&golem, &["time"]).cloned().unwrap_or(J::Null),
                    "endLoop": loop_length_j.clone(),
                    "endTime": jnum(end_secs),
                    "duration": jnum(end_secs - start_time),
                    "team": get(&golem, &["team"]).cloned().unwrap_or(J::Null),
                });
                let team_key = js_prop(get(&golem, &["team"]));
                golems.insert(key, J::Null);
                objective_mut(match_)?
                    .get_mut(&team_key)
                    .and_then(J::as_array_mut)
                    .ok_or_else(|| Abort::Throw(format!("objective[{team_key}] absent")))?
                    .push(obj_event);
            }
        }
    } else if is_map(match_, "HauntedWoods") {
        // parser.js:2013-2044 — terrors still active
        let ct = st
            .current_terror
            .as_mut()
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("currentTerror undefined".into()))?;
        for tkey in js_for_in_keys(ct) {
            let terror = ct.get(&tkey).cloned().unwrap_or(J::Null);
            if js_truthy(get(&terror, &["active"])) {
                let team_key = js_parse_int(&tkey).map_or_else(|| "NaN".into(), |v| v.to_string());
                let slot = obj_slot_mut(match_, &team_key)?;
                let last = slot_events_mut(slot)?
                    .last_mut()
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw("events vide".into()))?;
                let duration = loop_length - js_number(last.get("loop"));
                last.insert("loopDuration".into(), jnum(duration));
                last.insert("duration".into(), jnum(loops_to_seconds(duration)));
                match get(&terror, &["player"]) {
                    Some(p) => last.insert("player".into(), p.clone()),
                    None => last.remove("player"),
                };
                ct.get_mut(&tkey)
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw("currentTerror[t] absent".into()))?
                    .insert("active".into(), J::from(false));
            }
        }
    } else if is_map(match_, "DragonShire") {
        // parser.js:2045-2059 — « a dragon can spawn well after the game ends »
        let team_ok = st.dragon.as_ref().is_some_and(|d| {
            let t = js_number(get(d, &["team"]));
            t == 0.0 || t == 1.0
        });
        if team_ok {
            let d = st.dragon.clone().unwrap_or(J::Null);
            let team_key = js_prop(get(&d, &["team"]));
            let slot = obj_slot_mut(match_, &team_key)?;
            let last = slot_events_mut(slot)?
                .last_mut()
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("events vide".into()))?;
            let loop_duration = loop_length - js_number(last.get("loop"));
            last.insert("loopDuration".into(), jnum(loop_duration));
            last.insert("duration".into(), jnum(loops_to_seconds(loop_duration)));
            match get(&d, &["player"]) {
                Some(p) => last.insert("player".into(), p.clone()),
                None => last.remove("player"),
            };
            st.dragon = None;
        }
    } else if is_map(match_, "Crypts") {
        // parser.js:2060-2072 — spider phase still active
        let cs = st
            .current_spiders
            .as_mut()
            .ok_or_else(|| Abort::Throw("currentSpiders undefined".into()))?;
        if cs.active {
            let spider = js_for_in_keys(&cs.units)
                .first()
                .and_then(|k| cs.units.get(k))
                .cloned()
                // empty units → units[undefined].loop throws, as in JS
                .ok_or_else(|| Abort::Throw("currentSpiders.units vide".into()))?;
            let spider_loop = js_number(get(&spider, &["loop"]));
            let team_key = js_key(cs.team);
            let slot = obj_slot_mut(match_, &team_key)?;
            let events = slot_events_mut(slot)?;
            let ev = js_idx(cs.event_idx)
                .and_then(|i| events.get_mut(i))
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("events[eventIdx] absent".into()))?;
            ev.insert(
                "duration".into(),
                jnum(loops_to_seconds(loop_length - spider_loop)),
            );
            ev.insert("endLoop".into(), loop_length_j.clone());
            ev.insert("end".into(), jnum(end_secs));
        }
    } else if is_map(match_, "Volskaya") {
        // parser.js:2073-2079 — protector still active
        let cp = st
            .current_protector
            .as_ref()
            .ok_or_else(|| Abort::Throw("currentProtector undefined".into()))?
            .clone();
        if js_truthy(get(&cp, &["active"])) {
            let duration = loops_to_seconds(loop_length - js_number(get(&cp, &["loop"])));
            let team_key = js_prop(get(&cp, &["team"]));
            let slot = obj_slot_mut(match_, &team_key)?;
            let events = slot_events_mut(slot)?;
            js_idx(Some(js_number(get(&cp, &["eventIdx"]))))
                .and_then(|i| events.get_mut(i))
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("events[eventIdx] absent".into()))?
                .insert("duration".into(), jnum(duration));
        }
    } else if is_map(match_, "Warhead Junction") {
        // parser.js:2080-2090 — sort of the nukes
        let nukes = st
            .nukes
            .as_ref()
            .and_then(J::as_object)
            .ok_or_else(|| Abort::Throw("nukes undefined".into()))?
            .clone();
        for id in js_for_in_keys(&nukes) {
            let nuke = nukes.get(&id).cloned().unwrap_or(J::Null);
            let team_key = js_prop(get(&nuke, &["team"]));
            let slot = obj_slot_mut(match_, &team_key)?;
            slot_events_mut(slot)?.push(nuke.clone());
            js_incr(slot, "count", 1.0);
            // success if true OR if it never died (missing key)
            let success_true = get(&nuke, &["success"]) == Some(&J::Bool(true));
            if success_true || get(&nuke, &["success"]).is_none() {
                js_incr(slot, "success", 1.0);
            }
        }
    } else if is_map(match_, "BattlefieldOfEternity") {
        // parser.js:2091-2097 — immortal still alive
        let im = st
            .immortal
            .as_ref()
            .and_then(J::as_object)
            .ok_or_else(|| Abort::Throw("immortal undefined".into()))?;
        if im.contains_key("tag") {
            let start = js_number(im.get("start"));
            obj_arr_mut(match_, "results")?
                .last_mut()
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("results vide".into()))?
                .insert(
                    "immortalDuration".into(),
                    jnum(loops_to_seconds(loop_length - start)),
                );
        }
    } else if is_map(match_, "AlteracPass") {
        // parser.js:2098-2108 — cavalries that never died
        for i in ["0", "1"] {
            let slot = obj_slot_mut(match_, i)?;
            for unit in slot_events_mut(slot)?.iter_mut() {
                let uo = unit
                    .as_object_mut()
                    .ok_or_else(|| Abort::Throw("event non-objet".into()))?;
                if !uo.contains_key("died") {
                    uo.insert("died".into(), jnum(end_secs));
                }
            }
        }
    }
    Ok(())
}

/// `getFirstObjectiveTeam(match)` (parser.js:3116-3302) — try/catch → null, never throws.
pub(super) fn get_first_objective_team(match_: &Map<String, J>) -> J {
    if objective_less_map(match_) {
        return J::Null; // extended ARAM maps: no objective (see EXTRA_MAPS)
    }
    first_objective_inner(match_).unwrap_or(J::Null)
}

pub(super) fn first_objective_inner(match_: &Map<String, J>) -> R<J> {
    let obj = match_
        .get("objective")
        .ok_or_else(|| Abort::Throw("objective absent".into()))?;
    let events = |t: &str| {
        get(obj, &[t, "events"])
            .and_then(J::as_array)
            .ok_or_else(|| Abort::Throw(format!("objective[{t}].events absent")))
    };
    if is_map(match_, "DragonShire")
        || is_map(match_, "Crypts")
        || is_map(match_, "Volskaya")
        || is_map(match_, "AlteracPass")
        || is_map(match_, "BlackheartsBay")
    {
        // parser.js:3118-3149 — NB: the Alterac events have no time field →
        // undefined === undefined → null as soon as both teams have an event
        let (e0, e1) = (events("0")?, events("1")?);
        if e0.is_empty() && e1.is_empty() {
            return Ok(J::Null);
        }
        if e0.is_empty() && !e1.is_empty() {
            return Ok(J::from(1));
        }
        if e1.is_empty() && !e0.is_empty() {
            return Ok(J::from(0));
        }
        let (t0, t1) = (get(&e0[0], &["time"]), get(&e1[0], &["time"]));
        if js_strict_eq(t0, t1) {
            return Ok(J::Null);
        }
        Ok(J::from(if js_number(t0) < js_number(t1) { 0 } else { 1 }))
    } else if is_map(match_, "HauntedWoods") {
        // parser.js:3150-3168 — no "both empty" case: units[0].loop throws → null
        let units = |t: &str| {
            get(obj, &[t, "units"])
                .and_then(J::as_array)
                .ok_or_else(|| Abort::Throw(format!("objective[{t}].units absent")))
        };
        let (u0, u1) = (units("0")?, units("1")?);
        if u0.is_empty() && !u1.is_empty() {
            return Ok(J::from(1));
        }
        if u1.is_empty() && !u0.is_empty() {
            return Ok(J::from(0));
        }
        let l0 = get(
            u0.first()
                .ok_or_else(|| Abort::Throw("empty units".into()))?,
            &["loop"],
        );
        let l1 = get(
            u1.first()
                .ok_or_else(|| Abort::Throw("empty units".into()))?,
            &["loop"],
        );
        if js_strict_eq(l0, l1) {
            return Ok(J::Null);
        }
        Ok(J::from(if js_number(l0) < js_number(l1) { 0 } else { 1 }))
    } else if is_map(match_, "ControlPoints") || is_map(match_, "TowersOfDoom") {
        // parser.js:3169-3220 — who dominates the first 90 shots / the first 3 altars
        let take = if is_map(match_, "ControlPoints") {
            90
        } else {
            3
        };
        let mut all: Vec<&J> = events("0")?.iter().chain(events("1")?.iter()).collect();
        all.sort_by(|a, b| js_cmp(js_number(get(a, &["loop"])), js_number(get(b, &["loop"]))));
        let (mut blue, mut red) = (0i64, 0i64);
        for e in all.iter().take(take) {
            let t = js_number(get(e, &["team"]));
            if t == rt_int("TeamType", "Blue") as f64 {
                blue += 1;
            } else if t == rt_int("TeamType", "Red") as f64 {
                red += 1;
            }
        }
        if blue == red {
            return Ok(J::Null);
        }
        Ok(J::from(if blue > red {
            rt_int("TeamType", "Blue")
        } else {
            rt_int("TeamType", "Red")
        }))
    } else if is_map(match_, "CursedHollow") {
        // parser.js:3221-3243 — first to 3 tributes
        let mut all: Vec<&J> = events("0")?.iter().chain(events("1")?.iter()).collect();
        all.sort_by(|a, b| js_cmp(js_number(get(a, &["loop"])), js_number(get(b, &["loop"]))));
        let (mut blue, mut red) = (0i64, 0i64);
        for e in &all {
            let t = js_number(get(e, &["team"]));
            if t == rt_int("TeamType", "Blue") as f64 {
                blue += 1;
            } else if t == rt_int("TeamType", "Red") as f64 {
                red += 1;
            }
            if blue >= 3 {
                return Ok(J::from(rt_int("TeamType", "Blue")));
            }
            if red >= 3 {
                return Ok(J::from(rt_int("TeamType", "Red")));
            }
        }
        Ok(J::Null)
    } else if is_map(match_, "Warhead Junction") {
        // parser.js:3244-3272 — best of the first 4 successful nukes
        let mut all: Vec<&J> = events("0")?.iter().chain(events("1")?.iter()).collect();
        all.sort_by(|a, b| js_cmp(js_number(get(a, &["loop"])), js_number(get(b, &["loop"]))));
        let (mut blue, mut red, mut total) = (0i64, 0i64, 0i64);
        for e in &all {
            if js_truthy(get(e, &["success"])) {
                let t = js_number(get(e, &["team"]));
                if t == rt_int("TeamType", "Blue") as f64 {
                    blue += 1;
                } else if t == rt_int("TeamType", "Red") as f64 {
                    red += 1;
                }
                total += 1;
            }
            if total >= 4 {
                break;
            }
        }
        if blue == red {
            return Ok(J::Null);
        }
        Ok(J::from(if blue > red {
            rt_int("TeamType", "Blue")
        } else {
            rt_int("TeamType", "Red")
        }))
    } else if is_map(match_, "BattlefieldOfEternity") {
        // parser.js:3273-3278
        let results = get(obj, &["results"])
            .and_then(J::as_array)
            .ok_or_else(|| Abort::Throw("results absent".into()))?;
        Ok(results
            .first()
            .map_or(J::Null, |r| get(r, &["winner"]).cloned().unwrap_or(J::Null)))
    } else if is_map(match_, "Shrines") {
        // parser.js:3279-3284
        let shrines = get(obj, &["shrines"])
            .and_then(J::as_array)
            .ok_or_else(|| Abort::Throw("shrines absent".into()))?;
        Ok(shrines
            .first()
            .map_or(J::Null, |s| get(s, &["team"]).cloned().unwrap_or(J::Null)))
    } else if is_map(match_, "BraxisHoldout") {
        // parser.js:3285-3293 — startScore missing → undefined[0] throws → null
        let waves = get(obj, &["waves"])
            .and_then(J::as_array)
            .ok_or_else(|| Abort::Throw("waves absent".into()))?;
        match waves.first() {
            Some(w) => {
                let ss = get(w, &["startScore"])
                    .ok_or_else(|| Abort::Throw("startScore absent".into()))?;
                let (s0, s1) = (js_number(get(ss, &["0"])), js_number(get(ss, &["1"])));
                Ok(J::from(if s0 > s1 { 0 } else { 1 }))
            }
            None => Ok(J::Null),
        }
    } else {
        // Haunted Mines (« unsure how to detect first objective »), Hanamura
        Ok(J::Null)
    }
}

/// Names of the "per-map objective" Stat events (parser.js:974-1189 + 1214-1238).
pub(super) fn is_map_objective_stat_event(name: &str) -> bool {
    [
        "SkyTempleShotsFired",
        "AltarCaptured",
        "ImmortalDefeated",
        "TributeCollected",
        "DragonKnightActivated",
        "GardenTerrorActivated",
        "ShrineCaptured",
        "PunisherKilled",
        "SpidersSpawned",
        "SixTowersStart",
        "SixTowersEnd",
        "TowersFortCaptured",
        "BraxisWaveStart",
        "GhostShipCaptured",
    ]
    .iter()
    .any(|k| rt_str("StatEventType", k) == name)
}

/// Unit types consumed by the per-map branches of UnitBorn (parser.js:1262-1475).
pub(super) fn is_map_objective_unit_born(unit_type: &str) -> bool {
    [
        "MinesBoss",
        "RavenLordTribute",
        "MoonShrine",
        "SunShrine",
        "GardenTerrorVehicle",
        "GardenTerror",
        "DragonVehicle",
        "Webweaver",
        "Triglav",
        "Nuke",
        "BraxisZergPath",
        "BraxisControlPoint",
        "ImmortalHeaven",
        "ImmortalHell",
        "WarheadSpawn",
        "WarheadDropped",
        "AllianceCavalry",
        "HordeCavalry",
        "NeutralPayload",
    ]
    .iter()
    .any(|k| rt_str("UnitType", k) == unit_type)
        || replay_types()["BraxisUnitType"]
            .as_object()
            .is_some_and(|o| o.contains_key(unit_type))
}
