//! Protocols exported as JSON (spike/export_protocols.py): loading + stream decoding,
//! following heroprotocol's canonical chain (hero_cli.py / protocolXXXXX.py).

use crate::versioned::{parse_typeinfos, TypeInfo, Value, VersionedDecoder};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct Protocol {
    #[allow(dead_code)] // useful for debugging, not read by the binary yet
    pub base_build: u32,
    typeinfos: Vec<TypeInfo>,
    replay_header_typeid: usize,
    game_details_typeid: usize,
    tracker_eventid_typeid: usize,
    svaruint32_typeid: usize,
    /// eventid → (typeid of the struct, event name)
    tracker_event_types: HashMap<i64, (usize, String)>,
}

impl Protocol {
    pub fn load(path: &std::path::Path) -> Result<Protocol> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let json: serde_json::Value = serde_json::from_slice(&raw)?;
        let get = |k: &str| -> Result<usize> {
            json.get(k)
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .ok_or_else(|| anyhow!("{k} missing from {}", path.display()))
        };
        let mut tracker_event_types = HashMap::new();
        if let Some(map) = json.get("tracker_event_types").and_then(|v| v.as_object()) {
            for (k, v) in map {
                let pair = v.as_array().ok_or_else(|| anyhow!("tracker_event_types"))?;
                tracker_event_types.insert(
                    k.parse::<i64>()?,
                    (
                        pair[0].as_u64().ok_or_else(|| anyhow!("event typeid"))? as usize,
                        pair[1].as_str().unwrap_or("?").to_owned(),
                    ),
                );
            }
        }
        Ok(Protocol {
            base_build: get("base_build")? as u32,
            typeinfos: parse_typeinfos(
                json.get("typeinfos").ok_or_else(|| anyhow!("typeinfos missing"))?,
            )?,
            replay_header_typeid: get("replay_header_typeid")?,
            game_details_typeid: get("game_details_typeid")?,
            tracker_eventid_typeid: get("tracker_eventid_typeid")?,
            svaruint32_typeid: get("svaruint32_typeid")?,
            tracker_event_types,
        })
    }

    pub fn decode_header(&self, content: &[u8]) -> Result<Value> {
        VersionedDecoder::new(content, &self.typeinfos).instance(self.replay_header_typeid)
    }

    pub fn decode_details(&self, content: &[u8]) -> Result<Value> {
        VersionedDecoder::new(content, &self.typeinfos).instance(self.game_details_typeid)
    }

    /// Port of `_decode_event_stream` (tracker: no userid, byte-aligned by construction).
    pub fn decode_tracker_events(&self, content: &[u8]) -> Result<Vec<(String, Value)>> {
        let mut decoder = VersionedDecoder::new(content, &self.typeinfos);
        let mut events = Vec::new();
        let mut gameloop: i64 = 0;
        while !decoder.done() {
            let delta = decoder
                .instance(self.svaruint32_typeid)?
                .first_field_int()
                .ok_or_else(|| anyhow!("invalid svaruint32 delta"))?;
            gameloop += delta;
            let eventid = decoder
                .instance(self.tracker_eventid_typeid)?
                .as_int()
                .ok_or_else(|| anyhow!("invalid eventid"))?;
            let (typeid, name) = self
                .tracker_event_types
                .get(&eventid)
                .ok_or_else(|| anyhow!("unknown eventid {eventid} at byte {}", decoder.used_bytes()))?;
            let mut event = decoder.instance(*typeid)?;
            if let Value::Struct(fields) = &mut event {
                fields.push(("_gameloop".into(), Value::Int(gameloop)));
            }
            events.push((name.clone(), event));
        }
        Ok(events)
    }
}

/// Protocol directory + cache (a protocol is parsed only once per bench,
/// like the Python imports or the compiled tables of the .NET parser).
pub struct ProtocolStore {
    dir: PathBuf,
    latest_build: u32,
    cache: HashMap<u32, Protocol>,
}

impl ProtocolStore {
    pub fn open(dir: PathBuf) -> Result<ProtocolStore> {
        let latest_build = std::fs::read_to_string(dir.join("latest.txt"))
            .with_context(|| format!("latest.txt missing from {}", dir.display()))?
            .trim()
            .parse::<u32>()?;
        Ok(ProtocolStore { dir, latest_build, cache: HashMap::new() })
    }

    pub fn latest(&mut self) -> Result<&Protocol> {
        self.for_exact_build(self.latest_build)
    }

    /// Protocol of the requested build, "latest known protocol" fallback (spec behaviour).
    pub fn for_build(&mut self, build: u32) -> Result<&Protocol> {
        let build = if self.dir.join(format!("{build}.json")).exists() {
            build
        } else {
            self.latest_build
        };
        self.for_exact_build(build)
    }

    fn for_exact_build(&mut self, build: u32) -> Result<&Protocol> {
        if !self.cache.contains_key(&build) {
            let p = Protocol::load(&self.dir.join(format!("{build}.json")))?;
            self.cache.insert(build, p);
        }
        Ok(self.cache.get(&build).expect("inserted above"))
    }
}
