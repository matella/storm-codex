//! Projection d'un `storm_stats::Output` (parse réussi) en lignes Postgres, dans une
//! transaction idempotente : delete-then-insert par fingerprint (re-process piloté par
//! `parser_version`). Grosses structures en JSONB `data`, scalaires promus pour les filtres.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value as J};
use sqlx::PgPool;
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("base : {0}")]
    Db(#[from] sqlx::Error),
    #[error("sortie de parse sans match/players")]
    Empty,
}

fn i(m: &Map<String, J>, k: &str) -> Option<i64> {
    m.get(k).and_then(J::as_i64)
}
fn gs_i(p: &J, k: &str) -> Option<i64> {
    p.get("gameStats")
        .and_then(|g| g.get(k))
        .and_then(J::as_f64)
        .map(|v| v as i64)
}

/// Héros réellement joué, dérivé des talents via `dim_talents`. Locale-indépendant et immunisé au
/// shuffle ARAM (où l'attribut 4002 lu par le parser peut être un des héros *proposés*, pas celui
/// joué — cf. talents de Johanna stockés alors que `hero`="D.Va"). Vote majoritaire : un talent
/// générique ambigu ne peut pas l'emporter sur les talents spécifiques du héros. `None` si aucun
/// talent n'est résoluble (joueur sans talent, ou référentiel `dim_talents` vide/incomplet) →
/// l'appelant garde alors le `hero` du parser.
fn talent_hero(p: &J, lut: &HashMap<String, String>) -> Option<String> {
    let talents = p.get("talents").and_then(J::as_object)?;
    let mut tally: HashMap<&str, u32> = HashMap::new();
    for tid in talents.values().filter_map(J::as_str) {
        if let Some(h) = lut.get(tid) {
            *tally.entry(h.as_str()).or_default() += 1;
        }
    }
    tally.into_iter().max_by_key(|(_, n)| *n).map(|(h, _)| h.to_string())
}

/// Toon → héros joué, pour les seuls joueurs dont le héros résolu par les talents diffère du
/// héros du parser (attribut 4002).
fn hero_corrections(
    players: &Map<String, J>,
    lut: &HashMap<String, String>,
) -> HashMap<String, String> {
    players
        .iter()
        .filter_map(|(toon, p)| {
            let played = talent_hero(p, lut)?;
            (p.get("hero").and_then(J::as_str) != Some(played.as_str()))
                .then(|| (toon.clone(), played))
        })
        .collect()
}

/// Réécrit le héros de chaque copie `victim`/`killers[]` d'un tableau de takedowns, par handle
/// du joueur (jamais par nom de héros : deux joueurs peuvent partager le même héros parser).
/// Le killer `player: "0"` (« Nexus Forces ») n'est dans aucune correction → intact.
fn fix_takedown_heroes(tds: Option<&mut J>, fix: &HashMap<String, String>) {
    let Some(tds) = tds.and_then(J::as_array_mut) else {
        return;
    };
    let mut fix_one = |who: &mut J| {
        let hero = who
            .get("player")
            .and_then(J::as_str)
            .and_then(|h| fix.get(h));
        if let (Some(hero), Some(obj)) = (hero, who.as_object_mut()) {
            obj.insert("hero".into(), J::from(hero.clone()));
        }
    };
    for td in tds {
        if let Some(v) = td.get_mut("victim") {
            fix_one(v);
        }
        if let Some(ks) = td.get_mut("killers").and_then(J::as_array_mut) {
            ks.iter_mut().for_each(&mut fix_one);
        }
    }
}

/// Applique les corrections de héros partout où storm-stats a copié le héros du parser :
/// `players[].hero` (original tracé dans `heroCorrectedFrom`), `players[].deaths`,
/// `players[].takedowns` (de **tous** les joueurs : un joueur non corrigé peut avoir un killer
/// corrigé) et `match.takedowns`. Aucune correction → entrée inchangée.
fn apply_hero_corrections(
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    fix: &HashMap<String, String>,
) {
    if fix.is_empty() {
        return;
    }
    for (toon, p) in players.iter_mut() {
        let Some(obj) = p.as_object_mut() else {
            continue;
        };
        if let Some(h) = fix.get(toon) {
            let orig = obj.get("hero").cloned().unwrap_or(J::Null);
            obj.insert("heroCorrectedFrom".into(), orig);
            obj.insert("hero".into(), J::from(h.clone()));
        }
        fix_takedown_heroes(obj.get_mut("deaths"), fix);
        fix_takedown_heroes(obj.get_mut("takedowns"), fix);
    }
    fix_takedown_heroes(match_.get_mut("takedowns"), fix);
}

/// `true` si l'erreur est un deadlock (40P01) ou un échec de sérialisation (40001) — transitoire,
/// donc re-tentable.
fn is_retryable(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if matches!(db.code().as_deref(), Some("40P01") | Some("40001")))
}

/// Projette le match avec reprise sur deadlock (backfill concurrent : plusieurs transactions
/// touchent les mêmes lignes `players` / `matches`).
pub async fn project_match(
    db: &PgPool,
    fingerprint: &str,
    parser_version: i32,
    output: &storm_stats::Output,
) -> Result<i64, ProjectError> {
    let mut attempt = 0;
    loop {
        match project_once(db, fingerprint, parser_version, output).await {
            Err(ProjectError::Db(e)) if is_retryable(&e) && attempt < 4 => {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(20 * attempt)).await;
            }
            other => return other,
        }
    }
}

/// Insère/replace le match et ses joueurs (une tentative). Renvoie l'id du match.
async fn project_once(
    db: &PgPool,
    fingerprint: &str,
    parser_version: i32,
    output: &storm_stats::Output,
) -> Result<i64, ProjectError> {
    let m = output.match_.as_ref().ok_or(ProjectError::Empty)?;
    let players = output.players.as_ref().ok_or(ProjectError::Empty)?;

    let build = m
        .get("version")
        .and_then(|v| v.get("m_build"))
        .and_then(J::as_i64)
        .map(|v| v as i32);
    let mode = i(m, "mode").map(|v| v as i32);
    let map = m.get("map").and_then(J::as_str);
    let duration_loops = i(m, "loopLength").map(|v| v as i32);
    let length = m.get("length").and_then(J::as_f64);
    let played_at: Option<DateTime<Utc>> = m
        .get("date")
        .and_then(J::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc));
    let winner = i(m, "winner").map(|v| v as i32);
    let first_pick_win = m.get("firstPickWin").and_then(J::as_bool);
    let first_objective = i(m, "firstObjective").map(|v| v as i32);
    let first_fort = i(m, "firstFort").map(|v| v as i32);
    let first_keep = i(m, "firstKeep").map(|v| v as i32);

    let mut tx = db.begin().await?;
    // idempotence : un match existant (même replay) est remplacé (cascade match_players)
    sqlx::query("DELETE FROM matches WHERE fingerprint = $1")
        .bind(fingerprint)
        .execute(&mut *tx)
        .await?;

    // Correction héros (cf. talent_hero) : on résout le héros joué via dim_talents pour tous les
    // talents du match en une requête, AVANT d'écrire `matches.data` (qui porte `takedowns`).
    // Best-effort — référentiel vide → LUT vide → on garde le héros du parser.
    let talent_ids: Vec<String> = players
        .values()
        .filter_map(|p| p.get("talents").and_then(J::as_object))
        .flat_map(|t| t.values().filter_map(J::as_str).map(str::to_owned))
        .collect();
    let talent_lut: HashMap<String, String> = if talent_ids.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, (String, String)>(
            "SELECT tree_id, hero_id FROM dim_talents WHERE tree_id = ANY($1)",
        )
        .bind(&talent_ids)
        .fetch_all(&mut *tx)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect()
    };
    let corrections = hero_corrections(players, &talent_lut);
    let mut match_obj = m.clone();
    let mut players = players.clone();
    apply_hero_corrections(&mut match_obj, &mut players, &corrections);
    let match_json = J::Object(match_obj);

    let match_id: i64 = sqlx::query_scalar(
        "INSERT INTO matches (fingerprint, build, mode, map, duration_loops, length, played_at,
            winner, first_pick_win, first_objective, first_fort, first_keep, parser_version, data)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING id",
    )
    .bind(fingerprint)
    .bind(build)
    .bind(mode)
    .bind(map)
    .bind(duration_loops)
    .bind(length)
    .bind(played_at)
    .bind(winner)
    .bind(first_pick_win)
    .bind(first_objective)
    .bind(first_fort)
    .bind(first_keep)
    .bind(parser_version)
    .bind(&match_json)
    .fetch_one(&mut *tx)
    .await?;

    for (toon, p) in &players {
        sqlx::query(
            "INSERT INTO match_players (match_id, toon_handle, name, hero, team, win, hero_level,
                kills, takedowns, deaths, hero_damage, healing, experience, data)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(match_id)
        .bind(toon)
        .bind(p.get("name").and_then(J::as_str))
        .bind(p.get("hero").and_then(J::as_str))
        .bind(p.get("team").and_then(J::as_i64).map(|v| v as i32))
        .bind(p.get("win").and_then(J::as_bool))
        .bind(p.get("heroLevel").and_then(J::as_i64).map(|v| v as i32))
        .bind(gs_i(p, "SoloKill").map(|v| v as i32))
        .bind(gs_i(p, "Takedowns").map(|v| v as i32))
        .bind(gs_i(p, "Deaths").map(|v| v as i32))
        .bind(gs_i(p, "HeroDamage"))
        .bind(gs_i(p, "Healing"))
        .bind(gs_i(p, "ExperienceContribution"))
        .bind(p)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // Référentiel joueurs (agrégat dénormalisé, dérivable de match_players) : UPSERT HORS de la
    // transaction du match, en statements autonomes. Toutes les parties partagent des joueurs
    // (le propriétaire de l'archive est dans toutes) ; garder ces UPSERT dans la transaction
    // sérialiserait tout le backfill sur cette ligne. En autocommit, le verrou n'est tenu qu'un
    // instant. Best-effort : un échec ici ne défait pas le match déjà projeté.
    let mut roster: Vec<(&String, &str)> = players
        .iter()
        .filter_map(|(toon, p)| p.get("name").and_then(J::as_str).map(|n| (toon, n)))
        .collect();
    roster.sort_unstable_by(|a, b| a.0.cmp(b.0));
    for (toon, name) in roster {
        let _ = sqlx::query(
            "INSERT INTO players (toon_handle, last_name, names, updated_at)
             VALUES ($1,$2, jsonb_build_array($2::text), now())
             ON CONFLICT (toon_handle) DO UPDATE SET
                last_name = EXCLUDED.last_name,
                names = CASE WHEN players.names ? EXCLUDED.last_name THEN players.names
                             ELSE players.names || jsonb_build_array(EXCLUDED.last_name) END,
                updated_at = now()",
        )
        .bind(toon)
        .bind(name)
        .execute(db)
        .await;
    }

    Ok(match_id)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use std::path::PathBuf;

    #[test]
    fn talent_hero_vote_majoritaire() {
        let lut: HashMap<String, String> = [
            ("CrusaderPassiveDivineFortress", "Johanna"),
            ("CrusaderBlessedHammer", "Johanna"),
            ("GenericMountTalent", "Autre"), // talent ambigu (partagé) → minoritaire
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        // 2 talents Johanna + 1 ambigu → Johanna l'emporte (immunisé au shuffle ARAM).
        let p = serde_json::json!({"talents": {
            "Tier1Choice": "CrusaderPassiveDivineFortress",
            "Tier2Choice": "GenericMountTalent",
            "Tier3Choice": "CrusaderBlessedHammer",
        }});
        assert_eq!(talent_hero(&p, &lut).as_deref(), Some("Johanna"));

        // aucun talent résoluble → None (l'appelant garde le héros du parser).
        let p2 = serde_json::json!({"talents": {"Tier1Choice": "Inconnu"}});
        assert_eq!(talent_hero(&p2, &lut), None);
        // pas de talents du tout → None.
        assert_eq!(talent_hero(&serde_json::json!({}), &lut), None);
    }

    /// Régression (ARAM Braxis Outpost, build 98348) : `players[].hero` était corrigé mais les
    /// copies du héros dans les morts gardaient le héros parser (Kel'Thuzad affiché « Kharazim »).
    #[test]
    fn corrections_propagees_aux_morts_et_takedowns() {
        use serde_json::json;
        // A et B ont le MÊME héros parser (Stukov) mais jouent Arthas / Rehgar ; C est correct.
        let td_a_dies = json!({"victim": {"player": "A", "hero": "Stukov"},
            "killers": [{"player": "C", "hero": "Zagara"}, {"player": "B", "hero": "Stukov"}]});
        let td_c_dies = json!({"victim": {"player": "C", "hero": "Zagara"},
            "killers": [{"player": "A", "hero": "Stukov"}]});
        let td_creep = json!({"victim": {"player": "B", "hero": "Stukov"},
            "killers": [{"player": "0", "hero": "Nexus Forces"}]});
        let mut players: Map<String, J> = serde_json::from_value(json!({
            "A": {"hero": "Stukov", "talents": {"T1": "ArthasT"},
                  "deaths": [td_a_dies], "takedowns": [td_c_dies]},
            "B": {"hero": "Stukov", "talents": {"T1": "RehgarT"},
                  "deaths": [td_creep], "takedowns": [td_a_dies]},
            "C": {"hero": "Zagara", "talents": {"T1": "ZagaraT"},
                  "deaths": [td_c_dies], "takedowns": [td_a_dies]},
        }))
        .unwrap();
        let mut match_: Map<String, J> =
            serde_json::from_value(json!({"takedowns": [td_a_dies, td_c_dies, td_creep]})).unwrap();

        // référentiel vide → aucune correction → entrée strictement inchangée
        let empty = hero_corrections(&players, &HashMap::new());
        assert!(empty.is_empty());
        let (p0, m0) = (players.clone(), match_.clone());
        apply_hero_corrections(&mut match_, &mut players, &empty);
        assert_eq!((&players, &match_), (&p0, &m0));

        let lut: HashMap<String, String> = [
            ("ArthasT", "Arthas"),
            ("RehgarT", "Rehgar"),
            ("ZagaraT", "Zagara"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let fix = hero_corrections(&players, &lut);
        assert_eq!(fix.len(), 2, "C est déjà correct : {fix:?}");
        apply_hero_corrections(&mut match_, &mut players, &fix);

        assert_eq!(players["A"]["hero"], "Arthas");
        assert_eq!(players["A"]["heroCorrectedFrom"], "Stukov");
        assert!(players["C"].get("heroCorrectedFrom").is_none());
        // victime corrigée, killers corrigés par handle (A ≠ B malgré le même héros parser)
        assert_eq!(players["A"]["deaths"][0]["victim"]["hero"], "Arthas");
        assert_eq!(players["A"]["deaths"][0]["killers"][1]["hero"], "Rehgar");
        // joueur non corrigé (C) : ses copies voient quand même le killer corrigé
        assert_eq!(players["C"]["deaths"][0]["victim"]["hero"], "Zagara");
        assert_eq!(players["C"]["deaths"][0]["killers"][0]["hero"], "Arthas");
        assert_eq!(players["C"]["takedowns"][0]["victim"]["hero"], "Arthas");
        // « Nexus Forces » intact
        assert_eq!(
            players["B"]["deaths"][0]["killers"][0]["hero"],
            "Nexus Forces"
        );
        assert_eq!(players["B"]["deaths"][0]["victim"]["hero"], "Rehgar");
        // match.takedowns aussi
        assert_eq!(match_["takedowns"][0]["victim"]["hero"], "Arthas");
        assert_eq!(match_["takedowns"][1]["killers"][0]["hero"], "Arthas");
        assert_eq!(match_["takedowns"][2]["victim"]["hero"], "Rehgar");
    }

    /// Test d'intégration : projette un replay et vérifie l'idempotence. Ignoré si DATABASE_URL
    /// absent (CI sans DB) — lancer avec le Postgres Docker du dev.
    #[tokio::test]
    async fn projette_et_reste_idempotent() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL absent → test de projection ignoré");
            return;
        };
        let db = PgPoolOptions::new().connect(&url).await.expect("connexion");
        sqlx::migrate!("./migrations")
            .run(&db)
            .await
            .expect("migrations");

        // un replay du mini-corpus de storm-replay (committé)
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../storm-replay/tests/data");
        let replay = std::fs::read_dir(&dir)
            .expect("tests/data")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|x| x == "StormReplay"))
            .expect("au moins un replay");

        let out = storm_stats::process_replay(&replay, "test.StormReplay");
        assert_eq!(out.status, 1, "replay de test non parsé");

        let fp = "test-projection-fingerprint";
        sqlx::query("DELETE FROM matches WHERE fingerprint = $1")
            .bind(fp)
            .execute(&db)
            .await
            .unwrap();

        let id1 = project_match(&db, fp, 1, &out).await.expect("projection 1");
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM match_players WHERE match_id = $1")
            .bind(id1)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(n, 10, "attendu 10 joueurs");

        // re-projection (re-process) : remplace, pas de doublon
        let id2 = project_match(&db, fp, 1, &out).await.expect("projection 2");
        let matches: i64 =
            sqlx::query_scalar("SELECT count(*) FROM matches WHERE fingerprint = $1")
                .bind(fp)
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(matches, 1, "re-process a créé un doublon");
        assert_ne!(id1, id2, "l'id devrait changer après delete+insert");
        let n2: i64 = sqlx::query_scalar("SELECT count(*) FROM match_players WHERE match_id = $1")
            .bind(id2)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(n2, 10);

        sqlx::query("DELETE FROM matches WHERE fingerprint = $1")
            .bind(fp)
            .execute(&db)
            .await
            .unwrap();
    }
}
