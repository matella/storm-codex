//! Typed errors of the crate — the classes are stable: they will feed the server's
//! parse failure statistics (milestone 3, table `uploads.error_class`).

/// Decoding error of a `.StormReplay`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file cannot be read.
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    /// The MPQ archive is invalid or unreadable.
    #[error("invalid MPQ archive: {0}")]
    Mpq(String),

    /// An expected embedded file is missing or unreadable (e.g. replay.tracker.events).
    #[error("stream {0} missing or unreadable: {1}")]
    MissingStream(&'static str, String),

    /// End of data reached in the middle of decoding.
    #[error("truncated data: {0}")]
    Truncated(String),

    /// The data does not follow the protocol grammar.
    #[error("corrupted data: {0}")]
    Corrupted(String),

    /// Invalid protocol table or typeid outside the table (generation bug, not a replay bug).
    #[error("invalid protocol table: {0}")]
    Protocol(String),
}

pub type Result<T> = std::result::Result<T, Error>;
