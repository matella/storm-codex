//! Événements de partie : XP, morts de joueurs, camps, montées de niveau, unités, positions, talents.

use super::*;

/// `event.m_unitTagIndex + '-' + event.m_unitTagRecycle` (concaténation JS).
pub(super) fn unit_uid(event: &J) -> String {
    format!(
        "{}-{}",
        js_prop(get(event, &["m_unitTagIndex"])),
        js_prop(get(event, &["m_unitTagRecycle"]))
    )
}

/// `xpb.breakdown` : m_fixedData → `{ m_key: m_value/4096 }` (parser.js:850-853, 874-877).
pub(super) fn xp_fixed_breakdown(event: &J) -> R<Map<String, J>> {
    let mut breakdown = Map::new();
    // `for..in` sur undefined : zéro itération, pas d'exception
    if let Some(fd) = get(event, &["m_fixedData"]).and_then(J::as_array) {
        for item in fd {
            if !item.is_object() {
                return Err(Abort::Throw("xp: m_fixedData non-objet".into()));
            }
            breakdown.insert(
                js_prop(get(item, &["m_key"])),
                jnum(js_number(get(item, &["m_value"])) / 4096.0),
            );
        }
    }
    Ok(breakdown)
}

/// PeriodicXPBreakdown (parser.js:831-855) → match.XPBreakdown.
pub(super) fn process_periodic_xp(
    event: &J,
    match_: &mut Map<String, J>,
    loop_game_start: f64,
    possible_minion_xp: &[f64; 2],
) -> R<()> {
    let gameloop = js_number(get(event, &["_gameloop"]));
    let mut xpb = Map::new();
    xpb.insert(
        "loop".into(),
        get(event, &["_gameloop"]).cloned().unwrap_or(J::Null),
    );
    xpb.insert(
        "time".into(),
        jnum(loops_to_seconds(gameloop - loop_game_start)),
    );
    let d0 = get(event, &["m_intData", "0"])
        .ok_or_else(|| Abort::Throw("xp: m_intData[0] absent".into()))?;
    // « team is 1-indexed in this event? »
    let team = js_number(get(d0, &["m_value"])) - 1.0;
    xpb.insert("team".into(), jnum(team));
    let d1 = get(event, &["m_intData", "1"])
        .ok_or_else(|| Abort::Throw("xp: m_intData[1] absent".into()))?;
    xpb.insert(
        "teamLevel".into(),
        get(d1, &["m_value"]).cloned().unwrap_or(J::Null),
    );
    xpb.insert("breakdown".into(), J::Object(xp_fixed_breakdown(event)?));
    xpb.insert(
        "theoreticalMinionXP".into(),
        match js_num_str(team).as_str() {
            "0" => jnum(possible_minion_xp[0]),
            "1" => jnum(possible_minion_xp[1]),
            _ => J::Null, // possibleMinionXP[team] indéfini → undefined → null
        },
    );
    match_
        .get_mut("XPBreakdown")
        .and_then(J::as_array_mut)
        .ok_or_else(|| Abort::Throw("XPBreakdown absent".into()))?
        .push(J::Object(xpb));
    Ok(())
}

/// EndOfGameXPBreakdown (parser.js:856-883) : cache le dernier breakdown de chaque équipe,
/// poussé dans match.XPBreakdown après la boucle (parser.js:2128-2129).
pub(super) fn process_eog_xp(
    event: &J,
    players: &Map<String, J>,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
    possible_minion_xp: &[f64; 2],
    team_xp_end: &mut [Option<J>; 2],
) -> R<()> {
    let gameloop = js_number(get(event, &["_gameloop"]));
    let mut xpb = Map::new();
    xpb.insert(
        "loop".into(),
        get(event, &["_gameloop"]).cloned().unwrap_or(J::Null),
    );
    xpb.insert(
        "time".into(),
        jnum(loops_to_seconds(gameloop - loop_game_start)),
    );
    let d0 = get(event, &["m_intData", "0"])
        .ok_or_else(|| Abort::Throw("xp: m_intData[0] absent".into()))?;
    let key = js_prop(get(d0, &["m_value"]));
    let handle = player_id_map
        .get(&key)
        .and_then(J::as_str)
        .ok_or_else(|| Abort::Throw(format!("xp: tracker id {key} non mappé")))?;
    let player = players
        .get(handle)
        .ok_or_else(|| Abort::Throw(format!("xp: joueur {handle} inconnu")))?;
    let team = get(player, &["team"]).cloned().unwrap_or(J::Null);
    xpb.insert("team".into(), team.clone());
    xpb.insert(
        "theoreticalMinionXP".into(),
        match js_prop(Some(&team)).as_str() {
            "0" => jnum(possible_minion_xp[0]),
            "1" => jnum(possible_minion_xp[1]),
            _ => J::Null,
        },
    );
    xpb.insert("breakdown".into(), J::Object(xp_fixed_breakdown(event)?));
    if team.as_i64() == Some(rt_int("TeamType", "Blue")) {
        team_xp_end[0] = Some(J::Object(xpb));
    } else if team.as_i64() == Some(rt_int("TeamType", "Red")) {
        team_xp_end[1] = Some(J::Object(xpb));
    }
    Ok(())
}

/// PlayerDeath (parser.js:884-941) : objet takedown partagé entre match.takedowns, les
/// deaths de la victime et les takedowns de chaque killer (clones ici — jamais muté ensuite).
pub(super) fn process_player_death(
    event: &J,
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
) -> R<()> {
    let gameloop = js_number(get(event, &["_gameloop"]));
    let mut td = Map::new();
    td.insert(
        "loop".into(),
        get(event, &["_gameloop"]).cloned().unwrap_or(J::Null),
    );
    td.insert(
        "time".into(),
        jnum(loops_to_seconds(gameloop - loop_game_start)),
    );
    let fd0 = get(event, &["m_fixedData", "0"])
        .ok_or_else(|| Abort::Throw("death: m_fixedData[0] absent".into()))?;
    td.insert(
        "x".into(),
        get(fd0, &["m_value"]).cloned().unwrap_or(J::Null),
    );
    let fd1 = get(event, &["m_fixedData", "1"])
        .ok_or_else(|| Abort::Throw("death: m_fixedData[1] absent".into()))?;
    td.insert(
        "y".into(),
        get(fd1, &["m_value"]).cloned().unwrap_or(J::Null),
    );

    let mut victim: Option<String> = None;
    let mut victim_doc: Option<J> = None;
    let mut td_killers: Vec<J> = Vec::new();
    let mut killer_handles: Vec<Option<String>> = Vec::new();
    let int_data = get(event, &["m_intData"])
        .and_then(J::as_array)
        .ok_or_else(|| Abort::Throw("death: m_intData absent".into()))?;
    for entry in int_data {
        let key = get_str(entry, &["m_key"]);
        if key == Some("PlayerID") {
            let pid = js_prop(get(entry, &["m_value"]));
            let handle = player_id_map
                .get(&pid)
                .and_then(J::as_str)
                .map(str::to_owned);
            // players[playerIDMap[id]].hero : id non mappé → players[undefined].hero lève
            let hero = match &handle {
                Some(h) => get(
                    players
                        .get(h)
                        .ok_or_else(|| Abort::Throw(format!("death: joueur {h} inconnu")))?,
                    &["hero"],
                )
                .cloned()
                .unwrap_or(J::Null),
                None => return Err(Abort::Throw(format!("death: victime {pid} non mappée"))),
            };
            victim_doc = Some(json!({
                "player": handle.as_deref().map_or(J::Null, J::from),
                "hero": hero,
            }));
            victim = handle;
        } else if key == Some("KillingPlayer") {
            let pid = js_prop(get(entry, &["m_value"]));
            let handle = player_id_map
                .get(&pid)
                .and_then(J::as_str)
                .map(str::to_owned);
            let tdo = match &handle {
                // « this poor person died to a creep »
                None => json!({"player": "0", "hero": "Nexus Forces"}),
                Some(h) => {
                    let hero = get(
                        players
                            .get(h)
                            .ok_or_else(|| Abort::Throw(format!("death: killer {h} inconnu")))?,
                        &["hero"],
                    )
                    .cloned()
                    .unwrap_or(J::Null);
                    json!({"player": h.as_str(), "hero": hero})
                }
            };
            killer_handles.push(handle); // undefined poussé tel quel en JS, filtré plus bas
            td_killers.push(tdo);
        }
    }
    td.insert("killers".into(), J::Array(td_killers));
    if let Some(v) = victim_doc {
        td.insert("victim".into(), v);
    }
    let td = J::Object(td);

    // players[victim].team : aucun PlayerID → players[undefined] lève
    let victim = victim.ok_or_else(|| Abort::Throw("death: pas de PlayerID".into()))?;
    let vteam = players
        .get(&victim)
        .and_then(|p| get(p, &["team"]))
        .and_then(J::as_i64);
    let counter = if vteam == Some(rt_int("TeamType", "Blue")) {
        Some("team1Takedowns")
    } else if vteam == Some(rt_int("TeamType", "Red")) {
        Some("team0Takedowns")
    } else {
        None
    };
    if let Some(c) = counter {
        let n = match_.get(c).and_then(J::as_i64).unwrap_or(0) + 1;
        match_.insert(c.into(), J::from(n));
    }
    match_
        .get_mut("takedowns")
        .and_then(J::as_array_mut)
        .ok_or_else(|| Abort::Throw("takedowns absent".into()))?
        .push(td.clone());
    players
        .get_mut(&victim)
        .and_then(J::as_object_mut)
        .and_then(|p| p.get_mut("deaths"))
        .and_then(J::as_array_mut)
        .ok_or_else(|| Abort::Throw("death: deaths absent".into()))?
        .push(td.clone());
    for h in killer_handles.into_iter().flatten() {
        players
            .get_mut(&h)
            .and_then(J::as_object_mut)
            .and_then(|p| p.get_mut("takedowns"))
            .and_then(J::as_array_mut)
            .ok_or_else(|| Abort::Throw(format!("death: killer {h} inconnu")))?
            .push(td.clone());
    }
    Ok(())
}

/// CampCapture (parser.js:1127-1158) — mercs + branche Towers of Doom (boss → match.objective).
pub(super) fn process_camp_capture(
    event: &J,
    match_: &mut Map<String, J>,
    loop_game_start: f64,
) -> R<()> {
    let gameloop = js_number(get(event, &["_gameloop"]));
    let gameloop_j = get(event, &["_gameloop"]).cloned().unwrap_or(J::Null);
    let sd0 = get(event, &["m_stringData", "0"])
        .ok_or_else(|| Abort::Throw("camp: m_stringData[0] absent".into()))?;
    let fd0 = get(event, &["m_fixedData", "0"])
        .ok_or_else(|| Abort::Throw("camp: m_fixedData[0] absent".into()))?;
    let team = js_number(get(fd0, &["m_value"])) / 4096.0 - 1.0;
    let time = loops_to_seconds(gameloop - loop_game_start);
    let mut cap = Map::new();
    cap.insert("loop".into(), gameloop_j.clone());
    cap.insert(
        "type".into(),
        get(sd0, &["m_value"]).cloned().unwrap_or(J::Null),
    );
    cap.insert("team".into(), jnum(team));
    cap.insert("time".into(), jnum(time));
    match_
        .get_mut("mercs")
        .and_then(J::as_object_mut)
        .and_then(|m| m.get_mut("captures"))
        .and_then(J::as_array_mut)
        .ok_or_else(|| Abort::Throw("mercs.captures absent".into()))?
        .push(J::Object(cap));
    // Towers of Doom : boss → match.objective (parser.js:1138-1151)
    if is_map(match_, "TowersOfDoom") && get_str(sd0, &["m_value"]) == Some("Boss Camp") {
        let slot = obj_slot_mut(match_, &js_num_str(team))?;
        slot_events_mut(slot)?.push(json!({
            "team": jnum(team), "loop": gameloop_j, "time": jnum(time),
            "type": "boss", "damage": 4,
        }));
        js_incr(slot, "damage", 4.0);
        js_incr(slot, "count", 1.0);
    }
    Ok(())
}

/// LevelUp (parser.js:1190-1200) → match.levelTimes[team][level].
pub(super) fn process_level_up(
    event: &J,
    match_: &mut Map<String, J>,
    players: &Map<String, J>,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
) -> R<()> {
    let d1 = get(event, &["m_intData", "1"])
        .ok_or_else(|| Abort::Throw("levelup: m_intData[1] absent".into()))?;
    let level = get(d1, &["m_value"]).cloned().unwrap_or(J::Null);
    let d0 = get(event, &["m_intData", "0"])
        .ok_or_else(|| Abort::Throw("levelup: m_intData[0] absent".into()))?;
    let key = js_prop(get(d0, &["m_value"]));
    let handle = player_id_map
        .get(&key)
        .and_then(J::as_str)
        .ok_or_else(|| Abort::Throw(format!("levelup: tracker id {key} non mappé")))?;
    let player = players
        .get(handle)
        .ok_or_else(|| Abort::Throw(format!("levelup: joueur {handle} inconnu")))?;
    let team = get(player, &["team"]).cloned().unwrap_or(J::Null);
    let gameloop = js_number(get(event, &["_gameloop"]));
    let lobj = json!({
        "loop": get(event, &["_gameloop"]).cloned().unwrap_or(J::Null),
        "level": level,
        "team": team,
        "time": jnum(loops_to_seconds(gameloop - loop_game_start)),
    });
    let team_key = js_prop(Some(&team));
    let bucket = match_
        .get_mut("levelTimes")
        .and_then(J::as_object_mut)
        .and_then(|lt| lt.get_mut(&team_key))
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw(format!("levelTimes[{team_key}] absent")))?;
    bucket.insert(js_prop(Some(&level)), lobj);
    Ok(())
}

/// UnitBorn (parser.js:1239-1540) : XP théorique des minions, unités d'objectif par carte,
/// mercs, structures, unités héros.
pub(super) fn process_unit_born(
    event: &J,
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
    possible_minion_xp: &mut [f64; 2],
    st: &mut ObjState,
) -> R<()> {
    let rt = replay_types();
    let unit_type = get_str(event, &["m_unitTypeName"]);
    let in_group =
        |g: &str| unit_type.is_some_and(|t| rt[g].as_object().is_some_and(|o| o.contains_key(t)));
    let gameloop = js_number(get(event, &["_gameloop"]));
    if in_group("MinionXP") {
        // parser.js:1245-1261 — XP théorique par minute écoulée (plafond 30)
        let t = unit_type.unwrap_or_default();
        let secs = loops_to_seconds(gameloop - loop_game_start);
        let mut mins = js_parse_int_num(secs / 60.0);
        if mins > 30.0 {
            mins = 30.0;
        }
        let table = if match_.get("map").and_then(J::as_str) == Some(rt_str("MapType", "Crypts")) {
            get(rt, &["TombMinionXP", t])
        } else {
            get(rt, &["MinionXP", t])
        }
        .ok_or_else(|| Abort::Throw(format!("table XP minion absente pour {t}")))?;
        // index hors bornes/négatif/NaN → undefined → accumulation NaN, comme en JS
        let xp = get(table, &[js_num_str(mins).as_str()]).map_or(f64::NAN, |v| js_number(Some(v)));
        let upkeep = get(event, &["m_upkeepPlayerId"]).and_then(J::as_i64);
        if upkeep == Some(11) {
            possible_minion_xp[0] += xp;
        } else if upkeep == Some(12) {
            possible_minion_xp[1] += xp;
        }
    } else if unit_type.is_some_and(is_map_objective_unit_born) {
        // golems, tributs, sanctuaires, terreurs, vagues Braxis… (parser.js:1262-1475)
        obj_unit_born(event, match_, st, players, player_id_map, loop_game_start)?;
    } else if in_group("MercUnitType") {
        // parser.js:1476-1500
        let mut unit = Map::new();
        unit.insert(
            "loop".into(),
            get(event, &["_gameloop"]).cloned().unwrap_or(J::Null),
        );
        unit.insert(
            "team".into(),
            jnum(js_number(get(event, &["m_controlPlayerId"])) - 11.0),
        );
        unit.insert(
            "type".into(),
            get(event, &["m_unitTypeName"]).cloned().unwrap_or(J::Null),
        );
        unit.insert(
            "locations".into(),
            json!([{
                "x": get(event, &["m_x"]).cloned().unwrap_or(J::Null),
                "y": get(event, &["m_y"]).cloned().unwrap_or(J::Null),
            }]),
        );
        unit.insert(
            "time".into(),
            jnum(loops_to_seconds(gameloop - loop_game_start)),
        );
        match_
            .get_mut("mercs")
            .and_then(J::as_object_mut)
            .and_then(|m| m.get_mut("units"))
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("mercs.units absent".into()))?
            .insert(unit_uid(event), J::Object(unit));
    } else if in_group("StructureStrings") {
        // parser.js:1501-1512
        let t = unit_type.unwrap_or_default();
        let mut s = Map::new();
        s.insert("type".into(), J::from(t));
        s.insert("name".into(), rt["StructureStrings"][t].clone());
        s.insert(
            "tag".into(),
            get(event, &["m_unitTagIndex"]).cloned().unwrap_or(J::Null),
        );
        s.insert(
            "rtag".into(),
            get(event, &["m_unitTagRecycle"])
                .cloned()
                .unwrap_or(J::Null),
        );
        s.insert("x".into(), get(event, &["m_x"]).cloned().unwrap_or(J::Null));
        s.insert("y".into(), get(event, &["m_y"]).cloned().unwrap_or(J::Null));
        s.insert(
            "team".into(),
            jnum(js_number(get(event, &["m_controlPlayerId"])) - 11.0),
        );
        match_
            .get_mut("structures")
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("structures absent".into()))?
            .insert(unit_uid(event), J::Object(s));
    } else if unit_type
        .ok_or_else(|| Abort::Throw("UnitBorn sans m_unitTypeName".into()))?
        .starts_with("Hero")
    {
        // parser.js:1513-1539 — « see: lost vikings », le contrôleur TLV n'est pas une unité
        let t = unit_type.unwrap_or_default();
        if t != "HeroLostVikingsController" {
            let key = js_prop(get(event, &["m_controlPlayerId"]));
            if let Some(handle) = player_id_map.get(&key).and_then(J::as_str) {
                let born = loops_to_seconds(gameloop - loop_game_start);
                let life = json!({
                    "born": jnum(born),
                    "locations": [{
                        "x": get(event, &["m_x"]).cloned().unwrap_or(J::Null),
                        "y": get(event, &["m_y"]).cloned().unwrap_or(J::Null),
                        "time": jnum(born),
                    }],
                });
                players
                    .get_mut(handle)
                    .and_then(J::as_object_mut)
                    .and_then(|p| p.get_mut("units"))
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw(format!("born: joueur {handle} inconnu")))?
                    .insert(unit_uid(event), json!({"lives": [life], "name": t}));
            }
        }
    }
    Ok(())
}

/// UnitRevived (parser.js:1541-1559) : nouvelle vie pour l'unité héros correspondante.
pub(super) fn process_unit_revived(
    event: &J,
    players: &mut Map<String, J>,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
) -> R<()> {
    let uid = unit_uid(event);
    let born = loops_to_seconds(js_number(get(event, &["_gameloop"])) - loop_game_start);
    for pid in js_for_in_keys(player_id_map) {
        let handle = player_id_map
            .get(&pid)
            .and_then(J::as_str)
            .unwrap_or_default()
            .to_owned();
        let units = players
            .get_mut(&handle)
            .and_then(J::as_object_mut)
            .and_then(|p| p.get_mut("units"))
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw(format!("revive: joueur {handle} inconnu")))?;
        if let Some(lives) = units
            .get_mut(&uid)
            .and_then(J::as_object_mut)
            .and_then(|u| u.get_mut("lives"))
            .and_then(J::as_array_mut)
        {
            lives.push(json!({
                "born": jnum(born),
                "locations": [{
                    "x": get(event, &["m_x"]).cloned().unwrap_or(J::Null),
                    "y": get(event, &["m_y"]).cloned().unwrap_or(J::Null),
                    "time": jnum(born),
                }],
            }));
        }
    }
    Ok(())
}

/// UnitPositions (parser.js:1560-1595) : trace des héros (vie courante) et des mercs vivants.
pub(super) fn process_unit_positions(
    event: &J,
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
) -> R<()> {
    let items = get(event, &["m_items"])
        .and_then(J::as_array)
        .ok_or_else(|| Abort::Throw("positions: m_items absent".into()))?
        .clone();
    let mut unit_index = js_number(get(event, &["m_firstUnitIndex"]));
    let time = jnum(loops_to_seconds(
        js_number(get(event, &["_gameloop"])) - loop_game_start,
    ));
    let handles: Vec<String> = js_for_in_keys(player_id_map)
        .iter()
        .map(|pid| {
            player_id_map
                .get(pid)
                .and_then(J::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    let mut i = 0usize;
    while i < items.len() {
        unit_index += js_number(items.get(i));
        let x = items.get(i + 1).cloned().unwrap_or(J::Null);
        let y = items.get(i + 2).cloned().unwrap_or(J::Null);
        // BUG parser.js:1573/1588 reproduit : startsWith(index) — « 12 » matche aussi « 120-0 »
        let prefix = js_num_str(unit_index);
        for handle in &handles {
            let units = players
                .get_mut(handle)
                .and_then(J::as_object_mut)
                .and_then(|p| p.get_mut("units"))
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw(format!("positions: joueur {handle} inconnu")))?;
            let uids: Vec<String> = units.keys().cloned().collect();
            for uid in uids {
                if uid.starts_with(&prefix) {
                    units
                        .get_mut(&uid)
                        .and_then(J::as_object_mut)
                        .and_then(|u| u.get_mut("lives"))
                        .and_then(J::as_array_mut)
                        .and_then(|l| l.last_mut())
                        .and_then(J::as_object_mut)
                        .and_then(|l| l.get_mut("locations"))
                        .and_then(J::as_array_mut)
                        .ok_or_else(|| Abort::Throw("positions: vie courante absente".into()))?
                        .push(json!({"x": x.clone(), "y": y.clone(), "time": time.clone()}));
                }
            }
        }
        // mercs : index correspondant ET unité vivante (duration falsy → pas encore morte)
        let mercs = match_
            .get_mut("mercs")
            .and_then(J::as_object_mut)
            .and_then(|m| m.get_mut("units"))
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("mercs.units absent".into()))?;
        let uids: Vec<String> = mercs.keys().cloned().collect();
        for uid in uids {
            if uid.starts_with(&prefix) {
                let unit = mercs
                    .get_mut(&uid)
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw("positions: merc non-objet".into()))?;
                if !js_truthy(unit.get("duration")) {
                    unit.get_mut("locations")
                        .and_then(J::as_array_mut)
                        .ok_or_else(|| Abort::Throw("positions: locations absent".into()))?
                        .push(json!({"x": x.clone(), "y": y.clone()}));
                }
            }
        }
        i += 3;
    }
    Ok(())
}

/// UnitDied (parser.js:1596-1653) : cores (vraie fin de partie), mercs, structures,
/// vies de héros ; les branches par carte (golems, terreurs, dragon…) suivent dans
/// `obj_unit_died`, appelé juste après par la boucle principale.
pub(super) fn process_unit_died(
    event: &J,
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    player_id_map: &Map<String, J>,
    cores: &HashSet<String>,
    loop_game_start: f64,
) -> R<()> {
    let uid = unit_uid(event);
    let gameloop_j = get(event, &["_gameloop"]).cloned().unwrap_or(J::Null);
    let gameloop = js_number(get(event, &["_gameloop"]));
    // mort du core → match.loopLength + match.length réassignés (parser.js:1602-1606)
    if cores.contains(&uid) {
        match_.insert("loopLength".into(), gameloop_j.clone());
        match_.insert(
            "length".into(),
            jnum(loops_to_seconds(gameloop - loop_game_start)),
        );
    }
    // mercs (parser.js:1608-1619)
    let mercs = match_
        .get_mut("mercs")
        .and_then(J::as_object_mut)
        .and_then(|m| m.get_mut("units"))
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw("mercs.units absent".into()))?;
    if let Some(unit) = mercs.get_mut(&uid).and_then(J::as_object_mut) {
        let duration = loops_to_seconds(gameloop - js_number(unit.get("loop")));
        unit.insert("duration".into(), jnum(duration));
        unit.get_mut("locations")
            .and_then(J::as_array_mut)
            .ok_or_else(|| Abort::Throw("died: locations absent".into()))?
            .push(json!({
                "x": get(event, &["m_x"]).cloned().unwrap_or(J::Null),
                "y": get(event, &["m_y"]).cloned().unwrap_or(J::Null),
            }));
    }
    // structures (parser.js:1621-1635)
    if let Some(s) = match_
        .get_mut("structures")
        .and_then(J::as_object_mut)
        .and_then(|o| o.get_mut(&uid))
        .and_then(J::as_object_mut)
    {
        s.insert("destroyedLoop".into(), gameloop_j.clone());
        s.insert(
            "destroyed".into(),
            jnum(loops_to_seconds(gameloop - loop_game_start)),
        );
    }
    // unités héros (parser.js:1637-1653)
    let died = loops_to_seconds(gameloop - loop_game_start);
    for pid in js_for_in_keys(player_id_map) {
        let handle = player_id_map
            .get(&pid)
            .and_then(J::as_str)
            .unwrap_or_default()
            .to_owned();
        let units = players
            .get_mut(&handle)
            .and_then(J::as_object_mut)
            .and_then(|p| p.get_mut("units"))
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw(format!("died: joueur {handle} inconnu")))?;
        if units.contains_key(&uid) {
            let last = units
                .get_mut(&uid)
                .and_then(J::as_object_mut)
                .and_then(|u| u.get_mut("lives"))
                .and_then(J::as_array_mut)
                .and_then(|l| l.last_mut())
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("died: vie courante absente".into()))?;
            let born = js_number(last.get("born"));
            last.insert("died".into(), jnum(died));
            last.insert("duration".into(), jnum(died - born));
            last.get_mut("locations")
                .and_then(J::as_array_mut)
                .ok_or_else(|| Abort::Throw("died: locations absent".into()))?
                .push(json!({
                    "x": get(event, &["m_x"]).cloned().unwrap_or(J::Null),
                    "y": get(event, &["m_y"]).cloned().unwrap_or(J::Null),
                    "time": jnum(died),
                }));
        }
    }
    Ok(())
}

/// EndOfGameTalentChoices (parser.js:801-830) : win, internalHeroName, talents Tier*.
pub(super) fn process_talent_choices(
    event: &J,
    players: &mut Map<String, J>,
    player_id_map: &Map<String, J>,
) -> R<()> {
    // event.m_intData[0].m_value : absence → exception JS quelque part avant l'écriture
    let key = js_prop(get(event, &["m_intData", "0", "m_value"]));
    let handle = player_id_map
        .get(&key)
        .and_then(J::as_str)
        .ok_or_else(|| Abort::Throw(format!("talents: tracker id {key} non mappé")))?
        .to_owned();
    let sd = get(event, &["m_stringData"])
        .and_then(J::as_array)
        .ok_or_else(|| Abort::Throw("talents: m_stringData absent".into()))?
        .clone();
    let sd1 = sd
        .get(1)
        .ok_or_else(|| Abort::Throw("talents: m_stringData[1] absent".into()))?;
    let win = get_str(sd1, &["m_value"]) == Some("Win");
    let sd0 = sd
        .first()
        .ok_or_else(|| Abort::Throw("talents: m_stringData[0] absent".into()))?;
    let internal = get(sd0, &["m_value"]).cloned();
    let pdoc = players
        .get_mut(&handle)
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw(format!("talents: joueur {handle} inconnu")))?;
    pdoc.insert("win".into(), J::from(win));
    if let Some(v) = internal {
        pdoc.insert("internalHeroName".into(), v);
    }
    let talents = pdoc
        .get_mut("talents")
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw("talents: objet absent".into()))?;
    for item in &sd {
        // m_key absent → undefined.startsWith lève
        let k = get_str(item, &["m_key"])
            .ok_or_else(|| Abort::Throw("talents: m_key absent".into()))?;
        if k.starts_with("Tier") {
            // legacyTalentKeys=false (défaut) : clé sans espaces
            if let Some(v) = get(item, &["m_value"]) {
                talents.insert(js_strip_ws(k), v.clone());
            }
        }
    }
    Ok(())
}
