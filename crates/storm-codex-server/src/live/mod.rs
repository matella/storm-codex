//! Feed live d'une partie en cours : les blocs de `replay.tracker.events` / `replay.game.events`
//! écrits par HotS pendant la partie arrivent par `POST /api/live/chunk`, sont décodés au fil de
//! l'eau (`storm_replay::StreamDecoder`) et réduits en un état en mémoire, jetable : le replay
//! archivé reste la source de vérité. Spec : `docs/specs/2026-10-24-feed-live-en-partie-design.md`.
pub mod api;
pub mod state;

use serde_json::{json, Value as J};
use state::{LiveState, Scope};
use storm_replay::{StreamDecoder, StreamKind};

/// Version du schéma de la vue `GET /api/live`.
pub const LIVE_SCHEMA_VERSION: u32 = 1;
/// Plafond par stream et par session. Mesuré : ~240 Ko de tracker et ~64 Ko de game events pour
/// 10 min contre l'IA ; une partie matchmakée de 30 min reste très en dessous. Au-delà, le
/// morceau est refusé (`413`) — garde-fou contre un client fou, pas une limite d'usage.
pub const MAX_STREAM_BYTES: u64 = 64 * 1024 * 1024;

/// Que faire d'un morceau, selon ce qui a déjà été reçu sur ce stream.
#[derive(Debug, PartialEq, Eq)]
pub enum ChunkPlan {
    /// Déjà reçu en entier : ignoré (idempotence).
    Duplicate,
    /// Trou avant ce morceau : le client doit renvoyer depuis `expected`.
    Gap { expected: u64 },
    /// À appliquer, en sautant les `skip` premiers octets déjà reçus (chevauchement).
    Apply { skip: usize },
}

/// Fonction pure : `received` = octets déjà acceptés sur le stream.
#[must_use]
pub fn plan_chunk(received: u64, offset: u64, len: usize) -> ChunkPlan {
    if offset > received {
        return ChunkPlan::Gap { expected: received };
    }
    let end = offset + len as u64;
    if end <= received {
        return ChunkPlan::Duplicate;
    }
    ChunkPlan::Apply {
        skip: usize::try_from(received - offset).unwrap_or(usize::MAX),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Live,
    Ended,
    /// Le décodage a échoué (protocole faux après un patch, ou données corrompues).
    Unreadable(String),
}

impl Status {
    fn as_str(&self) -> &'static str {
        match self {
            Status::Live => "live",
            Status::Ended => "ended",
            Status::Unreadable(_) => "unreadable",
        }
    }
}

struct StreamFeed {
    decoder: StreamDecoder,
    received: u64,
}

/// Résultat de l'application d'un morceau, renvoyé tel quel au client.
#[derive(Debug, PartialEq, Eq)]
pub enum ChunkOutcome {
    Applied {
        consumed: u64,
        last_loop: i64,
        events: usize,
    },
    Duplicate,
    Gap {
        expected: u64,
    },
    /// Session illisible : le morceau est compté reçu mais n'est plus décodé.
    Ignored,
    /// Le stream dépasserait [`MAX_STREAM_BYTES`] : refusé.
    TooLarge,
}

/// Une partie en cours (une seule à la fois).
pub struct LiveSession {
    pub id: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub last_chunk_at: chrono::DateTime<chrono::Utc>,
    pub status: Status,
    build: Option<u32>,
    fallback: Option<(u32, u32)>,
    tracker: StreamFeed,
    game: StreamFeed,
    pub state: LiveState,
}

impl LiveSession {
    /// `build = None` → dernier protocole embarqué (repli signalé dans la vue).
    pub fn new(id: String, build: Option<u32>) -> storm_replay::Result<LiveSession> {
        let tracker = StreamDecoder::new(StreamKind::Tracker, build)?;
        let game = StreamDecoder::new(StreamKind::Game, build)?;
        let now = chrono::Utc::now();
        Ok(LiveSession {
            id,
            started_at: now,
            last_chunk_at: now,
            status: Status::Live,
            build,
            fallback: tracker.protocol_fallback(),
            tracker: StreamFeed {
                decoder: tracker,
                received: 0,
            },
            game: StreamFeed {
                decoder: game,
                received: 0,
            },
            state: LiveState::default(),
        })
    }

    /// Applique un morceau `bytes` reçu à `offset` sur le stream `kind`.
    pub fn apply_chunk(&mut self, kind: StreamKind, offset: u64, bytes: &[u8]) -> ChunkOutcome {
        let feed = match kind {
            StreamKind::Tracker => &mut self.tracker,
            StreamKind::Game => &mut self.game,
        };
        let skip = match plan_chunk(feed.received, offset, bytes.len()) {
            ChunkPlan::Duplicate => return ChunkOutcome::Duplicate,
            ChunkPlan::Gap { expected } => return ChunkOutcome::Gap { expected },
            ChunkPlan::Apply { skip } => skip,
        };
        let fresh = &bytes[skip.min(bytes.len())..];
        if feed.received + fresh.len() as u64 > MAX_STREAM_BYTES {
            return ChunkOutcome::TooLarge;
        }
        feed.received += fresh.len() as u64;
        self.last_chunk_at = chrono::Utc::now();
        if matches!(self.status, Status::Unreadable(_)) {
            return ChunkOutcome::Ignored;
        }
        match feed.decoder.feed(fresh) {
            Ok(events) => {
                for ev in &events {
                    match kind {
                        StreamKind::Tracker => self.state.apply_tracker(ev),
                        StreamKind::Game => self.state.apply_game(ev),
                    }
                }
                ChunkOutcome::Applied {
                    consumed: feed.decoder.consumed(),
                    last_loop: feed.decoder.gameloop(),
                    events: events.len(),
                }
            }
            Err(e) => {
                let build = self
                    .build
                    .map_or("dernier embarqué".to_owned(), |b| b.to_string());
                tracing::warn!("feed live illisible ({kind:?}, build {build}) : {e}");
                self.status = Status::Unreadable(format!("{kind:?} : {e}"));
                ChunkOutcome::Ignored
            }
        }
    }

    /// Vue servie au front : état filtré + métadonnées de session.
    #[must_use]
    pub fn view(
        &self,
        scope: Scope,
        me: Option<i64>,
        lobby_slot: Option<i64>,
        map: Option<String>,
    ) -> J {
        let (me, me_source) = self.state.resolve_me(me, lobby_slot);
        let mut v = self.state.view(scope, me);
        if let Some(o) = v.as_object_mut() {
            o.insert("schema_version".into(), json!(LIVE_SCHEMA_VERSION));
            o.insert("session".into(), json!(self.id));
            o.insert("status".into(), json!(self.status.as_str()));
            if let Status::Unreadable(reason) = &self.status {
                o.insert("reason".into(), json!(reason));
            }
            o.insert("me_source".into(), json!(me_source));
            o.insert("map".into(), json!(map));
            o.insert("started_at".into(), json!(self.started_at.to_rfc3339()));
            o.insert(
                "last_chunk_at".into(),
                json!(self.last_chunk_at.to_rfc3339()),
            );
            o.insert(
                "protocol".into(),
                json!({
                    "build": self.build,
                    "fallback": self.fallback.map(|(asked, used)| json!({ "requested": asked, "used": used })),
                    "assumed_latest": self.build.is_none(),
                }),
            );
            o.insert(
                "bytes".into(),
                json!({ "tracker": self.tracker.received, "game": self.game.received }),
            );
        }
        v
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn plan_chunk_cases() {
        assert_eq!(plan_chunk(0, 0, 4096), ChunkPlan::Apply { skip: 0 });
        assert_eq!(plan_chunk(4096, 4096, 4096), ChunkPlan::Apply { skip: 0 });
        assert_eq!(plan_chunk(4096, 0, 4096), ChunkPlan::Duplicate);
        assert_eq!(
            plan_chunk(4096, 2048, 4096),
            ChunkPlan::Apply { skip: 2048 }
        );
        assert_eq!(
            plan_chunk(4096, 8192, 10),
            ChunkPlan::Gap { expected: 4096 }
        );
        assert_eq!(plan_chunk(10, 0, 0), ChunkPlan::Duplicate);
    }

    fn raw_streams() -> (Vec<u8>, Vec<u8>, u32) {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../storm-replay/tests/data");
        let path = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|x| x == "StormReplay"))
            .unwrap();
        let r = storm_replay::Replay::open(&path).unwrap();
        (
            r.tracker_events_raw().unwrap(),
            r.game_events_raw().unwrap(),
            r.header.base_build,
        )
    }

    #[test]
    fn duplicates_and_overlaps_do_not_double_count() {
        let (tracker, _, build) = raw_streams();
        let mut once = LiveSession::new("a".into(), Some(build)).unwrap();
        let mut noisy = LiveSession::new("b".into(), Some(build)).unwrap();
        let mut off = 0usize;
        for chunk in tracker.chunks(4096) {
            once.apply_chunk(StreamKind::Tracker, off as u64, chunk);
            // le même morceau deux fois, puis un chevauchement d'un demi-bloc
            noisy.apply_chunk(StreamKind::Tracker, off as u64, chunk);
            assert_eq!(
                noisy.apply_chunk(StreamKind::Tracker, off as u64, chunk),
                ChunkOutcome::Duplicate
            );
            off += chunk.len();
        }
        let back = off.saturating_sub(2048);
        noisy.apply_chunk(StreamKind::Tracker, back as u64, &tracker[back..]);
        assert_eq!(
            once.view(Scope::All, None, None, None)["players"],
            noisy.view(Scope::All, None, None, None)["players"]
        );
        assert_eq!(
            noisy.apply_chunk(StreamKind::Tracker, off as u64 + 1, b"x"),
            ChunkOutcome::Gap {
                expected: off as u64
            }
        );
    }

    #[test]
    fn garbage_marks_session_unreadable() {
        let mut s = LiveSession::new("c".into(), None).unwrap();
        let junk: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let out = s.apply_chunk(StreamKind::Tracker, 0, &junk);
        // Soit le hasard forme des événements tronqués (attente), soit c'est illisible ; jamais de panique.
        if s.status != Status::Live {
            assert_eq!(out, ChunkOutcome::Ignored);
            assert_eq!(s.view(Scope::Me, None, None, None)["status"], "unreadable");
            assert_eq!(
                s.apply_chunk(StreamKind::Tracker, 4096, &junk),
                ChunkOutcome::Ignored
            );
        }
    }
}
