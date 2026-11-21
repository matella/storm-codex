//! Match the owner encoded by a standard HotS folder against the replay projection, plus the
//! persisted "confirmed" accounts (docs/specs/2026-11-21-standalone-confirmed-identity.md): a
//! ToonHandle kept in "my identity" independent of whether its replay folder stays configured.

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

/// Sorted, deduplicated union of the live folder toons and the persisted confirmed toons. Pure
/// (no I/O) so the merge logic has a native unit test independent of the database.
fn union_toons(mut live: Vec<String>, confirmed: Vec<String>) -> Vec<String> {
    live.extend(confirmed);
    live.sort_unstable();
    live.dedup();
    live
}

/// Every persisted confirmed toon (`POST`/`DELETE /api/admin/identity/confirm`).
async fn confirmed_toons(db: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT toon FROM confirmed_identity")
        .fetch_all(db)
        .await
}

/// Candidate handles: the union of the effective folder list (a read by handle can only match
/// projected rows) and the persisted confirmed toons, independent of whether their folder is
/// still configured.
pub(crate) async fn operator_toons(app: &AppState) -> Vec<String> {
    let live: Vec<String> = match &app.standalone {
        Some(st) => lock(&st.view)
            .folders
            .iter()
            .filter_map(|p| folder_toon(p).map(str::to_owned))
            .collect(),
        None => Vec::new(),
    };
    let confirmed = match confirmed_toons(&app.db).await {
        Ok(confirmed) => confirmed,
        Err(e) => {
            tracing::error!(error = %e, "reading the confirmed identity");
            Vec::new()
        }
    };
    union_toons(live, confirmed)
}

/// `POST /api/admin/identity/confirm` — idempotent upsert, keyed by the toon the frontend already
/// displays (no re-validation of its format: the server only ever sees toons it itself produced).
pub(crate) async fn confirm(db: &PgPool, toon: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO confirmed_identity (toon) VALUES ($1) ON CONFLICT (toon) DO NOTHING")
        .bind(toon)
        .execute(db)
        .await
        .map(|_| ())
}

/// `DELETE /api/admin/identity/confirm/{toon}` — idempotent (removing an unconfirmed toon stays
/// a success).
pub(crate) async fn unconfirm(db: &PgPool, toon: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM confirmed_identity WHERE toon = $1")
        .bind(toon)
        .execute(db)
        .await
        .map(|_| ())
}

#[derive(Serialize)]
pub(super) struct FolderIdentity {
    folder: String,
    toon: Option<String>,
    name: Option<String>,
    aliases: Vec<String>,
    /// `true` when this folder's toon is in the persisted confirmed set.
    confirmed: bool,
}

/// Read it from the projection on demand: a restarted or no-op backfill needs no writes, and
/// an asynchronously parsed upload (HTTP 202) becomes visible only after its projection commits.
pub(super) async fn folder_identities(
    db: &PgPool,
    st: &StandaloneState,
) -> Result<Vec<FolderIdentity>, sqlx::Error> {
    let folders = lock(&st.view).folders.clone();
    let confirmed: std::collections::HashSet<String> =
        confirmed_toons(db).await?.into_iter().collect();
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
        let toon = candidate.filter(|_| name.is_some()).map(str::to_owned);
        identities.push(FolderIdentity {
            folder: folder.to_string_lossy().into_owned(),
            confirmed: toon.as_deref().is_some_and(|t| confirmed.contains(t)),
            toon,
            name,
            aliases,
        });
    }
    Ok(identities)
}

#[cfg(test)]
mod tests {
    use super::union_toons;

    #[test]
    fn union_toons_dedups_a_toon_present_in_both() {
        let live = vec!["b".to_string(), "a".to_string()];
        let confirmed = vec!["a".to_string(), "c".to_string()];
        assert_eq!(union_toons(live, confirmed), vec!["a", "b", "c"]);
    }

    #[test]
    fn union_toons_keeps_a_confirmed_toon_whose_folder_was_removed() {
        let live = Vec::new();
        let confirmed = vec!["2-Hero-1-10".to_string()];
        assert_eq!(
            union_toons(live, confirmed),
            vec!["2-Hero-1-10".to_string()]
        );
    }

    #[test]
    fn union_toons_is_sorted_and_empty_when_both_are() {
        assert_eq!(union_toons(Vec::new(), Vec::new()), Vec::<String>::new());
        assert_eq!(
            union_toons(vec!["z".to_string()], vec!["a".to_string()]),
            vec!["a".to_string(), "z".to_string()]
        );
    }
}
