use std::path::{Path, PathBuf};

/// Auto-detect HotS replay directories by scanning the standard install path:
/// Documents/Heroes of the Storm/Accounts/{account_id}/{toon_handle}/Replays/Multiplayer
pub fn detect_replay_dirs() -> Vec<PathBuf> {
    let mut results = Vec::new();

    let docs = match dirs::document_dir() {
        Some(d) => d,
        None => return results,
    };

    let accounts_dir = docs.join("Heroes of the Storm").join("Accounts");
    if !accounts_dir.is_dir() {
        return results;
    }

    // Iterate: Accounts/{account_id}/{toon_handle}/Replays/Multiplayer
    let account_entries = match std::fs::read_dir(&accounts_dir) {
        Ok(e) => e,
        Err(_) => return results,
    };

    for account_entry in account_entries.flatten() {
        if !account_entry.path().is_dir() {
            continue;
        }
        if let Ok(toon_entries) = std::fs::read_dir(account_entry.path()) {
            for toon_entry in toon_entries.flatten() {
                let replay_dir = toon_entry.path().join("Replays").join("Multiplayer");
                if replay_dir.is_dir() {
                    results.push(replay_dir);
                }
            }
        }
    }

    results
}

/// Détection sous un dossier **parent** : parcourt l'arborescence (profondeur bornée) et renvoie
/// chaque dossier contenant au moins un `.StormReplay`. Permet de pointer un seul dossier parent
/// (ex. un disque de sauvegarde, plusieurs comptes) et de laisser le client trouver tous les
/// dossiers de replays, sans énumérer la structure Accounts/<id>/<toon>/Replays/Multiplayer.
pub fn find_replay_dirs_under(parent: &Path) -> Vec<PathBuf> {
    const MAX_DEPTH: u32 = 8;
    let mut out = Vec::new();
    let mut stack = vec![(parent.to_path_buf(), 0u32)];

    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut has_replay = false;
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < MAX_DEPTH {
                    subdirs.push(path);
                }
            } else if !has_replay
                && path
                    .extension()
                    .map(|e| e.eq_ignore_ascii_case("StormReplay"))
                    .unwrap_or(false)
            {
                has_replay = true;
            }
        }
        if has_replay {
            out.push(dir);
        }
        stack.extend(subdirs.into_iter().map(|p| (p, depth + 1)));
    }

    out.sort();
    out.dedup();
    out
}
