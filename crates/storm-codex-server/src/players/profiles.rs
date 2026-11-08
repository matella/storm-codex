//! Profils de raccourcis clavier (`m_hotkeyProfile` du replay) : nom choisi à la main par le joueur,
//! donc indice fort de « même personne » quand deux comptes le portent. Spec :
//! docs/specs/2026-11-08-profils-hotkeys-comptes-design.md.
//!
//! Un nom est **ignoré** s'il est livré par le jeu (Standard, langues, `0_default`), trop court, ou
//! listé dans `app_settings.ignored_profile_names`. Il devient **générique** s'il est porté par
//! `GENERIC_MIN_TOONS` comptes distincts ou plus (ex. un nom de héros) : il n'identifie plus personne.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};

/// Comptes distincts à partir desquels un nom de profil est jugé générique.
pub const GENERIC_MIN_TOONS: i64 = 5;
/// Longueur minimale (en caractères) d'un nom exploitable.
const MIN_LEN: usize = 3;

/// Noms livrés avec le jeu : profils par défaut et noms de langues (anglais et forme native).
const BUILTIN_IGNORED: &[&str] = &[
    "standard",
    "standard for lefties",
    "0_default",
    "default",
    "english",
    "français",
    "francais",
    "french",
    "deutsch",
    "german",
    "español",
    "espanol",
    "spanish",
    "español (méxico)",
    "italiano",
    "italian",
    "português",
    "portugues",
    "portuguese",
    "polski",
    "polish",
    "русский",
    "russian",
    "한국어",
    "korean",
    "中文",
    "简体中文",
    "繁體中文",
    "chinese",
    "日本語",
    "japanese",
];

/// Forme comparable d'un nom : espaces de bord retirés, minuscules.
pub fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// `true` si ce nom n'identifie personne (livré par le jeu, trop court, ou ignoré par l'opérateur).
pub fn is_ignored(name: &str, extra: &HashSet<String>) -> bool {
    let k = key(name);
    k.chars().count() < MIN_LEN || BUILTIN_IGNORED.contains(&k.as_str()) || extra.contains(&k)
}

/// Noms ignorés en plus des noms livrés (`app_settings.ignored_profile_names`, clés normalisées).
pub async fn extra_ignored(db: &PgPool) -> Result<HashSet<String>, sqlx::Error> {
    let v: Vec<String> = sqlx::query_scalar(
        "SELECT jsonb_array_elements_text(value) FROM app_settings
         WHERE key = 'ignored_profile_names' AND jsonb_typeof(value) = 'array'",
    )
    .fetch_all(db)
    .await?;
    Ok(v.iter().map(|n| key(n)).collect())
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, sqlx::FromRow)]
pub struct ProfileSpan {
    pub name: String,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub games: i64,
}

/// Profils exploitables d'un compte, récent d'abord. Les noms ignorés sont écartés ; un nom générique
/// reste affiché (c'est un fait du replay) mais ne crée aucun lien (voir `shared_with`).
pub async fn history(db: &PgPool, toon: &str) -> Result<Vec<ProfileSpan>, sqlx::Error> {
    let extra = extra_ignored(db).await?;
    let rows: Vec<ProfileSpan> = sqlx::query_as(
        "SELECT min(mp.hotkey_profile) AS name, min(m.played_at) AS first_seen,
                max(m.played_at) AS last_seen, count(*) AS games
         FROM match_players mp JOIN matches m ON m.id = mp.match_id
         WHERE mp.toon_handle = $1 AND mp.hotkey_profile IS NOT NULL
         GROUP BY lower(btrim(mp.hotkey_profile))
         ORDER BY max(m.played_at) DESC NULLS LAST, 1 ASC",
    )
    .bind(toon)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|r| !is_ignored(&r.name, &extra))
        .collect())
}

/// `autre compte → noms de profil partagés avec toon`, hors noms ignorés et génériques.
pub async fn shared_with(
    db: &PgPool,
    toon: &str,
) -> Result<HashMap<String, Vec<String>>, sqlx::Error> {
    let extra = extra_ignored(db).await?;
    let mine: Vec<String> = history(db, toon)
        .await?
        .into_iter()
        .map(|p| key(&p.name))
        .collect();
    if mine.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(String, Vec<String>)> = sqlx::query_as(
        "SELECT min(hotkey_profile), array_agg(DISTINCT toon_handle)
         FROM match_players
         WHERE hotkey_profile IS NOT NULL AND lower(btrim(hotkey_profile)) = ANY($1)
         GROUP BY lower(btrim(hotkey_profile))
         HAVING count(DISTINCT toon_handle) < $2",
    )
    .bind(&mine)
    .bind(GENERIC_MIN_TOONS)
    .fetch_all(db)
    .await?;
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for (name, toons) in rows {
        if is_ignored(&name, &extra) {
            continue;
        }
        for t in toons.into_iter().filter(|t| t != toon) {
            out.entry(t).or_default().push(name.clone());
        }
    }
    for v in out.values_mut() {
        v.sort();
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::players::testutil::*;

    #[test]
    fn noms_livres_par_le_jeu_et_trop_courts_sont_ignores() {
        let none = HashSet::new();
        for n in [
            "Standard",
            "Standard for Lefties",
            "  ENGLISH ",
            "Русский",
            "0_default",
            "aa",
            "",
        ] {
            assert!(is_ignored(n, &none), "{n:?} doit être ignoré");
        }
        for n in ["Kure1", "munky", "Гиппократ", "TheRequiem"] {
            assert!(!is_ignored(n, &none), "{n:?} doit être gardé");
        }
        assert!(is_ignored("Kure1", &HashSet::from(["kure1".to_owned()])));
    }

    async fn set_profile(db: &PgPool, toon: &str, profile: &str) {
        sqlx::query("UPDATE match_players SET hotkey_profile = $2 WHERE toon_handle = $1")
            .bind(toon)
            .bind(profile)
            .execute(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn nom_rare_partage_lie_les_comptes_mais_pas_un_nom_generique() {
        let Some(db) = test_pool().await else { return };
        let u = uid();
        let (a, b) = (format!("t-pa{u}"), format!("t-pb{u}"));
        let (rare, common) = (format!("Kure{u}"), format!("Common{u}"));
        insert_match(&db, None, &[row(&a, "A", 0, "Muradin")]).await;
        insert_match(&db, None, &[row(&b, "B", 0, "Diablo")]).await;
        set_profile(&db, &a, &rare).await;
        set_profile(&db, &b, &rare.to_uppercase()).await; // casse différente : même nom
        let got = shared_with(&db, &a).await.unwrap();
        assert_eq!(got.get(&b).map(Vec::len), Some(1), "profil partagé détecté");

        // le même nom porté par GENERIC_MIN_TOONS comptes devient générique
        let toons: Vec<String> = (0..GENERIC_MIN_TOONS)
            .map(|i| format!("t-g{i}{u}"))
            .collect();
        for t in &toons {
            insert_match(&db, None, &[row(t, "G", 0, "Muradin")]).await;
            set_profile(&db, t, &common).await;
        }
        let got = shared_with(&db, &toons[0]).await.unwrap();
        assert!(got.is_empty(), "nom générique : aucun lien");
        assert!(history(&db, &a)
            .await
            .unwrap()
            .iter()
            .any(|p| p.name == rare));
    }
}
