//! Lobby live : état courant d'une partie en cours de chargement. Singleton persisté (calque de
//! `draft_live`) — le lobby courant écrase le précédent, il n'y a pas d'historique de lobbies : le
//! replay archivé reste la source de vérité (étage 1 des trois étages de données).
pub mod api;
pub mod enrich;
pub mod store;

use serde::{Deserialize, Serialize};
use serde_json::Value as J;

/// Version du schéma de `lobby_live.state` — un état d'une version inconnue est ignoré au
/// démarrage plutôt que désérialisé de travers.
pub const LOBBY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyPlayerState {
    pub name: String,
    pub discriminant: String,
    /// `"nom#1234"` — la clé d'identité issue du blob.
    pub battletag: String,
    /// Équipe : celle déduite par `storm-lobby`, ou celle fixée à la main (tâche 5).
    pub team: Option<u8>,
    /// `true` si l'équipe a été corrigée à la main — le front l'affiche différemment.
    #[serde(default)]
    pub team_manual: bool,
    /// Identité applicative, résolue contre l'archive. `None` = joueur jamais croisé.
    pub toon_handle: Option<String>,
    /// Agrégats d'archive (tâche 4). `null` tant que non enrichi ou si joueur inconnu.
    #[serde(default)]
    pub history: Option<J>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyState {
    pub schema_version: u32,
    pub detected_at: String,
    pub players: Vec<LobbyPlayerState>,
    /// Carte : déduite des hashes `.s2ma`, ou saisie à la main.
    pub map: Option<String>,
    #[serde(default)]
    pub map_manual: bool,
    /// Héros de l'opérateur — jamais dans le blob, toujours saisi (tâche 5).
    pub hero: Option<String>,
    /// Index dans `players` du joueur identifié comme l'opérateur, si trouvé.
    pub me: Option<usize>,
    /// Build suggéré + alternatives (tâche 4).
    #[serde(default)]
    pub build: Option<J>,
    /// Stats de l'opérateur sur ce héros / cette carte (tâche 4).
    #[serde(default)]
    pub me_stats: Option<J>,
    /// Rempli quand le replay correspondant est parsé (tâche 6) → bascule en debrief.
    #[serde(default)]
    pub match_id: Option<i64>,
    /// `parse_failed` quand le blob est illisible : la page affiche alors le sélecteur manuel
    /// plutôt qu'une page vide.
    #[serde(default)]
    pub status: Option<String>,
}

impl LobbyState {
    /// Construit l'état brut depuis un lobby décodé, avant enrichissement.
    #[must_use]
    pub fn from_lobby(lobby: &storm_lobby::Lobby, detected_at: String) -> Self {
        Self {
            schema_version: LOBBY_SCHEMA_VERSION,
            detected_at,
            players: lobby
                .players
                .iter()
                .map(|p| LobbyPlayerState {
                    name: p.name.clone(),
                    discriminant: p.discriminant.clone(),
                    battletag: p.battletag(),
                    team: p.team,
                    team_manual: false,
                    toon_handle: None,
                    history: None,
                })
                .collect(),
            map: lobby.map.clone(),
            map_manual: false,
            hero: None,
            me: None,
            build: None,
            me_stats: None,
            match_id: None,
            status: None,
        }
    }

    /// État dégradé quand le blob n'a pas pu être décodé (nouveau build Blizzard). La page reste
    /// utilisable : l'opérateur saisit son héros, le build s'affiche.
    #[must_use]
    pub fn unreadable(detected_at: String) -> Self {
        Self {
            schema_version: LOBBY_SCHEMA_VERSION,
            detected_at,
            players: Vec::new(),
            map: None,
            map_manual: false,
            hero: None,
            me: None,
            build: None,
            me_stats: None,
            match_id: None,
            status: Some("parse_failed".into()),
        }
    }
}
