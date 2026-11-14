use std::io;
use std::path::Path;

const ONE_GIB: u64 = 1_073_741_824;

#[derive(Debug, thiserror::Error)]
pub enum DiskError {
    #[error("insufficient disk space: {free} bytes free, {needed} needed")]
    Insufficient { free: u64, needed: u64 },
    #[error("disk space check failed: {0}")]
    Io(#[from] io::Error),
}

/// Space needed to archive `replay_bytes` of replays: a copy, working room, plus 1 GiB headroom.
pub fn required_free_bytes(replay_bytes: u64) -> u64 {
    replay_bytes.saturating_mul(2).saturating_add(ONE_GIB)
}

/// Bytes available to the current user on the filesystem holding `path` (must exist).
pub fn free_space(path: &Path) -> io::Result<u64> {
    fs4::available_space(path)
}

/// Checks that the filesystem holding `path` (must exist) has enough free space.
pub fn check_free_space(path: &Path, replay_bytes: u64) -> Result<(), DiskError> {
    let free = free_space(path)?;
    let needed = required_free_bytes(replay_bytes);
    if free < needed {
        return Err(DiskError::Insufficient { free, needed });
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn required_free_bytes_is_twice_plus_one_gib() {
        assert_eq!(required_free_bytes(10), 20 + 1_073_741_824);
    }

    #[test]
    fn check_free_space_reports_insufficient() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = check_free_space(tmp.path(), u64::MAX / 4).expect_err("must fail");
        assert!(matches!(err, DiskError::Insufficient { .. }));
    }
}
