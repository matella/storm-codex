//! Enrichment of the lobby against the archive. The blob only carries `name#discriminator`: the
//! `toon_handle` is found in `match_players` (name + `data->>'tag'`), that is in the
//! games already played. A player never met stays unresolved (`toon_handle: None`) — without
//! consequence, since they have no history to show anyway (`history: None`, never
//! an empty history disguised as a real zero).
//!
//! The p95 < 100 ms contract of `/api/lobby` (spec) forbids one SQL round trip per player: the
//! two steps that depend on the number of players (identity resolution, history) are therefore
//! each ONE batched query over the whole lobby rather than a loop of ten.
use std::collections::HashMap;

use serde_json::Value as J;
use sqlx::PgPool;

use crate::lobby::LobbyState;

/// `name#tag` → `toon_handle` for all the players of the lobby in one query (LATERAL join on an
/// `UNNEST` of the two columns): a single round trip whatever the number of players. The order
/// of the result follows that of the input arrays (`WITH ORDINALITY`).
async fn resoudre_tout(
    db: &PgPool,
    names: &[String],
    discriminants: &[String],
) -> Vec<Option<String>> {
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
            tracing::warn!("lobby identity resolution: unexpected number of rows");
            vec![None; attendu]
        }
        Err(e) => {
            tracing::warn!("lobby identity resolution failed: {e}");
            vec![None; attendu]
        }
    }
}

/// History of several `toon_handle`s at once — games together, games against, win rates,
/// favourite heroes, from the operator's point of view. Each requested `toon_handle` gets an entry,
/// even at zero: a known player never crossed with the operator has a real zero, which
/// differs from an unknown player (never in the map, so `history: None` on the caller's side).
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
            -- At most one row per match. `operator_names` is a filter, not a key: if two
            -- `match_players` rows of the same match satisfy it (two accounts of the operator
            -- in the same game, or a namesake in the archive — the discriminator is not part
            -- of the criterion), a join over all the rows would count each
            -- opponent twice, and if those two rows are on opposite sides, the same
            -- game would be counted both together and against. `DISTINCT ON` freezes an
            -- arbitrary but unique choice per match — enough: the question asked is: did I
            -- cross this player in this match, on which side; not: which of my rows.
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
            tracing::warn!("lobby history failed: {e}");
            Vec::new()
        }
    };
    rows.into_iter().collect()
}

/// Operator stats on this hero, this map, and the pair. `hero` can be `None` as long as
/// the operator has not entered it: the map stats then stay computable.
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
            tracing::warn!("lobby operator stats failed: {e}");
            None
        }
    }
}

/// Default build of the hero + alternatives. `None` if no build is stored: the front then offers
/// the import from the best game on this hero.
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
            tracing::warn!("lobby hero build failed: {e}");
            None
        }
    }
}

/// Fills the state with everything the archive knows. Best-effort: a failing query leaves the
/// field at `None` (with a `warn` trace) rather than failing the ingestion — a half
/// enriched lobby is better than an empty page.
pub async fn enrich(db: &PgPool, state: &mut LobbyState) {
    let noms_operateur: Vec<String> = sqlx::query_scalar(
        "SELECT lower(jsonb_array_elements_text(value)) FROM app_settings WHERE key = 'operator_names'",
    )
    .fetch_all(db)
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("reading operator_names failed: {e}");
        Vec::new()
    });

    // Identity resolution: one query for the whole lobby (not one per player).
    let names: Vec<String> = state.players.iter().map(|p| p.name.clone()).collect();
    let discriminants: Vec<String> = state
        .players
        .iter()
        .map(|p| p.discriminant.clone())
        .collect();
    let toons = resoudre_tout(db, &names, &discriminants).await;
    for (p, toon) in state.players.iter_mut().zip(toons) {
        p.toon_handle = toon;
    }

    // The operator: the first player of the lobby whose name is in `operator_names` (the plan
    // said "the last one"; the first one is kept, without consequence: a legitimate lobby never
    // contains two accounts of the operator, so `position()` finds at most a single
    // match anyway).
    state.me = state
        .players
        .iter()
        .position(|p| noms_operateur.iter().any(|n| n == &p.name.to_lowercase()));

    // History: one query for all the resolved toon_handles (not one per player),
    // excluding the operator themself — otherwise the "together" formula counts their own
    // games as played with themself. Their tile does not need it anyway: their stats
    // live in `state.me_stats`. `state.me` is already determined above.
    let toons_connus: Vec<String> = state
        .players
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != state.me)
        .filter_map(|(_, p)| p.toon_handle.clone())
        .collect();
    // `operator_names` not configured: `state.me` is always `None` above, so "with/against
    // me" makes no sense. Does not call `historiques` in that case — a `games_with: 0` returned
    // for everyone would be a false zero indistinguishable from a real zero, exactly what
    // the header of this module promises to avoid.
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
