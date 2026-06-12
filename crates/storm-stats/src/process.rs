//! Port de `processReplay` (hots-parser/parser.js) — phase par phase, sémantique identique
//! (bugs compris : tout écart volontaire est une tolérance documentée du harnais).

use crate::constants::{hero_attribute, replay_types, rt_int, rt_str, status};
use crate::convert::jval;
use serde_json::{json, Map, Value as J};
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
        Output { status, match_: None, players: None }
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
    /// Exception JS (accès indéfini, etc.) → catch → Failure.
    Throw(String),
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
    let (mut days, mut rem) = (total_ms.div_euclid(86_400_000), total_ms.rem_euclid(86_400_000));
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
                get(p, &["m_toon", "m_region"]).and_then(J::as_i64).unwrap_or_default(),
                get_str(p, &["m_toon", "m_programId"]).unwrap_or_default(),
                get(p, &["m_toon", "m_realm"]).and_then(J::as_i64).unwrap_or_default(),
                get(p, &["m_toon", "m_id"]).and_then(J::as_i64).unwrap_or_default(),
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
    pub replay: Replay, // game/message events décodés à la demande (gros streams)
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
    Ok(Ctx { header, details, initdata, attributes, tracker, tags, replay })
}

fn process_inner(path: &Path, filename: &str) -> R<Output> {
    let data = load(path)?;
    let mut match_ = Map::new();
    let mut players = Map::new();

    // ===== Phase identité (parser.js:256-465) =====
    match_.insert("version".into(), get(&data.header, &["m_version"]).cloned().unwrap_or(J::Null));
    // (pas de check MAX_SUPPORTED_BUILD : on est toujours en overrideVerifiedBuild)
    match_.insert("type".into(), get(&data.header, &["m_type"]).cloned().unwrap_or(J::Null));
    match_.insert(
        "loopLength".into(),
        get(&data.header, &["m_elapsedGameLoops"]).cloned().unwrap_or(J::Null),
    );
    match_.insert("filename".into(), J::from(filename));

    let mut mode = get(
        &data.initdata,
        &["m_syncLobbyState", "m_gameDescription", "m_gameOptions", "m_ammId"],
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
        get(&data.details, &["m_timeUTC"]).cloned().unwrap_or(J::Null),
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
        pdoc.insert("name".into(), get(pdata, &["m_name"]).cloned().unwrap_or(J::Null));
        pdoc.insert("uuid".into(), get(pdata, &["m_toon", "m_id"]).cloned().unwrap_or(J::Null));
        pdoc.insert(
            "region".into(),
            get(pdata, &["m_toon", "m_region"]).cloned().unwrap_or(J::Null),
        );
        pdoc.insert(
            "realm".into(),
            get(pdata, &["m_toon", "m_realm"]).cloned().unwrap_or(J::Null),
        );
        let toon = format!(
            "{}-{}-{}-{}",
            get(pdata, &["m_toon", "m_region"]).and_then(J::as_i64).unwrap_or_default(),
            get_str(pdata, &["m_toon", "m_programId"]).unwrap_or_default(),
            get(pdata, &["m_toon", "m_realm"]).and_then(J::as_i64).unwrap_or_default(),
            get(pdata, &["m_toon", "m_id"]).and_then(J::as_i64).unwrap_or_default(),
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
            get(pdata, &["m_toon", "m_region"]).cloned().unwrap_or(J::Null),
        );
        pdoc.insert("team".into(), get(pdata, &["m_teamId"]).cloned().unwrap_or(J::Null));

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
            get(&data.header, &["m_version", "m_build"]).cloned().unwrap_or(J::Null),
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
    let _ = &player_id_map; // utilisé par les phases suivantes (T3+)

    Ok(Output { status: status::OK, match_: Some(match_), players: Some(players) })
}
