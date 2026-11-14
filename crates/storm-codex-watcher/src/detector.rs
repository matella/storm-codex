//! Replay folder detection.
//!
//! Heroes of the Storm stores replays under
//! `Documents/Heroes of the Storm/Accounts/{account_id}/{toon_handle}/Replays/Multiplayer`.
//! The Documents folder is resolved through the known-folder API (`dirs::document_dir`), never
//! by joining `%USERPROFILE%\Documents`, so a folder relocated by OneDrive is still found.

use std::path::{Path, PathBuf};

/// Maximum folder depth explored by [`find_replay_dirs_under`].
const MAX_DEPTH: u32 = 8;

/// Every `Replays/Multiplayer` folder of the local HotS install (empty folders included, they
/// are watched for new games). Empty when the game is not installed.
pub fn detect_replay_dirs() -> Vec<PathBuf> {
    match dirs::document_dir() {
        Some(documents) => detect_replay_dirs_in(&documents),
        None => Vec::new(),
    }
}

/// [`detect_replay_dirs`] for an explicit Documents folder.
pub fn detect_replay_dirs_in(documents: &Path) -> Vec<PathBuf> {
    let accounts = documents.join("Heroes of the Storm").join("Accounts");
    let mut out = Vec::new();
    for account in subdirs(&accounts) {
        for toon in subdirs(&account) {
            let replay_dir = toon.join("Replays").join("Multiplayer");
            if replay_dir.is_dir() {
                out.push(replay_dir);
            }
        }
    }
    out.sort();
    out
}

/// Every folder under `parent` (depth ≤ 8, `parent` included) that contains at least one
/// `.StormReplay`. Lets the user point at a single parent (a backup drive, several accounts)
/// without knowing the `Accounts/<id>/<toon>/Replays/Multiplayer` layout.
pub fn find_replay_dirs_under(parent: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(parent.to_path_buf(), 0u32)];

    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut has_replay = false;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if !has_replay && is_storm_replay(&path) {
                has_replay = true;
            }
        }
        if has_replay {
            out.push(dir);
        }
    }

    out.sort();
    out.dedup();
    out
}

/// True for a `.StormReplay` file name (extension compared case-insensitively).
pub fn is_storm_replay(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("StormReplay"))
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}
