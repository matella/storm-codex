//! Port de `processReplay` (hots-parser/parser.js) — phase par phase, sémantique identique
//! (bugs compris : tout écart volontaire est une tolérance documentée du harnais).

use crate::constants::{hero_attribute, replay_types, rt_int, rt_str, status};
use crate::convert::jval;
use serde_json::{json, Map, Value as J};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use storm_replay::Replay;

/// Résultat de processReplay : `{match, players, status}` (ou `{status}` si rejet).
pub struct Output {
    pub status: i64,
    pub match_: Option<Map<String, J>>,
    pub players: Option<Map<String, J>>,
}

impl Output {
    fn rejected(status: i64) -> Output {
        Output {
            status,
            match_: None,
            players: None,
        }
    }

    pub fn to_json(&self) -> J {
        let mut o = Map::new();
        if let Some(m) = &self.match_ {
            o.insert("match".into(), J::Object(m.clone()));
        }
        if let Some(p) = &self.players {
            o.insert("players".into(), J::Object(p.clone()));
        }
        o.insert("status".into(), J::from(self.status));
        J::Object(o)
    }
}

/// Équivalent du `try/catch` de processReplay : toute « exception » du port → status Failure.
enum Abort {
    /// `return { status: X }` explicite de parser.js.
    Status(i64),
    /// Exception JS (accès indéfini, etc.) → catch → Failure. Le message ne sert qu'au
    /// débogage local (hots-parser ne le sort pas non plus).
    Throw(#[allow(dead_code)] String),
}

impl From<storm_replay::Error> for Abort {
    fn from(e: storm_replay::Error) -> Abort {
        Abort::Throw(e.to_string())
    }
}

type R<T> = Result<T, Abort>;

/// `new Date(ms)` JS sérialisé par JSON.stringify : ISO 8601 UTC avec millisecondes.
fn js_date_iso(ms: f64) -> String {
    let total_ms = ms.trunc() as i64;
    let (mut days, mut rem) = (
        total_ms.div_euclid(86_400_000),
        total_ms.rem_euclid(86_400_000),
    );
    let _ = &mut days;
    let msec = rem % 1000;
    rem /= 1000;
    let (h, m, s) = (rem / 3600, (rem / 60) % 60, rem % 60);
    // civil_from_days (Howard Hinnant) — époque 1970-01-01
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.{msec:03}Z")
}

fn win_file_time_to_date(filetime: f64) -> String {
    js_date_iso(filetime / 10_000.0 - 11_644_473_600_000.0)
}

/// `Number` JS sérialisé : NaN/Infinity → null (JSON.stringify).
fn jnum(f: f64) -> J {
    serde_json::Number::from_f64(f).map_or(J::Null, J::Number)
}

/// `Math.max(a, b)` JS : NaN si un des arguments est NaN (≠ `f64::max` Rust).
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `Math.min(a, b)` JS : NaN si un des arguments est NaN (≠ `f64::min` Rust).
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// Coercition `Number(x)` JS sur nos valeurs JSON : undefined → NaN, null → 0, bool → 0/1,
/// chaîne → parse décimal (vide → 0). Arrays/objets : jamais atteints sur nos données.
fn js_number(v: Option<&J>) -> f64 {
    match v {
        None => f64::NAN,
        Some(J::Null) => 0.0,
        Some(J::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(J::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(J::String(s)) => {
            let t = s.trim_matches(is_js_ws);
            if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Some(_) => f64::NAN,
    }
}

/// Truthiness JS (`if (x)`, `!x`) sur nos valeurs JSON.
fn js_truthy(v: Option<&J>) -> bool {
    match v {
        None | Some(J::Null) => false,
        Some(J::Bool(b)) => *b,
        Some(J::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(J::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `ToString(number)` JS — suffisant pour nos grandeurs (jamais de notation exponentielle) :
/// le Display Rust est aussi la plus courte représentation qui re-parse à l'identique.
fn js_num_str(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else if x == 0.0 {
        "0".into() // couvre -0 (JS : String(-0) === "0")
    } else {
        format!("{x}")
    }
}

/// `parseInt(<nombre>)` JS (ToString puis parseInt) : pour nos grandeurs (< 1e21, jamais de
/// notation exponentielle), équivaut à une troncature vers zéro ; NaN/Infinity → NaN.
fn js_parse_int_num(x: f64) -> f64 {
    if x.is_finite() {
        x.trunc()
    } else {
        f64::NAN
    }
}

/// `loopsToSeconds` (parser.js:3335-3338) : 16 boucles par seconde.
fn loops_to_seconds(loops: f64) -> f64 {
    loops / 16.0
}

/// Ordre d'itération `for..in` JS : indices de tableau canoniques en ordre croissant,
/// puis les autres clés en ordre d'insertion.
fn js_for_in_keys(o: &Map<String, J>) -> Vec<String> {
    let mut idx: Vec<(u32, String)> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    for k in o.keys() {
        match k.parse::<u32>() {
            Ok(n) if n.to_string() == *k && n != u32::MAX => idx.push((n, k.clone())),
            _ => rest.push(k.clone()),
        }
    }
    idx.sort_by_key(|(n, _)| *n);
    idx.into_iter().map(|(_, k)| k).chain(rest).collect()
}

/// Comparateur de tri JS « a < b → -1, a > b → 1, sinon 0 » (NaN → Equal/Greater incohérent
/// comme en JS ; jamais rencontré sur nos données). `sort_by` Rust est stable comme TimSort V8.
fn js_cmp(a: f64, b: f64) -> std::cmp::Ordering {
    if a < b {
        std::cmp::Ordering::Less
    } else if a > b {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// `combineIntervals` (parser.js:3084-3114) : tri par borne gauche puis fusion par
/// chevauchement (`prev[1] >= c[0]`, faux si NaN — comme en JS).
fn combine_intervals(mut intervals: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if intervals.len() <= 1 {
        return intervals;
    }
    intervals.sort_by(|a, b| js_cmp(a.0, b.0));
    let mut result = Vec::new();
    let mut prev = intervals[0];
    for c in &intervals[1..] {
        if prev.1 >= c.0 {
            prev = (prev.0, js_max(prev.1, c.1));
        } else {
            result.push(prev);
            prev = *c;
        }
    }
    result.push(prev);
    result
}

/// Classe `\s` des regex JS (≠ `char::is_whitespace` Rust : pas U+0085, mais U+FEFF).
fn is_js_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\x0B' | '\x0C' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `key.replace(/\s+/g, '')` de parser.js:826.
fn js_strip_ws(s: &str) -> String {
    s.chars().filter(|c| !is_js_ws(*c)).collect()
}

/// Clé de propriété JS (`obj[v]`) pour nos cas : nombre → `ToString(number)` JS (un Number
/// f64 entier comme `jnum(1.0)` donne "1", pas "1.0"), null → "null", undefined → "undefined".
fn js_prop(v: Option<&J>) -> String {
    match v {
        None => "undefined".into(),
        Some(J::Null) => "null".into(),
        Some(J::Bool(b)) => b.to_string(),
        Some(J::Number(n)) => js_num_str(n.as_f64().unwrap_or(f64::NAN)),
        Some(J::String(s)) => s.clone(),
        // jamais atteint sur nos données (arrays/objets : ToString JS différerait)
        Some(other) => other.to_string(),
    }
}

/// `parseInt` JS : préfixe numérique (signe + chiffres), NaN → None.
fn js_parse_int(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().ok().map(|v| sign * v)
}

/// Accès générique « à la JS » : index manquant → undefined (None).
fn get<'a>(v: &'a J, path: &[&str]) -> Option<&'a J> {
    let mut cur = v;
    for p in path {
        cur = match cur {
            J::Object(o) => o.get(*p)?,
            J::Array(a) => a.get(p.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn get_str<'a>(v: &'a J, path: &[&str]) -> Option<&'a str> {
    get(v, path).and_then(J::as_str)
}

/// `getBattletags(buffer, playerList)` — regex sur le battlelobby brut.
fn get_battletags(buffer: &[u8], player_list: &[J]) -> Vec<J> {
    // XRegExp('(\\p{L}|\\d){3,24}#\\d{4,10}[zØ]?', 'g') sur buffer.toString()
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"[\p{L}\d]{3,24}#\d{4,10}[zØ]?").unwrap_or_else(|e| panic!("{e}"))
    });
    let text = String::from_utf8_lossy(buffer);
    let mut tag_map = Vec::new();
    let mut i = 0usize;
    for m in re.find_iter(&text) {
        let full = m.as_str();
        let Some(hash) = full.find('#') else { continue };
        let (name, tag) = (&full[..hash], &full[hash + 1..]);
        let Some(p) = player_list.get(i) else { break };
        if get_str(p, &["m_name"]) == Some(name) {
            let toon = format!(
                "{}-{}-{}-{}",
                get(p, &["m_toon", "m_region"])
                    .and_then(J::as_i64)
                    .unwrap_or_default(),
                get_str(p, &["m_toon", "m_programId"]).unwrap_or_default(),
                get(p, &["m_toon", "m_realm"])
                    .and_then(J::as_i64)
                    .unwrap_or_default(),
                get(p, &["m_toon", "m_id"])
                    .and_then(J::as_i64)
                    .unwrap_or_default(),
            );
            tag_map.push(json!({"tag": tag, "name": name, "full": full, "ToonHandle": toon}));
            i += 1;
        }
    }
    tag_map
}

/// Données décodées une fois (équivalent de `parse(file, AllReplayData)`).
pub(crate) struct Ctx {
    pub header: J,
    pub details: J,
    pub initdata: J,
    pub attributes: J,
    pub tracker: Vec<J>,
    pub tags: Vec<J>,
    // game/message events décodés à la demande (gros streams) — consommés à partir du T6
    #[allow(dead_code)]
    pub replay: Replay,
}

fn attributes_json(a: &storm_replay::Attributes) -> J {
    let mut scopes = Map::new();
    for (scope, attrs) in &a.scopes {
        let mut inner = Map::new();
        for (attrid, values) in attrs {
            inner.insert(
                attrid.to_string(),
                J::Array(
                    values
                        .iter()
                        .map(|v| {
                            json!({
                                "namespace": v.namespace,
                                "attrid": v.attrid,
                                "value": String::from_utf8_lossy(&v.value).into_owned(),
                            })
                        })
                        .collect(),
                ),
            );
        }
        scopes.insert(scope.to_string(), J::Object(inner));
    }
    json!({"source": a.source, "mapNamespace": a.map_namespace, "scopes": scopes})
}

pub fn process_replay(path: &Path, filename: &str) -> Output {
    match process_inner(path, filename) {
        Ok(out) => out,
        Err(Abort::Status(s)) => Output::rejected(s),
        Err(Abort::Throw(msg)) => {
            if std::env::var_os("STORM_STATS_DEBUG").is_some() {
                eprintln!("throw: {msg}");
            }
            Output::rejected(status::FAILURE)
        }
    }
}

fn load(path: &Path) -> R<Ctx> {
    let replay = Replay::open(path)?;
    let header = jval(&replay.header_raw);
    let details = jval(&replay.details_raw()?);
    let initdata = jval(&replay.initdata_raw()?);
    let attributes = attributes_json(&replay.attributes()?);
    let tracker: Vec<J> = replay.tracker_events()?.iter().map(jval).collect();
    let lobby = replay.battlelobby_raw()?;
    let empty = Vec::new();
    let player_list = get(&details, &["m_playerList"])
        .and_then(J::as_array)
        .unwrap_or(&empty);
    let tags = get_battletags(&lobby, player_list);
    Ok(Ctx {
        header,
        details,
        initdata,
        attributes,
        tracker,
        tags,
        replay,
    })
}

fn process_inner(path: &Path, filename: &str) -> R<Output> {
    let data = load(path)?;
    let mut match_ = Map::new();
    let mut players = Map::new();

    // ===== Phase identité (parser.js:256-465) =====
    match_.insert(
        "version".into(),
        get(&data.header, &["m_version"])
            .cloned()
            .unwrap_or(J::Null),
    );
    // (pas de check MAX_SUPPORTED_BUILD : on est toujours en overrideVerifiedBuild)
    match_.insert(
        "type".into(),
        get(&data.header, &["m_type"]).cloned().unwrap_or(J::Null),
    );
    match_.insert(
        "loopLength".into(),
        get(&data.header, &["m_elapsedGameLoops"])
            .cloned()
            .unwrap_or(J::Null),
    );
    match_.insert("filename".into(), J::from(filename));

    let mut mode = get(
        &data.initdata,
        &[
            "m_syncLobbyState",
            "m_gameDescription",
            "m_gameOptions",
            "m_ammId",
        ],
    )
    .cloned()
    .unwrap_or(J::Null);
    if mode.is_null() {
        mode = J::from(-1);
    }
    if mode.as_i64() == Some(rt_int("GameMode", "Brawl")) {
        return Err(Abort::Status(status::UNSUPPORTED));
    }
    match_.insert("mode".into(), mode);

    // carte interne via EndOfGameTalentChoices
    let stat_eventid = rt_int("TrackerEvent", "Stat");
    let eog_talent_choices = rt_str("StatEventType", "EndOfGameTalentChoices");
    for event in &data.tracker {
        if get(event, &["_eventid"]).and_then(J::as_i64) == Some(stat_eventid)
            && get_str(event, &["m_eventName"]) == Some(eog_talent_choices)
        {
            let internal = get_str(event, &["m_stringData", "2", "m_value"]).unwrap_or("");
            match replay_types()["MapType"].get(internal) {
                Some(pretty) => {
                    match_.insert("map".into(), pretty.clone());
                }
                // BUG parser.js:312 reproduit : `ReplayStats` (indéfini) → throw → Failure
                None => return Err(Abort::Throw(format!("carte interne inconnue {internal}"))),
            }
            break;
        }
    }

    let time_utc = get(&data.details, &["m_timeUTC"])
        .and_then(J::as_f64)
        .ok_or_else(|| Abort::Throw("m_timeUTC absent".into()))?;
    match_.insert("date".into(), J::from(win_file_time_to_date(time_utc)));
    match_.insert(
        "rawDate".into(),
        get(&data.details, &["m_timeUTC"])
            .cloned()
            .unwrap_or(J::Null),
    );

    // joueurs préliminaires
    let empty = Vec::new();
    let player_details = get(&data.details, &["m_playerList"])
        .and_then(J::as_array)
        .unwrap_or(&empty)
        .clone();
    let mut player_ids: Vec<J> = Vec::new();
    for (i, pdata) in player_details.iter().enumerate() {
        let mut pdoc = Map::new();
        let mut hero = get(pdata, &["m_hero"]).cloned().unwrap_or(J::Null);
        if hero.as_str() == Some("Lúcio") {
            hero = J::from("Lucio");
        }
        pdoc.insert("hero".into(), hero);
        pdoc.insert(
            "name".into(),
            get(pdata, &["m_name"]).cloned().unwrap_or(J::Null),
        );
        pdoc.insert(
            "uuid".into(),
            get(pdata, &["m_toon", "m_id"]).cloned().unwrap_or(J::Null),
        );
        pdoc.insert(
            "region".into(),
            get(pdata, &["m_toon", "m_region"])
                .cloned()
                .unwrap_or(J::Null),
        );
        pdoc.insert(
            "realm".into(),
            get(pdata, &["m_toon", "m_realm"])
                .cloned()
                .unwrap_or(J::Null),
        );
        let toon = format!(
            "{}-{}-{}-{}",
            get(pdata, &["m_toon", "m_region"])
                .and_then(J::as_i64)
                .unwrap_or_default(),
            get_str(pdata, &["m_toon", "m_programId"]).unwrap_or_default(),
            get(pdata, &["m_toon", "m_realm"])
                .and_then(J::as_i64)
                .unwrap_or_default(),
            get(pdata, &["m_toon", "m_id"])
                .and_then(J::as_i64)
                .unwrap_or_default(),
        );
        pdoc.insert("ToonHandle".into(), J::from(toon.clone()));

        // tag : recherche en avant dans data.tags à partir de l'index i
        for tag in data.tags.iter().skip(i) {
            if get_str(tag, &["ToonHandle"]) == Some(toon.as_str()) {
                if let Some(t) = get_str(tag, &["tag"]).and_then(js_parse_int) {
                    pdoc.insert("tag".into(), J::from(t));
                }
                break;
            }
        }

        match_.insert(
            "region".into(),
            get(pdata, &["m_toon", "m_region"])
                .cloned()
                .unwrap_or(J::Null),
        );
        pdoc.insert(
            "team".into(),
            get(pdata, &["m_teamId"]).cloned().unwrap_or(J::Null),
        );

        pdoc.insert("gameStats".into(), json!({"awards": []}));
        pdoc.insert("talents".into(), json!({}));
        pdoc.insert("takedowns".into(), json!([]));
        pdoc.insert("deaths".into(), json!([]));
        pdoc.insert("bsteps".into(), json!([]));
        pdoc.insert("voiceLines".into(), json!([]));
        pdoc.insert("sprays".into(), json!([]));
        pdoc.insert("taunts".into(), json!([]));
        pdoc.insert("dances".into(), json!([]));
        pdoc.insert("units".into(), json!({}));
        pdoc.insert("votes".into(), J::from(0));
        pdoc.insert("rawDate".into(), match_["rawDate"].clone());
        pdoc.insert("map".into(), match_.get("map").cloned().unwrap_or(J::Null));
        pdoc.insert("date".into(), match_["date"].clone());
        pdoc.insert(
            "build".into(),
            get(&data.header, &["m_version", "m_build"])
                .cloned()
                .unwrap_or(J::Null),
        );
        pdoc.insert("mode".into(), match_["mode"].clone());
        pdoc.insert("version".into(), match_["version"].clone());
        pdoc.insert("globes".into(), json!({"count": 0, "events": []}));

        players.insert(toon.clone(), J::Object(pdoc));
        player_ids.push(J::from(toon));
    }
    match_.insert("playerIDs".into(), J::Array(player_ids));
    match_.insert("heroes".into(), json!([]));
    match_.insert("levelTimes".into(), json!({"0": {}, "1": {}}));

    // playerIDMap (PlayerInit) + GatesOpen + héros/heroLevel par attributs
    // + détection des cores (parser.js:424-479) : leur mort donne la vraie fin de partie (T4)
    let mut player_id_map: Map<String, J> = Map::new();
    let mut cores: HashSet<String> = HashSet::new();
    match_.insert("loopGameStart".into(), J::from(0));
    let player_init = rt_str("StatEventType", "PlayerInit");
    let gates_open = rt_str("StatEventType", "GatesOpen");
    let unit_born_eventid = rt_int("TrackerEvent", "UnitBorn");
    for event in &data.tracker {
        let eid = get(event, &["_eventid"]).and_then(J::as_i64);
        if eid == Some(unit_born_eventid) {
            let t = get_str(event, &["m_unitTypeName"]);
            if t == Some(rt_str("UnitType", "KingsCore"))
                || t == Some(rt_str("UnitType", "VanndarStormpike"))
                || t == Some(rt_str("UnitType", "DrekThar"))
            {
                cores.insert(unit_uid(event));
            }
            continue;
        }
        if eid != Some(stat_eventid) {
            continue;
        }
        let name = get_str(event, &["m_eventName"]).unwrap_or("");
        if name == player_init {
            if get_str(event, &["m_stringData", "0", "m_value"]) == Some("Computer") {
                return Err(Abort::Status(status::COMPUTER_PLAYER_FOUND));
            }
            let tracker_id = get(event, &["m_intData", "0", "m_value"])
                .and_then(J::as_i64)
                .ok_or_else(|| Abort::Throw("PlayerInit sans id".into()))?;
            let handle = get_str(event, &["m_stringData", "1", "m_value"])
                .ok_or_else(|| Abort::Throw("PlayerInit sans handle".into()))?
                .to_owned();
            player_id_map.insert(tracker_id.to_string(), J::from(handle.clone()));

            let scope_path = tracker_id.to_string();
            let attr_name = get_str(
                &data.attributes,
                &["scopes", &scope_path, "4002", "0", "value"],
            )
            .ok_or_else(|| Abort::Throw("attribut 4002 absent".into()))?
            .to_owned();
            let hero_level = get_str(
                &data.attributes,
                &["scopes", &scope_path, "4008", "0", "value"],
            )
            .and_then(js_parse_int)
            .ok_or_else(|| Abort::Throw("attribut 4008 absent".into()))?;

            let pdoc = players
                .get_mut(&handle)
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw(format!("joueur {handle} inconnu")))?;
            pdoc.insert("heroLevel".into(), J::from(hero_level));
            let hero = hero_attribute().get(&attr_name).cloned().unwrap_or(J::Null);
            pdoc.insert("hero".into(), hero.clone());
            if let Some(heroes) = match_.get_mut("heroes").and_then(J::as_array_mut) {
                heroes.push(hero);
            }
        } else if name == gates_open {
            match_.insert(
                "loopGameStart".into(),
                get(event, &["_gameloop"]).cloned().unwrap_or(J::from(0)),
            );
        }
    }
    // ===== Longueur + cosmétiques lobby (parser.js:482-536) =====
    // match.length sera réassigné à la mort du core (parser.js:1604-1605, T4) ; la valeur
    // posée ici (header) est celle copiée dans players.*.length (parser.js:507).
    let length_secs = {
        let ll = match_
            .get("loopLength")
            .and_then(J::as_f64)
            .unwrap_or(f64::NAN);
        let gs = match_
            .get("loopGameStart")
            .and_then(J::as_f64)
            .unwrap_or(f64::NAN);
        (ll - gs) / 16.0
    };
    match_.insert("length".into(), jnum(length_secs));

    let slots = get(
        &data.initdata,
        &["m_syncLobbyState", "m_lobbyState", "m_slots"],
    )
    .and_then(J::as_array)
    .ok_or_else(|| Abort::Throw("m_slots absent".into()))?
    .clone();
    // playerLobbyID : m_userId (clé de propriété JS) → ToonHandle
    let mut player_lobby_id: HashMap<String, String> = HashMap::new();
    for p in &slots {
        let Some(id) = get_str(p, &["m_toonHandle"]).map(str::to_owned) else {
            continue;
        };
        if id.is_empty() || !players.contains_key(&id) {
            continue;
        }
        let pdoc = players
            .get_mut(&id)
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("joueur non-objet".into()))?;
        // assignation JS d'un champ absent → undefined → null au stringify ≡ clé absente
        for (src, dst) in [
            ("m_skin", "skin"),
            ("m_announcerPack", "announcer"),
            ("m_mount", "mount"),
            ("m_hasSilencePenalty", "silenced"),
        ] {
            if let Some(v) = get(p, &[src]) {
                pdoc.insert(dst.into(), v.clone());
            }
        }
        if let Some(v) = get(p, &["m_hasVoiceSilencePenalty"]) {
            pdoc.insert("voiceSilenced".into(), v.clone());
        }
        pdoc.insert("length".into(), jnum(length_secs));
        player_lobby_id.insert(js_prop(get(p, &["m_userId"])), id);
    }

    // playerWorkingSlotID (parser.js:510-534) — NB : 'Hero' codé en dur (pas m_programId)
    let mut slot_to_toon: HashMap<String, String> = HashMap::new();
    for pl in &player_details {
        let toon_obj = get(pl, &["m_toon"]).ok_or_else(|| Abort::Throw("m_toon absent".into()))?;
        let toon = format!(
            "{}-Hero-{}-{}",
            js_prop(get(toon_obj, &["m_region"])),
            js_prop(get(toon_obj, &["m_realm"])),
            js_prop(get(toon_obj, &["m_id"]))
        );
        slot_to_toon.insert(js_prop(get(pl, &["m_workingSetSlotId"])), toon);
    }
    if slot_to_toon.contains_key("null") {
        // fallback : reconstruit depuis m_slots via playerLobbyID
        slot_to_toon.clear();
        for slot in &slots {
            if let Some(toon) = player_lobby_id.get(&js_prop(get(slot, &["m_userId"]))) {
                slot_to_toon.insert(js_prop(get(slot, &["m_workingSetSlotId"])), toon.clone());
            }
        }
    }

    // ===== Draft : bans + picks + turn (parser.js:538-691) =====
    process_draft(&data, &mut match_, &mut players, &slot_to_toon)?;

    // ===== Conteneurs du pipeline unités (parser.js:693-700) =====
    match_.insert("XPBreakdown".into(), json!([]));
    match_.insert("takedowns".into(), json!([]));
    match_.insert("mercs".into(), json!({"captures": [], "units": {}}));
    match_.insert("team0Takedowns".into(), J::from(0));
    match_.insert("team1Takedowns".into(), J::from(0));
    match_.insert("structures".into(), json!({}));

    // ===== match.objective + état hissé par carte (parser.js:702-784) =====
    // Carte absente → TooOld (parser.js:777) ; carte hors liste → UnsupportedMap (parser.js:783).
    let mut ost = ObjState::default();
    {
        let m = |k: &str| match_.get("map").and_then(J::as_str) == Some(rt_str("MapType", k));
        let mut obj = Map::new();
        obj.insert("type".into(), match_.get("map").cloned().unwrap_or(J::Null));
        if m("ControlPoints") {
            obj.insert("0".into(), json!({"count": 0, "damage": 0, "events": []}));
            obj.insert("1".into(), json!({"count": 0, "damage": 0, "events": []}));
        } else if m("TowersOfDoom") {
            obj.insert("sixTowerEvents".into(), json!([]));
            obj.insert("structures".into(), json!([]));
            obj.insert("0".into(), json!({"count": 0, "damage": 0, "events": []}));
            obj.insert("1".into(), json!({"count": 0, "damage": 0, "events": []}));
        } else if m("CursedHollow") {
            obj.insert("tributes".into(), json!([]));
            obj.insert("0".into(), json!({"count": 0, "events": []}));
            obj.insert("1".into(), json!({"count": 0, "events": []}));
        } else if m("DragonShire") {
            ost.moon = Some(json!({}));
            ost.sun = Some(json!({}));
            // `var dragon = null` : null ≡ undefined dans notre port (mêmes effets)
            obj.insert("shrines".into(), json!({"moon": [], "sun": []}));
            obj.insert("0".into(), json!({"count": 0, "events": []}));
            obj.insert("1".into(), json!({"count": 0, "events": []}));
        } else if m("HauntedWoods") {
            ost.current_terror = Some(json!({"0": {}, "1": {}}));
            obj.insert("0".into(), json!({"count": 0, "events": [], "units": []}));
            obj.insert("1".into(), json!({"count": 0, "events": [], "units": []}));
        } else if m("HauntedMines") {
            ost.golems = Some(json!({"0": null, "1": null}));
            obj.insert("0".into(), json!([]));
            obj.insert("1".into(), json!([]));
        } else if m("BattlefieldOfEternity") {
            ost.immortal = Some(json!({}));
            obj.insert("results".into(), json!([]));
        } else if m("Shrines") {
            obj.insert("shrines".into(), json!([]));
            obj.insert("0".into(), json!({"count": 0, "events": []}));
            obj.insert("1".into(), json!({"count": 0, "events": []}));
        } else if m("Crypts") {
            ost.current_spiders = Some(Spiders::default());
            obj.insert("0".into(), json!({"count": 0, "events": []}));
            obj.insert("1".into(), json!({"count": 0, "events": []}));
        } else if m("Volskaya") {
            ost.current_protector = Some(json!({"active": false}));
            obj.insert("0".into(), json!({"count": 0, "events": []}));
            obj.insert("1".into(), json!({"count": 0, "events": []}));
        } else if m("Warhead Junction") {
            ost.nukes = Some(json!({}));
            obj.insert("0".into(), json!({"count": 0, "success": 0, "events": []}));
            obj.insert("1".into(), json!({"count": 0, "success": 0, "events": []}));
            obj.insert("warheads".into(), json!([]));
        } else if m("AlteracPass") {
            obj.insert("0".into(), json!({"events": []}));
            obj.insert("1".into(), json!({"events": []}));
        } else if m("BraxisHoldout") {
            ost.wave_units = Some(json!({"0": {}, "1": {}}));
            ost.wave_id = Some(-1.0);
            ost.beacons = Some(json!({}));
            obj.insert("beacons".into(), json!([]));
            obj.insert("waves".into(), json!([]));
        } else if m("BlackheartsBay") {
            obj.insert("0".into(), json!({"count": 0, "events": []}));
            obj.insert("1".into(), json!({"count": 0, "events": []}));
        } else if m("Hanamura") {
            // match.objective réassigné en entier : pas de champ `type` (parser.js:776)
            obj = Map::new();
            obj.insert("events".into(), json!([]));
        } else if match_.get("map").is_none() {
            return Err(Abort::Status(status::TOO_OLD));
        } else {
            return Err(Abort::Status(status::UNSUPPORTED_MAP));
        }
        match_.insert("objective".into(), J::Object(obj));
    }

    // ===== Boucle tracker, parties T3 + T4 : score screen + talents + votes + globes
    // (parser.js:794-830, 1201-1213, 2421-2458) + pipeline unités (parser.js:831-1979) =====
    let score_eventid = rt_int("TrackerEvent", "Score");
    let upvote = rt_str("StatEventType", "Upvote");
    let regen_globe = rt_str("StatEventType", "RegenGlobePickedUp");
    let periodic_xp = rt_str("StatEventType", "PeriodicXPBreakdown");
    let eog_xp = rt_str("StatEventType", "EndOfGameXPBreakdown");
    let player_death = rt_str("StatEventType", "PlayerDeath");
    let loot_spray = rt_str("StatEventType", "LootSprayUsed");
    let loot_voice_line = rt_str("StatEventType", "LootVoiceLineUsed");
    let camp_capture = rt_str("StatEventType", "CampCapture");
    let level_up = rt_str("StatEventType", "LevelUp");
    let unit_died_eventid = rt_int("TrackerEvent", "UnitDied");
    let unit_owner_change_eventid = rt_int("TrackerEvent", "UnitOwnerChange");
    let unit_positions_eventid = rt_int("TrackerEvent", "UnitPositions");
    let unit_revived_eventid = rt_int("TrackerEvent", "UnitRevived");
    let mut team_xp_end: [Option<J>; 2] = [None, None];
    let mut possible_minion_xp = [0.0f64, 0.0];
    let loop_game_start = match_
        .get("loopGameStart")
        .and_then(J::as_f64)
        .unwrap_or(f64::NAN);
    for event in &data.tracker {
        let eid = get(event, &["_eventid"]).and_then(J::as_i64);
        if eid == Some(score_eventid) {
            let instances = get(event, &["m_instanceList"])
                .and_then(J::as_array)
                .ok_or_else(|| Abort::Throw("m_instanceList absent".into()))?;
            process_score_array(instances, &mut players, &player_id_map)?;
        } else if eid == Some(stat_eventid) {
            let name = get_str(event, &["m_eventName"]).unwrap_or("");
            if name == eog_talent_choices {
                process_talent_choices(event, &mut players, &player_id_map)?;
            } else if name == periodic_xp {
                process_periodic_xp(event, &mut match_, loop_game_start, &possible_minion_xp)?;
            } else if name == eog_xp {
                process_eog_xp(
                    event,
                    &players,
                    &player_id_map,
                    loop_game_start,
                    &possible_minion_xp,
                    &mut team_xp_end,
                )?;
            } else if name == player_death {
                process_player_death(
                    event,
                    &mut match_,
                    &mut players,
                    &player_id_map,
                    loop_game_start,
                )?;
            } else if name == loot_spray || name == loot_voice_line {
                // T6 (BM) : players.*.sprays / players.*.voiceLines (parser.js:942-973).
                // Structures identiques ; seul le tableau cible diffère.
                let id = get_str(event, &["m_stringData", "1", "m_value"])
                    .ok_or_else(|| Abort::Throw("loot: m_stringData[1] absent".into()))?
                    .to_owned();
                let kind = get(event, &["m_stringData", "2", "m_value"])
                    .cloned()
                    .unwrap_or(J::Null);
                let x = get(event, &["m_fixedData", "0", "m_value"])
                    .cloned()
                    .unwrap_or(J::Null);
                let y = get(event, &["m_fixedData", "1", "m_value"])
                    .cloned()
                    .unwrap_or(J::Null);
                let gameloop = get(event, &["_gameloop"]).cloned().unwrap_or(J::Null);
                let time = (js_number(Some(&gameloop)) - loop_game_start) / 16.0;
                let obj = json!({
                    "kind": kind, "x": x, "y": y, "loop": gameloop,
                    "time": jnum(time), "kills": 0, "deaths": 0,
                });
                let field = if name == loot_spray {
                    "sprays"
                } else {
                    "voiceLines"
                };
                players
                    .get_mut(&id)
                    .and_then(J::as_object_mut)
                    .and_then(|p| p.get_mut(field))
                    .and_then(J::as_array_mut)
                    .ok_or_else(|| Abort::Throw(format!("loot: joueur {id} inconnu")))?
                    .push(obj);
            } else if is_map_objective_stat_event(name) {
                // objectifs par carte (parser.js:974-1189, 1214-1238)
                obj_stat_event(name, event, &mut match_, &mut ost, loop_game_start)?;
            } else if name == camp_capture {
                process_camp_capture(event, &mut match_, loop_game_start)?;
            } else if name == level_up {
                process_level_up(
                    event,
                    &mut match_,
                    &players,
                    &player_id_map,
                    loop_game_start,
                )?;
            } else if name == upvote {
                // parser.js:1201-1203 — affectation directe : le dernier événement gagne
                let key = js_prop(get(event, &["m_intData", "0", "m_value"]));
                let handle = player_id_map
                    .get(&key)
                    .and_then(J::as_str)
                    .ok_or_else(|| Abort::Throw(format!("votes: tracker id {key} non mappé")))?
                    .to_owned();
                // m_intData[2] absent → undefined.m_value lève (catch JS)
                let d2 = get(event, &["m_intData", "2"])
                    .ok_or_else(|| Abort::Throw("votes: m_intData[2] absent".into()))?;
                let pdoc = players
                    .get_mut(&handle)
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw(format!("votes: joueur {handle} inconnu")))?;
                pdoc.insert(
                    "votes".into(),
                    get(d2, &["m_value"]).cloned().unwrap_or(J::Null),
                );
            } else if name == regen_globe {
                // parser.js:1205-1213
                let gameloop = get(event, &["_gameloop"]);
                let time =
                    (gameloop.and_then(J::as_f64).unwrap_or(f64::NAN) - loop_game_start) / 16.0;
                let globe = json!({
                    "loop": gameloop.cloned().unwrap_or(J::Null),
                    "time": jnum(time),
                });
                let key = js_prop(get(event, &["m_intData", "0", "m_value"]));
                let handle = player_id_map
                    .get(&key)
                    .and_then(J::as_str)
                    .ok_or_else(|| Abort::Throw(format!("globes: tracker id {key} non mappé")))?
                    .to_owned();
                let globes = players
                    .get_mut(&handle)
                    .and_then(J::as_object_mut)
                    .and_then(|p| p.get_mut("globes"))
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw(format!("globes: joueur {handle} inconnu")))?;
                let count = globes.get("count").and_then(J::as_i64).unwrap_or(0) + 1;
                globes.insert("count".into(), J::from(count));
                globes
                    .get_mut("events")
                    .and_then(J::as_array_mut)
                    .ok_or_else(|| Abort::Throw("globes: events absent".into()))?
                    .push(globe);
            }
        } else if eid == Some(unit_born_eventid) {
            process_unit_born(
                event,
                &mut match_,
                &mut players,
                &player_id_map,
                loop_game_start,
                &mut possible_minion_xp,
                &mut ost,
            )?;
        } else if eid == Some(unit_revived_eventid) {
            process_unit_revived(event, &mut players, &player_id_map, loop_game_start)?;
        } else if eid == Some(unit_positions_eventid) {
            process_unit_positions(
                event,
                &mut match_,
                &mut players,
                &player_id_map,
                loop_game_start,
            )?;
        } else if eid == Some(unit_died_eventid) {
            process_unit_died(
                event,
                &mut match_,
                &mut players,
                &player_id_map,
                &cores,
                loop_game_start,
            )?;
            // branches par carte de UnitDied (parser.js:1655-1896)
            obj_unit_died(event, &mut match_, &mut ost, loop_game_start)?;
        } else if eid == Some(unit_owner_change_eventid) {
            // shrines/terreurs/beacons/payload (parser.js:1897-1979)
            obj_unit_owner_change(
                event,
                &mut match_,
                &mut ost,
                &player_id_map,
                loop_game_start,
            )?;
        }
    }

    // ===== Cleanup objectifs par carte post-boucle (parser.js:1982-2109) =====
    obj_cleanup(&mut match_, &mut ost, loop_game_start)?;

    // ===== Cleanup des vies de héros (parser.js:2111-2124) =====
    let final_loop_len_secs =
        loops_to_seconds(js_number(match_.get("loopLength")) - loop_game_start);
    for pid in js_for_in_keys(&player_id_map) {
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
            .ok_or_else(|| Abort::Throw("cleanup: units absent".into()))?;
        let uids: Vec<String> = units.keys().cloned().collect();
        for uid in uids {
            let last = units
                .get_mut(&uid)
                .and_then(J::as_object_mut)
                .and_then(|u| u.get_mut("lives"))
                .and_then(J::as_array_mut)
                .and_then(|l| l.last_mut())
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("cleanup: vie absente".into()))?;
            // « won't record death time? didn't technically die »
            if !js_truthy(last.get("died")) {
                let born = js_number(last.get("born"));
                last.insert("duration".into(), jnum(final_loop_len_secs - born));
            }
        }
    }

    // ===== XP de fin de partie (parser.js:2126-2129) : undefined → null au stringify =====
    {
        let xpb = match_
            .get_mut("XPBreakdown")
            .and_then(J::as_array_mut)
            .ok_or_else(|| Abort::Throw("XPBreakdown absent".into()))?;
        xpb.push(team_xp_end[0].clone().unwrap_or(J::Null));
        xpb.push(team_xp_end[1].clone().unwrap_or(J::Null));
    }

    // ===== Passe finale (parser.js:2140-2210) =====
    // match.length/team0Takedowns/team1Takedowns sont désormais finaux (mort du core +
    // PlayerDeath, T4) ; match.teams (ids/names/heroes/tags/level) reste T7.
    let final_length = js_number(match_.get("length"));
    let team_takedowns = [
        js_number(match_.get("team0Takedowns")),
        js_number(match_.get("team1Takedowns")),
    ];
    let order: Vec<String> = match_
        .get("playerIDs")
        .and_then(J::as_array)
        .map(|a| a.iter().filter_map(J::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let mut team_ids: [Vec<J>; 2] = [Vec::new(), Vec::new()];
    let mut winner: Option<usize> = None;
    for handle in &order {
        let pdoc = players
            .get_mut(handle)
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("joueur absent".into()))?;
        let team = pdoc.get("team").and_then(J::as_i64);
        let win = matches!(pdoc.get("win"), Some(J::Bool(true)));
        let gs = pdoc
            .get_mut("gameStats")
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("gameStats absent".into()))?;
        let num = |k: &str| gs.get(k).and_then(J::as_f64).unwrap_or(f64::NAN);
        let (takedowns, deaths) = (num("Takedowns"), num("Deaths"));
        let (hero_damage, damage_taken) = (num("HeroDamage"), num("DamageTaken"));
        let healing = num("Healing") + num("SelfHealing") + num("ProtectionGivenToAllies");
        let xp_contribution = num("ExperienceContribution");
        gs.insert("KDA".into(), jnum(takedowns / js_max(deaths, 1.0)));
        gs.insert(
            "damageDonePerDeath".into(),
            jnum(hero_damage / js_max(1.0, deaths)),
        );
        gs.insert(
            "damageTakenPerDeath".into(),
            jnum(damage_taken / js_max(1.0, deaths)),
        );
        gs.insert(
            "healingDonePerDeath".into(),
            jnum(healing / js_max(1.0, deaths)),
        );
        gs.insert("DPM".into(), jnum(hero_damage / (final_length / 60.0)));
        gs.insert("HPM".into(), jnum(healing / (final_length / 60.0)));
        gs.insert("XPM".into(), jnum(xp_contribution / (final_length / 60.0)));
        if team == Some(rt_int("TeamType", "Blue")) {
            gs.insert(
                "KillParticipation".into(),
                jnum(takedowns / team_takedowns[0]),
            );
            gs.insert("length".into(), jnum(final_length));
            team_ids[0].push(J::from(handle.as_str()));
            if win {
                winner = Some(0);
            }
        } else if team == Some(rt_int("TeamType", "Red")) {
            gs.insert(
                "KillParticipation".into(),
                jnum(takedowns / team_takedowns[1]),
            );
            gs.insert("length".into(), jnum(final_length));
            team_ids[1].push(J::from(handle.as_str()));
            if win {
                winner = Some(1);
            }
        }
    }
    // match sans vainqueur = incomplet (parser.js:2205-2208)
    let Some(w) = winner else {
        return Err(Abort::Status(status::INCOMPLETE));
    };
    match_.insert("winner".into(), J::from(w as i64));
    match_.insert("winningPlayers".into(), J::Array(team_ids[w].clone()));

    // ===== messages + BM (taunts/dances/bsteps) — parser.js:2214-2346 =====
    // (with/against dépendent de match.teams complet → T7)
    process_messages_and_bm(
        &data,
        &mut match_,
        &mut players,
        &player_lobby_id,
        loop_game_start,
    )?;

    // ===== collectTeamStats, partie structures (parser.js:2659+, 2704-2729) =====
    // Interne, consommé par getFirstFortTeam/getFirstKeepTeam : par équipe, nom de structure →
    // `first` (instant de la première destruction chez l'adversaire, init match.length).
    // Le reste (mercUptime, KDA équipe, PPK, totals…) va dans match.teams[].stats : T7.
    let team_structures: [HashMap<String, f64>; 2] = {
        let empty = Map::new();
        let structs = match_
            .get("structures")
            .and_then(J::as_object)
            .unwrap_or(&empty);
        let mut out = [HashMap::new(), HashMap::new()];
        for (t, slot) in out.iter_mut().enumerate() {
            let other = 1 - t;
            for s in structs.values() {
                let name = js_prop(get(s, &["name"]));
                let entry = slot.entry(name).or_insert(final_length);
                // `'destroyed' in structure` — présence de la clé, même null
                if get(s, &["destroyed"]).is_some() && js_number(get(s, &["team"])) == other as f64
                {
                    *entry = js_min(*entry, js_number(get(s, &["destroyed"])));
                }
            }
        }
        out
    };

    // ===== computeLevelDiff (parser.js:2973-3045) =====
    // NB : mute match.levelTimes — chaque entrée reçoit team = "0"/"1" (chaîne, clé for..in),
    // visible dans la sortie sérialisée.
    let mut adv: Vec<(String, f64, f64)> = Vec::new(); // (team, time, level)
    {
        let lt = match_
            .get_mut("levelTimes")
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("levelTimes absent".into()))?;
        for t in js_for_in_keys(lt) {
            let bucket = lt
                .get_mut(&t)
                .and_then(J::as_object_mut)
                .ok_or_else(|| Abort::Throw("levelTimes: équipe non-objet".into()))?;
            let keys = js_for_in_keys(bucket);
            for k in &keys {
                let lobj = bucket
                    .get_mut(k)
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw("levelTimes: entrée non-objet".into()))?;
                lobj.insert("team".into(), J::from(t.as_str()));
                adv.push((
                    t.clone(),
                    js_number(lobj.get("time")),
                    js_number(lobj.get("level")),
                ));
            }
            // niveau final au temps match.length ; équipe sans LevelUp →
            // levelTimes[t][undefined].level lève (catch JS)
            let last = keys
                .last()
                .ok_or_else(|| Abort::Throw("levelTimes: équipe vide".into()))?;
            let level = js_number(bucket.get(last).and_then(|l| get(l, &["level"])));
            adv.push((t.clone(), final_length, level));
        }
    }
    adv.sort_by(|a, b| js_cmp(a.1, b.1));
    let mut start = 0.0f64;
    let mut current_diff = 0.0f64;
    let (mut blue_level, mut red_level) = (1.0f64, 1.0f64);
    let mut timeline: Vec<(f64, f64, f64)> = Vec::new(); // (start, end, levelDiff)
    for (team, time, level) in &adv {
        if team == "0" {
            blue_level = *level;
        } else {
            red_level = *level;
        }
        let new_diff = blue_level - red_level;
        // `!==` JS : NaN ≠ NaN aussi — `!=` f64 a la même table de vérité
        if new_diff != current_diff {
            timeline.push((start, *time, current_diff));
            start = *time;
            current_diff = new_diff;
        }
    }
    timeline.push((start, final_length, blue_level - red_level));
    match_.insert(
        "levelAdvTimeline".into(),
        J::Array(
            timeline
                .iter()
                .map(|(s, e, d)| {
                    json!({"start": jnum(*s), "end": jnum(*e), "levelDiff": jnum(*d),
                           "length": jnum(e - s)})
                })
                .collect(),
        ),
    );

    // ===== analyzeLevelAdv (parser.js:3047-3080) — partie copiée dans gameStats =====
    // (maxLevelAdv/avgLevelAdv ne vont que dans match.teams[].stats : T7)
    let mut level_adv_time = [0.0f64, 0.0];
    for (s, e, d) in &timeline {
        if *d > 0.0 {
            level_adv_time[0] += e - s;
        } else if *d < 0.0 {
            level_adv_time[1] += e - s;
        }
    }
    let level_adv_pct = [
        level_adv_time[0] / final_length,
        level_adv_time[1] / final_length,
    ];

    // ===== analyzeUptime, partie lifespan (parser.js:2793-2795 + 2831-2847) =====
    // (uptime/wipes/aces d'équipe : T7)
    for handle in &order {
        let pdoc = players
            .get_mut(handle)
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("lifespan: joueur absent".into()))?;
        let length = js_number(pdoc.get("length"));
        let mut intervals: Vec<(f64, f64)> = Vec::new();
        if let Some(units) = pdoc.get("units").and_then(J::as_object) {
            for unit in units.values() {
                if let Some(lives) = get(unit, &["lives"]).and_then(J::as_array) {
                    for life in lives {
                        let born = js_number(get(life, &["born"]));
                        if js_truthy(get(life, &["died"])) {
                            intervals.push((born, js_number(get(life, &["died"]))));
                        } else {
                            intervals.push((born, length));
                        }
                    }
                }
            }
        }
        let spans = combine_intervals(intervals);
        pdoc.insert(
            "lifespan".into(),
            J::Array(
                spans
                    .iter()
                    .map(|(a, b)| json!([jnum(*a), jnum(*b)]))
                    .collect(),
            ),
        );
    }

    // ===== XP passive (parser.js:2355-2375) : lève si un EndOfGameXPBreakdown manque =====
    let trickle = [
        js_number(get(
            team_xp_end[0]
                .as_ref()
                .ok_or_else(|| Abort::Throw("team0XPEnd absent".into()))?,
            &["breakdown", "TrickleXP"],
        )),
        js_number(get(
            team_xp_end[1]
                .as_ref()
                .ok_or_else(|| Abort::Throw("team1XPEnd absent".into()))?,
            &["breakdown", "TrickleXP"],
        )),
    ];
    let baseline_passive = 20.0 * final_length; // « normal rate is 20 xp/s »
    let passive_rate = [trickle[0] / final_length, trickle[1] / final_length];
    let passive_diff = [trickle[0] / baseline_passive, trickle[1] / baseline_passive];
    let passive_gain = [trickle[0] - baseline_passive, trickle[1] - baseline_passive];

    // ===== Copies stats d'équipe → gameStats (parser.js:2379-2391), partie T4 =====
    // passiveXP* et levelAdv* (calculés directement) ; aces/wipes/timeWithHeroAdv/
    // pctWithHeroAdv sont recopiés depuis match.teams[].stats par build_teams (T7).
    for handle in &order {
        let pdoc = players
            .get_mut(handle)
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("teamStats: joueur absent".into()))?;
        let t = match pdoc.get("team").and_then(J::as_i64) {
            Some(0) => 0usize,
            Some(1) => 1usize,
            // match.teams[team] indéfini → .stats lève (catch JS)
            _ => return Err(Abort::Throw("teamStats: équipe invalide".into())),
        };
        let gs = pdoc
            .get_mut("gameStats")
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("teamStats: gameStats absent".into()))?;
        gs.insert("passiveXPRate".into(), jnum(passive_rate[t]));
        gs.insert("passiveXPDiff".into(), jnum(passive_diff[t]));
        gs.insert("passiveXPGain".into(), jnum(passive_gain[t]));
        gs.insert("levelAdvTime".into(), jnum(level_adv_time[t]));
        gs.insert("levelAdvPct".into(), jnum(level_adv_pct[t]));
    }

    // ===== match.teams + collectTeamStats + analyzeLevelAdv + analyzeUptime (T7) =====
    // (parser.js:2132-2191, 2348-2391, 2659-2926) — peuple match.teams[].stats puis recopie
    // aces/wipes/timeWithHeroAdv/pctWithHeroAdv dans gameStats.
    build_teams(
        &mut match_,
        &mut players,
        &order,
        final_length,
        &passive_rate,
        &passive_diff,
        &passive_gain,
    )?;

    // ===== with/against (parser.js:2194-2203) — match.teams complet : référence à l'équipe =====
    for handle in &order {
        let team = players
            .get(handle)
            .and_then(|p| get(p, &["team"]).and_then(J::as_i64))
            .ok_or_else(|| Abort::Throw("with/against: équipe absente".into()))?;
        let with = match_
            .get("teams")
            .and_then(|t| get(t, &[team.to_string().as_str()]))
            .cloned()
            .unwrap_or(J::Null);
        let against = match_
            .get("teams")
            .and_then(|t| get(t, &[(1 - team).to_string().as_str()]))
            .cloned()
            .unwrap_or(J::Null);
        let pdoc = players
            .get_mut(handle)
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("with/against: joueur absent".into()))?;
        pdoc.insert("with".into(), with);
        // against assigné seulement pour team 0/1 (parser.js:2198-2202) — toujours le cas ici
        pdoc.insert("against".into(), against);
    }

    // firstPickWin (parser.js:2397-2403) : picks.first === winner ; pas de draft (QM) → false.
    let first_pick_win = match match_.get("picks") {
        Some(p) => get(p, &["first"]).and_then(J::as_i64) == Some(w as i64),
        None => false,
    };
    match_.insert("firstPickWin".into(), J::from(first_pick_win));

    // ===== firstObjective/firstObjectiveWin (parser.js:2405-2406, 3116-3302) =====
    let first_objective = get_first_objective_team(&match_);
    let first_objective_win = js_strict_eq(Some(&J::from(w as i64)), Some(&first_objective));
    match_.insert("firstObjective".into(), first_objective);
    match_.insert("firstObjectiveWin".into(), J::from(first_objective_win));

    // ===== firstFort / firstKeep (parser.js:2407-2410 + 3304-3328) =====
    let first_fort: i64 = {
        // pas de Fort → undefined.first lève (catch JS)
        let t0 = team_structures[0]
            .get("Fort")
            .copied()
            .ok_or_else(|| Abort::Throw("structures.Fort absent (équipe 0)".into()))?;
        let t1 = team_structures[1]
            .get("Fort")
            .copied()
            .ok_or_else(|| Abort::Throw("structures.Fort absent (équipe 1)".into()))?;
        if t0 > t1 {
            1
        } else if t0 < t1 {
            0
        } else {
            -1 // même instant (ou NaN)
        }
    };
    // « towers of doom has keeps but they upgrade from forts » : absent → -2
    let first_keep: i64 = match (
        team_structures[0].get("Keep"),
        team_structures[1].get("Keep"),
    ) {
        (Some(t0), Some(t1)) => {
            if t0 > t1 {
                1
            } else if t0 < t1 {
                0
            } else {
                -1
            }
        }
        _ => -2,
    };
    match_.insert("firstFort".into(), J::from(first_fort));
    match_.insert("firstKeep".into(), J::from(first_keep));
    match_.insert("firstFortWin".into(), J::from(w as i64 == first_fort));
    match_.insert("firstKeepWin".into(), J::from(w as i64 == first_keep));

    Ok(Output {
        status: status::OK,
        match_: Some(match_),
        players: Some(players),
    })
}

/// Construit `match.teams` (squelette + per-player) puis `collectTeamStats` (2659-2787),
/// `analyzeLevelAdv` (3047-3081), `analyzeUptime` (2789-2926) et les copies passive XP, et
/// recopie aces/wipes/timeWithHeroAdv/pctWithHeroAdv dans gameStats (2385-2391).
#[allow(clippy::too_many_arguments)]
fn build_teams(
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    order: &[String],
    length: f64,
    passive_rate: &[f64; 2],
    passive_diff: &[f64; 2],
    passive_gain: &[f64; 2],
) -> R<()> {
    // ---- squelette match.teams + remplissage par joueur (parser.js:2132-2191) ----
    let team_td = [
        js_number(match_.get("team0Takedowns")),
        js_number(match_.get("team1Takedowns")),
    ];
    let mut teams: [Map<String, J>; 2] = Default::default();
    for (t, team) in teams.iter_mut().enumerate() {
        team.insert("ids".into(), json!([]));
        team.insert("names".into(), json!([]));
        team.insert("heroes".into(), json!([]));
        team.insert("tags".into(), json!([]));
        team.insert("takedowns".into(), jnum(team_td[t]));
    }
    for handle in order {
        let p = players.get(handle);
        let team = p.and_then(|p| get(p, &["team"]).and_then(J::as_i64));
        let t = match team {
            Some(0) => 0usize,
            Some(1) => 1usize,
            _ => continue, // observateurs : ignorés (parser.js n'a pas de branche)
        };
        let level = p
            .and_then(|p| get(p, &["gameStats", "Level"]))
            .cloned()
            .unwrap_or(J::Null);
        let hero = p
            .and_then(|p| get(p, &["hero"]))
            .cloned()
            .unwrap_or(J::Null);
        let name = p
            .and_then(|p| get(p, &["name"]))
            .cloned()
            .unwrap_or(J::Null);
        let tag = p.and_then(|p| get(p, &["tag"])).cloned().unwrap_or(J::Null);
        teams[t].insert("level".into(), level);
        for (field, val) in [("heroes", hero), ("names", name), ("tags", tag)] {
            if let Some(a) = teams[t].get_mut(field).and_then(J::as_array_mut) {
                a.push(val);
            }
        }
        if let Some(a) = teams[t].get_mut("ids").and_then(J::as_array_mut) {
            a.push(J::from(handle.as_str()));
        }
    }
    let team_ids: [Vec<J>; 2] = [
        teams[0]
            .get("ids")
            .and_then(J::as_array)
            .cloned()
            .unwrap_or_default(),
        teams[1]
            .get("ids")
            .and_then(J::as_array)
            .cloned()
            .unwrap_or_default(),
    ];

    // ---- collectTeamStats (parser.js:2659-2787) ----
    let empty = Map::new();
    let mercs_captures = match_
        .get("mercs")
        .and_then(|m| get(m, &["captures"]))
        .and_then(J::as_array)
        .cloned()
        .unwrap_or_default();
    let mercs_units: Vec<J> = match_
        .get("mercs")
        .and_then(|m| get(m, &["units"]))
        .and_then(J::as_object)
        .map(|u| u.values().cloned().collect())
        .unwrap_or_default();
    let structures_all: Vec<J> = match_
        .get("structures")
        .and_then(J::as_object)
        .map(|s| s.values().cloned().collect())
        .unwrap_or_default();
    let takedowns = match_
        .get("takedowns")
        .and_then(J::as_array)
        .cloned()
        .unwrap_or_default();
    let level_times = match_.get("levelTimes").cloned().unwrap_or(J::Null);

    const TOTAL_KEYS: [&str; 16] = [
        "DamageTaken",
        "CreepDamage",
        "Healing",
        "HeroDamage",
        "MinionDamage",
        "SelfHealing",
        "SiegeDamage",
        "ProtectionGivenToAllies",
        "TeamfightDamageTaken",
        "TeamfightHealingDone",
        "TeamfightHeroDamage",
        "TimeCCdEnemyHeroes",
        "TimeRootingEnemyHeroes",
        "TimeSpentDead",
        "TimeStunningEnemyHeroes",
        "TimeSilencingEnemyHeroes",
    ];

    let mut team_stats: [Map<String, J>; 2] = Default::default();
    for t in 0..2 {
        let other = 1 - t;
        let stats = &mut team_stats[t];

        // NB : structure.team / merc.team sont des flottants (0.0/1.0) ; en JS `0.0 === 0`
        // est vrai → comparaison numérique, pas as_i64 (qui échoue sur un Number flottant).
        let team_eq = |v: Option<&J>, k: usize| js_number(v) == k as f64;

        // merc captures
        let merc_captures = mercs_captures
            .iter()
            .filter(|c| team_eq(get(c, &["team"]), t))
            .count() as i64;
        stats.insert("mercCaptures".into(), J::from(merc_captures));

        // merc uptime (combine intervals, somme des durées)
        let mut intervals: Vec<(f64, f64)> = Vec::new();
        for unit in &mercs_units {
            if team_eq(get(unit, &["team"]), t) {
                let time = js_number(get(unit, &["time"]));
                let end = if get(unit, &["duration"]).is_none() {
                    length
                } else {
                    time + js_number(get(unit, &["duration"]))
                };
                intervals.push((time, end));
            }
        }
        let merged = combine_intervals(intervals);
        let merc_uptime: f64 = merged.iter().map(|(a, b)| b - a).sum();
        stats.insert("mercUptime".into(), jnum(merc_uptime));
        stats.insert("mercUptimePercent".into(), jnum(merc_uptime / length));

        // structures par nom {lost, destroyed, first}
        let mut structures: Map<String, J> = Map::new();
        for s in &structures_all {
            let name = js_prop(get(s, &["name"]));
            if !structures.contains_key(&name) {
                structures.insert(
                    name.clone(),
                    json!({"lost": 0, "destroyed": 0, "first": jnum(length)}),
                );
            }
            if get(s, &["destroyed"]).is_some() {
                let st = get(s, &["team"]);
                let entry = structures.get_mut(&name).and_then(J::as_object_mut);
                if let Some(entry) = entry {
                    if team_eq(st, t) {
                        let v = entry.get("lost").and_then(J::as_i64).unwrap_or(0) + 1;
                        entry.insert("lost".into(), J::from(v));
                    } else if team_eq(st, other) {
                        let v = entry.get("destroyed").and_then(J::as_i64).unwrap_or(0) + 1;
                        entry.insert("destroyed".into(), J::from(v));
                        let first = js_min(
                            js_number(entry.get("first")),
                            js_number(get(s, &["destroyed"])),
                        );
                        entry.insert("first".into(), jnum(first));
                    }
                }
            }
        }
        stats.insert("structures".into(), J::Object(structures));

        // KDA d'équipe
        stats.insert("KDA".into(), jnum(team_td[t] / js_max(team_td[other], 1.0)));

        // people per kill
        let mut ppk = 0.0;
        for td in &takedowns {
            let victim = get(td, &["victim", "player"]).cloned().unwrap_or(J::Null);
            if team_ids[other]
                .iter()
                .any(|id| js_strict_eq(Some(id), Some(&victim)))
            {
                ppk += get(td, &["killers"])
                    .and_then(J::as_array)
                    .map_or(0.0, |k| k.len() as f64);
            }
        }
        stats.insert("PPK".into(), jnum(ppk / js_max(team_td[t], 1.0)));

        // timeTo10 / timeTo20 (clés conditionnelles)
        let lt = get(&level_times, &[t.to_string().as_str()]).unwrap_or(&J::Null);
        for lvl in ["10", "20"] {
            if let Some(e) = get(lt, &[lvl]) {
                stats.insert(
                    format!("timeTo{lvl}"),
                    get(e, &["time"]).cloned().unwrap_or(J::Null),
                );
            }
        }

        // totals
        let mut totals: Map<String, J> = Map::new();
        let mut acc = [0.0f64; 16];
        for handle in order {
            let p = players.get(handle);
            if p.and_then(|p| get(p, &["team"]).and_then(J::as_i64)) == Some(t as i64) {
                for (i, k) in TOTAL_KEYS.iter().enumerate() {
                    acc[i] += js_number(p.and_then(|p| get(p, &["gameStats", k])));
                }
            }
        }
        for (i, k) in TOTAL_KEYS.iter().enumerate() {
            totals.insert((*k).into(), jnum(acc[i]));
        }
        let time_spent_dead = acc[13]; // TimeSpentDead
        let avg_dead = time_spent_dead / 5.0;
        totals.insert("avgTimeSpentDead".into(), jnum(avg_dead));
        totals.insert("timeDeadPct".into(), jnum(avg_dead / length));
        stats.insert("totals".into(), J::Object(totals));
        let _ = &empty;
    }

    // ---- analyzeLevelAdv (parser.js:3047-3081) depuis match.levelAdvTimeline ----
    let timeline = match_
        .get("levelAdvTimeline")
        .and_then(J::as_array)
        .cloned()
        .unwrap_or_default();
    let mut adv_time = [0.0f64, 0.0];
    let mut max_adv = [0.0f64, 0.0];
    let mut lvl_avg = [0.0f64, 0.0];
    for lv in &timeline {
        let diff = js_number(get(lv, &["levelDiff"]));
        let len = js_number(get(lv, &["length"]));
        let idx = if diff > 0.0 {
            Some(0)
        } else if diff < 0.0 {
            Some(1)
        } else {
            None
        };
        if let Some(i) = idx {
            adv_time[i] += len;
            lvl_avg[i] += len * diff.abs();
            if diff.abs() > max_adv[i] {
                max_adv[i] = diff.abs();
            }
        }
    }
    for t in 0..2 {
        team_stats[t].insert("levelAdvTime".into(), jnum(adv_time[t]));
        team_stats[t].insert("maxLevelAdv".into(), jnum(max_adv[t]));
        team_stats[t].insert("avgLevelAdv".into(), jnum(lvl_avg[t] / length));
        team_stats[t].insert("levelAdvPct".into(), jnum(adv_time[t] / length));
    }

    // ---- analyzeUptime (parser.js:2789-2926) ----
    let uptime = [
        analyze_team_uptime(0, players, order),
        analyze_team_uptime(1, players, order),
    ];
    let lifespan_json = |tl: &[(f64, f64)]| -> J {
        J::Array(
            tl.iter()
                .map(|(t, h)| json!({"time": jnum(*t), "heroes": jnum(*h)}))
                .collect(),
        )
    };
    let histo_json = |hc: &[(f64, f64)]| -> J {
        // heroCount : objet {str: durée}, clés dans l'ordre de première rencontre
        let mut m = Map::new();
        for (s, d) in hc {
            m.insert(js_prop(Some(&jnum(*s))), jnum(*d));
        }
        J::Object(m)
    };
    for t in 0..2 {
        let u = &uptime[t];
        team_stats[t].insert("uptime".into(), lifespan_json(&u.team_lifespan));
        team_stats[t].insert("uptimeHistogram".into(), histo_json(&u.hero_count));
        team_stats[t].insert("wipes".into(), J::from(u.wipes));
        team_stats[t].insert("avgHeroesAlive".into(), jnum(u.avg_heroes_alive));
        team_stats[t].insert("aces".into(), J::from(uptime[1 - t].wipes)); // aces = wipes adverse
    }
    for t in 0..2 {
        let twha = time_with_hero_adv(
            &uptime[t].team_lifespan,
            &uptime[1 - t].team_lifespan,
            length,
        );
        team_stats[t].insert("timeWithHeroAdv".into(), jnum(twha));
        team_stats[t].insert("pctWithHeroAdv".into(), jnum(twha / length));
    }

    // ---- passive XP → stats (parser.js:2357-2375) ----
    for t in 0..2 {
        team_stats[t].insert("passiveXPRate".into(), jnum(passive_rate[t]));
        team_stats[t].insert("passiveXPDiff".into(), jnum(passive_diff[t]));
        team_stats[t].insert("passiveXPGain".into(), jnum(passive_gain[t]));
    }

    // monte les stats dans les équipes puis dans match.teams
    let mut teams_map = Map::new();
    for (t, mut team) in teams.into_iter().enumerate() {
        team.insert(
            "stats".into(),
            J::Object(std::mem::take(&mut team_stats[t])),
        );
        teams_map.insert(t.to_string(), J::Object(team));
    }
    match_.insert("teams".into(), J::Object(teams_map));

    // ---- copies aces/wipes/timeWithHeroAdv/pctWithHeroAdv dans gameStats (2385-2388) ----
    for handle in order {
        let team = players
            .get(handle)
            .and_then(|p| get(p, &["team"]).and_then(J::as_i64));
        let t = match team {
            Some(0) => 0usize,
            Some(1) => 1usize,
            _ => return Err(Abort::Throw("teamStats copie: équipe invalide".into())),
        };
        let copy: Vec<(String, J)> = ["aces", "wipes", "timeWithHeroAdv", "pctWithHeroAdv"]
            .iter()
            .map(|k| ((*k).into(), team_stats_value(match_, t, k)))
            .collect();
        let gs = players
            .get_mut(handle)
            .and_then(J::as_object_mut)
            .and_then(|p| p.get_mut("gameStats"))
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("teamStats copie: gameStats absent".into()))?;
        for (k, v) in copy {
            gs.insert(k, v);
        }
    }
    Ok(())
}

fn team_stats_value(match_: &Map<String, J>, t: usize, key: &str) -> J {
    match_
        .get("teams")
        .and_then(|teams| get(teams, &[t.to_string().as_str(), "stats", key]))
        .cloned()
        .unwrap_or(J::Null)
}

struct TeamUptime {
    team_lifespan: Vec<(f64, f64)>, // (time, heroes)
    hero_count: Vec<(f64, f64)>,    // (heroes, durée), ordre de première rencontre
    wipes: i64,
    avg_heroes_alive: f64,
}

/// `analyzeTeamPlayerUptime` (parser.js:2849-2926).
fn analyze_team_uptime(team: i64, players: &Map<String, J>, order: &[String]) -> TeamUptime {
    let mut events: Vec<(f64, f64)> = Vec::new(); // (time, str)
    let mut match_length = 0.0;
    for handle in order {
        let p = players.get(handle);
        if p.and_then(|p| get(p, &["team"]).and_then(J::as_i64)) != Some(team) {
            continue;
        }
        match_length = js_number(p.and_then(|p| get(p, &["length"])));
        let lifespan = p
            .and_then(|p| get(p, &["lifespan"]))
            .and_then(J::as_array)
            .cloned()
            .unwrap_or_default();
        for life in &lifespan {
            let l0 = js_number(get(life, &["0"]));
            let l1 = js_number(get(life, &["1"]));
            if l0 > 0.0 {
                events.push((l0, 1.0));
            }
            if l1 != match_length {
                events.push((l1, -1.0));
            }
        }
    }
    events.sort_by(|a, b| js_cmp(a.0, b.0));

    let mut team_lifespan = vec![(0.0f64, 5.0f64)];
    let mut current = 5.0;
    for (time, str_) in &events {
        current += str_;
        team_lifespan.push((*time, current));
    }

    let mut hero_count: Vec<(f64, f64)> = Vec::new();
    let mut wipes = 0i64;
    let mut avg = 0.0;
    for i in 0..team_lifespan.len() {
        let next_time = if i + 1 >= team_lifespan.len() {
            match_length
        } else {
            team_lifespan[i + 1].0
        };
        let dur = next_time - team_lifespan[i].0;
        let str_ = team_lifespan[i].1;
        match hero_count.iter_mut().find(|(s, _)| *s == str_) {
            Some(e) => e.1 += dur,
            None => hero_count.push((str_, dur)),
        }
        if str_ == 0.0 {
            wipes += 1;
        }
        avg += str_ * dur;
    }
    TeamUptime {
        team_lifespan,
        hero_count,
        wipes,
        avg_heroes_alive: avg / match_length,
    }
}

/// `timeWithHeroAdv` (parser.js:2928-2960).
fn time_with_hero_adv(base: &[(f64, f64)], compare: &[(f64, f64)], match_length: f64) -> f64 {
    let mut xs: Vec<f64> = base.iter().map(|(t, _)| *t).collect();
    xs.extend(compare.iter().map(|(t, _)| *t));
    xs.sort_by(|a, b| js_cmp(*a, *b));
    let str_at = |data: &[(f64, f64)], time: f64| -> f64 {
        let mut s = 0.0;
        for (t, h) in data {
            if *t <= time {
                s = *h;
            }
        }
        s
    };
    let mut adv = 0.0;
    for i in 0..xs.len() {
        if str_at(base, xs[i]) > str_at(compare, xs[i]) {
            adv += if i + 1 >= xs.len() {
                match_length - xs[i]
            } else {
                xs[i + 1] - xs[i]
            };
        }
    }
    adv
}

/// Messages (parser.js:2214-2247) + détection BM via game events (2249-2346).
/// Décode les game events (~100k/replay) une seule fois.
fn process_messages_and_bm(
    data: &Ctx,
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    player_lobby_id: &HashMap<String, String>,
    loop_game_start: f64,
) -> R<()> {
    let loop_to_sec = |loop_: f64| (loop_ - loop_game_start) / 16.0;
    let team_of = |players: &Map<String, J>, handle: &str| -> J {
        players
            .get(handle)
            .and_then(|p| get(p, &["team"]).cloned())
            .unwrap_or(J::Null)
    };

    // --- messages (replay.message.events) ---
    let messages = data.replay.message_events().map_err(Abort::from)?;
    let mtype_loading = rt_int("MessageType", "LoadingProgress");
    let mtype_ping = rt_int("MessageType", "Ping");
    let mtype_chat = rt_int("MessageType", "Chat");
    let mtype_announce = rt_int("MessageType", "PlayerAnnounce");
    let mut msgs: Vec<J> = Vec::new();
    for message in &messages {
        let m = jval(message);
        let mtype = get(&m, &["_eventid"]).and_then(J::as_i64);
        if mtype == Some(mtype_loading) {
            continue;
        }
        // `!(m_userId in playerLobbyID)` → on saute les non-joueurs (observateurs)
        let user_key = js_prop(get(&m, &["_userid", "m_userId"]));
        let Some(player) = player_lobby_id.get(&user_key) else {
            continue;
        };
        let mut msg = Map::new();
        msg.insert(
            "type".into(),
            get(&m, &["_eventid"]).cloned().unwrap_or(J::Null),
        );
        msg.insert("player".into(), J::from(player.as_str()));
        msg.insert("team".into(), team_of(players, player));
        msg.insert(
            "recipient".into(),
            get(&m, &["m_recipient"]).cloned().unwrap_or(J::Null),
        );
        // ordre JS : type, player, team, recipient, loop, time, puis charge utile
        let loop_ = get(&m, &["_gameloop"]).cloned().unwrap_or(J::Null);
        let time = loop_to_sec(js_number(Some(&loop_)));
        msg.insert("loop".into(), loop_);
        msg.insert("time".into(), jnum(time));
        if mtype == Some(mtype_ping) {
            // NB : m_point.x/y suivent le décodeur Blizzard ; hots-parser (port GaryIrick)
            // diffère sur l'interprétation signée (tolérance documentée).
            msg.insert(
                "point".into(),
                json!({
                    "x": get(&m, &["m_point", "x"]).cloned().unwrap_or(J::Null),
                    "y": get(&m, &["m_point", "y"]).cloned().unwrap_or(J::Null),
                }),
            );
        } else if mtype == Some(mtype_chat) {
            msg.insert(
                "text".into(),
                get(&m, &["m_string"]).cloned().unwrap_or(J::Null),
            );
        } else if mtype == Some(mtype_announce) {
            msg.insert(
                "announcement".into(),
                get(&m, &["m_announcement"]).cloned().unwrap_or(J::Null),
            );
        }
        msgs.push(J::Object(msg));
    }
    match_.insert("messages".into(), J::Array(msgs));

    // --- game events : b-step chains + taunts/dances (eventid 27) ---
    let build = match_
        .get("version")
        .and_then(|v| get(v, &["m_build"]))
        .and_then(J::as_f64)
        .unwrap_or(f64::NAN);
    // abilLink de l'action « b » par tranche de build (parser.js:2261-2284)
    let b_abil_link: f64 = if build < 61872.0 {
        200.0
    } else if build < 68740.0 {
        119.0
    } else if build < 70682.0 {
        116.0
    } else if build < 77525.0 {
        112.0
    } else if build < 79033.0 {
        114.0
    } else {
        115.0
    };
    let taunt_abil_link: f64 = if build < 68740.0 { 19.0 } else { 22.0 };
    const BSTEP_FRAME_THRESHOLD: f64 = 8.0;

    // On visite les game events au niveau storm_replay::Value (~100 000/replay) SANS les
    // matérialiser en Vec ni les convertir en JSON : seuls les `SCmdEvent` (eventid 27) avec
    // m_abil pertinent sont traités. `srv_int` lit les champs typés directement.
    fn srv_int(v: &storm_replay::Value, f: &str) -> Option<i64> {
        v.field(f).and_then(storm_replay::Value::as_int)
    }
    // playerBSeq : handle → séquences de (gameloop, m_sequence)
    let mut player_bseq: HashMap<String, Vec<Vec<(f64, f64)>>> = HashMap::new();
    // une « exception JS » dans la visite est capturée ici (la closure ne peut pas `?`)
    let mut abort: Option<Abort> = None;
    data.replay
        .visit_game_events(|ev| {
            if abort.is_some() || srv_int(&ev, "_eventid") != Some(27) {
                return;
            }
            let Some(abil) = ev.field("m_abil") else {
                return;
            };
            if matches!(abil, storm_replay::Value::Null) {
                return;
            }
            // m_abilLink absent → pas de correspondance (comme undefined === N en JS)
            let Some(abil_link) = srv_int(abil, "m_abilLink").map(|v| v as f64) else {
                return;
            };
            let user_id = ev.field("_userid").and_then(|u| srv_int(u, "m_userId"));
            let user_key = user_id.map_or_else(|| "undefined".into(), |v| v.to_string());
            let gameloop = srv_int(&ev, "_gameloop").map_or(f64::NAN, |v| v as f64);
            if abil_link == b_abil_link {
                // chaîne de « b » : id = playerLobbyID[playerID] (undefined → clé "undefined")
                let id = player_lobby_id
                    .get(&user_key)
                    .cloned()
                    .unwrap_or_else(|| "undefined".into());
                let seq = srv_int(&ev, "m_sequence").map_or(f64::NAN, |v| v as f64);
                let seqs = player_bseq.entry(id).or_default();
                if let Some(&(cur_loop, cur_sequence)) = seqs.last().and_then(|s| s.last()) {
                    if (cur_loop - gameloop).abs() <= BSTEP_FRAME_THRESHOLD
                        && (cur_sequence - seq).abs() > 1.0
                    {
                        let last = seqs.len() - 1;
                        seqs[last].push((gameloop, seq));
                    } else {
                        seqs.push(vec![(gameloop, seq)]);
                    }
                } else {
                    seqs.push(vec![(gameloop, seq)]);
                }
            } else if abil_link == taunt_abil_link {
                // taunt (abilCmdIndex 4) ou dance (3) — players[id] undefined → throw (catch JS)
                let Some(id) = player_lobby_id.get(&user_key).cloned() else {
                    abort = Some(Abort::Throw("taunt/dance: joueur non mappé".into()));
                    return;
                };
                let cmd = srv_int(abil, "m_abilCmdIndex").map_or(f64::NAN, |v| v as f64);
                let field = if cmd == 4.0 {
                    Some("taunts")
                } else if cmd == 3.0 {
                    Some("dances")
                } else {
                    None
                };
                if let Some(field) = field {
                    let obj = json!({
                        "loop": jnum(gameloop), "time": jnum(loop_to_sec(gameloop)),
                        "kills": 0, "deaths": 0,
                    });
                    match players
                        .get_mut(&id)
                        .and_then(J::as_object_mut)
                        .and_then(|p| p.get_mut(field))
                        .and_then(J::as_array_mut)
                    {
                        Some(a) => a.push(obj),
                        None => {
                            abort = Some(Abort::Throw(format!("taunt/dance: joueur {id} inconnu")))
                        }
                    }
                }
            }
        })
        .map_err(Abort::from)?;
    if let Some(a) = abort {
        return Err(a);
    }
    process_taunt_data(match_, players, &player_bseq)?;
    Ok(())
}

/// Contexte kills/deaths des bsteps/taunts/voiceLines/sprays/dances (parser.js:2460-2592).
/// Reproduit fidèlement le bug : la boucle `dances` est imbriquée dans la boucle `sprays`,
/// donc les dances ne reçoivent leur contexte que si le joueur a ≥ 1 spray, et l'accumulation
/// est répétée `sprays.length` fois.
fn process_taunt_data(
    match_: &Map<String, J>,
    players: &mut Map<String, J>,
    player_bseq: &HashMap<String, Vec<Vec<(f64, f64)>>>,
) -> R<()> {
    // takedowns pré-extraits : (loop, victim.player, [killer.player])
    let takedowns: Vec<(f64, J, Vec<J>)> = match_
        .get("takedowns")
        .and_then(J::as_array)
        .map(|arr| {
            arr.iter()
                .map(|td| {
                    let loop_ = js_number(get(td, &["loop"]));
                    let victim = get(td, &["victim", "player"]).cloned().unwrap_or(J::Null);
                    let killers = get(td, &["killers"])
                        .and_then(J::as_array)
                        .map(|ks| {
                            ks.iter()
                                .map(|k| get(k, &["player"]).cloned().unwrap_or(J::Null))
                                .collect()
                        })
                        .unwrap_or_default();
                    (loop_, victim, killers)
                })
                .collect()
        })
        .unwrap_or_default();

    // compte (deaths, kills) dans une fenêtre [min, max] pour un joueur donné
    let context = |min: f64, max: f64, id: &str| -> (i64, i64) {
        let idj = J::from(id);
        let (mut kills, mut deaths) = (0i64, 0i64);
        for (time, victim, killers) in &takedowns {
            if min <= *time && *time <= max {
                if js_strict_eq(Some(victim), Some(&idj)) {
                    deaths += 1;
                }
                if killers.iter().any(|k| js_strict_eq(Some(k), Some(&idj))) {
                    kills += 1;
                }
            }
        }
        (kills, deaths)
    };

    // bsteps (ordre des ids sans incidence : chaque id écrit dans un joueur distinct)
    for (id, seqs) in player_bseq {
        for seq in seqs {
            if seq.len() > 2 {
                let start = seq[0].0;
                let stop = seq[seq.len() - 1].0;
                let (kills, deaths) = context(start - 80.0, stop + 80.0, id);
                let bstep = json!({
                    "start": jnum(start), "stop": jnum(stop), "duration": jnum(stop - start),
                    "kills": kills, "deaths": deaths,
                });
                players
                    .get_mut(id)
                    .and_then(J::as_object_mut)
                    .and_then(|p| p.get_mut("bsteps"))
                    .and_then(J::as_array_mut)
                    .ok_or_else(|| Abort::Throw(format!("bsteps: joueur {id} inconnu")))?
                    .push(bstep);
            }
        }
    }

    // taunts / voiceLines / sprays(+dances imbriqué) — ordre d'insertion des joueurs
    let order: Vec<String> = players.keys().cloned().collect();
    for id in &order {
        // collecte des loops par catégorie (emprunt immuable d'abord, écriture ensuite)
        let (taunt_loops, voice_loops, spray_loops, dance_loops) = {
            let p = players.get(id).and_then(J::as_object);
            let loops = |p: Option<&Map<String, J>>, field: &str| -> Vec<f64> {
                p.and_then(|p| p.get(field))
                    .and_then(J::as_array)
                    .map(|a| a.iter().map(|e| js_number(get(e, &["loop"]))).collect())
                    .unwrap_or_default()
            };
            (
                loops(p, "taunts"),
                loops(p, "voiceLines"),
                loops(p, "sprays"),
                loops(p, "dances"),
            )
        };

        let apply = |players: &mut Map<String, J>, field: &str, i: usize, k: i64, d: i64| {
            if let Some(ev) = players
                .get_mut(id)
                .and_then(J::as_object_mut)
                .and_then(|p| p.get_mut(field))
                .and_then(J::as_array_mut)
                .and_then(|a| a.get_mut(i))
                .and_then(J::as_object_mut)
            {
                if let Some(kn) = ev.get("kills").and_then(J::as_i64) {
                    ev.insert("kills".into(), J::from(kn + k));
                }
                if let Some(dn) = ev.get("deaths").and_then(J::as_i64) {
                    ev.insert("deaths".into(), J::from(dn + d));
                }
            }
        };

        for (i, &t) in taunt_loops.iter().enumerate() {
            let (k, d) = context(t - 80.0, t + 80.0, id); // |x|<=80 ⇔ fenêtre symétrique
                                                          // parser.js utilise Math.abs(tauntTime - time) <= 80 (fenêtre identique)
            apply(players, "taunts", i, k, d);
        }
        for (i, &t) in voice_loops.iter().enumerate() {
            let (k, d) = context(t - 80.0, t + 80.0, id);
            apply(players, "voiceLines", i, k, d);
        }
        for (i, &s) in spray_loops.iter().enumerate() {
            let (k, d) = context(s - 80.0, s + 80.0, id);
            apply(players, "sprays", i, k, d);
            // BUG reproduit : boucle dances imbriquée → s'exécute pour CHAQUE spray
            for (j, &dl) in dance_loops.iter().enumerate() {
                let (k, d) = context(dl - 80.0, dl + 80.0, id);
                apply(players, "dances", j, k, d);
            }
        }
    }
    Ok(())
}

/// Élément du tableau `selections` du draft (parser.js:650-683) : `indexOf` y est un `===` JS —
/// un objet ban ne peut jamais égaler un nom de héros, undefined === undefined est vrai.
#[derive(PartialEq)]
enum Sel {
    Undef,
    Val(J),
    Ban,
}

fn to_sel(v: Option<&J>) -> Sel {
    match v {
        // J::Null représente l'undefined JS dans notre port (heroAttribute manquant, etc.)
        None | Some(J::Null) => Sel::Undef,
        Some(x) => Sel::Val(x.clone()),
    }
}

/// Draft : `match.bans`, `match.picks` (+ `first`), `players.*.turn` (parser.js:538-691).
fn process_draft(
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
fn collect_pick_order(
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
fn process_score_array(
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

/// Variables `var` hissées des branches objectifs par carte (parser.js:705-784). `None` ≡
/// undefined JS (et le `null` initial de `dragon` : mêmes effets observables) — un accès de
/// propriété dessus → Throw (catch → Failure) ; une affectation directe (`dragon = {...}`)
/// fonctionne sur toute carte, comme en JS (les branches UnitBorn ne sont pas gardées par carte).
#[derive(Default)]
struct ObjState {
    moon: Option<J>,
    sun: Option<J>,
    dragon: Option<J>,
    current_terror: Option<J>,
    /// Tableau JS `[null, null]` : objet `{"0","1"}` + itération for..in numérique.
    golems: Option<J>,
    immortal: Option<J>,
    current_spiders: Option<Spiders>,
    current_protector: Option<J>,
    nukes: Option<J>,
    wave_units: Option<J>,
    /// `var waveID` : None ≡ undefined (`waveID += 1` → NaN, index → undefined).
    wave_id: Option<f64>,
    beacons: Option<J>,
}

/// `currentSpiders` (Crypts, parser.js:746) — jamais sérialisé : champs scalaires portés en
/// natif, `None` ≡ undefined.
#[derive(Default)]
struct Spiders {
    units: Map<String, J>,
    active: bool,
    team: Option<f64>,
    event_idx: Option<f64>,
    unit_idx: Option<f64>,
    max_duration: Option<f64>,
}

/// `===` JS sur nos valeurs (scalaires uniquement ; `None` ≡ undefined ≠ null).
fn js_strict_eq(a: Option<&J>, b: Option<&J>) -> bool {
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

/// Clé de propriété JS depuis un nombre optionnel (undefined → "undefined").
fn js_key(x: Option<f64>) -> String {
    x.map_or_else(|| "undefined".into(), js_num_str)
}

/// Index de tableau JS depuis un nombre optionnel (non entier/négatif/NaN → undefined).
fn js_idx(x: Option<f64>) -> Option<usize> {
    let x = x?;
    (x.is_finite() && x >= 0.0 && x.fract() == 0.0).then_some(x as usize)
}

/// `match.map === ReplayTypes.MapType.<k>`.
fn is_map(match_: &Map<String, J>, k: &str) -> bool {
    match_.get("map").and_then(J::as_str) == Some(rt_str("MapType", k))
}

/// `match.objective` (objet) — absent/non-objet → Throw.
fn objective_mut(match_: &mut Map<String, J>) -> R<&mut Map<String, J>> {
    match_
        .get_mut("objective")
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw("match.objective absent".into()))
}

/// `match.objective[team]` objet — clé absente ou non-objet (tableau des Mines…) → Throw,
/// comme `undefined.events` en JS.
fn obj_slot_mut<'a>(match_: &'a mut Map<String, J>, team: &str) -> R<&'a mut Map<String, J>> {
    objective_mut(match_)?
        .get_mut(team)
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw(format!("objective[{team}] absent")))
}

/// `slot.events` tableau — absent → Throw (`undefined.push`).
fn slot_events_mut(slot: &mut Map<String, J>) -> R<&mut Vec<J>> {
    slot.get_mut("events")
        .and_then(J::as_array_mut)
        .ok_or_else(|| Abort::Throw("objective.events absent".into()))
}

/// `match.objective.<key>` tableau (tributes, results, warheads, waves…) — absent → Throw.
fn obj_arr_mut<'a>(match_: &'a mut Map<String, J>, key: &str) -> R<&'a mut Vec<J>> {
    objective_mut(match_)?
        .get_mut(key)
        .and_then(J::as_array_mut)
        .ok_or_else(|| Abort::Throw(format!("objective.{key} absent")))
}

/// `slot.<key> += by` (clé absente → undefined → NaN, comme en JS).
fn js_incr(slot: &mut Map<String, J>, key: &str, by: f64) {
    let v = js_number(slot.get(key)) + by;
    slot.insert(key.into(), jnum(v));
}

/// `match.objective.waves[waveID]` — hors bornes/NaN/undefined → Throw (`undefined.x`).
fn wave_mut(match_: &mut Map<String, J>, wave_id: Option<f64>) -> R<&mut Map<String, J>> {
    let waves = obj_arr_mut(match_, "waves")?;
    js_idx(wave_id)
        .and_then(|i| waves.get_mut(i))
        .and_then(J::as_object_mut)
        .ok_or_else(|| Abort::Throw("waves[waveID] absent".into()))
}

/// `match.version.m_build` (nombre JS — absent → NaN).
fn match_build(match_: &Map<String, J>) -> f64 {
    js_number(match_.get("version").and_then(|v| get(v, &["m_build"])))
}

/// `braxisWaveStrength(units, build)` (parser.js:2594-2657). Clés et valeurs de
/// BraxisUnitType sont identiques : l'init à 0 et l'incrément par `units[u].type` coïncident.
fn braxis_wave_strength(units: &Map<String, J>, build: f64) -> f64 {
    let mut types: HashMap<String, f64> = HashMap::new();
    if let Some(o) = replay_types()["BraxisUnitType"].as_object() {
        for k in o.keys() {
            types.insert(k.clone(), 0.0);
        }
    }
    for u in units.values() {
        // type hors table → undefined + 1 = NaN, comme en JS
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
        // 2.47.0 — « only cause Ultralisks to be ignored » (build NaN tombe ici, comme en JS)
        let score = 0.1 * n("ZergBaneling");
        let score = js_max(score, 0.24 * n("ZergHydralisk"));
        let score = js_max(score, 0.30 * n("ZergGuardian"));
        js_max(score, 0.45 * (n("ZergUltralisk") - 3.0))
    }
}

/// Forces des deux vagues Braxis (`waveUnits[0]`, `waveUnits[1]`) — waveUnits indéfini → Throw.
fn braxis_strengths(st: &ObjState, build: f64) -> R<(f64, f64)> {
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

/// Stat events « objectifs par carte » (parser.js:974-1189, 1214-1238).
fn obj_stat_event(
    name: &str,
    event: &J,
    match_: &mut Map<String, J>,
    st: &mut ObjState,
    loop_game_start: f64,
) -> R<()> {
    let s = |k: &str| rt_str("StatEventType", k);
    let gameloop_j = get(event, &["_gameloop"]).cloned().unwrap_or(J::Null);
    let gameloop = js_number(get(event, &["_gameloop"]));
    let time = jnum(loops_to_seconds(gameloop - loop_game_start));
    // `event.m_intData[i].m_value` : [i] absent → Throw, m_value absent → undefined
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

/// Branches par carte de UnitBorn (parser.js:1262-1475) — testées par type d'unité, sans
/// garde sur la carte (reproduit tel quel).
fn obj_unit_born(
    event: &J,
    match_: &mut Map<String, J>,
    st: &mut ObjState,
    players: &Map<String, J>,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
) -> R<()> {
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
        // parser.js:1318-1341 — currentProtector est AUSSI l'objet poussé dans events :
        // l'eventIdx posé après le push est visible dans la sortie.
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
                    .ok_or_else(|| Abort::Throw(format!("nuke: joueur {h} inconnu")))?,
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

/// Branches par carte de UnitDied (parser.js:1655-1896) — gardées par `match.map`.
fn obj_unit_died(
    event: &J,
    match_: &mut Map<String, J>,
    st: &mut ObjState,
    loop_game_start: f64,
) -> R<()> {
    let tag = get(event, &["m_unitTagIndex"]);
    let rtag = get(event, &["m_unitTagRecycle"]);
    let uid = unit_uid(event);
    let gameloop_j = get(event, &["_gameloop"]).cloned().unwrap_or(J::Null);
    let gameloop = js_number(get(event, &["_gameloop"]));
    let secs = loops_to_seconds(gameloop - loop_game_start);

    if is_map(match_, "HauntedMines") {
        // parser.js:1656-1687 — mort d'un golem suivi
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
                // match.objective[team] est ici un TABLEAU (init Mines)
                objective_mut(match_)?
                    .get_mut(&team_key)
                    .and_then(J::as_array_mut)
                    .ok_or_else(|| Abort::Throw(format!("objective[{team_key}] absent")))?
                    .push(obj_event);
            }
        }
    } else if is_map(match_, "HauntedWoods") {
        // parser.js:1688-1720 — mort de la plante activée
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
                // team = parseInt(t) → clé de propriété String(number)
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
                    None => last.remove("player"), // = undefined → absent au stringify
                };
                ct.get_mut(&tkey)
                    .and_then(J::as_object_mut)
                    .ok_or_else(|| Abort::Throw("currentTerror[t] absent".into()))?
                    .insert("active".into(), J::from(false));
            }
        }
        // parser.js:1722-1734 — mort d'une terreur (unités) ; itère TOUTES les clés de
        // objective, y compris "type" (chaîne → .units undefined → boucle vide)
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
                    // BUG parser.js:1731 reproduit : u.start n'existe pas → NaN → null
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
            // dragon jamais activé → team undefined → objective[undefined] → Throw, comme en JS
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
                // ultralisk mort-né : la vague adverse démarre à 100 (parser.js:1820-1828)
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

/// UnitOwnerChange par carte (parser.js:1897-1979) — quatre `if` indépendants (non else-if).
fn obj_unit_owner_change(
    event: &J,
    match_: &mut Map<String, J>,
    st: &mut ObjState,
    player_id_map: &Map<String, J>,
    loop_game_start: f64,
) -> R<()> {
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
            // team !== 0 → -11 ; sinon -1 (« our blue team is already 0 »)
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

/// Cleanups post-boucle des objectifs par carte (parser.js:1982-2109).
fn obj_cleanup(match_: &mut Map<String, J>, st: &mut ObjState, loop_game_start: f64) -> R<()> {
    let loop_length = js_number(match_.get("loopLength"));
    let loop_length_j = match_.get("loopLength").cloned().unwrap_or(J::Null);
    let end_secs = loops_to_seconds(loop_length - loop_game_start);

    if is_map(match_, "HauntedMines") {
        // parser.js:1983-2011 — golems encore en vie à la fin
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
        // parser.js:2013-2044 — terreurs encore actives
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
        // parser.js:2060-2072 — phase araignées encore active
        let cs = st
            .current_spiders
            .as_mut()
            .ok_or_else(|| Abort::Throw("currentSpiders undefined".into()))?;
        if cs.active {
            let spider = js_for_in_keys(&cs.units)
                .first()
                .and_then(|k| cs.units.get(k))
                .cloned()
                // units vide → units[undefined].loop lève, comme en JS
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
        // parser.js:2073-2079 — protecteur encore actif
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
        // parser.js:2080-2090 — tri des nukes
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
            // succès si true OU si jamais morte (clé absente)
            let success_true = get(&nuke, &["success"]) == Some(&J::Bool(true));
            if success_true || get(&nuke, &["success"]).is_none() {
                js_incr(slot, "success", 1.0);
            }
        }
    } else if is_map(match_, "BattlefieldOfEternity") {
        // parser.js:2091-2097 — immortel encore en vie
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
        // parser.js:2098-2108 — cavaleries jamais mortes
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

/// `getFirstObjectiveTeam(match)` (parser.js:3116-3302) — try/catch → null, ne lève jamais.
fn get_first_objective_team(match_: &Map<String, J>) -> J {
    first_objective_inner(match_).unwrap_or(J::Null)
}

fn first_objective_inner(match_: &Map<String, J>) -> R<J> {
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
        // parser.js:3118-3149 — NB : les events d'Alterac n'ont pas de champ time →
        // undefined === undefined → null dès que les deux équipes ont un événement
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
        // parser.js:3150-3168 — pas de cas « les deux vides » : units[0].loop lève → null
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
                .ok_or_else(|| Abort::Throw("units vides".into()))?,
            &["loop"],
        );
        let l1 = get(
            u1.first()
                .ok_or_else(|| Abort::Throw("units vides".into()))?,
            &["loop"],
        );
        if js_strict_eq(l0, l1) {
            return Ok(J::Null);
        }
        Ok(J::from(if js_number(l0) < js_number(l1) { 0 } else { 1 }))
    } else if is_map(match_, "ControlPoints") || is_map(match_, "TowersOfDoom") {
        // parser.js:3169-3220 — qui domine les 90 premiers tirs / les 3 premiers autels
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
        // parser.js:3221-3243 — premier à 3 tributs
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
        // parser.js:3244-3272 — meilleur des 4 premières nukes réussies
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
        // parser.js:3285-3293 — startScore absent → undefined[0] lève → null
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

/// Noms d'événements Stat « objectifs par carte » (parser.js:974-1189 + 1214-1238).
fn is_map_objective_stat_event(name: &str) -> bool {
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

/// Types d'unités consommés par les branches par carte de UnitBorn (parser.js:1262-1475).
fn is_map_objective_unit_born(unit_type: &str) -> bool {
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

/// `event.m_unitTagIndex + '-' + event.m_unitTagRecycle` (concaténation JS).
fn unit_uid(event: &J) -> String {
    format!(
        "{}-{}",
        js_prop(get(event, &["m_unitTagIndex"])),
        js_prop(get(event, &["m_unitTagRecycle"]))
    )
}

/// `xpb.breakdown` : m_fixedData → `{ m_key: m_value/4096 }` (parser.js:850-853, 874-877).
fn xp_fixed_breakdown(event: &J) -> R<Map<String, J>> {
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
fn process_periodic_xp(
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
fn process_eog_xp(
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
fn process_player_death(
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
fn process_camp_capture(event: &J, match_: &mut Map<String, J>, loop_game_start: f64) -> R<()> {
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
fn process_level_up(
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
fn process_unit_born(
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
fn process_unit_revived(
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
fn process_unit_positions(
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
fn process_unit_died(
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
fn process_talent_choices(
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
