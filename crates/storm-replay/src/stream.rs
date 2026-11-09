//! Decoding of a **bare** event stream (without an MPQ archive), fed in chunks.
//!
//! During a game, HotS writes `replay.tracker.events` and `replay.game.events` into
//! `%TEMP%\Heroes of the Storm\TempWriteReplayP1\` in 4 KB blocks; the last event of a
//! block is often cut. [`StreamDecoder`] returns the complete events as they come and keeps
//! the cut tail for the next chunk. Measurements: `docs/research/2026-10-24-tempwritereplay-live.md`.

use crate::error::{Error, Result};
use crate::protocol::{self, Protocol, StreamKind};
use crate::value::Value;
use std::sync::Arc;

/// Resumable decoder for a tracker or game events stream.
///
/// ```no_run
/// # fn main() -> storm_replay::Result<()> {
/// use storm_replay::{StreamDecoder, StreamKind};
/// let mut dec = StreamDecoder::new(StreamKind::Tracker, None)?;
/// let bloc = std::fs::read("replay.tracker.events")?;
/// for chunk in bloc.chunks(4096) {
///     for event in dec.feed(chunk)? {
///         // complete events only
///         let _ = event;
///     }
/// }
/// # Ok(()) }
/// ```
pub struct StreamDecoder {
    protocol: Arc<Protocol>,
    kind: StreamKind,
    /// Bytes received but not yet decoded into a complete event.
    pending: Vec<u8>,
    /// Gameloop of the last complete event (the stream's deltas start from there).
    gameloop: i64,
    consumed: u64,
    fallback: Option<(u32, u32)>,
}

impl StreamDecoder {
    /// `build = None` → last embedded protocol. A build without an exact table is decoded with the
    /// last known protocol; [`StreamDecoder::protocol_fallback`] flags it.
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

    /// The stream decoded by this decoder.
    pub fn kind(&self) -> StreamKind {
        self.kind
    }

    /// `Some((requested build, used build))` if the requested build has no exact table.
    pub fn protocol_fallback(&self) -> Option<(u32, u32)> {
        self.fallback
    }

    /// Adds bytes and returns the **complete** events they finish. An event cut
    /// at the end of a chunk waits for the next one. An error other than an end of data (grammar,
    /// unknown eventid — typically a wrong protocol) is returned, and the decoder stays in its
    /// state from before the call: the faulty bytes are not consumed.
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
            return Err(Error::Corrupted("partial decode beyond the buffer".into()));
        }
        self.pending.drain(..partial.consumed);
        self.consumed += partial.consumed as u64;
        self.gameloop = partial.gameloop;
        Ok(partial.events)
    }

    /// Bytes already decoded into complete events since the start of the stream.
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Pending bytes (a cut event).
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Gameloop of the last complete event (0 before the first one).
    pub fn gameloop(&self) -> i64 {
        self.gameloop
    }
}
