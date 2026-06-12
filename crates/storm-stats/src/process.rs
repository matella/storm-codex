//! Port de `processReplay` (hots-parser/parser.js) — phase par phase, sémantique identique
//! (bugs compris : tout écart volontaire est une tolérance documentée du harnais).

use crate::constants::{hero_attribute, replay_types, rt_int, rt_str, status};
use crate::convert::jval;
use serde_json::{json, Map, Value as J};
use std::collections::HashMap;
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

/// Clé de propriété JS (`obj[v]`) pour nos cas : nombre → chiffres, null → "null",
/// undefined → "undefined" (m_userId/m_workingSetSlotId/m_controllingPlayer : entiers ou null).
fn js_prop(v: Option<&J>) -> String {
    match v {
        None => "undefined".into(),
        Some(J::Null) => "null".into(),
        Some(J::Bool(b)) => b.to_string(),
        Some(J::Number(n)) => n.to_string(),
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
        Err(Abort::Throw(_)) => Output::rejected(status::FAILURE),
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
    let mut player_id_map: Map<String, J> = Map::new();
    match_.insert("loopGameStart".into(), J::from(0));
    let player_init = rt_str("StatEventType", "PlayerInit");
    let gates_open = rt_str("StatEventType", "GatesOpen");
    for event in &data.tracker {
        if get(event, &["_eventid"]).and_then(J::as_i64) != Some(stat_eventid) {
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

    // ===== Dispatch objectif par carte (parser.js:702-784) — partie statut seulement =====
    // L'init des conteneurs match.objective est T5 ; mais carte absente → TooOld (parser.js:777)
    // et carte hors liste → UnsupportedMap (parser.js:783) doivent tomber ici, avant le score.
    match match_.get("map").and_then(J::as_str) {
        None => return Err(Abort::Status(status::TOO_OLD)),
        Some(map) => {
            let known = [
                "ControlPoints",
                "TowersOfDoom",
                "CursedHollow",
                "DragonShire",
                "HauntedWoods",
                "HauntedMines",
                "BattlefieldOfEternity",
                "Shrines",
                "Crypts",
                "Volskaya",
                "Warhead Junction",
                "AlteracPass",
                "BraxisHoldout",
                "BlackheartsBay",
                "Hanamura",
            ];
            if !known.iter().any(|k| rt_str("MapType", k) == map) {
                return Err(Abort::Status(status::UNSUPPORTED_MAP));
            }
        }
    }

    // ===== Boucle tracker, partie T3 : score screen + talents + votes + globes
    // (parser.js:794-830, 1201-1213, 2421-2458) =====
    let score_eventid = rt_int("TrackerEvent", "Score");
    let upvote = rt_str("StatEventType", "Upvote");
    let regen_globe = rt_str("StatEventType", "RegenGlobePickedUp");
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
        }
    }

    // ===== Passe finale, partie T3 (parser.js:2140-2210) =====
    // KillParticipation, DPM/HPM/XPM et gameStats.length dépendent du match.length final
    // (mort du core, T4) et des takedowns d'équipe (T4) — posés plus tard.
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
        if team == Some(rt_int("TeamType", "Blue")) {
            team_ids[0].push(J::from(handle.as_str()));
            if win {
                winner = Some(0);
            }
        } else if team == Some(rt_int("TeamType", "Red")) {
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

    // firstPickWin (parser.js:2397-2403) : picks.first === winner ; pas de draft (QM) → false.
    // (firstObjective/firstFort/firstKeep & co : T4/T5.)
    let first_pick_win = match match_.get("picks") {
        Some(p) => get(p, &["first"]).and_then(J::as_i64) == Some(w as i64),
        None => false,
    };
    match_.insert("firstPickWin".into(), J::from(first_pick_win));

    Ok(Output {
        status: status::OK,
        match_: Some(match_),
        players: Some(players),
    })
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
