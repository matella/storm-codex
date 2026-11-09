//! Standalone parser of the Heroes of the Storm lobby file.
//!
//! The game writes `replay.server.battlelobby` during the loading screen, before the replay
//! exists. This crate reads that blob **alone** — without the replay's `details` stream — to identify
//! the players of a game in progress.
//!
//! What the format really exposes, and what it does not, is recorded in
//! `docs/research/2026-08-27-lobby-format.md`: the BattleTags are in clear text, but neither the toon
//! handle, nor the picked hero, nor the map, nor an explicit team field are in it. The public
//! type therefore only carries what is really decodable. The resolution BattleTag → application
//! identity happens downstream, on the server side, against the archive of games already played.
//!
//! Pure crate: no I/O, no dependency on `storm-replay`.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use thiserror::Error;

mod maps;

/// Minimum length of a blob that can contain a valid BattleTag: name (3 characters) + `#` +
/// discriminator (4 digits). Below that, no match is possible by construction — no point
/// running the regex, better to report it explicitly via `LobbyError::TooShort`.
const MIN_BLOB_LEN: usize = 8;

/// Production regex, identical to the one of `storm-stats::process::get_battletags`
/// (`crates/storm-stats/src/process/mod.rs`) — validated on the reference corpus. `\p{L}` covers
/// non-ASCII alphabets (Cyrillic in particular); the optional `[zØ]?` suffix captures binary
/// noise stuck after the discriminator, removed afterwards (see `parse`).
fn battletag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[\p{L}\d]{3,24}#\d{4,10}[zØ]?")
            .unwrap_or_else(|e| unreachable!("invalid battletag regex: {e}"))
    })
}

/// A lobby player.
///
/// `#[non_exhaustive]`: this crate is meant for crates.io and will likely gain fields
/// (the picked hero, if Blizzard ever exposes it in this blob) — adding a field must not
/// break construction by external consumers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyPlayer {
    /// Account name, without the discriminator. May contain non-ASCII UTF-8 (see the
    /// Cyrillic case documented in the format report).
    pub name: String,
    /// Discriminator alone, the part after `#`.
    pub discriminant: String,
    /// 0 or 1. `None` when the membership could not be determined — the format carries no
    /// team field, it is deduced from the order (see `parse`).
    pub team: Option<u8>,
}

impl LobbyPlayer {
    /// `"name#1234"` — the player's identity key. It is what the server will match against
    /// `match_players.name` + `match_players.data->>'tag'` to find their history.
    #[must_use]
    pub fn battletag(&self) -> String {
        format!("{}#{}", self.name, self.discriminant)
    }
}

/// A decoded lobby. `players` is in order of appearance in the blob.
///
/// `#[non_exhaustive]` for the same reason as [`LobbyPlayer`]: room to evolve without breaking
/// semver for a crate published on crates.io.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
    /// Map, deduced from the `.s2ma` hashes via a table derived from the archive. `None` if no
    /// known hash — the format never names the map.
    ///
    /// Scope of the guarantee: the table is derived from the archive **and** validated on it, which
    /// is circular by construction. It cannot be wrong on a replay used for the
    /// derivation; the residual risk is a hash shared with a map missing from this corpus —
    /// a map never played, or a replay discarded during processing (failed parse, unreadable blob). The
    /// consumer must therefore treat this value as a convenience, not as a truth —
    /// which is why manual entry of the map stays available on the server side.
    pub map: Option<String>,
}

/// `#[non_exhaustive]` here too: it is the idiomatic tool to keep room for adding a
/// variant without a semver break — and it is what will one day allow removing `Unrecognized`
/// (not constructible today) without breaking the callers.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LobbyError {
    #[error("blob too short ({0} bytes) to contain a lobby")]
    TooShort(usize),
    #[error("no identifiable player in the blob")]
    NoPlayers,
    /// Not constructed today: the current decoding is a simple regex search, which does not
    /// distinguish any binary structure to validate. Kept (rather than removed) for a future
    /// structural decoding (e.g. reading typed binary fields instead of raw text) that could
    /// detect a recognisable but invalid blob shape — removing this public variant
    /// today would mean reintroducing it later as a semver break for a published crate.
    #[error("unrecognised lobby structure: {0}")]
    Unrecognized(String),
}

/// Hashes of the `.s2ma` files (maps) referenced by the blob, in order of appearance,
/// deduplicated. The blob never names the map: these hashes are the only lead, and their
/// mapping to a name lives in [`maps`] (derived from the archive).
#[must_use]
pub fn map_hashes(bytes: &[u8]) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Same pattern as `parse()`: the literal is checked by the tests, so a failure to
    // compile the regex is impossible in normal use — but no bare `unwrap()`.
    let re = RE.get_or_init(|| match Regex::new(r"([0-9a-f]{32})\.s2ma") {
        Ok(r) => r,
        Err(e) => unreachable!("invalid .s2ma regex: {e}"),
    });
    let text = String::from_utf8_lossy(bytes);
    let mut out: Vec<String> = Vec::new();
    for c in re.captures_iter(&text) {
        if let Some(h) = c.get(1) {
            let h = h.as_str().to_string();
            if !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out
}

/// Decodes a `replay.server.battlelobby` blob.
///
/// # Errors
/// Returns [`LobbyError`] if the blob is too short or contains no identifiable player.
/// Never panics, whatever the input.
pub fn parse(bytes: &[u8]) -> Result<Lobby, LobbyError> {
    if bytes.len() < MIN_BLOB_LEN {
        return Err(LobbyError::TooShort(bytes.len()));
    }

    // `from_utf8_lossy` decodes valid multi-byte UTF-8 sequences (Cyrillic BattleTags
    // included) and does not crash the search on any input: invalid bytes become
    // `U+FFFD`s, which match neither `\p{L}` nor `\d` and therefore act as separators.
    let text = String::from_utf8_lossy(bytes);
    let re = battletag_regex();

    let mut seen = HashSet::new();
    let mut players = Vec::new();
    for m in re.find_iter(&text) {
        let full = m.as_str();
        let Some(hash) = full.find('#') else {
            continue;
        };
        let name = &full[..hash];
        // Keep only the digits of the discriminator: the regex allows a final `z`/`Ø` suffix
        // that belongs to the surrounding binary noise, not to the BattleTag (see brief task 3,
        // resolution 1). The reference parser compares to an integer (`js_parse_int`).
        //
        // This truncation to ASCII digits is deliberate and aligned with `js_parse_int`
        // (`crates/storm-stats/src/process/js.rs`): both parsers must produce exactly the
        // same `name#discriminator` key so that a lobby decoded live and an archive decoded after
        // the fact meet on the same identity. A disagreement on this point would break the
        // replay ↔ lobby link documented in the companion spec, silently.
        let discriminant: String = full[hash + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if discriminant.is_empty() || name.is_empty() {
            continue;
        }

        let battletag = format!("{name}#{discriminant}");
        if !seen.insert(battletag) {
            // Already seen: we keep the first occurrence (and therefore the order it carries).
            continue;
        }

        players.push(LobbyPlayer {
            name: name.to_string(),
            discriminant,
            team: None,
        });
    }

    if players.is_empty() {
        return Err(LobbyError::NoPlayers);
    }

    // The format carries no explicit team field (see format report, Q3). The order
    // of appearance is only proof of team on a complete 5v5 game: elsewhere, a
    // wrong team would be worse than a missing team (see brief task 3, resolution 4).
    if players.len() == 10 {
        for (i, p) in players.iter_mut().enumerate() {
            p.team = Some(u8::from(i >= 5));
        }
    }

    // The blob never names the map: it is deduced from the `.s2ma` hashes it references, via the
    // table derived from the archive (see `maps`). `None` if no known hash.
    let map = map_hashes(bytes).iter().find_map(|h| {
        maps::MAP_BY_HASH
            .iter()
            .find(|(hash, _)| hash == h)
            .map(|(_, nom)| (*nom).to_string())
    });

    Ok(Lobby { players, map })
}
