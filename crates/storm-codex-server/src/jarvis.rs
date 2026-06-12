//! Émission d'événements vers Jarvis (Redis). Respecte les invariants du spine :
//! `schema_version`, `correlation_id`/`causation_id`, `occurred_at`/`recorded_at`,
//! type `entity.verb` au passé (`hots.match.completed`). Absent `REDIS_URL` → no-op.

use chrono::Utc;
use serde_json::{json, Value as J};

/// Construit l'événement `hots.match.completed` (invariants spine) depuis un match projeté.
pub fn match_completed_event(match_id: i64, out: &storm_stats::Output) -> J {
    let now = Utc::now().to_rfc3339();
    let m = out.match_.as_ref();
    let map = m.and_then(|m| m.get("map")).cloned().unwrap_or(J::Null);
    let mode = m.and_then(|m| m.get("mode")).cloned().unwrap_or(J::Null);
    let length = m.and_then(|m| m.get("length")).cloned().unwrap_or(J::Null);
    let winner = m.and_then(|m| m.get("winner")).cloned().unwrap_or(J::Null);

    // joueurs résumés (héros, équipe, victoire, KDA) — Jarvis extrait la perspective voulue
    let players: Vec<J> = out
        .players
        .as_ref()
        .map(|ps| {
            ps.values()
                .map(|p| {
                    let g = p.get("gameStats");
                    let gi = |k: &str| g.and_then(|g| g.get(k)).and_then(J::as_f64).unwrap_or(0.0);
                    json!({
                        "hero": p.get("hero").cloned().unwrap_or(J::Null),
                        "name": p.get("name").cloned().unwrap_or(J::Null),
                        "team": p.get("team").cloned().unwrap_or(J::Null),
                        "win": p.get("win").cloned().unwrap_or(J::Null),
                        "kda": { "kills": gi("SoloKill"), "deaths": gi("Deaths"), "takedowns": gi("Takedowns") },
                        "heroDamage": gi("HeroDamage"),
                        "healing": gi("Healing"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    json!({
        "schema_version": 1,
        "type": "hots.match.completed",
        "correlation_id": uuid::Uuid::new_v4().to_string(),
        "causation_id": uuid::Uuid::new_v4().to_string(),
        "occurred_at": now,   // fin de partie (≈ instant du parse, source unique)
        "recorded_at": now,
        "data": {
            "match_id": match_id,
            "map": map,
            "mode": mode,
            "length": length,
            "winner": winner,
            "players": players,
        }
    })
}

/// Publie l'événement sur le canal Redis (`JARVIS_CHANNEL`, défaut `jarvis:events`).
/// Best-effort : une panne Redis ne casse jamais le parse.
pub async fn publish(redis_url: &str, channel: &str, event: &J) {
    match try_publish(redis_url, channel, event).await {
        Ok(_) => {}
        Err(e) => tracing::warn!("publication Jarvis échouée : {e}"),
    }
}

async fn try_publish(redis_url: &str, channel: &str, event: &J) -> redis::RedisResult<()> {
    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    let payload = event.to_string();
    redis::cmd("PUBLISH").arg(channel).arg(payload).query_async::<()>(&mut conn).await?;
    Ok(())
}
