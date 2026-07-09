use crate::model::*;
use crate::Error;
use std::collections::HashMap;
use storm_replay::{Replay, Value};

const EPS: f64 = 0.004;

fn loop_to_sec(gameloop: i64) -> f64 {
    (gameloop as f64 - LOOP_OFFSET as f64) / LOOPS_PER_SEC
}

pub(crate) fn build(replay: &Replay) -> Result<ViewerModel, Error> {
    let tracker = replay.tracker_events()?;
    let details = replay.details()?;

    // 1) Étendue de carte via SStatGameEvent{GameStart}.MapSizeX/Y (×4096)
    let (msx, msy) = map_size(&tracker).ok_or(Error::Missing("MapSize"))?;
    let (map_w, map_h) = (msx / FIXED, msy / FIXED); // en tuiles
    let norm_tile = |x: i64, y: i64| (x as f64 / map_w, y as f64 / map_h);
    // Coords game (TargetPoint) = tuiles ×4096, msx/msy aussi ×4096 → le facteur 4096 s'annule.
    let norm_world = |x: i64, y: i64| (x as f64 / msx, y as f64 / msy);

    // 2) unité(tagIndex) → (player, isHero) depuis SUnitBornEvent.
    //    Clé = m_unitTagIndex SEUL (pas (index, recycle)) : SUnitPositionsEvent référence les
    //    unités par index uniquement, donc on résout l'occupant COURANT de cet index — « dernier
    //    Born gagne » donne exactement cela. Sûr pour les héros : une unité-héros ne libère jamais
    //    son index en cours de partie (Died→Revived le conserve), donc l'occupant courant est
    //    toujours le bon héros.
    //    ⚠️ PAS sûr tel quel pour structures/minions (fast-follow) : créés/détruits en masse, un
    //    index peut être recyclé pendant qu'un frère est vivant → résolution ambiguë. Avant
    //    d'étendre ce pipeline aux unités non-héros, clé par (index, recycle) avec résolution par
    //    événement (via m_unitTagRecycle).
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
        if (1..=10).contains(&p) {
            unit_player.insert(idx, (p, is_hero));
        }
    }

    // 3) samples exacts depuis SUnitPositionsEvent (m_items = [idxDelta, x, y]*)
    let mut samples: HashMap<i64, Vec<Sample>> = HashMap::new();
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
        // Le premier index n'est pas 0 : m_firstUnitIndex donne le point de départ de la
        // somme cumulative des deltas (les triplets suivants sont relatifs au précédent).
        let mut idx = field_int(e, "m_firstUnitIndex").unwrap_or(0);
        for tri in items.chunks_exact(3) {
            idx += tri[0]; // index cumulatif
            if let Some(&(p, true)) = unit_player.get(&idx) {
                let (x, y) = norm_tile(tri[1], tri[2]);
                samples.entry(p).or_default().push(Sample {
                    t,
                    x: q3(x),
                    y: q3(y),
                    exact: true,
                });
            }
        }
    }

    // 4) densification via game events SCmd*/TargetPoint clé par _userid → playerId.
    //    Mapping AUTORITAIRE via SPlayerSetupEvent (m_userId → m_playerId) — PAS l'hypothèse
    //    fragile « slot+1 ». `user_to_player` construit une seule fois depuis le tracker.
    let user_to_player = user_to_player(&tracker); // m_userId → m_playerId (1..=10)
    let mut cmd_samples: HashMap<i64, Vec<Sample>> = HashMap::new();
    // US-18 : casts d'aptitude — SCmdEvent avec m_abil non nul. On n'identifie PAS quelle
    // aptitude (m_abilLink) — juste « un cast a eu lieu » ; c'est pourquoi cette collecte vit
    // dans la MÊME passe que la densification de commandes plutôt qu'une passe dédiée.
    let mut cast_times: HashMap<i64, Vec<f64>> = HashMap::new();
    replay.visit_game_events(|ev: Value| {
        let uid = ev
            .field("_userid")
            .and_then(|u| u.field("m_userId"))
            .and_then(Value::as_int);
        let p = uid.and_then(|u| user_to_player.get(&u).copied());

        // Cast d'aptitude : m_abil présent et non nul. Vérifié AVANT le early-return sur
        // TargetPoint ci-dessous — un même SCmdEvent peut porter m_abil ET m_data.TargetPoint,
        // les deux doivent pouvoir être extraits du même événement.
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

    // Tri + dédup temporelle des casts : un cast dans les CAST_DEDUP_SEC qui suivent le
    // précédent cast retenu (même joueur) est jeté — évite un flash quasi continu quand
    // plusieurs SCmdEvent d'affilée portent m_abil pour la même pression de touche.
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

    // 5) fusion samples (exacts + commande), tri par t croissant, dédup des points
    //    quasi-immobiles (delta < EPS depuis le dernier retenu, même exact), clamp [0,1].
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
        });
    }

    // 6) intervalles de vie depuis Born(spawn)/Died/Revived de l'unité-héros du joueur
    let duration_sec = loop_to_sec(replay.header.elapsed_game_loops as i64);
    let mut life: HashMap<i64, Vec<Interval>> = HashMap::new();
    let mut open_from: HashMap<i64, f64> = HashMap::new();

    // On rejoue tracker dans l'ordre pour construire born→died→revived par unité-héros.
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
    // Clore le dernier intervalle ouvert de chaque héros à la fin de la partie.
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

    // 7) deaths depuis SUnitDiedEvent des unités-héros suivies
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

    // 8) heroes déjà construits dans l'ordre de player_ids (trié + dédupliqué) — tri défensif pour
    //    garantir l'invariant côté consommateur ; warnings = vec![] (US-7 plus tard).
    heroes.sort_by_key(|h| h.player_id);

    // 9) structures : SUnitBornEvent contrôlées par 11 (équipe 0) / 12 (équipe 1), types
    //    Town*/KingsCore/HallOfStorms, en excluant le fantôme visuel post-destruction
    //    ("...Destroyed"). Destruction = SUnitDiedEvent apparié par (tagIndex, tagRecycle) — PAS
    //    tagIndex seul : les structures peuvent recycler un index pendant qu'une sœur est vivante.
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

    // 10) feed unifié : réutilise deaths/structures déjà calculés (aucune passe tracker lourde en
    //     plus) + un scan léger des SStatGameEvent{JungleCampCapture} pour les camps. `team` reste
    //     `None` pour les takedowns — c'est le FRONT qui le dérive de `players[]` (garde ce crate
    //     indépendant du mapping playerId→équipe).
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
        if e.field("m_eventName").and_then(Value::as_str_lossy).as_deref() != Some("JungleCampCapture")
        {
            continue;
        }
        let t = loop_to_sec(field_int(e, "_gameloop").unwrap_or(0));
        // Équipe capturante = m_fixedData["TeamID"] en fixed-point (÷4096) : 1 = bleu, 2 = rouge
        // (cf. docs/research/hots-replay-data-reference.md). Notre convention interne : 0 = bleu,
        // 1 = rouge → team = résolu − 1. None si le champ est absent.
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

    Ok(ViewerModel {
        meta: Meta {
            map_name: details.title.clone(),
            map_size: [map_w, map_h],
            duration_sec,
            loop_offset: LOOP_OFFSET,
            viewer_version: VIEWER_VERSION,
        },
        heroes,
        deaths,
        structures,
        events,
        warnings: Vec::new(),
    })
}

fn event_name(e: &Value) -> String {
    e.field("_event")
        .and_then(Value::as_str_lossy)
        .and_then(|s| s.rsplit('.').next().map(str::to_string))
        .unwrap_or_default()
}
fn field_int(e: &Value, k: &str) -> Option<i64> {
    e.field(k).and_then(Value::as_int)
}

// Valeur d'un SStatGameEvent.m_fixedData par clé (même schéma key/value que map_size).
fn fixed_data_int(e: &Value, key: &str) -> Option<i64> {
    let fixed = e.field("m_fixedData").and_then(Value::as_array)?;
    for kv in fixed {
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

// Quantif à 3 décimales (stabilise le golden + réduit le payload).
fn q3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

// AUTORITAIRE : m_userId (des game events `_userid`) → m_playerId tracker (1..=10),
// depuis SPlayerSetupEvent (m_type == 1 = joueur). Remplace l'hypothèse « slot+1 ».
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

// playerId (1..=10) → toon_handle : SPlayerSetupEvent (m_playerId → m_slotId) croisé avec
// details().players[*].working_set_slot_id → toon_handle. Utilisé par le serveur (Chunk 2).
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
