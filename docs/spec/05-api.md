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
| `GET /api/dim/hero-attributes` | code attribut héros 4 lettres → nom canonique du parser (`heroAttribute` de `attr.json`, storm-stats) ; statique, sans DB. Résout les bans du draft, stockés bruts (`match.bans[t][i].hero` = `Crus`, `DEAT`…) — `dim_heroes` ne porte pas l'`attributeId` |
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
| `POST /api/admin/reprocess` | re-parse idempotent (piloté par `parser_version`) ; couvre aussi `scouting_games`, puis recalcule les rapports touchés |

## Scouting (`scouting/api.rs`) — écritures 🔒 si `ADMIN_TOKEN` défini

Spec : `docs/specs/2026-09-26-rapports-scouting-design.md`. Tables dédiées : ces routes n'écrivent
jamais dans `uploads`/`matches`/`match_players`. Toute mutation relance le recalcul des faits
(`facts_version + 1`).

| Route | Rôle |
|---|---|
| `GET /api/scouting` | liste (plus récent d'abord) : titre, équipe, dates, nb de parties, bilan, statut `empty`/`ready`/`analyzed`/`stale` |
| `POST /api/scouting` 🔒 | créer `{title, target_name?}` → `201 {id}` |
| `GET /api/scouting/{id}` | rapport complet : `snapshot` = `{facts, detection{candidates, ambiguous}, roster, roster_auto}`, `games` (équipes, côté cible + `target_source` roster/anchor/manual), `analysis` avec preuves résolues contre les faits courants + `analysis_tally`, `status` |
| `PATCH /api/scouting/{id}` 🔒 | `title`, `target_name`, `roster`, `anchors` (les deux derniers → recalcul) ; renvoie le rapport |
| `DELETE /api/scouting/{id}` 🔒 | supprime (parties en cascade, bruts archivés conservés) |
| `POST /api/scouting/{id}/replays` 🔒 | octets bruts + `X-Filename` → `added` (+ `gid`, côté détecté) / `duplicate` / `parse_failed` (+ `error_class`) |
| `PATCH /api/scouting/{id}/replays/{gid}` 🔒 | `{target_team: 0\|1\|null}` — côté manuel, `null` = retour à l'automatique |
| `DELETE /api/scouting/{id}/replays/{gid}` 🔒 | retire une partie |
| `GET /api/scouting/{id}/pack.md` | pack LLM Markdown (prompt, joueurs `pN`, faits avec ids, parties, format de réponse) |
| `GET /api/scouting/{id}/pack.xlsx` | mêmes faits en classeur (README, Facts, Players, Heroes, Maps, Draft, Games) |
| `PUT /api/scouting/{id}/analysis` 🔒 | corps = texte brut de la réponse du LLM ; extrait/valide/écrase → `{warnings, tally}` ; `422` si JSON introuvable/invalide, `format_version` ≠ 2 (plan de draft carte par carte ; la v1 est refusée) ou `report_id` d'un autre rapport (rien n'est écrasé) |
| `DELETE /api/scouting/{id}/analysis` 🔒 | retire l'analyse |

## Simulateur de draft (`draft/api.rs`)

`GET /api/draft` (état) · `POST /api/draft/config` · `/action` (pick/ban) · `/undo` · `/reset` ·
`/unavailable` (fearless) · `/score` · `/teams` (noms) · `/series/next` · `/series/new`.
Chaque mutation → WS `draft.updated` ; l'overlay et la console se re-fetchent.

## Feed live d'une partie en cours (`live/api.rs`)

Spec : `docs/specs/2026-10-24-feed-live-en-partie-design.md`. État **en mémoire**, une partie à la
fois, aucune table.

| Route | Rôle |
|---|---|
| `POST /api/live/chunk` 🔑 | octets ajoutés d'un stream, `Bearer` d'upload. En-têtes : `X-Session` (≤ 128 car. ; **SHA-256 hexadécimal du battlelobby** de la partie — seule façon pour le serveur de rapprocher la session du lobby courant et d'en tirer carte et opérateur ; un id nouveau **remplace** la session), `X-Stream` (`tracker` \| `game`), `X-Offset` (position du morceau dans le stream), `X-Build` (facultatif ; absent = dernier protocole embarqué). → `200 {status: "applied", consumed, last_loop, events}` ; `200 {status: "duplicate"}` (déjà reçu) ; `200 {status: "unreadable"}` (session illisible) ; `409 {status: "gap", expected_offset}` (renvoyer depuis là) ; `413 {status: "too_large"}` (stream > 64 Mo) ; `400` en-tête invalide ; `401` |
| `POST /api/live/end` 🔑 | `{session}` → statut `ended` ; `404` si ce n'est pas la session courante |
| `GET /api/live?scope=self\|allies\|all&me=<playerId>` | vue filtrée (défaut `scope=self`) ; `204` si aucune partie. Champs : `status` (`live`/`ended`/`unreadable` + `reason`), `session`, `map` et opérateur tirés du lobby **seulement si son hash = id de session**, `me` + `me_source` (`explicit`/`lobby`/`single_human`/`null` — indéterminé = aucune aptitude en `self`), `gates_loop`, `tracker_loop`, `game_loop`, `last_chunk_at`, `protocol {build, fallback, assumed_latest}`, `team_levels`, `players[]` (`player_id`, `team`, `hero`, `hero_unit`, `level`, `talents[{tree_id, loop}]`, `deaths`, `takedowns`, `abilities` = `null` si hors niveau, sinon `[{link, cmd, count, recent[loops des 3 dernières minutes]}]`), `feed[]` (200 plus récents : `death`/`talent`/`level`/`camp`/`objective`) |
| `DELETE /api/live` 🔒 | ferme la session (garde `ADMIN_TOKEN`, mode ouvert s'il n'est pas configuré) |

🔑 = jeton d'upload (comme `/api/upload-raw`). Chaque morceau qui ajoute des événements, et chaque
changement de statut → WS `live.updated` (signal seul).

## Statique

`/` = SPA (fallback `index.html` en `no-cache` — un redeploy change le hash du bundle) ;
`/images` = portraits héros + fonds de carte vendorisés ; `/assets` = bundle Vite fingerprinté.

## WebSocket `/ws`

Diffusion broadcast à tous les clients ; voir [07-evenements.md](07-evenements.md) pour les
types. Pas de messages entrants utiles (ping/close seulement). Un client en retard est
« laggé » sans déconnexion (events non critiques, re-fetch par TanStack Query).
