//! Port de `processReplay` (hots-parser/parser.js) — phase par phase, sémantique identique
//! (bugs compris : tout écart volontaire est une tolérance documentée du harnais).

use crate::constants::{hero_attribute, replay_types, rt_int, rt_str, status};
use crate::convert::jval;
use serde_json::{json, Map, Value as J};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use storm_replay::Replay;

mod draft;
mod events;
mod js;
mod messages;
mod objectives;
mod teams;
use draft::*;
use events::*;
use js::*;
use messages::*;
use objectives::*;
use teams::*;

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

/// Extension storm-stats au-delà de hots-parser 7.55.7 : les cartes ARAM (voie unique) récentes,
/// absentes de la `MapType` de la référence (qui les rejette). Décision opérateur 2026-06-12
/// (cf. docs/research/2026-06-12-jalon2-parite.md) : on les supporte pour couvrir ~30 % de
/// l'archive. Ces cartes n'ont PAS d'objectifs PvE → `match.objective = {type}` et les handlers
/// d'objectifs no-op (gardés par `objective_less_map`). Identité/score/draft/takedowns/talents/
/// BM/équipes utilisent les chemins universels déjà prouvés en parité sur les cartes classiques.
const EXTRA_MAPS: [(&str, &str); 4] = [
    ("SilverCity", "Silver City"),
    ("LostCavern", "Lost Cavern"),
    ("BraxisOutpost", "Braxis Outpost"),
    ("IndustrialDistrict", "Industrial District"),
];

fn extra_map_pretty(internal: &str) -> Option<&'static str> {
    EXTRA_MAPS
        .iter()
        .find(|(i, _)| *i == internal)
        .map(|(_, p)| *p)
}

/// `match.map` est une carte ARAM étendue (sans objectifs) → les handlers d'objectifs no-op.
fn objective_less_map(match_: &Map<String, J>) -> bool {
    match_
        .get("map")
        .and_then(J::as_str)
        .is_some_and(|m| EXTRA_MAPS.iter().any(|(_, p)| *p == m))
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
                // Extension storm-stats : cartes ARAM récentes (hors MapType de hots-parser).
                None => match extra_map_pretty(internal) {
                    Some(pretty) => {
                        match_.insert("map".into(), J::from(pretty));
                    }
                    // BUG parser.js:312 reproduit : `ReplayStats` (indéfini) → throw → Failure
                    None => return Err(Abort::Throw(format!("carte interne inconnue {internal}"))),
                },
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
        } else if objective_less_map(&match_) {
            // Extension : cartes ARAM sans objectif PvE → objectif minimal `{type}` (obj garde
            // son champ `type` déjà inséré ; aucun handler d'objectif ne s'exécutera).
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
