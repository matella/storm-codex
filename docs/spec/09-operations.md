# Opérations — dev, déploiement, sauvegardes

## Dev local (PC de jeu Windows)

```bash
docker compose -f docker-compose.dev.yml up -d   # Postgres 17 :5433 + Redis :6380
cp .env.example .env                             # DATABASE_URL etc.
cargo run -p storm-codex-server                  # migrations auto, :8088
npm run dev --prefix web                         # ou build : npm run build --prefix web + WEB_DIR
```

Tests : `cargo test` (le test de projection s'ignore sans `DATABASE_URL`) ; parité :
`tools/parity-harness/` ; benchs : `storm-stats-dump --bench`, `backfill_bench.py`.

⚠️ **Le clone local peut être en retard** : du dev se fait aussi depuis le Mac/box.
`git fetch origin` **avant** de lire le code ou planifier ; après un pull,
`npm install --prefix web` si de nouvelles deps.

## Prod (box 192.168.129.85, `~/apps/storm-codex`)

- **Stack** : `docker-compose.yml` — Postgres dédié `storm-codex-pg` (volume `storm-codex-pgdata`)
  + `storm-codex-server` (build multi-stage : SPA Vite → Rust release → Debian slim ; volume
  `/data` = archive + caches + images). Port hôte **5102** (8088 pris par gluetun).
  Healthcheck sur `/api/health`.
- **Réseaux** : `default` + `jarvis_default` (externe) pour publier sur jarvis-redis.
  `DATABASE_URL` pointe le **nom de conteneur** `storm-codex-pg` (l'alias `postgres` collisionne
  avec jarvis-postgres sur le réseau partagé).
- **Secrets** : uniquement dans le `.env` du box (jamais commité). `POSTGRES_PASSWORD` a
  initialisé le volume — récupérable depuis le conteneur vivant (`docker inspect`).
- **Déployer** : rsync du repo → `docker compose up -d --build`. Ou, pour un changement
  front/serveur isolé : récupérer les fichiers depuis GitHub (repo public) puis rebuild — le
  cache Docker ne reconstruit que l'étage touché.
  ⚠️ **JAMAIS `rsync --delete` sans `--exclude .env`** (il supprimerait le `.env` du box).
- **Vérifier après déploiement** : conteneurs healthy, `/api/health` 200, hash du bundle
  (`index-*.js` dans `/`) changé.
- **Accès distant** : Tailscale → 192.168.129.85 (SSH + :5102), le soir quand le box tourne.
  Les clés SSH existent sur le Mac (pas sur le PC de jeu).

## Uploader (PC de jeu)

`client-rs` (repo **Hots-Overlay**) : `.env` racine avec `SERVER_URL=http://192.168.129.85:5102`
+ `AUTH_TOKEN=<token nominatif>`. Au lancement : backfill **complet** du non-uploadé
(oldest-first, idempotent — set persisté + 409 serveur), puis watch du dossier replays.

## CI — publication d'image (`.github/workflows/publish.yml`)

**Chaque push sur `main`** (et chaque tag `v*`) build l'image Docker complète et la pousse sur
**`ghcr.io/matella/storm-codex:latest`** (GITHUB_TOKEN, rien à configurer). Conséquences :

- Un push sur `main` = une image publique à jour ; le box peut donc aussi se mettre à jour par
  `docker pull ghcr.io/matella/storm-codex:latest` au lieu d'un build local (alternative au
  rsync + build documenté ci-dessus — le compose actuel du box fait `build: .`).
- Ne jamais pousser sur `main` un état qui ne build pas (le Dockerfile est le gate : SPA
  `tsc + vite` puis Rust release).

## Sauvegardes

`scripts/backup.sh [dest]` : `pg_dump` → `.sql.gz` horodaté, rétention 14. La base est de toute
façon **reconstruisible depuis l'archive brute** (re-upload / reprocess) — le backup évite juste
un re-backfill. L'archive brute (`/data/archive` du volume) est la seule donnée irremplaçable.

## Dépendances externes (toutes best-effort)

| Service | Où | Si absent |
|---|---|---|
| HotsPatchNotes | box :5001 (`host.docker.internal`) | pas de refresh dim_* / images (données déjà répliquées conservées) |
| Redis Jarvis | réseau `jarvis_default` | pas d'émission d'events (parse intact) |
| Orpheus | box :3010 | `/api/now-playing` vide (widget musique inerte) |

## Runbooks (procédures opérateur détaillées)

`docs/runbooks/` : bascule/décommission du Node overlay · reverse proxy + ntfy · publication
crates.io (`2026-06-13-publication-crates.md`).
