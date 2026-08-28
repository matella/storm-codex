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

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use thiserror::Error;

/// Longueur minimale d'un blob pouvant contenir un BattleTag valide : nom (3 caractères) + `#` +
/// discriminant (4 chiffres). En dessous, aucun match n'est possible par construction — inutile de
/// lancer la regex, autant le signaler explicitement via `LobbyError::TooShort`.
const MIN_BLOB_LEN: usize = 8;

/// Regex de production, identique à celle de `storm-stats::process::get_battletags`
/// (`crates/storm-stats/src/process.rs`) — validée sur le corpus de référence. `\p{L}` couvre les
/// alphabets non-ASCII (cyrillique notamment) ; le suffixe `[zØ]?` optionnel capture du bruit
/// binaire collé après le discriminant, retiré ensuite (cf. `parse`).
fn battletag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[\p{L}\d]{3,24}#\d{4,10}[zØ]?")
            .unwrap_or_else(|e| unreachable!("regex de battletag invalide : {e}"))
    })
}

/// Un joueur du lobby.
///
/// `#[non_exhaustive]` : ce crate est destiné à crates.io et gagnera vraisemblablement des champs
/// (le héros pické, si Blizzard l'expose un jour dans ce blob) — un ajout de champ ne doit pas
/// casser la construction par les consommateurs externes.
#[non_exhaustive]
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
///
/// `#[non_exhaustive]` pour la même raison que [`LobbyPlayer`] : marge d'évolution sans rupture de
/// semver pour un crate publié sur crates.io.
#[non_exhaustive]
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
    /// Non construite aujourd'hui : le décodage actuel est une simple recherche par regex, qui ne
    /// distingue pas de structure binaire à valider. Conservée (plutôt que supprimée) pour un futur
    /// décodage structurel (ex. lecture de champs binaires typés au lieu du texte brut) qui pourrait
    /// détecter une forme de blob reconnaissable mais invalide — retirer cette variante publique
    /// aujourd'hui reviendrait à la réintroduire plus tard en rupture de semver pour un crate publié.
    #[error("structure de lobby non reconnue : {0}")]
    Unrecognized(String),
}

/// Décode un blob `replay.server.battlelobby`.
///
/// # Errors
/// Retourne [`LobbyError`] si le blob est trop court ou ne contient aucun joueur identifiable.
/// Ne panique jamais, quelle que soit l'entrée.
pub fn parse(bytes: &[u8]) -> Result<Lobby, LobbyError> {
    if bytes.len() < MIN_BLOB_LEN {
        return Err(LobbyError::TooShort(bytes.len()));
    }

    // `from_utf8_lossy` décode les séquences UTF-8 multi-octets valides (BattleTags cyrilliques
    // inclus) et ne fait planter la recherche sur aucune entrée : les octets invalides deviennent
    // des `U+FFFD`, qui ne matchent ni `\p{L}` ni `\d` et agissent donc comme des séparateurs.
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
        // Ne conserver que les chiffres du discriminant : la regex autorise un suffixe `z`/`Ø`
        // final qui appartient au bruit binaire environnant, pas au BattleTag (cf. brief tâche 3,
        // résolution 1). Le parseur de référence compare à un entier (`js_parse_int`).
        //
        // Cette troncature aux chiffres ASCII est délibérée et alignée sur `js_parse_int`
        // (`crates/storm-stats/src/process.rs`) : les deux parseurs doivent produire exactement la
        // même clé `nom#discriminant` pour qu'un lobby décodé en live et une archive décodée après
        // coup se rejoignent sur la même identité. Un désaccord sur ce point casserait la liaison
        // replay ↔ lobby documentée dans la spec companion, en silence.
        let discriminant: String = full[hash + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if discriminant.is_empty() || name.is_empty() {
            continue;
        }

        let battletag = format!("{name}#{discriminant}");
        if !seen.insert(battletag) {
            // Déjà vu : on garde la première occurrence (et donc l'ordre qu'elle porte).
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

    // Le format ne porte aucun champ d'équipe explicite (cf. rapport de format, Q3). L'ordre
    // d'apparition n'est une preuve d'équipe que sur une partie 5v5 complète : ailleurs, une
    // équipe fausse serait pire qu'une équipe absente (cf. brief tâche 3, résolution 4).
    if players.len() == 10 {
        for (i, p) in players.iter_mut().enumerate() {
            p.team = Some(u8::from(i >= 5));
        }
    }

    Ok(Lobby { players })
}
