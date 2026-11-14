use std::io;
use std::path::PathBuf;

/// Layout of the application data directory.
#[derive(Debug, Clone)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Creates the root and every subdirectory (idempotent).
    pub fn ensure(&self) -> io::Result<()> {
        for dir in [
            self.pg(),
            self.archive(),
            self.raw_cache(),
            self.images(),
            self.logs(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn pg(&self) -> PathBuf {
        self.root.join("pg")
    }
    pub fn archive(&self) -> PathBuf {
        self.root.join("archive")
    }
    pub fn raw_cache(&self) -> PathBuf {
        self.root.join("raw-cache")
    }
    pub fn images(&self) -> PathBuf {
        self.root.join("images")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    /// Upload ledger file (not a directory).
    pub fn ledger(&self) -> PathBuf {
        self.root.join("uploaded.json")
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn datadir_ensure_creates_all_subdirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let d = DataDir::new(tmp.path().join("data"));
        d.ensure().expect("ensure");
        for p in [d.pg(), d.archive(), d.raw_cache(), d.images(), d.logs()] {
            assert!(p.is_dir(), "{p:?}");
        }
        assert_eq!(d.ledger(), tmp.path().join("data").join("uploaded.json"));
        d.ensure().expect("idempotent");
    }
}
