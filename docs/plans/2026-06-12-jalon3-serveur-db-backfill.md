# Jalon 3 — `storm-codex-server` : upload + Postgres + workers + WS + backfill

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. **MCP Context7 obligatoire**
> avant d'utiliser axum / sqlx / tokio / tower (versions qui bougent) — `resolve-library-id`
> puis `query-docs`. Ne pas coder ces API de mémoire.

**Goal :** un binaire Rust `storm-codex-server` (axum + Postgres) qui reçoit les replays
(`POST /api/upload`, token), les archive, les parse en pool (storm-replay→storm-stats), projette
en Postgres (`parser_version` partout, re-process idempotent), pousse `match.parsed` en WebSocket,
sert les endpoints REST de lecture, le dump `…/raw` (cache LRU), et l'admin ; plus le mode
**backfill** de `client-rs`.

**Accept (spec) :** 100 % de l'archive archivée et tentée ; **≥ 99 % parsée** (échecs listés et
classés dans Admin) ; **fin de partie → page à jour < 5 s**. Budgets : écriture PG + push WS
< 200 ms ; backfill 3 ans (~3 000 replays) < 5 min (workers = nb cœurs).

**Architecture :** workspace existant (storm-replay, storm-stats). Nouveau crate binaire
`crates/storm-codex-server`. **Dev = Postgres local Docker** (`docker compose` dans le repo) ;
déploiement box plus tard. sqlx (compile-time checked queries, migrations). client-rs cloné :
`C:\Users\matth\Desktop\Coding\Hots-Overlay\client-rs`.

**3 étages (rappel spec) :** archive brute (fichiers, source de vérité) ; Postgres (projection
complète) ; dump décodé à la demande + cache LRU borné. **Jamais de pré-décodage massif.**

---

### Task 1 : Docker Postgres dev + scaffold serveur + /api/health
- `docker-compose.dev.yml` : Postgres 17 (port 5433 pour ne pas heurter un PG existant), volume,
  `POSTGRES_*` ; `.env.example` (DATABASE_URL, BIND_ADDR, ARCHIVE_DIR, RAW_CACHE_MAX_BYTES).
- `crates/storm-codex-server` : axum, tokio, sqlx (runtime tokio + postgres + macros + migrate),
  tower-http (trace), thiserror, serde. `main.rs` : config par env, pool sqlx, `/api/health`
  (renvoie `{status:"ok", parser_version, db:"up"}`).
- **Context7** axum (router, State, handlers) + sqlx (PgPool, migrate) avant de coder.
- Test : `docker compose -f docker-compose.dev.yml up -d`, `cargo run -p storm-codex-server`,
  `curl /api/health` → 200. Commit.

### Task 2 : schéma DB (migrations sqlx) + parser_version
- `migrations/0001_init.sql` : `upload_tokens` (id, name, token_hash, created_at, revoked) ;
  `uploads` (id, token_id, filename, fingerprint UNIQUE, archived_path, status
  enum[pending|parsed|duplicate|parse_failed], error_class, error_msg, parser_version,
  replay_version_build, created_at, parsed_at) ; `matches` (id, fingerprint UNIQUE, build, mode,
  map, duration_loops, length, date, winner, first* , patch_codex, parser_version, created_at) ;
  `match_players` (match_id, slot, toon_handle, hero, team, win, score JSONB, talents JSONB,
  awards JSONB, hero_level, …) ; `talents`, `draft` (picks/bans JSONB ou lignes) ;
  `timeline_events` (match_id, kind, payload JSONB) — takedowns/objectifs/structures/xp/
  teamfights/messages/votes ; `players` (toon_handle PK, names JSONB/alias, tags) ;
  `teams`/`leagues`/`collections` (rosters) ; `dim_heroes`/`dim_talents` (référentiel).
  Index sur match(map,mode,date,build), match_players(toon_handle,hero,match_id).
- Décision JSONB vs colonnes : score screen complet en JSONB `score` (80 stats — éviter 80
  colonnes), + colonnes promues pour les axes de filtre/tri chauds (hero, team, win, kills,
  deaths, hero_dmg…). Documenter dans le SQL.
- Test : migration s'applique, `\d` cohérent. Commit.

### Task 3 : projection storm-stats → lignes Postgres
- Module `project.rs` : `MatchStats` (sortie storm-stats `{match,players}`) → structs typées →
  INSERT transactionnel idempotent (UPSERT par fingerprint ; un re-process supprime/replace les
  lignes du match et ré-insère, piloté par `parser_version`). Le fingerprint = même formule que
  l'existant/HeroesProfile (MD5 BlizzIDs+random) — vérifier la formule dans client-rs/HeroesProfile.
- storm-stats expose ce qu'il faut (ajouter des accesseurs typés si besoin plutôt que reparser
  le JSON). Idempotence : re-insérer le même replay ne crée pas de doublon, met à jour si
  `parser_version` change.
- Test d'intégration : projeter 3 replays du mini-corpus → lignes attendues (10 match_players,
  draft, timeline non vide). Commit.

### Task 4 : pipeline d'upload + pool de workers + sémantique de réponse
- `POST /api/upload` (Bearer token validé contre `upload_tokens`) : lit le fichier, calcule le
  fingerprint, **archive d'abord** (ARCHIVE_DIR), insère `uploads(pending)` ; si fingerprint
  déjà `parsed` → réponse `duplicate` immédiate. Sinon enfile un job dans le **pool tokio**
  (taille = nb cœurs, jamais sur le thread HTTP) : parse storm-replay→storm-stats→project, met
  `uploads` à `parsed`/`parse_failed`(+error_class typé depuis `storm_replay::Error` / panic
  isolé via `catch_unwind`). **Sémantique** : la requête attend le résultat jusqu'à 2 s (typique
  < 0,5 s) et renvoie le statut final ; pool saturé → `202 {status:"accepted"}`. Aucun échec ne
  bloque la file.
- **Context7** tokio (mpsc/Semaphore/spawn_blocking pour le parse CPU) + axum (extractors, body).
- Tests : upload OK → parsed ; ré-upload → duplicate ; fichier corrompu → parse_failed classé ;
  budget < 200 ms hors parse. Commit.

### Task 5 : WebSocket `/ws` + push `match.parsed` (critère < 5 s)
- `/ws` (axum ws + broadcast tokio) : à la fin d'un parse réussi, diffuse
  `{type:"match.parsed", match_id, map, …}`. Backfill : progression périodique.
- Test : client WS reçoit l'event < 200 ms après parse ; bout-en-bout upload→event < 5 s.
  **Context7** axum WebSocket + tokio::sync::broadcast. Commit.

### Task 6 : endpoints REST de lecture (l'API que le front consommera au jalon 4)
- `GET /api/matches` (filtres carte/héros/joueur/date/mode/patch, pagination) ; `/api/matches/{id}`
  (score 10 joueurs, draft, timeline) ; `/api/players/{toon}` (résumé, hero pool) ;
  `/api/heroes`, `/api/heroes/{h}` ; trends/classements minimaux ; exports CSV/JSON.
  Vues/maté-vues pour les agrégats chauds. **Budget p95 < 100 ms** (mesurer sur l'archive
  backfillée). Commit (peut être découpé en plusieurs).

### Task 7 : dump `…/raw` + cache LRU borné
- `GET /api/matches/{id}/raw?stream=…` : décode le fichier archivé à la volée (storm-replay),
  cache disque compressé **LRU borné** (`RAW_CACHE_MAX_BYTES`, défaut 5 GB). Jamais de
  pré-décodage massif. Test : 2 requêtes → 2e servie du cache ; éviction quand plafond atteint.
  Commit.

### Task 8 : admin (tokens, re-process, backfill, santé)
- `POST /api/admin/tokens` (créer/révoquer) ; `POST /api/admin/reprocess` (tout ou par filtre,
  idempotent, suit `parser_version`) ; `GET /api/admin/uploads` (état/santé, échecs classés) ;
  état backfill. Auth admin simple (token admin env). Commit.

### Task 9 : client-rs — re-pointage + mode backfill + latence
- Dans `Hots-Overlay/client-rs` : re-pointer vers `/api/upload` (au lieu de `/api/upload-raw`) ;
  **mode backfill** (scan complet de l'archive, upload throttlé, reprise sur interruption, barre
  de progression tray) ; abaisser la stabilisation fichier (~5 s → ~1 s). Construire et tester
  contre le serveur local. Commit dans le repo Hots-Overlay (séparé).

### Task 10 : backfill réel + critère d'acceptation + bench + STATUS + push
- Backfill de l'archive locale complète (2 821 replays) vers le serveur (Postgres Docker).
  **Vérifier : 100 % archivés et tentés ; ≥ 99 % parsés** (échecs listés/classés dans Admin —
  les rejets « carte inconnue » de storm-stats comptent comme tentés/classés, pas comme perte) ;
  backfill < 5 min ; p95 API < 100 ms. Rapport `docs/research/2026-06-12-jalon3-bench.md`,
  STATUS, commit + **push**.

---

## Pièges connus
- **Context7 d'abord** pour axum 0.7+/sqlx 0.8+ (extractors, State, ws, migrate, PgPool) — API
  mouvantes, ne pas coder de mémoire (règle CLAUDE.md).
- Parse = CPU-bound : `spawn_blocking` ou pool dédié, jamais sur le runtime HTTP.
- `parser_version` : bumper quand storm-stats change → le re-process recalcule.
- Fingerprint : réutiliser la formule exacte de l'existant (anti-doublon cross-source) — la lire
  dans client-rs/uploader.rs et HeroesProfile, ne pas inventer.
- Postgres Docker en 5433 (un PG peut déjà tourner en 5432).
- storm-stats rejette les cartes hors `MapType` (statut -2) : côté serveur, c'est un
  `parse_failed(error_class="unsupported_map")` **tenté et classé**, pas une perte — le critère
  « ≥ 99 % parsé » doit considérer ces rejets connus (à documenter ; lever la table de cartes est
  un chantier post-parité, cf. rapport jalon 2).
- Idempotence re-process : transaction, delete-then-insert par fingerprint.
