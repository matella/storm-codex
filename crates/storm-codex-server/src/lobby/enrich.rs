//! Enrichissement du lobby contre l'archive. Le blob ne porte que `nom#discriminant` : le
//! `toon_handle` se retrouve dans `match_players` (nom + `data->>'tag'`), c'est-à-dire dans les
//! parties déjà jouées. Un joueur jamais croisé reste non résolu (`toon_handle: None`) — sans
//! conséquence, puisqu'il n'a de toute façon aucun historique à afficher (`history: None`, jamais
//! un historique vide déguisé en vrai zéro).
//!
//! Le contrat p95 < 100 ms de `/api/lobby` (spec) interdit un aller-retour SQL par joueur : les
//! deux étapes qui dépendent du nombre de joueurs (résolution d'identité, historique) sont donc
//! chacune UNE requête batchée sur tout le lobby plutôt qu'une boucle de dix.
use std::collections::HashMap;

use serde_json::Value as J;
use sqlx::PgPool;

use crate::lobby::LobbyState;

/// `nom#tag` → `toon_handle` pour tous les joueurs du lobby en une requête (LATERAL join sur un
/// `UNNEST` des deux colonnes) : un seul aller-retour quel que soit le nombre de joueurs. L'ordre
/// du résultat suit celui des tableaux d'entrée (`WITH ORDINALITY`).
async fn resoudre_tout(db: &PgPool, names: &[String], discriminants: &[String]) -> Vec<Option<String>> {
    if names.is_empty() {
        return Vec::new();
    }
    let attendu = names.len();
    match sqlx::query_scalar::<_, Option<String>>(
        "SELECT mp.toon_handle
         FROM UNNEST($1::text[], $2::text[]) WITH ORDINALITY AS input(name, discriminant, idx)
         LEFT JOIN LATERAL (
             SELECT toon_handle FROM match_players
             WHERE lower(name) = lower(input.name) AND data ->> 'tag' = input.discriminant
             ORDER BY match_id DESC LIMIT 1
         ) mp ON true
         ORDER BY input.idx",
    )
    .bind(names)
    .bind(discriminants)
    .fetch_all(db)
    .await
    {
        Ok(rows) if rows.len() == attendu => rows,
        Ok(_) => {
            tracing::warn!("résolution d'identité du lobby : nombre de lignes inattendu");
            vec![None; attendu]
        }
        Err(e) => {
            tracing::warn!("résolution d'identité du lobby échouée : {e}");
            vec![None; attendu]
        }
    }
}

/// Historique de plusieurs `toon_handle` à la fois — parties ensemble, parties contre, winrates,
/// héros favoris, du point de vue de l'opérateur. Chaque `toon_handle` demandé reçoit une entrée,
/// même à zéro : un joueur connu mais jamais recoupé avec l'opérateur a un vrai zéro, ce qui
/// diffère d'un joueur inconnu (jamais dans la map, donc `history: None` côté appelant).
async fn historiques(db: &PgPool, toons: &[String]) -> HashMap<String, J> {
    if toons.is_empty() {
        return HashMap::new();
    }
    let rows: Vec<(String, J)> = match sqlx::query_as(
        "WITH moi AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         ),
         mes_parties AS (
            -- Au plus une ligne par match. `operator_names` est un filtre, pas une clé : si deux
            -- lignes de `match_players` d'un même match le satisfont (deux comptes de l'opérateur
            -- dans la même partie, ou un homonyme dans l'archive — le discriminant n'entre pas
            -- dans le critère), une jointure sur toutes les lignes ferait compter chaque
            -- adversaire deux fois, et si ces deux lignes sont dans des camps opposés, la même
            -- partie serait comptée à la fois « ensemble » et « contre ». `DISTINCT ON` fige un
            -- choix arbitraire mais unique par match — suffisant : la question posée est « ai-je
            -- croisé ce joueur dans ce match, dans quel camp », pas « laquelle de mes lignes ».
            SELECT DISTINCT ON (mp.match_id) mp.match_id, mp.team
            FROM match_players mp
            WHERE lower(mp.name) IN (SELECT name FROM moi)
            ORDER BY mp.match_id
         ),
         cibles AS (
            SELECT DISTINCT toon_handle FROM UNNEST($1::text[]) AS toon_handle
         ),
         croisements AS (
            SELECT mp.toon_handle, (mp.team = mes.team) AS ensemble, mp.win, m.played_at
            FROM match_players mp
            JOIN mes_parties mes ON mes.match_id = mp.match_id
            JOIN matches m ON m.id = mp.match_id
            WHERE mp.toon_handle = ANY($1)
         ),
         heroes_classes AS (
            SELECT toon_handle, hero, count(*) AS games, count(*) FILTER (WHERE win) AS wins,
                   row_number() OVER (PARTITION BY toon_handle ORDER BY count(*) DESC) AS rang
            FROM match_players
            WHERE toon_handle = ANY($1) AND hero IS NOT NULL
            GROUP BY toon_handle, hero
         ),
         top_heroes AS (
            SELECT toon_handle,
                   jsonb_agg(jsonb_build_object('hero', hero, 'games', games, 'wins', wins)
                             ORDER BY games DESC) AS heroes
            FROM heroes_classes
            WHERE rang <= 3
            GROUP BY toon_handle
         )
         SELECT cibles.toon_handle,
             jsonb_build_object(
                'toon', cibles.toon_handle,
                'games_with', count(*) FILTER (WHERE cr.ensemble),
                'wins_with', count(*) FILTER (WHERE cr.ensemble AND cr.win),
                'games_against', count(*) FILTER (WHERE NOT cr.ensemble),
                'wins_against', count(*) FILTER (WHERE NOT cr.ensemble AND cr.win),
                'last_seen', max(cr.played_at),
                'top_heroes', COALESCE(
                    (SELECT th.heroes FROM top_heroes th WHERE th.toon_handle = cibles.toon_handle),
                    '[]'::jsonb)
             ) AS history
         FROM cibles
         LEFT JOIN croisements cr ON cr.toon_handle = cibles.toon_handle
         GROUP BY cibles.toon_handle",
    )
    .bind(toons)
    .fetch_all(db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("historique du lobby échoué : {e}");
            Vec::new()
        }
    };
    rows.into_iter().collect()
}

/// Stats de l'opérateur sur ce héros, cette carte, et le couple. `hero` peut être `None` tant que
/// l'opérateur ne l'a pas saisi : les stats de carte restent alors calculables.
async fn stats_operateur(db: &PgPool, hero: Option<&str>, map: Option<&str>) -> Option<J> {
    match sqlx::query_scalar(
        "WITH moi AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         ),
         miennes AS (
            SELECT mp.hero, mp.win, m.map
            FROM match_players mp JOIN matches m ON m.id = mp.match_id
            WHERE lower(mp.name) IN (SELECT name FROM moi)
         )
         SELECT jsonb_build_object(
            'hero_games',     (SELECT count(*) FROM miennes WHERE hero = $1),
            'hero_wins',      (SELECT count(*) FROM miennes WHERE hero = $1 AND win),
            'map_games',      (SELECT count(*) FROM miennes WHERE map = $2),
            'map_wins',       (SELECT count(*) FROM miennes WHERE map = $2 AND win),
            'hero_map_games', (SELECT count(*) FROM miennes WHERE hero = $1 AND map = $2),
            'hero_map_wins',  (SELECT count(*) FROM miennes WHERE hero = $1 AND map = $2 AND win))",
    )
    .bind(hero)
    .bind(map)
    .fetch_optional(db)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("stats opérateur du lobby échouées : {e}");
            None
        }
    }
}

/// Build par défaut du héros + alternatives. `None` si aucun build enregistré : le front propose
/// alors l'import depuis la meilleure partie sur ce héros.
async fn build_du_heros(db: &PgPool, hero: &str) -> Option<J> {
    match sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'default', (SELECT to_jsonb(b) FROM (
                SELECT id, name, picks, notes FROM builds
                WHERE hero_id = $1 AND is_default LIMIT 1) b),
            'alternates', COALESCE((SELECT jsonb_agg(a ORDER BY a.name) FROM (
                SELECT id, name, picks, notes FROM builds
                WHERE hero_id = $1 AND NOT is_default) a), '[]'::jsonb))",
    )
    .bind(hero)
    .fetch_optional(db)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("build du héros du lobby échoué : {e}");
            None
        }
    }
}

/// Remplit l'état avec tout ce que l'archive sait. Best-effort : une requête qui échoue laisse le
/// champ à `None` (avec une trace `warn`) plutôt que de faire échouer l'ingestion — un lobby à
/// moitié enrichi vaut mieux qu'une page vide.
pub async fn enrich(db: &PgPool, state: &mut LobbyState) {
    let noms_operateur: Vec<String> = sqlx::query_scalar(
        "SELECT lower(jsonb_array_elements_text(value)) FROM app_settings WHERE key = 'operator_names'",
    )
    .fetch_all(db)
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("lecture operator_names échouée : {e}");
        Vec::new()
    });

    // Résolution d'identité : une requête pour tout le lobby (pas une par joueur).
    let names: Vec<String> = state.players.iter().map(|p| p.name.clone()).collect();
    let discriminants: Vec<String> = state.players.iter().map(|p| p.discriminant.clone()).collect();
    let toons = resoudre_tout(db, &names, &discriminants).await;
    for (p, toon) in state.players.iter_mut().zip(toons) {
        p.toon_handle = toon;
    }

    // L'opérateur : le premier joueur du lobby dont le nom figure dans `operator_names` (le plan
    // disait « le dernier » ; le premier est retenu, sans conséquence : un lobby légitime ne
    // contient jamais deux comptes de l'opérateur, donc `position()` ne trouve au plus qu'une
    // seule correspondance de toute façon).
    state.me = state
        .players
        .iter()
        .position(|p| noms_operateur.iter().any(|n| n == &p.name.to_lowercase()));

    // Historique : une requête pour tous les toon_handle résolus (pas une par joueur), à
    // l'exclusion de l'opérateur lui-même — sinon la formule « ensemble » compte ses propres
    // parties comme jouées avec lui-même. Sa tuile n'en a de toute façon pas besoin : ses stats
    // vivent dans `state.me_stats`. `state.me` est déjà déterminé ci-dessus.
    let toons_connus: Vec<String> = state
        .players
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != state.me)
        .filter_map(|(_, p)| p.toon_handle.clone())
        .collect();
    // `operator_names` non configuré : `state.me` est toujours `None` ci-dessus, donc « avec/contre
    // moi » n'a aucun sens. N'appelle pas `historiques` dans ce cas — un `games_with: 0` renvoyé
    // pour tout le monde serait un faux zéro indiscernable d'un vrai zéro, exactement ce que
    // l'en-tête de ce module promet d'éviter.
    let historiques_par_toon = if noms_operateur.is_empty() {
        HashMap::new()
    } else {
        historiques(db, &toons_connus).await
    };
    for (i, p) in state.players.iter_mut().enumerate() {
        p.history = if Some(i) == state.me {
            None
        } else {
            p.toon_handle
                .as_deref()
                .and_then(|t| historiques_par_toon.get(t).cloned())
        };
    }

    state.me_stats = stats_operateur(db, state.hero.as_deref(), state.map.as_deref()).await;
    state.build = match state.hero.as_deref() {
        Some(h) => build_du_heros(db, h).await,
        None => None,
    };
}
