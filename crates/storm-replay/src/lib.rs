//! `storm-replay` — décodeur de replays Heroes of the Storm (`.StormReplay`).
//!
//! Un replay est une archive MPQ : un *header* (section user data) + 6 fichiers embarqués.
//! Les protocoles par build sont embarqués dans le crate (générés par `tools/protocol_gen.py`
//! depuis [Blizzard/heroprotocol](https://github.com/Blizzard/heroprotocol)) ; un build inconnu
//! est décodé avec le dernier protocole connu (cf. [`Replay::protocol_fallback`]).
//!
//! ```no_run
//! # fn main() -> storm_replay::Result<()> {
//! let replay = storm_replay::Replay::open("match.StormReplay")?;
//! println!("build {} — {} game loops", replay.header.base_build, replay.header.elapsed_game_loops);
//! let details = replay.details()?;
//! for p in &details.players {
//!     println!("{} ({}) — équipe {}", p.name, p.hero, p.team_id);
//! }
//! let events = replay.tracker_events()?; // décodage paresseux, par stream
//! # Ok(()) }
//! ```

mod attributes;
mod bitpacked;
mod error;
mod protocol;
mod typeinfo;
mod value;
mod versioned;

pub use attributes::{AttributeValue, Attributes};
pub use error::{Error, Result};
pub use protocol::LATEST_BUILD;
pub use value::Value;

use protocol::Protocol;
use std::path::Path;
use std::sync::Arc;

/// Header du replay (décodé à l'ouverture, avec le dernier protocole connu — comportement
/// de référence : le header se décode avec n'importe quel protocole).
#[derive(Debug, Clone)]
pub struct ReplayHeader {
    /// Build du protocole (`m_version.m_baseBuild`) — détermine la table de décodage.
    pub base_build: u32,
    /// Version affichée `major.minor.revision.build`.
    pub version: (i64, i64, i64, i64),
    /// Durée en game loops (16 par seconde).
    pub elapsed_game_loops: u64,
}

/// Vue typée de `replay.details`.
#[derive(Debug, Clone)]
pub struct ReplayDetails {
    /// Nom de carte localisé (`m_title`).
    pub title: String,
    /// Horodatage Windows FILETIME (`m_timeUTC`, unités de 100 ns depuis 1601-01-01 UTC).
    pub time_utc: i64,
    /// Décalage local (`m_timeLocalOffset`, mêmes unités).
    pub time_local_offset: i64,
    pub players: Vec<PlayerDetails>,
}

/// Un joueur de `replay.details` (`m_playerList`).
#[derive(Debug, Clone)]
pub struct PlayerDetails {
    pub name: String,
    /// Nom de héros localisé.
    pub hero: String,
    /// 0 = équipe gauche/bleue, 1 = droite/rouge.
    pub team_id: i64,
    /// 1 = victoire, 2 = défaite.
    pub result: i64,
    /// Slot dans la liste de travail (lie details ↔ tracker/lobby), absent pour observateurs.
    pub working_set_slot_id: Option<i64>,
    /// Identifiant Battle.net stable « region-Hero-realm-id ».
    pub toon_handle: String,
}

/// Un `.StormReplay` ouvert : header décodé, streams **paresseux** (chaque accesseur décode
/// son stream à l'appel — rien n'est mis en cache, l'appelant garde la main).
pub struct Replay {
    bytes: Vec<u8>,
    mpq: nom_mpq::MPQ,
    /// Header brut complet (dont `m_ngdpRootKey`, `m_dataBuildNum`…).
    pub header_raw: Value,
    pub header: ReplayHeader,
    protocol: Arc<Protocol>,
    fallback: Option<(u32, u32)>,
}

impl Replay {
    pub fn open(path: impl AsRef<Path>) -> Result<Replay> {
        Replay::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Replay> {
        let (_, mpq) = nom_mpq::parser::parse(&bytes).map_err(|e| Error::Mpq(format!("{e:?}")))?;
        let user_data = mpq
            .user_data
            .as_ref()
            .ok_or_else(|| Error::Mpq("pas de section user data (header)".into()))?;
        let header_raw = protocol::latest_protocol()?.decode_header(&user_data.content)?;

        let version = header_raw
            .field("m_version")
            .ok_or_else(|| Error::Corrupted("header sans m_version".into()))?;
        let vfield = |name: &str| -> Result<i64> {
            version
                .field(name)
                .and_then(Value::as_int)
                .ok_or_else(|| Error::Corrupted(format!("header sans m_version.{name}")))
        };
        let base_build = u32::try_from(vfield("m_baseBuild")?)
            .map_err(|_| Error::Corrupted("m_baseBuild négatif".into()))?;
        let header = ReplayHeader {
            base_build,
            version: (
                vfield("m_major")?,
                vfield("m_minor")?,
                vfield("m_revision")?,
                vfield("m_build")?,
            ),
            elapsed_game_loops: header_raw
                .field("m_elapsedGameLoops")
                .and_then(Value::as_int)
                .and_then(|v| u64::try_from(v).ok())
                .unwrap_or(0),
        };

        let (protocol, used_fallback) = protocol::protocol_for_build(base_build)?;
        let fallback = used_fallback.then_some((base_build, LATEST_BUILD));
        Ok(Replay {
            bytes,
            mpq,
            header_raw,
            header,
            protocol,
            fallback,
        })
    }

    /// `Some((build demandé, build utilisé))` si le build du replay n'a pas de table exacte
    /// (replay plus récent que les protocoles embarqués) — à logger côté appelant.
    pub fn protocol_fallback(&self) -> Option<(u32, u32)> {
        self.fallback
    }

    fn stream(&self, name: &'static str) -> Result<Vec<u8>> {
        let (_, data) = self
            .mpq
            .read_mpq_file_sector(name, false, &self.bytes)
            .map_err(|e| Error::MissingStream(name, format!("{e:?}")))?;
        Ok(data)
    }

    /// `replay.details` brut (tout `m_playerList`, handles de cache…).
    pub fn details_raw(&self) -> Result<Value> {
        self.protocol
            .decode_details(&self.stream("replay.details")?)
    }

    /// Vue typée de `replay.details`.
    pub fn details(&self) -> Result<ReplayDetails> {
        let raw = self.details_raw()?;
        let title = raw
            .field("m_title")
            .and_then(Value::as_str_lossy)
            .ok_or_else(|| Error::Corrupted("details sans m_title".into()))?;
        let int = |v: &Value, name: &str| -> i64 {
            v.field(name).and_then(Value::as_int).unwrap_or_default()
        };
        let mut players = Vec::new();
        if let Some(list) = raw.field("m_playerList").and_then(Value::as_array) {
            for p in list {
                let toon = p.field("m_toon");
                let toon_handle = toon
                    .map(|t| {
                        format!(
                            "{}-Hero-{}-{}",
                            int(t, "m_region"),
                            int(t, "m_realm"),
                            int(t, "m_id")
                        )
                    })
                    .unwrap_or_default();
                players.push(PlayerDetails {
                    name: p
                        .field("m_name")
                        .and_then(Value::as_str_lossy)
                        .unwrap_or_default(),
                    hero: p
                        .field("m_hero")
                        .and_then(Value::as_str_lossy)
                        .unwrap_or_default(),
                    team_id: int(p, "m_teamId"),
                    result: int(p, "m_result"),
                    working_set_slot_id: p.field("m_workingSetSlotId").and_then(Value::as_int),
                    toon_handle,
                });
            }
        }
        Ok(ReplayDetails {
            title,
            time_utc: raw
                .field("m_timeUTC")
                .and_then(Value::as_int)
                .unwrap_or_default(),
            time_local_offset: raw
                .field("m_timeLocalOffset")
                .and_then(Value::as_int)
                .unwrap_or_default(),
            players,
        })
    }

    /// `replay.initData` brut (lobby complet — bitpacked).
    pub fn initdata_raw(&self) -> Result<Value> {
        self.protocol
            .decode_initdata(&self.stream("replay.initData")?)
    }

    /// `replay.attributes.events` (mode de jeu, difficulté, compositions de lobby…).
    pub fn attributes(&self) -> Result<Attributes> {
        attributes::decode_attributes(&self.stream("replay.attributes.events")?)
    }

    /// `replay.tracker.events` — stats, unités, score de fin de partie.
    pub fn tracker_events(&self) -> Result<Vec<Value>> {
        self.protocol
            .decode_tracker_events(&self.stream("replay.tracker.events")?)
    }

    /// `replay.game.events` — entrées joueur (ordres, sélections, talents…).
    pub fn game_events(&self) -> Result<Vec<Value>> {
        self.protocol
            .decode_game_events(&self.stream("replay.game.events")?)
    }

    /// `replay.message.events` — chat et pings.
    pub fn message_events(&self) -> Result<Vec<Value>> {
        self.protocol
            .decode_message_events(&self.stream("replay.message.events")?)
    }

    /// Taille du stream game events décompressé sans le décoder (diagnostic de perf).
    #[doc(hidden)]
    pub fn game_events_raw_len(&self) -> Result<usize> {
        Ok(self.stream("replay.game.events")?.len())
    }

    /// `replay.server.battlelobby` brut (non décodé — utilisé pour extraire les BattleTags,
    /// comme hots-parser).
    pub fn battlelobby_raw(&self) -> Result<Vec<u8>> {
        self.stream("replay.server.battlelobby")
    }
}
