//! Draft (bans, picks, ordre) et tableau des scores.

use super::*;

/// Élément du tableau `selections` du draft (parser.js:650-683) : `indexOf` y est un `===` JS —
/// un objet ban ne peut jamais égaler un nom de héros, undefined === undefined est vrai.
#[derive(PartialEq)]
pub(super) enum Sel {
    Undef,
    Val(J),
    Ban,
}

pub(super) fn to_sel(v: Option<&J>) -> Sel {
    match v {
        // J::Null représente l'undefined JS dans notre port (heroAttribute manquant, etc.)
        None | Some(J::Null) => Sel::Undef,
        Some(x) => Sel::Val(x.clone()),
    }
}

/// Draft : `match.bans`, `match.picks` (+ `first`), `players.*.turn` (parser.js:538-691).
pub(super) fn process_draft(
    data: &Ctx,
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    slot_to_toon: &HashMap<String, String>,
) -> R<()> {
    let mode = match_.get("mode").and_then(J::as_i64);
    let draft = [
        "UnrankedDraft",
        "HeroLeague",
        "TeamLeague",
        "StormLeague",
        "Custom",
    ]
    .iter()
    .any(|m| mode == Some(rt_int("GameMode", m)));
    if !draft {
        return Ok(());
    }

    let mut bans: [Vec<J>; 2] = [Vec::new(), Vec::new()];
    // comparaisons JS `build < 66292` : NaN → false des deux côtés dans la collecte des bans
    let build = match_
        .get("version")
        .and_then(|v| get(v, &["m_build"]))
        .and_then(J::as_f64)
        .unwrap_or(f64::NAN);
    let ban = |hero: &J, order: i64, absolute: i64| json!({"hero": hero, "order": order, "absolute": absolute});
    if let Some(attr) = get(&data.attributes, &["scopes", "16"]).and_then(J::as_object) {
        // for..in JS : clés entières en ordre numérique croissant
        let mut keys: Vec<&String> = attr.keys().collect();
        keys.sort_by_key(|k| k.parse::<u64>().unwrap_or(u64::MAX));
        for k in keys {
            let obj = get(&attr[k.as_str()], &["0"])
                .ok_or_else(|| Abort::Throw("attribut scope 16 vide".into()))?;
            let attrid = get(obj, &["attrid"]).and_then(J::as_i64);
            let hero = get(obj, &["value"]).cloned().unwrap_or(J::Null);
            // premiers bans
            if attrid == Some(4023) {
                bans[0].push(ban(&hero, 1, 1));
            } else if attrid == Some(4028) {
                bans[1].push(ban(&hero, 1, 1));
            }
            // deuxièmes bans (ordre différent avant/après le build 66292)
            if build < 66292.0 {
                if attrid == Some(4025) {
                    bans[0].push(ban(&hero, 2, 2));
                } else if attrid == Some(4030) {
                    bans[1].push(ban(&hero, 2, 2));
                }
            } else if build >= 66292.0 {
                if attrid == Some(4025) {
                    bans[0].push(ban(&hero, 1, 2));
                } else if attrid == Some(4030) {
                    bans[1].push(ban(&hero, 1, 2));
                }
            }
            // troisièmes bans
            if attrid == Some(4043) {
                bans[0].push(ban(&hero, 2, 3));
            } else if attrid == Some(4045) {
                bans[1].push(ban(&hero, 2, 3));
            }
        }
    }

    // picks (parser.js:594-639) — pickOrder : (héros, clé m_controllingPlayer)
    let mut pick_order: [Vec<(Option<String>, String)>; 2] = [Vec::new(), Vec::new()];
    let mut picks_first: Option<J> = None;
    // try/catch interne : toute exception vide pickOrder, mais picks.first déjà posé survit
    if collect_pick_order(
        data,
        players,
        slot_to_toon,
        &mut pick_order,
        &mut picks_first,
    )
    .is_err()
    {
        pick_order[0].clear();
        pick_order[1].clear();
    }

    // map vers les noms de héros (try/catch : un échec laisse les [] initiaux)
    let map_picks = |po: &[(Option<String>, String)]| -> R<Vec<J>> {
        po.iter()
            .map(|(_, id)| {
                let pl = slot_to_toon
                    .get(id)
                    .and_then(|t| players.get(t))
                    .ok_or_else(|| Abort::Throw("picks: joueur inconnu".into()))?;
                Ok(get(pl, &["hero"]).cloned().unwrap_or(J::Null))
            })
            .collect()
    };
    let mut picks_arr: [Vec<J>; 2] = [Vec::new(), Vec::new()];
    if let Ok(v0) = map_picks(&pick_order[0]) {
        picks_arr[0] = v0;
        if let Ok(v1) = map_picks(&pick_order[1]) {
            picks_arr[1] = v1;
        }
    }

    // sélections en ordre de draft (parser.js:641-683)
    let (mut a, mut b) = (1usize, 0usize);
    if picks_first.as_ref().and_then(J::as_i64) == Some(0) {
        (a, b) = (b, a);
    }
    let ban_sel = |t: usize, i: usize| {
        if bans[t].get(i).is_some() {
            Sel::Ban
        } else {
            Sel::Undef
        }
    };
    let pick_sel = |t: usize, i: usize| to_sel(picks_arr[t].get(i));
    let mut selections: Vec<Sel> = Vec::new();
    selections.push(ban_sel(a, 0));
    selections.push(ban_sel(b, 0));
    if build < 66292.0 {
        // NB : `else` simple ici (≠ else-if de la collecte des bans) — un build NaN
        // (`NaN < 66292` faux) tombe donc dans la branche bans, comme en JS.
        selections.push(Sel::Val(J::from("N/A")));
        selections.push(Sel::Val(J::from("N/A")));
    } else {
        selections.push(ban_sel(a, 1));
        selections.push(ban_sel(b, 1));
    }
    selections.push(pick_sel(a, 0));
    selections.push(pick_sel(b, 0));
    selections.push(pick_sel(b, 1));
    selections.push(pick_sel(a, 1));
    selections.push(pick_sel(a, 2));
    if build < 66292.0 {
        selections.push(ban_sel(b, 1));
        selections.push(ban_sel(a, 1));
    } else {
        selections.push(ban_sel(b, 2));
        selections.push(ban_sel(a, 2));
    }
    selections.push(pick_sel(b, 2));
    selections.push(pick_sel(b, 3));
    selections.push(pick_sel(a, 3));
    selections.push(pick_sel(a, 4));
    selections.push(pick_sel(b, 4));

    // turn = selections.indexOf(player.hero) pour chaque joueur (parser.js:686-688)
    for pdoc in players.values_mut() {
        let hero_sel = to_sel(pdoc.get("hero"));
        let turn = selections
            .iter()
            .position(|s| *s == hero_sel)
            .map_or(-1, |i| i as i64);
        if let Some(o) = pdoc.as_object_mut() {
            o.insert("turn".into(), J::from(turn));
        }
    }

    let mut picks_obj = Map::new();
    picks_obj.insert("0".into(), J::Array(picks_arr[0].clone()));
    picks_obj.insert("1".into(), J::Array(picks_arr[1].clone()));
    if let Some(f) = picks_first {
        picks_obj.insert("first".into(), f);
    }
    match_.insert("bans".into(), json!({"0": bans[0], "1": bans[1]}));
    match_.insert("picks".into(), J::Object(picks_obj));
    Ok(())
}

/// Boucle SHeroPickedEvent/SHeroSwappedEvent (parser.js:596-622) — corps du try interne.
pub(super) fn collect_pick_order(
    data: &Ctx,
    players: &Map<String, J>,
    slot_to_toon: &HashMap<String, String>,
    pick_order: &mut [Vec<(Option<String>, String)>; 2],
    picks_first: &mut Option<J>,
) -> R<()> {
    for msg in &data.tracker {
        let ev = get_str(msg, &["_event"]);
        if ev == Some("NNet.Replay.Tracker.SHeroPickedEvent") {
            let key = js_prop(get(msg, &["m_controllingPlayer"]));
            // players[playerWorkingSlotID[id]] undefined → player.team lève (catch JS)
            let player = slot_to_toon
                .get(&key)
                .and_then(|t| players.get(t))
                .ok_or_else(|| Abort::Throw("pick: joueur inconnu".into()))?;
            let team = get(player, &["team"]);
            if picks_first.is_none() {
                *picks_first = Some(team.cloned().unwrap_or(J::Null));
            }
            // pickOrder[team] inexistant → .push lève
            let ti = team
                .and_then(J::as_i64)
                .filter(|t| *t == 0 || *t == 1)
                .ok_or_else(|| Abort::Throw("pick: équipe invalide".into()))?
                as usize;
            pick_order[ti].push((get_str(msg, &["m_hero"]).map(str::to_owned), key));
        } else if ev == Some("NNet.Replay.Tracker.SHeroSwappedEvent") {
            let key = js_prop(get(msg, &["m_newControllingPlayer"]));
            let player = slot_to_toon
                .get(&key)
                .and_then(|t| players.get(t))
                .ok_or_else(|| Abort::Throw("swap: joueur inconnu".into()))?;
            let ti = get(player, &["team"])
                .and_then(J::as_i64)
                .filter(|t| *t == 0 || *t == 1)
                .ok_or_else(|| Abort::Throw("swap: équipe invalide".into()))?
                as usize;
            let hero = get_str(msg, &["m_hero"]).map(str::to_owned);
            // findIndex → -1 : pickOrder[team][-1].id lève (catch JS)
            let idx = pick_order[ti]
                .iter()
                .position(|(h, _)| *h == hero)
                .ok_or_else(|| Abort::Throw("swap: héros introuvable".into()))?;
            pick_order[ti][idx].1 = key;
        }
    }
    Ok(())
}

/// `processScoreArray` (parser.js:2421-2458) : SScoreResultEvent → gameStats + awards.
pub(super) fn process_score_array(
    instances: &[J],
    players: &mut Map<String, J>,
    player_id_map: &Map<String, J>,
) -> R<()> {
    for inst in instances {
        let name = get_str(inst, &["m_name"])
            .ok_or_else(|| Abort::Throw("score: m_name absent".into()))?
            .to_owned();
        let values = get(inst, &["m_values"])
            .and_then(J::as_array)
            .ok_or_else(|| Abort::Throw("score: m_values absent".into()))?
            .clone();
        let is_award = name.starts_with("EndOfMatchAward");
        let mut real_index = 0i64;
        for v in &values {
            if v.is_null() {
                return Err(Abort::Throw("score: entrée null".into()));
            }
            let len = v.as_array().map_or(0, Vec::len);
            if len == 0 {
                continue;
            }
            let player_id = real_index + 1;
            let m_value = get(v, &["0", "m_value"]);
            // l'accès players[playerIDMap[id]].gameStats (qui peut lever) n'a lieu côté award
            // que si la valeur vaut 1 (parser.js:2448)
            if !is_award || m_value.and_then(J::as_f64) == Some(1.0) {
                let handle = player_id_map
                    .get(&player_id.to_string())
                    .and_then(J::as_str)
                    .ok_or_else(|| Abort::Throw(format!("score: joueur {player_id} non mappé")))?
                    .to_owned();
                let gs = players
                    .get_mut(&handle)
                    .and_then(J::as_object_mut)
                    .and_then(|p| p.get_mut("gameStats"))
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw(format!("score: joueur {handle} inconnu")))?;
                if is_award {
                    if let Some(awards) = gs.get_mut("awards").and_then(J::as_array_mut) {
                        awards.push(J::from(name.as_str()));
                    }
                } else {
                    gs.insert(name.clone(), m_value.cloned().unwrap_or(J::Null));
                }
            }
            real_index += 1;
        }
    }
    Ok(())
}
