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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn fake_output() -> storm_stats::Output {
        let match_: Map<String, J> = serde_json::from_value(json!({
            "map": "Sky Temple", "mode": 50101, "length": 900.5, "winner": 1,
        }))
        .unwrap();
        let players: Map<String, J> = serde_json::from_value(json!({
            "1-Hero-1-111": { "hero": "Jaina", "name": "Alice", "team": 0, "win": false,
                "gameStats": {"SoloKill": 3.0, "Deaths": 2.0, "Takedowns": 7.0,
                               "HeroDamage": 40000.0, "Healing": 0.0} },
            "1-Hero-1-222": { "hero": "Yrel", "name": "Bob", "team": 1, "win": true,
                "gameStats": {"SoloKill": 1.0, "Deaths": 0.0, "Takedowns": 9.0,
                               "HeroDamage": 15000.0, "Healing": 30000.0} },
        }))
        .unwrap();
        storm_stats::Output { status: 1, match_: Some(match_), players: Some(players) }
    }

    /// Invariants spine (règle dure n° 6) : schema_version, type `entity.verb` au passé,
    /// correlation/causation, occurred_at/recorded_at RFC3339.
    #[test]
    fn evenement_respecte_les_invariants_spine() {
        let ev = match_completed_event(42, &fake_output());
        assert_eq!(ev["schema_version"], 1);
        assert_eq!(ev["type"], "hots.match.completed");
        for id in ["correlation_id", "causation_id"] {
            uuid::Uuid::parse_str(ev[id].as_str().expect(id)).expect("uuid valide");
        }
        for ts in ["occurred_at", "recorded_at"] {
            chrono::DateTime::parse_from_rfc3339(ev[ts].as_str().expect(ts)).expect("rfc3339");
        }
    }

    #[test]
    fn data_resume_le_match_et_les_joueurs() {
        let ev = match_completed_event(42, &fake_output());
        let d = &ev["data"];
        assert_eq!(d["match_id"], 42);
        assert_eq!(d["map"], "Sky Temple");
        assert_eq!(d["winner"], 1);
        let players = d["players"].as_array().expect("players");
        assert_eq!(players.len(), 2);
        let bob = players.iter().find(|p| p["name"] == "Bob").expect("Bob");
        assert_eq!(bob["win"], true);
        assert_eq!(bob["kda"]["takedowns"], 9.0);
        assert_eq!(bob["healing"], 30000.0);
    }
}
