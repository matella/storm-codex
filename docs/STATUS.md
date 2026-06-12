# STATUS — lire d'abord, mettre à jour en dernier

## Où on en est (2026-06-12)
- **Recherche terminée** : anatomie SotS + hots-parser, verdicts dépendances, comparatif moteurs,
  écosystème (HeroesProfile vivant, HeroesMatchTracker archivé). → `docs/research/`.
- **Spec programme validée** (opérateur + revue agent, 2 passes) : Rust core, Postgres,
  3 étages de données, parité SotS totale en V1 (ligues incluses), remplacement du serveur Node
  local, widget stream, scope C (auto-hébergé V1, open-source, public-ready V2), multi-tokens.
  → `docs/specs/2026-06-12-storm-codex-design.md`.
- **Maquettes validées** (14 écrans, couverture vue-par-vue auditée contre SotS ; widget défaite
  enrichi sur retour opérateur). → `docs/specs/2026-06-12-storm-codex-mockup.html`.
- **Jalon 0 : GO** (2026-06-12, exécuté sur le PC de jeu). Spike Rust `spike/storm-decode` :
  50/50 replays décodés (header+details+tracker), médiane **12 ms** / p95 21 ms vs .NET 37 ms
  (1 échec) et Python 161 ms ; cross-check heroprotocol 3/3 identique. Repli .NET écarté.
  → rapport : `docs/research/2026-06-12-jalon0-bench.md` · plan exécuté :
  `docs/plans/2026-06-12-jalon0-spike-decode.md`.

- **Jalon 1 : FAIT** (2026-06-12). Crate `crates/storm-replay` : 7 streams décodés (versioned +
  bitpacked + attributes), tables protocol-gen embarquées (390 builds → 32 dédupliquées),
  fallback builds inconnus signalé, erreurs typées. **Critères : archive réelle 2 821/2 821
  décodés (100 %, 22 builds 2024→2026) ; bench 7 streams médiane 102 ms < 150** (p95 205 ms,
  dont ~50–115 ms de plancher décompression bzip2 incompressible — point signalé à l'opérateur).
  Parité **bit-exacte** prouvée vs heroprotocol (deep-compare 7 streams × 4 replays,
  `tools/crosscheck_streams.py`). Plan : `docs/plans/2026-06-12-jalon1-storm-replay.md`.

- **Jalon 2 : FAIT** (2026-06-12). Crate `crates/storm-stats` : port fidèle de hots-parser
  (3 360 lignes JS) → `{match, players, status}`. **Critère : diff automatique champ par champ
  vs hots-parser 7.55.7 (Node) — 114/114 verts** (79 parse complet identique, 35 rejets
  identiques sur cartes absentes de la `MapType` de la référence). 1 tolérance documentée et
  favorable (coordonnées de ping : storm-stats plus correct que la référence). Bench parse
  complet **133 ms médiane** (échantillon représentatif, < 150 budget ; à-budget 151 ms sur le
  pire cas, décodage-dominé). Harnais : `tools/parity-harness/` (`dump.js`, `diff.py`,
  `tolerances.json`). Rapport : `docs/research/2026-06-12-jalon2-parite.md` · plan :
  `docs/plans/2026-06-12-jalon2-storm-stats.md`.

- **Jalon 2.5 (préalable jalon 3, décision opérateur)** : extension storm-stats aux 4 cartes
  ARAM récentes (Silver City, Lost Cavern, Braxis Outpost, Industrial District) que hots-parser
  7.55.7 rejette — ~30 % de l'archive. `EXTRA_MAPS`, objectif minimal, handlers gardés. Diff
  toujours 114/114 (79 parse complet + 25 extension + 10 rejets brawls).
- **Jalon 3 : FAIT** (2026-06-12, dev contre Postgres Docker local). `crates/storm-codex-server`
  (axum 0.8 + sqlx 0.8) : upload (token, archive, pool de parse, sémantique ≤ 2 s/202),
  projection Postgres idempotente, WebSocket `match.parsed`, REST (matches/match/player/heroes),
  dump `…/raw` + cache LRU, admin (tokens/reprocess/santé). **Critères : archive 2781/2781
  archivée+tentée ; 99,4 % parsée** (échecs classés, tous légitimes) ; **backfill 1,8 min < 5** ;
  **fin de partie → page 1,4 s < 5** ; **API p95 52 ms < 100**. Bug de deadlock concurrent trouvé
  par le backfill et corrigé. client-rs re-pointé (repo Hots-Overlay). Rapport :
  `docs/research/2026-06-12-jalon3-bench.md` · plan :
  `docs/plans/2026-06-12-jalon3-serveur-db-backfill.md`.

- **Jalon 4 : socle livré** (2026-06-12). SPA `web/` (Vite + React + TS + TanStack Query),
  design Nexus Codex (tokens de la maquette), servie par le binaire (`WEB_DIR` → ServeDir +
  fallback index.html pour le routing SPA). Pages data-backed vérifiées contre la DB backfillée
  (capture à l'appui) : **Session/dashboard, Matchs (filtrable, WS temps réel), Détail de match
  (score 10 joueurs ×2 équipes, draft), Héros (agrégat triable), Cartes, Joueur (hero pool)**.
  Nouvel endpoint `/api/maps`. Plan : `docs/plans/2026-06-12-jalon4-front-parite.md`.
  **Reste à compléter pour la parité 1:1** : trends par patch, classements, équipes/ligues/
  collections (définitions manuelles), admin/import UI, exports CSV/JSON, anneaux d'univers réels
  (`dim_heroes` depuis HotsPatchNotes :5001), timelines uPlot du détail de match.

- **Jalon 5 : code livré** (2026-06-12). Émetteur **Jarvis** (`jarvis.rs`) :
  `hots.match.completed` → Redis avec invariants spine (testé E2E contre Redis local Docker,
  event bien formé). **Widget OBS** (`/widget`, fond transparent, live WS). **Push Azure**
  (`azure.rs`, POST sortant authentifié — code prêt, non testé contre l'EBS réelle).
  **Bascule/décommission Node** : runbook `docs/runbooks/2026-06-12-bascule-decommission-node.md`
  (étape box/opérateur, le soir). Plan : `docs/plans/2026-06-12-jalon5-stream-jarvis.md`.
- **Jalon 6 : prêt à publier** — voir ci-dessous.

## Jalon 4 — compléments faits (2026-06-12, après le socle)
`dim_heroes` répliqué depuis HotsPatchNotes au démarrage (90 héros/6 univers) → **anneaux
d'univers réels** sur les avatars ; **timeline uPlot** d'avantage de niveau sur le détail de
match ; **export CSV** (`/api/matches.csv` + lien UI) ; détail de match **enrichi** (draft
ordonné + premiers objectif/fort/keep). Endpoint `/api/dim/heroes`.

## Jalon 4 — secondaire fait (2026-06-12)
**Trends par patch** (winrate/durée par build, `/api/trends`), **équipes/collections** (CRUD
admin-protégé, `/api/teams`+`/api/collections`), **page Admin/Import** (santé uploads, création
de tokens, gestion équipes/collections, re-process). Toutes les pages SotS ont leur équivalent.

## Déploiement box — FAIT (non invasif, 2026-06-12 soir)
Empaquetage livré : `Dockerfile` multi-stage (web Vite + serveur Rust + runtime slim, migrations
embarquées), `docker-compose.yml` prod (Postgres dédié `storm-codex-pg` + serveur ; push opt-in
laissé vide), `.dockerignore`, `.env.example`. **Déployé sur le box** dans `~/apps/storm-codex`
(rsync + `docker compose up -d --build`). Port hôte **5102** (8088 pris par gluetun). Vérifié :
les 2 conteneurs **healthy** ; `/api/health` 200 (db up) ; 11 tables migrées ; **dim_heroes = 90
héros répliqués depuis HotsPatchNotes :5001** (via `host.docker.internal`) ; SPA `/` 200 ; widget
`/widget` 200 ; DB vide prête ; joignable du Mac via Tailscale (`192.168.129.85:5102`).
**Non invasif** : Node overlay, Mongo et Redis Jarvis **non touchés** (REDIS_URL/AZURE_* vides) ;
aucune décommission. Le serveur est en place, sain, en attente d'uploads du PC de jeu.

## Ce qui reste (dépendances opérateur/PC de jeu)
- **Backfill réel** : pointer `client-rs` (PC de jeu) vers `http://192.168.129.85:5102` + token
  d'upload (créer via `POST /api/admin/tokens`, ADMIN_TOKEN dans le `.env` du box) ; lancer le
  mode backfill de l'archive (~2 800 replays). Validé en dev jalon 3 ; à rejouer en prod.
- **Bascule jalon 5** (le soir, partie réelle) : renseigner `REDIS_URL` (Redis Jarvis) +
  `AZURE_PUSH_URL/TOKEN` dans le `.env` du box → `docker compose up -d` ; jouer une partie →
  vérifier page < 5 s, widget OBS, event `hots.match.completed` sur Redis Jarvis, push Azure ;
  puis runbook de décommission du Node (`docs/runbooks/2026-06-12-bascule-decommission-node.md`).
- **Jalon 4 (résiduel mineur)** : ligues au-dessus des équipes, export JSON, `dim_talents`.
- **Jalon 6 (publication)** : `cargo publish` storm-replay puis storm-stats (compte crates.io
  opérateur) ; repos GitHub publics. `--dry-run` validé, LICENSE MIT en place.

## Jalons (résumé — détail et critères dans la spec)
0 spike **GO ✅** → 1 storm-replay **✅** → 2 storm-stats **✅** (+ 2.5 cartes ARAM) →
3 serveur+DB+backfill **✅** → 4 front parité **socle ✅** → 5 stream+Jarvis+bascule **code ✅ +
déployé box (non invasif) ✅** → 6 publication **prêt** (publish = action opérateur).

## Décisions verrouillées (ne pas rouvrir sans l'opérateur)
Rust (**confirmé par le spike** — repli .NET écarté) · Postgres · design Nexus Codex ·
remplacement serveur Node local (EBS Twitch Azure conservé en V1, alimenté par push) · V1 =
parité totale · pas de pré-game · aucune migration de données (backfill + recréation manuelle)
· nom Storm Codex.

## Bloquants / besoins opérateur
- ~~Échantillon de replays pour le jalon 0~~ — résolu : le jalon 0 a tourné sur le PC de jeu
  (2 652 replays locaux, corpus reproductible via `spike/sample_corpus.ps1`).
- Création des repos publics GitHub (storm-replay/storm-stats) au moment du jalon 6 — d'ici là,
  tout vit dans ce repo.
