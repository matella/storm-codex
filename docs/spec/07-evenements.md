# Événements — WebSocket `/ws` et contrat Jarvis

## WebSocket `/ws` (broadcast interne — site, overlays OBS)

Canal `AppState.events` (tokio broadcast, capacité 1024). Types émis aujourd'hui :

| `type` | Émis quand | Champs utiles | Consommateurs |
|---|---|---|---|
| `match.parsed` | projection d'un match réussie | `match_id`, `map`… | SPA (invalide les queries TanStack), widget OBS |
| `draft.updated` | toute mutation du simulateur de draft | (re-fetch `/api/draft`) | console `/draft`, overlay `/draft/overlay` |
| `patch.new` | nouveau patch détecté par le job référentiel | `internalId`, `name` | notif in-app (WhatsNew) |

Contrat côté client : les events sont des **signaux de re-fetch**, pas des données complètes —
un client laggé (RecvError::Lagged) continue sans perte fonctionnelle.
Ajouter un type d'event = l'ajouter à ce tableau.

## Jarvis (Redis pub/sub) — `jarvis.rs`

Opt-in (`REDIS_URL` vide = no-op ; best-effort : une panne Redis ne casse jamais le parse).
Canal prod box : `storm-codex:match_completed` (`JARVIS_CHANNEL`).

Événement `hots.match.completed`, invariants **spine** respectés :

```json
{
  "schema_version": 1,
  "type": "hots.match.completed",
  "correlation_id": "<uuid>", "causation_id": "<uuid>",
  "occurred_at": "<rfc3339>", "recorded_at": "<rfc3339>",
  "data": {
    "match_id": 123, "map": "…", "mode": 50101, "length": 536.2, "winner": 1,
    "players": [ { "hero", "name", "team", "win",
                   "kda": {"kills","deaths","takedowns"}, "heroDamage", "healing" } ]
  }
}
```

⚠️ **Boundary Jarvis** : le modèle Event du spine n'accepte qu'**un point** dans le type — le
bridge côté Jarvis (`jarvis/ingest/hots_matches.py`, repo Jarvis) adapte
`hots.match.completed` → `hots.match_completed` à l'ingestion. Ne pas « corriger » l'émetteur :
le format à deux points est l'invariant spine côté storm-codex ; l'adaptation appartient au
consommateur.

Chaîne aval (repo Jarvis) : bridge → table `events` du spine → worker brief FR
(`jarvis/notify/hots_brief.py`, perspective opérateur via `HOTS_PLAYER_NAME`) → ntfy.

## Webhook « nouveau patch » (sortant, optionnel)

`PATCH_WEBHOOK_URL` : POST JSON `{content, patchName, internalId}` (format Discord-compatible)
à chaque patch détecté. Best-effort.

## Azure / extension Twitch — DORMANT

`azure.rs` existe mais n'est pas câblé (décision opérateur 2026-06-13 : overlay local
uniquement). Ne pas raccorder sans décision inverse.
