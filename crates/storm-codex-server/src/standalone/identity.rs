//! Match the owner encoded by a standard HotS folder against the replay projection.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use sqlx::PgPool;

use super::{lock, StandaloneState};
use crate::{players::names::name_history, AppState};

/// Only recognize a full standard suffix, not a handle-like name on an arbitrary extra folder.
fn folder_toon(folder: &Path) -> Option<&str> {
    let mut parts = folder.iter().rev().map(|part| part.to_str());
    for expected in ["Multiplayer", "Replays"] {
        if !parts.next()??.eq_ignore_ascii_case(expected) {
            return None;
        }
    }
    let toon = parts.next()??;
    parts.next()??; // account directory
    for expected in ["Accounts", "Heroes of the Storm"] {
        if !parts.next()??.eq_ignore_ascii_case(expected) {
            return None;
        }
    }
    let (region, rest) = toon.split_once("-Hero-")?;
    let (realm, id) = rest.split_once('-')?;
    [region, realm, id]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
        .then_some(toon)
}

/// Candidate handles from the effective folder list; a read by handle can only match projected rows.
pub(crate) fn operator_toons(app: &AppState) -> Vec<String> {
    let Some(st) = &app.standalone else {
        return Vec::new();
    };
    let mut handles: Vec<_> = lock(&st.view)
        .folders
        .iter()
        .filter_map(|p| folder_toon(p).map(str::to_owned))
        .collect();
    handles.sort_unstable();
    handles.dedup();
    handles
}

#[derive(Serialize)]
pub(super) struct FolderIdentity {
    folder: String,
    toon: Option<String>,
    name: Option<String>,
    aliases: Vec<String>,
}

/// Read it from the projection on demand: a restarted or no-op backfill needs no writes, and
/// an asynchronously parsed upload (HTTP 202) becomes visible only after its projection commits.
pub(super) async fn folder_identities(
    db: &PgPool,
    st: &StandaloneState,
) -> Result<Vec<FolderIdentity>, sqlx::Error> {
    let folders = lock(&st.view).folders.clone();
    let mut aliases_by_toon = HashMap::<&str, Vec<String>>::new();
    let mut identities = Vec::with_capacity(folders.len());
    for folder in &folders {
        let candidate = folder_toon(folder);
        let mut aliases = Vec::new();
        if let Some(toon) = candidate {
            if let Some(existing) = aliases_by_toon.get(toon) {
                aliases.clone_from(existing);
            } else {
                aliases = name_history(db, toon)
                    .await?
                    .into_iter()
                    .map(|span| span.name)
                    .filter(|name| !name.trim().is_empty())
                    .collect();
                aliases_by_toon.insert(toon, aliases.clone());
            }
        }
        let name = aliases.first().cloned();
        identities.push(FolderIdentity {
            folder: folder.to_string_lossy().into_owned(),
            toon: candidate.filter(|_| name.is_some()).map(str::to_owned),
            name,
            aliases,
        });
    }
    Ok(identities)
}
