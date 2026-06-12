//! Tables de protocole embarquées (générées par `tools/protocol_gen.py`) et décodage des
//! streams, suivant l'enchaînement canonique de heroprotocol (hero_cli.py / protocolXXXXX.py).

use crate::bitpacked::BitPackedDecoder;
use crate::error::{Error, Result};
use crate::typeinfo::{parse_typeinfos, TypeInfo};
use crate::value::Value;
use crate::versioned::VersionedDecoder;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

mod embed {
    include!("../protocols/embed.rs");
}
pub use embed::LATEST_BUILD;

type EventTypes = crate::typeinfo::FastI64Map<(usize, Arc<str>)>;

pub struct Protocol {
    typeinfos: Vec<TypeInfo>,
    replay_header_typeid: usize,
    game_details_typeid: usize,
    replay_initdata_typeid: usize,
    svaruint32_typeid: usize,
    replay_userid_typeid: usize,
    tracker_eventid_typeid: Option<usize>,
    game_eventid_typeid: usize,
    message_eventid_typeid: usize,
    tracker_event_types: EventTypes,
    game_event_types: EventTypes,
    message_event_types: EventTypes,
}

fn parse_event_types(json: &serde_json::Value, key: &str) -> Result<EventTypes> {
    let mut out = EventTypes::default();
    if let Some(map) = json.get(key).and_then(|v| v.as_object()) {
        for (k, v) in map {
            let pair = v
                .as_array()
                .ok_or_else(|| Error::Protocol(format!("{key} : entrée invalide")))?;
            out.insert(
                k.parse::<i64>()
                    .map_err(|_| Error::Protocol(format!("{key} : id {k}")))?,
                (
                    pair.first()
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| Error::Protocol(format!("{key} : typeid")))?
                        as usize,
                    pair.get(1).and_then(|v| v.as_str()).unwrap_or("?").into(),
                ),
            );
        }
    }
    Ok(out)
}

impl Protocol {
    fn parse(raw: &str) -> Result<Protocol> {
        let json: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| Error::Protocol(format!("JSON : {e}")))?;
        let req = |k: &str| -> Result<usize> {
            json.get(k)
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .ok_or_else(|| Error::Protocol(format!("{k} absent")))
        };
        Ok(Protocol {
            typeinfos: parse_typeinfos(
                json.get("typeinfos")
                    .ok_or_else(|| Error::Protocol("typeinfos absentes".into()))?,
            )?,
            replay_header_typeid: req("replay_header_typeid")?,
            game_details_typeid: req("game_details_typeid")?,
            replay_initdata_typeid: req("replay_initdata_typeid")?,
            svaruint32_typeid: req("svaruint32_typeid")?,
            replay_userid_typeid: req("replay_userid_typeid")?,
            tracker_eventid_typeid: json
                .get("tracker_eventid_typeid")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize),
            game_eventid_typeid: req("game_eventid_typeid")?,
            message_eventid_typeid: req("message_eventid_typeid")?,
            tracker_event_types: parse_event_types(&json, "tracker_event_types")?,
            game_event_types: parse_event_types(&json, "game_event_types")?,
            message_event_types: parse_event_types(&json, "message_event_types")?,
        })
    }

    pub fn decode_header(&self, content: &[u8]) -> Result<Value> {
        VersionedDecoder::new(content, &self.typeinfos).instance(self.replay_header_typeid)
    }

    pub fn decode_details(&self, content: &[u8]) -> Result<Value> {
        VersionedDecoder::new(content, &self.typeinfos).instance(self.game_details_typeid)
    }

    pub fn decode_initdata(&self, content: &[u8]) -> Result<Value> {
        BitPackedDecoder::new(content, &self.typeinfos).instance(self.replay_initdata_typeid)
    }

    /// `replay.tracker.events` — versioned, sans userid.
    pub fn decode_tracker_events(&self, content: &[u8]) -> Result<Vec<Value>> {
        let eventid_typeid = self.tracker_eventid_typeid.ok_or_else(|| {
            Error::Protocol("protocole sans tracker events (build < 2014)".into())
        })?;
        let mut decoder = VersionedDecoder::new(content, &self.typeinfos);
        let mut events = Vec::new();
        let mut gameloop: i64 = 0;
        while !decoder.done() {
            let delta = decoder.svaruint32_value(self.svaruint32_typeid)?;
            gameloop += delta;
            let eventid = decoder
                .instance(eventid_typeid)?
                .as_int()
                .ok_or_else(|| Error::Corrupted("eventid invalide".into()))?;
            let (typeid, name) = self.tracker_event_types.get(&eventid).ok_or_else(|| {
                Error::Corrupted(format!(
                    "tracker eventid inconnu {eventid} à l'octet {}",
                    decoder.used_bytes()
                ))
            })?;
            let mut event = decoder.instance(*typeid)?;
            annotate(&mut event, name, eventid, gameloop, None);
            events.push(event);
            // byte_align : sans objet, le versioned est aligné par construction
        }
        Ok(events)
    }

    /// `replay.game.events` — bitpacked, avec userid.
    pub fn decode_game_events(&self, content: &[u8]) -> Result<Vec<Value>> {
        self.bitpacked_event_stream(content, self.game_eventid_typeid, &self.game_event_types)
    }

    /// `replay.message.events` — bitpacked, avec userid.
    pub fn decode_message_events(&self, content: &[u8]) -> Result<Vec<Value>> {
        self.bitpacked_event_stream(
            content,
            self.message_eventid_typeid,
            &self.message_event_types,
        )
    }

    /// Décode le flux et passe chaque événement (possédé) à `f` sans matérialiser de `Vec` —
    /// pour les consommateurs qui ne gardent qu'une fraction des ~100 000 game events.
    pub fn visit_game_events<F: FnMut(Value)>(&self, content: &[u8], f: F) -> Result<()> {
        self.bitpacked_event_stream_visit(
            content,
            self.game_eventid_typeid,
            &self.game_event_types,
            f,
        )
    }

    fn bitpacked_event_stream(
        &self,
        content: &[u8],
        eventid_typeid: usize,
        event_types: &EventTypes,
    ) -> Result<Vec<Value>> {
        let mut events = Vec::new();
        self.bitpacked_event_stream_visit(content, eventid_typeid, event_types, |e| {
            events.push(e)
        })?;
        Ok(events)
    }

    fn bitpacked_event_stream_visit<F: FnMut(Value)>(
        &self,
        content: &[u8],
        eventid_typeid: usize,
        event_types: &EventTypes,
        mut f: F,
    ) -> Result<()> {
        let mut decoder = BitPackedDecoder::new(content, &self.typeinfos);
        let mut gameloop: i64 = 0;
        while !decoder.done() {
            let delta = decoder.svaruint32_value(self.svaruint32_typeid)?;
            gameloop += delta;
            let userid = decoder.instance(self.replay_userid_typeid)?;
            let eventid = decoder
                .instance(eventid_typeid)?
                .as_int()
                .ok_or_else(|| Error::Corrupted("eventid invalide".into()))?;
            let (typeid, name) = event_types.get(&eventid).ok_or_else(|| {
                Error::Corrupted(format!(
                    "eventid inconnu {eventid} au bit {}",
                    decoder.buffer.used_bits()
                ))
            })?;
            let mut event = decoder.instance(*typeid)?;
            annotate(&mut event, name, eventid, gameloop, Some(userid));
            f(event);
            decoder.byte_align(); // l'événement suivant est aligné sur l'octet
        }
        Ok(())
    }
}

/// Champs `_event`/`_eventid`/`_gameloop`/`_userid` ajoutés par la référence.
/// Noms internés une fois par process — ce chemin tourne ~100 000 fois par replay.
fn annotate(
    event: &mut Value,
    name: &Arc<str>,
    eventid: i64,
    gameloop: i64,
    userid: Option<Value>,
) {
    static KEYS: OnceLock<[Arc<str>; 4]> = OnceLock::new();
    let [k_event, k_eventid, k_gameloop, k_userid] = KEYS.get_or_init(|| {
        [
            "_event".into(),
            "_eventid".into(),
            "_gameloop".into(),
            "_userid".into(),
        ]
    });
    if let Value::Struct(fields) = event {
        fields.push((Arc::clone(k_event), Value::Str(Arc::clone(name))));
        fields.push((Arc::clone(k_eventid), Value::Int(eventid)));
        fields.push((Arc::clone(k_gameloop), Value::Int(gameloop)));
        if let Some(u) = userid {
            fields.push((Arc::clone(k_userid), u));
        }
    }
}

/// Protocole du build demandé (tables embarquées, parse une fois par table et par process).
/// `fallback = true` si le build est inconnu → dernier protocole connu (comportement
/// heroprotocol documenté dans la spec).
pub fn protocol_for_build(build: u32) -> Result<(Arc<Protocol>, bool)> {
    let (hash, fallback) = match embed::BUILDS.binary_search_by_key(&build, |(b, _)| *b) {
        Ok(i) => (embed::BUILDS[i].1, false),
        Err(_) => (latest_hash()?, true),
    };
    Ok((protocol_for_hash(hash)?, fallback))
}

pub fn latest_protocol() -> Result<Arc<Protocol>> {
    protocol_for_hash(latest_hash()?)
}

fn latest_hash() -> Result<&'static str> {
    embed::BUILDS
        .binary_search_by_key(&embed::LATEST_BUILD, |(b, _)| *b)
        .map(|i| embed::BUILDS[i].1)
        .map_err(|_| Error::Protocol("LATEST_BUILD absent de l'index (bug protocol-gen)".into()))
}

fn protocol_for_hash(hash: &'static str) -> Result<Arc<Protocol>> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Arc<Protocol>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .map_err(|_| Error::Protocol("cache empoisonné".into()))?;
    if let Some(p) = guard.get(hash) {
        return Ok(Arc::clone(p));
    }
    let raw = embed::TABLES
        .iter()
        .find(|(h, _)| *h == hash)
        .map(|(_, raw)| *raw)
        .ok_or_else(|| Error::Protocol(format!("table {hash} absente (bug protocol-gen)")))?;
    let p = Arc::new(Protocol::parse(raw)?);
    guard.insert(hash, Arc::clone(&p));
    Ok(p)
}
