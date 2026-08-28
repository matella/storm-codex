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

/// Relie un match fraîchement parsé au lobby courant, si c'est la même partie. Critère : l'ensemble
/// des BattleTags. Le parse complet reconstruit les mêmes `nom#tag` depuis le blob embarqué dans le
/// replay (`storm_stats`, `get_battletags`), donc les deux côtés portent la même clé — sans rien
/// supposer du format binaire. Renvoie `true` si la liaison a eu lieu (l'appelant sait alors qu'il
/// doit persister et diffuser).
pub async fn lier_match(db: &sqlx::PgPool, state: &mut LobbyState, match_id: i64) -> bool {
    if state.match_id.is_some() || state.players.is_empty() {
        return false;
    }
    let tags_match: Vec<String> = sqlx::query_scalar(
        "SELECT name || '#' || (data ->> 'tag')
         FROM match_players
         WHERE match_id = $1 AND name IS NOT NULL AND data ->> 'tag' IS NOT NULL",
    )
    .bind(match_id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let lobby_tags: Vec<String> = state.players.iter().map(|p| p.battletag.clone()).collect();
    if !memes_battletags(&tags_match, &lobby_tags) {
        return false;
    }
    state.match_id = Some(match_id);
    true
}

/// Cœur de `lier_match`, extrait en fonction pure pour être testable sans pool Postgres (même
/// motif que `meme_lobby` dans `api.rs`) : deux ensembles de BattleTags décrivent la même partie
/// s'ils contiennent exactement les mêmes tags, indépendamment de l'ordre. Un ensemble vide n'est
/// jamais considéré comme correspondant à lui-même — sans joueur, la comparaison ne prouve rien.
fn memes_battletags(a: &[String], b: &[String]) -> bool {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return false;
    }
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort();
    b.sort();
    a == b
}

#[cfg(test)]
mod tests {
    use super::memes_battletags;

    fn tags(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn memes_tags_dans_un_ordre_different_sont_le_meme_ensemble() {
        let a = tags(&["a#1", "b#2", "c#3"]);
        let b = tags(&["c#3", "a#1", "b#2"]);
        assert!(memes_battletags(&a, &b));
    }

    #[test]
    fn un_tag_different_donne_des_ensembles_differents() {
        let a = tags(&["a#1", "b#2", "c#3"]);
        let b = tags(&["a#1", "b#2", "quelqu_un_d_autre#9"]);
        assert!(!memes_battletags(&a, &b));
    }

    #[test]
    fn des_tailles_differentes_ne_correspondent_jamais() {
        let a = tags(&["a#1", "b#2", "c#3"]);
        let b = tags(&["a#1", "b#2"]);
        assert!(!memes_battletags(&a, &b));
    }

    #[test]
    fn deux_ensembles_vides_ne_correspondent_jamais() {
        let a: Vec<String> = Vec::new();
        let b: Vec<String> = Vec::new();
        assert!(!memes_battletags(&a, &b));
    }
}
