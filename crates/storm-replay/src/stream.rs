//! Décodage d'un stream d'événements **nu** (sans archive MPQ), alimenté par morceaux.
//!
//! Pendant une partie, HotS écrit `replay.tracker.events` et `replay.game.events` dans
//! `%TEMP%\Heroes of the Storm\TempWriteReplayP1\` par blocs de 4 Ko ; le dernier événement d'un
//! bloc est souvent coupé. [`StreamDecoder`] rend les événements complets au fil de l'eau et garde
//! la queue coupée pour le morceau suivant. Mesures : `docs/research/2026-10-24-tempwritereplay-live.md`.

use crate::error::{Error, Result};
use crate::protocol::{self, Protocol, StreamKind};
use crate::value::Value;
use std::sync::Arc;

/// Décodeur reprenable d'un stream tracker ou game events.
///
/// ```no_run
/// # fn main() -> storm_replay::Result<()> {
/// use storm_replay::{StreamDecoder, StreamKind};
/// let mut dec = StreamDecoder::new(StreamKind::Tracker, None)?;
/// let bloc = std::fs::read("replay.tracker.events")?;
/// for chunk in bloc.chunks(4096) {
///     for event in dec.feed(chunk)? {
///         // événements complets uniquement
///         let _ = event;
///     }
/// }
/// # Ok(()) }
/// ```
pub struct StreamDecoder {
    protocol: Arc<Protocol>,
    kind: StreamKind,
    /// Octets reçus mais pas encore décodés en événement complet.
    pending: Vec<u8>,
    /// Gameloop du dernier événement complet (les deltas du stream partent de là).
    gameloop: i64,
    consumed: u64,
    fallback: Option<(u32, u32)>,
}

impl StreamDecoder {
    /// `build = None` → dernier protocole embarqué. Un build sans table exacte est décodé avec le
    /// dernier protocole connu ; [`StreamDecoder::protocol_fallback`] le signale.
    pub fn new(kind: StreamKind, build: Option<u32>) -> Result<StreamDecoder> {
        let (protocol, fallback) = match build {
            Some(b) => {
                let (p, fb) = protocol::protocol_for_build(b)?;
                (p, fb.then_some((b, protocol::LATEST_BUILD)))
            }
            None => (protocol::latest_protocol()?, None),
        };
        Ok(StreamDecoder {
            protocol,
            kind,
            pending: Vec::new(),
            gameloop: 0,
            consumed: 0,
            fallback,
        })
    }

    /// Le stream décodé par ce décodeur.
    pub fn kind(&self) -> StreamKind {
        self.kind
    }

    /// `Some((build demandé, build utilisé))` si le build demandé n'a pas de table exacte.
    pub fn protocol_fallback(&self) -> Option<(u32, u32)> {
        self.fallback
    }

    /// Ajoute des octets et rend les événements **complets** qu'ils terminent. Un événement coupé
    /// en fin de morceau attend le suivant. Une erreur autre qu'une fin de données (grammaire,
    /// eventid inconnu — typiquement un mauvais protocole) remonte, et le décodeur reste dans son
    /// état d'avant l'appel : les octets fautifs ne sont pas consommés.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Value>> {
        self.pending.extend_from_slice(bytes);
        let partial = match self
            .protocol
            .decode_partial(self.kind, &self.pending, self.gameloop)
        {
            Ok(p) => p,
            Err(e) => {
                self.pending.truncate(self.pending.len() - bytes.len());
                return Err(e);
            }
        };
        if partial.consumed > self.pending.len() {
            return Err(Error::Corrupted("décodage partiel au-delà du buffer".into()));
        }
        self.pending.drain(..partial.consumed);
        self.consumed += partial.consumed as u64;
        self.gameloop = partial.gameloop;
        Ok(partial.events)
    }

    /// Octets déjà décodés en événements complets depuis le début du stream.
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Octets en attente (un événement coupé).
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Gameloop du dernier événement complet (0 avant le premier).
    pub fn gameloop(&self) -> i64 {
        self.gameloop
    }
}
