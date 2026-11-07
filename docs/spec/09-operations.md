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

## CI — un seul workflow (`ci.yml`) : contrôles, tests, audit, puis publication d'image

**Jobs** (push `main`, tags `v*`, PR, lancement manuel) :
- `checks` : `cargo fmt --all -- --check` ; **migrations figées** (`scripts/check-migrations.sh`) :
  refuse toute modification, suppression ou renommage d'une migration déjà présente dans la base
  de comparaison (la branche cible pour une PR, le commit d'avant pour un push). sqlx vérifie la
  somme de contrôle de chaque migration appliquée : en réécrire une fait échouer le démarrage
  (`VersionMismatch`) de toute base qui l'a déjà appliquée. Un changement de schéma = une
  **nouvelle** migration. Modifier une migration ajoutée dans la même PR reste permis.
- `rust` : clippy strict (`-D warnings`) + `cargo test --workspace` contre un **service
  Postgres 17** (les tests d'intégration DB-gated tournent réellement).
- `audit` : `cargo audit` (base RustSec) sur `Cargo.lock`.
- `web` : `npm ci`, `npm audit --omit=dev --audit-level=high` (dépendances embarquées dans le
  bundle), `npm test` (vitest), `npm run build`. Version de Node : `.nvmrc` (22).
- `publish` : **seulement sur push** (ou lancement manuel) **et seulement si les quatre jobs
  précédents sont verts** (`needs`). Pousse l'image sur `ghcr.io/matella/storm-codex` avec les
  tags `sha-<12 premiers caractères du commit>` (toujours — permet un retour arrière),
  `latest` (push sur `main`) et le nom du tag git (tag `v*`). Cache de build Docker : GitHub
  Actions (`type=gha`).

Le box peut se mettre à jour par `docker pull ghcr.io/matella/storm-codex:latest` (ou revenir à
une version précise par `:sha-…`) au lieu d'un build local — le compose actuel du box fait `build: .`.

## Image Docker (`Dockerfile`)

- **Cache des dépendances Rust : cargo-chef.** L'étage `planner` calcule une recette des
  dépendances du workspace ; l'étage `server` les compile (`cargo chef cook`) dans une couche qui
  ne se reconstruit que si `Cargo.toml`/`Cargo.lock` changent. Mesuré le 2026-11-07 : après un
  changement de code seul, rebuild complet en **91 s**, couche `cargo chef cook` en cache.
- **Utilisateur non root** (`storm`, uid **10001**). `/data` lui appartient dans l'image. Un
  volume créé par une image plus ancienne (root) est remis à cet uid par le service
  `storm-codex-init` du `docker-compose.yml` (busybox, `chown -R 10001:10001 /data`, une fois par
  démarrage, avant le serveur). Vérifié : volume possédé par root → serveur non root qui écrit
  dans `/data/archive`, `raw-cache`, `images`.
- `stop_grace_period: 20s` dans le compose : `docker stop` laisse le serveur finir les requêtes
  en cours (arrêt propre, voir 04-serveur.md).

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

## Risques connus & dette (audit 2026-08-03 — à re-passer périodiquement)

Acceptés en l'état (contexte : auto-hébergé, LAN/Tailscale, un opérateur) :
- **`ADMIN_TOKEN` vide = mode ouvert** — voulu en local ; à définir si exposé (reverse proxy).
- **Panics d'init** dans storm-stats (`constants.rs`, regex de `src/process/`) — sur données
  **embarquées** à la compilation : un échec = build invalide, crash au démarrage acceptable.
- Auth admin : factorisée dans `auth.rs` (comparaison par empreinte, sans sortie anticipée) ;
  les écritures cast/draft sont gardées quand `ADMIN_TOKEN` est défini (2026-11-07).

Dépendances (décision opérateur du 2026-11-07 : « corrige le tout ») :
- **react-router-dom 7** (paquet conservé, ré-export de `react-router`), **vite 8**,
  **@vitejs/plugin-react 6**, **vitest 5**, **dompurify 3.4.17** : `npm audit` → 0 vulnérabilité.
- `cargo audit` tourne en CI (job `audit`).

## Runbooks (procédures opérateur détaillées)

`docs/runbooks/` : bascule/décommission du Node overlay · reverse proxy + ntfy · publication
crates.io (`2026-06-13-publication-crates.md`).
