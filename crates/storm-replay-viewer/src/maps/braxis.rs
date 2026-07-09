// US-21 : Braxis Holdout — inférence best-effort des vagues de zergs à partir des morts d'unités
// Zerg (SUnitDiedEvent). On ne dispose PAS d'un event "wave started/ended" explicite dans le
// tracker ; le clustering temporel des morts est une approximation raisonnable (validé
// manuellement sur un replay réel — voir le rapport de chunk, pas un test committé).
use crate::extract::{event_name, field_int, loop_to_sec};
use crate::model::Objective;
use std::collections::HashMap;
use storm_replay::Value;

// Écart (secondes) entre deux morts consécutives au-delà duquel on considère qu'une NOUVELLE
// vague commence. Choisi empiriquement : les zergs d'une même vague meurent en rafale (quelques
// secondes), tandis que l'écart entre deux vagues est de l'ordre de la minute.
const WAVE_GAP_SEC: f64 = 20.0;

pub(crate) fn waves(tracker: &[Value]) -> (Vec<Objective>, Vec<String>) {
    // SUnitDiedEvent ne porte PAS m_unitTypeName (seulement tagIndex/tagRecycle) — il faut
    // résoudre le type via l'événement SUnitBornEvent correspondant. Clé (idx, recycle), PAS
    // idx seul : les unités zerg sont créées/détruites en masse, un index peut être recyclé
    // pendant qu'une unité sœur est encore vivante (même prudence que pour les structures).
    let mut tag_to_zerg: HashMap<(i64, i64), bool> = HashMap::new();
    for e in tracker {
        if event_name(e) != "SUnitBornEvent" {
            continue;
        }
        let Some(type_name) = e.field("m_unitTypeName").and_then(Value::as_str_lossy) else {
            continue;
        };
        let is_zerg_unit = type_name.starts_with("Zerg")
            && type_name != "ZergHiveControlBeacon"
            && type_name != "ZergPathDummy";
        let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
        let recycle = field_int(e, "m_unitTagRecycle").unwrap_or(-1);
        tag_to_zerg.insert((idx, recycle), is_zerg_unit);
    }

    let mut times: Vec<f64> = tracker
        .iter()
        .filter(|e| event_name(e) == "SUnitDiedEvent")
        .filter_map(|e| {
            let idx = field_int(e, "m_unitTagIndex").unwrap_or(-1);
            let recycle = field_int(e, "m_unitTagRecycle").unwrap_or(-1);
            let is_zerg_unit = *tag_to_zerg.get(&(idx, recycle))?;
            is_zerg_unit.then(|| loop_to_sec(field_int(e, "_gameloop").unwrap_or(0)))
        })
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));

    let mut objectives: Vec<Objective> = Vec::new();
    let mut wave_start: Option<f64> = None;
    let mut wave_last: f64 = f64::NEG_INFINITY;
    let mut wave_count: i64 = 0;
    for t in times {
        if wave_start.is_none() || t - wave_last > WAVE_GAP_SEC {
            if let Some(start) = wave_start {
                objectives.push(Objective {
                    t: start,
                    kind: "zerg_wave".to_string(),
                    team: None,
                    value: Some(wave_count),
                });
            }
            wave_start = Some(t);
            wave_count = 0;
        }
        wave_count += 1;
        wave_last = t;
    }
    if let Some(start) = wave_start {
        objectives.push(Objective {
            t: start,
            kind: "zerg_wave".to_string(),
            team: None,
            value: Some(wave_count),
        });
    }

    (objectives, Vec::new()) // Braxis IS covered → pas de warning
}
