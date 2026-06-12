# storm-codex-server

Serveur unique de Storm Codex (jalon 3) : **upload → parse → Postgres → temps réel → REST**.
axum 0.8 + sqlx 0.8 + tokio. Remplace le serveur Node/Mongo local de Hots-Overlay.

## Démarrage (dev)

```bash
docker compose -f docker-compose.dev.yml up -d        # Postgres 17 sur :5433
cp .env.example .env                                  # DATABASE_URL, ARCHIVE_DIR…
cargo run -p storm-codex-server                       # migrations auto, écoute :8088
```

## Architecture (3 étages de données)
1. **Archive brute** : chaque `.StormReplay` conservé tel quel (`ARCHIVE_DIR`) — source de vérité.
2. **Postgres** : projection complète (`matches`, `match_players`, JSONB + colonnes promues),
   `parser_version` partout, re-process idempotent.
3. **Dump décodé à la demande** : `…/raw` + cache disque **LRU borné** (`RAW_CACHE_MAX_BYTES`).

## Endpoints
- `POST /api/upload` — Bearer token, archive d'abord, parse en pool (= nb cœurs, hors thread HTTP),
  projection. Réponse : `parsed` / `409 duplicate` / `parse_failed` (≤ 2 s), `202 accepted` si saturé.
- `GET /api/matches` (filtres carte/mode/héros/joueur, paginé) · `GET /api/matches/{id}` (détail
  `{match, players}`) · `GET /api/players/{toon}` · `GET /api/heroes`.
- `GET /api/matches/{id}/raw?stream=…` — dump décodé à la volée + cache LRU.
- `GET /ws` — WebSocket, push `match.parsed` (fin de partie → page à jour < 5 s).
- `POST /api/admin/tokens` · `DELETE /api/admin/tokens/{id}` · `GET /api/admin/uploads` (santé) ·
  `POST /api/admin/reprocess` (re-parse idempotent, piloté par `parser_version`) — Bearer `ADMIN_TOKEN`.

## Fingerprints (dédup à deux niveaux)
- `uploads.fingerprint` = SHA-256 du **contenu** (dédup fichier rapide, pré-parse).
- `matches.fingerprint` = SHA-256 de `date|map|length|toonsTriés` (dédup **partie**, compat overlay) ;
  `project_match` est idempotent (delete-then-insert), avec reprise sur deadlock.

## Déploiement
Dev = Postgres Docker local (PC de jeu). Cible : le box (192.168.129.85) via rsync + docker
compose build. Config 100 % par env (serveur stateless, V2-ready).

## Licence
MIT. Non affilié à Blizzard.
