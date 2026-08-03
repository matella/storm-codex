# API — référence REST + WebSocket

Routes définies dans `crates/storm-codex-server/src/main.rs` (source de vérité). Auth : seules
les routes marquées 🔒 exigent `Authorization: Bearer` (token d'upload nominatif pour l'upload ;
`ADMIN_TOKEN` pour l'admin — **si défini**, sinon mode ouvert).

## Upload

| Route | Rôle |
|---|---|
| `POST /api/upload` 🔒 | corps = octets bruts du `.StormReplay`, header `X-Filename` (percent-encodé accepté). Réponse ≤ 2 s : `parsed` / `409 duplicate` / `parse_failed` ; `202 accepted` si pool saturé (résultat par WS) |
| `POST /api/upload-raw` 🔒 | **alias strict** du précédent — compat client-rs (Hots-Overlay) |

## Lecture (`read.rs`)

| Route | Rôle |
|---|---|
| `GET /api/health` | `{status, parser_version, db}` — 200 ou 503 |
| `GET /api/matches` | liste paginée ; filtres `map`, `mode`, `hero`, `player`, `limit`/`offset` |
| `GET /api/matches.csv` | export CSV (mêmes filtres ; l'« export JSON » de l'UI = `/api/matches` avec `limit` élevé) |
| `GET /api/matches/{id}` | détail complet `{id, fingerprint, parser_version, match, players}` — `match` = objet storm-stats intégral (timeline, objectifs, `messages`…) |
| `GET /api/matches/{id}/raw?stream=…` | dump décodé à la volée (7 streams heroprotocol) + cache LRU |
| `GET /api/players/{toon}` | résumé joueur + hero pool |
| `GET /api/heroes` · `GET /api/hero/{hero}` · `GET /api/hero/{hero}/patches` | agrégats héros ; croisement patch notes |
| `GET /api/hero-changes` · `GET /api/hero-changes/heroes` | sections héros des patch notes (buff/nerf) |
| `GET /api/synergies` | paires de héros (winrates ensemble/contre) |
| `GET /api/patches` · `GET /api/patches/{id}` | liste `dim_patches` ; détail (contenu) |
| `GET /api/maps` | agrégat par carte |
| `GET /api/dim/heroes` · `GET /api/dim/talents` | référentiels répliqués |
| `GET /api/trends` | winrate/durée par build/patch |
| `GET /api/now-playing` | proxy Orpheus (widget musique) |
| `GET /api/settings` | réglages applicatifs (dont `operator_names`) |

## Gestion (`manage.rs`, `admin.rs`) — 🔒 si `ADMIN_TOKEN` défini

| Route | Rôle |
|---|---|
| `PUT /api/admin/settings` | écrit `app_settings` (ex. `operator_names`) |
| `GET/POST /api/teams` · `PUT/DELETE /api/teams/{id}` | équipes (+ champ `league`) |
| `GET/POST /api/collections` · `DELETE /api/collections/{id}` | collections de matchs |
| `POST /api/admin/tokens` · `DELETE /api/admin/tokens/{id}` | tokens d'upload nominatifs (le clair n'est montré qu'à la création) |
| `GET /api/admin/uploads` | santé des uploads (statuts, classes d'échec) |
| `POST /api/admin/reprocess` | re-parse idempotent (piloté par `parser_version`) |

## Simulateur de draft (`draft/api.rs`)

`GET /api/draft` (état) · `POST /api/draft/config` · `/action` (pick/ban) · `/undo` · `/reset` ·
`/unavailable` (fearless) · `/score` · `/teams` (noms) · `/series/next` · `/series/new`.
Chaque mutation → WS `draft.updated` ; l'overlay et la console se re-fetchent.

## Statique

`/` = SPA (fallback `index.html` en `no-cache` — un redeploy change le hash du bundle) ;
`/images` = portraits héros + fonds de carte vendorisés ; `/assets` = bundle Vite fingerprinté.

## WebSocket `/ws`

Diffusion broadcast à tous les clients ; voir [07-evenements.md](07-evenements.md) pour les
types. Pas de messages entrants utiles (ping/close seulement). Un client en retard est
« laggé » sans déconnexion (events non critiques, re-fetch par TanStack Query).
