# storm-codex-server — pipeline & configuration

Crate : `crates/storm-codex-server` (axum 0.8 + sqlx 0.8 + tokio). Un seul binaire : API + front
+ overlays. `PARSER_VERSION` (dans `main.rs`) pilote le re-process — **le bumper à chaque
changement de projection**.

## Pipeline d'upload (`upload.rs`)

```
POST /api/upload(-raw)  [Bearer token nominatif]
  → auth (SHA-256 du token vs upload_tokens.token_hash)
  → archive du fichier brut (ARCHIVE_DIR)          ← AVANT toute tentative de parse
  → dédup fichier (uploads.fingerprint = SHA-256 du contenu) → 409 duplicate
  → parse en pool (Semaphore = nb de cœurs, spawn_blocking hors thread HTTP)
  → projection Postgres (project.rs) + lien uploads.match_id
  → WS `match.parsed` + (opt-in) PUBLISH Redis Jarvis + (dormant) push Azure
```

- **Sémantique de réponse** : `parsed` / `409 duplicate` / `parse_failed` si le résultat arrive
  en ≤ 2 s ; sinon `202 accepted` (pool saturé — backfill), le résultat arrive par WS.
- **Échecs classés** (`uploads.error_class` + `reject_class()`) : classes storm-replay (io,
  décodage…) et statuts storm-stats (unsupported_map, stats_failure/-2, unverified_build/-7…) —
  exhaustif, visible dans `GET /api/admin/uploads`.
- **Corps limité à 64 Mo** (`DefaultBodyLimit`) : le défaut axum (2 Mo) rejetait en 413 muet les
  replays longs (> 2 Mo) et faisait boucler l'uploader.
- `X-Filename` est **percent-décodé** (apostrophes de cartes : Blackheart's Bay).

## Fingerprints (dédup à deux niveaux)

- `uploads.fingerprint` = SHA-256 du **contenu du fichier** — dédup rapide pré-parse.
- `matches.fingerprint` = hash de `date|map|length|toonsTriés` — dédup **de partie** (compat
  overlay historique). `project_match` est idempotent (delete-then-insert dans une transaction,
  reprise sur deadlock 40P01/40001, UPSERT `players` HORS transaction pour ne pas sérialiser le
  backfill).

## Référentiel (`dim.rs`) — deux sources mutuellement exclusives

Tâche de fond au démarrage puis toutes les 24 h, **best-effort** :
- `REFERENTIAL_URL` (**prioritaire**) : snapshot publié `referential.tar.gz` → bundle autonome
  sans HotsPatchNotes live (`dim::ingest_snapshot`).
- `HOTSPATCHNOTES_URL` : API live (setup mainteneur) → `sync_heroes`, `sync_talents`,
  `vendor_images` (portraits/cartes copiés dans `IMAGES_DIR`, servis sur `/images`),
  `sync_patches`, `backfill_hero_sections`.

Chaque **nouveau patch** détecté → event WS `patch.new` + webhook sortant optionnel
(`PATCH_WEBHOOK_URL`, format Discord-compatible).

## Simulateur de draft (`draft/`)

Moteur **pur, zéro I/O** (`mod.rs`) : un format = liste ordonnée d'étapes `{order, action}` ;
`first_pick: Side` découple l'ordre (First/Second) du côté visuel (blue/red). État singleton
autoritatif en mémoire (`AppState.draft`), **persisté** dans `draft_live` (JSONB, id=1),
rechargé au démarrage. Chaque mutation broadcast `draft.updated` sur `/ws`.
Design : `docs/specs/2026-06-19-draft-simulator-design.md`.

## Configuration (env — `config.rs`, modèle : `.env.example`)

| Variable | Défaut | Rôle |
|---|---|---|
| `DATABASE_URL` | **requise** | Postgres |
| `BIND_ADDR` | `127.0.0.1:8088` | écoute HTTP |
| `ARCHIVE_DIR` | `./.archive` | replays bruts (source de vérité) |
| `RAW_CACHE_DIR` / `RAW_CACHE_MAX_BYTES` | `./.raw-cache` / 5 Gio | cache LRU des dumps |
| `ADMIN_TOKEN` | vide = **mode ouvert** | vide : aucune auth admin (auto-hébergement LAN/Tailscale) ; défini : Bearer requis sur les écritures admin |
| `WEB_DIR` | vide = API seule | front buildé servi sur `/` (fallback SPA, index en `no-cache`) |
| `IMAGES_DIR` | `./.images` | images vendorisées, servies sur `/images` |
| `REDIS_URL` / `JARVIS_CHANNEL` | vide = pas d'émission / `jarvis:events` (prod box : `storm-codex:match_completed`) | événements Jarvis |
| `HOTSPATCHNOTES_URL` / `REFERENTIAL_URL` | vides | référentiel (cf. ci-dessus) |
| `ORPHEUS_URL` | vide | proxy musique pour `/api/now-playing` |
| `PATCH_WEBHOOK_URL` | vide | notif sortante nouveau patch |
| `AZURE_PUSH_URL` / `AZURE_PUSH_TOKEN` | vides | **dormant** (extension Twitch abandonnée) |

Serveur **stateless** hors dossiers de données : toute la config par env (V2-ready).
