//! Parseur autonome du fichier de lobby Heroes of the Storm.
//!
//! Le jeu écrit `replay.server.battlelobby` pendant l'écran de chargement, avant que le replay
//! n'existe. Ce crate lit ce blob **seul** — sans le stream `details` du replay — pour identifier
//! les joueurs d'une partie en cours.
//!
//! Ce que le format expose réellement, et ce qu'il n'expose pas, est constaté dans
//! `docs/research/2026-08-27-lobby-format.md` : les BattleTags sont en clair, mais ni le toon
//! handle, ni le héros pické, ni la carte, ni un champ d'équipe explicite ne s'y trouvent. Le type
//! public ne porte donc que ce qui est réellement décodable. La résolution BattleTag → identité
//! applicative se fait en aval, côté serveur, contre l'archive des parties déjà jouées.
//!
//! Crate pur : aucune I/O, aucune dépendance sur `storm-replay`.

use thiserror::Error;

/// Un joueur du lobby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyPlayer {
    /// Nom du compte, sans le discriminant. Peut contenir de l'UTF-8 non-ASCII (cf. le cas
    /// cyrillique documenté dans le rapport de format).
    pub name: String,
    /// Discriminant seul, la partie après `#`.
    pub discriminant: String,
    /// 0 ou 1. `None` quand l'appartenance n'a pas pu être déterminée — le format ne porte aucun
    /// champ d'équipe, elle est déduite de l'ordre (cf. `parse`).
    pub team: Option<u8>,
}

impl LobbyPlayer {
    /// `"nom#1234"` — la clé d'identité du joueur. C'est ce que le serveur rapprochera de
    /// `match_players.name` + `match_players.data->>'tag'` pour retrouver son historique.
    #[must_use]
    pub fn battletag(&self) -> String {
        format!("{}#{}", self.name, self.discriminant)
    }
}

/// Un lobby décodé. `players` est dans l'ordre d'apparition dans le blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
}

#[derive(Debug, Error)]
pub enum LobbyError {
    #[error("blob trop court ({0} octets) pour contenir un lobby")]
    TooShort(usize),
    #[error("aucun joueur identifiable dans le blob")]
    NoPlayers,
    #[error("structure de lobby non reconnue : {0}")]
    Unrecognized(String),
}

/// Décode un blob `replay.server.battlelobby`.
///
/// # Errors
/// Retourne [`LobbyError`] si le blob est trop court ou ne contient aucun joueur identifiable.
/// Ne panique jamais, quelle que soit l'entrée.
pub fn parse(bytes: &[u8]) -> Result<Lobby, LobbyError> {
    Err(LobbyError::Unrecognized(format!(
        "parse non implémenté ({} octets)",
        bytes.len()
    )))
}
