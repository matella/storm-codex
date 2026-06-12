//! Erreurs typées du crate — les classes sont stables : elles alimenteront les statistiques
//! d'échec de parse côté serveur (jalon 3, table `uploads.error_class`).

/// Erreur de décodage d'un `.StormReplay`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Lecture du fichier impossible.
    #[error("E/S : {0}")]
    Io(#[from] std::io::Error),

    /// L'archive MPQ est invalide ou illisible.
    #[error("archive MPQ invalide : {0}")]
    Mpq(String),

    /// Un fichier embarqué attendu manque dans l'archive (ex. replay.tracker.events).
    #[error("stream absent de l'archive : {0}")]
    MissingStream(&'static str),

    /// Fin de données atteinte en plein décodage.
    #[error("données tronquées : {0}")]
    Truncated(String),

    /// Les données ne respectent pas la grammaire du protocole.
    #[error("données corrompues : {0}")]
    Corrupted(String),

    /// Table de protocole invalide ou typeid hors table (bug de génération, pas du replay).
    #[error("table de protocole invalide : {0}")]
    Protocol(String),
}

pub type Result<T> = std::result::Result<T, Error>;
