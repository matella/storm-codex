# storm-codex-server

Single Storm Codex server (milestone 3): **upload → parse → Postgres → real time → REST**.
axum 0.8 + sqlx 0.8 + tokio. Replaces Hots-Overlay's local Node/Mongo server.

## Getting started (dev)

```bash
docker compose -f docker-compose.dev.yml up -d        # Postgres 17 on :5433
cp .env.example .env                                  # DATABASE_URL, ARCHIVE_DIR…
cargo run -p storm-codex-server                       # automatic migrations, listens on :8088
```

## Architecture (3 data tiers)
1. **Raw archive**: each `.StormReplay` kept as is (`ARCHIVE_DIR`) — source of truth.
2. **Postgres**: complete projection (`matches`, `match_players`, JSONB + promoted columns),
   `parser_version` partout, re-process idempotent.
3. **Dump decoded on demand**: `…/raw` + **bounded LRU** disk cache (`RAW_CACHE_MAX_BYTES`).

## Endpoints
- `POST /api/upload` — Bearer token, archive first, parse in a pool (= number of cores, off the HTTP thread),
  projection. Response: `parsed` / `409 duplicate` / `parse_failed` (≤ 2 s), `202 accepted` if saturated.
- `GET /api/matches` (map/mode/hero/player filters, paginated) · `GET /api/matches/{id}` (detail
  `{match, players}`) · `GET /api/players/{toon}` · `GET /api/heroes`.
- `GET /api/matches/{id}/raw?stream=…` — dump decoded on the fly + LRU cache.
- `GET /ws` — WebSocket, push `match.parsed` (end of game → page up to date < 5 s).
- `POST /api/admin/tokens` · `DELETE /api/admin/tokens/{id}` · `GET /api/admin/uploads` (health) ·
  `POST /api/admin/reprocess` (idempotent re-parse, driven by `parser_version`) — Bearer `ADMIN_TOKEN`.

## Fingerprints (two-level dedup)
- `uploads.fingerprint` = SHA-256 of the **content** (fast file dedup, pre-parse).
- `matches.fingerprint` = SHA-256 of `date|map|length|sortedToons` (**game** dedup, overlay compat);
  `project_match` is idempotent (delete-then-insert), with retry on deadlock.

## Deployment
Dev = local Docker Postgres (gaming PC). Target: the box (192.168.129.85) via rsync + docker
compose build. Config 100% through env (stateless server, V2-ready).

## Licence
MIT. Not affiliated with Blizzard.
