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
- **Jalon 5 : intégration Jarvis LIVE + vérifiée E2E** (2026-06-13). Backfill prod fait (2007
  matchs, 97,3 %). Le bridge Jarvis est écrit, déployé et **prouvé de bout en bout sur le box** :
  - storm-codex raccordé au réseau `jarvis_default` ; publie sur le canal pub/sub
    `storm-codex:match_completed` (REDIS_URL=redis://redis:6379/0). Collision d'alias `postgres`
    (jarvis-postgres) corrigée → DATABASE_URL via le nom de conteneur `storm-codex-pg`.
  - **Bridge Jarvis** (`jarvis/ingest/hots_matches.py`, dans le repo Jarvis) : SUBSCRIBE le canal
    → adapte en Event spine **`hots.match_completed`** (type à 1 point ; l'émetteur storm-codex
    produisait `hots.match.completed` à 2 points, **rejeté** par le modèle Event — corrigé au
    boundary). Gaté par `STORM_CODEX_MATCHES_ENABLED`. Worker démarré (`jarvis run … hots_matches`),
    abonné. + 3 tests (apostrophe préservée).
  - **Test E2E réel** : re-upload d'un replay archivé → parse → emit storm-codex → bridge →
    event `hots.match_completed | storm-codex | info | Silver City | winner 1` **présent dans la
    table `events` du spine Jarvis**. ✅
  - `reject_class` rendu exhaustif (stats_failure/-2, unverified_build/-7) → admin lisible.
  - **Brief proactif FR : LIVE + prouvé** (2026-06-13). Worker spine `jarvis/notify/hots_brief.py`
    (3e consumer-group, start `$`) : `hots.match_completed` → notif FR du point de vue de
    l'opérateur (identifié par `HOTS_PLAYER_NAME=matella`, auto-déduit = présent dans 2001/2007
    matchs). Ex. prouvé sur vrai upload : `🏆 HotS — Victoire — Tyrande sur Silver City — 6/8/3`
    (défaite = priorité haute). Délivré via le canal notify (`send`/ntfy). **Câblage ntfy complété**
    au passage : `NTFY_URL` était vide (oubli onboarding) → `http://ntfy:80` ; ça réactive aussi les
    autres notifs Jarvis (reminders…). Gaté `HOTS_BRIEF_ENABLED`. + 5 tests.
  - **Widget OBS local conforme maquette (écran 10)** (2026-06-13). Corrigé : il affichait
    `players[0]` + « équipe X gagne » générique → désormais **perspective opérateur** via
    `?me=<nom>` (browser source : `http://192.168.129.85:5102/widget?me=matella`). Montre
    V/D + héros + carte + **K/A/D + KP** (KP = takedowns / morts adverses ; la somme des
    takedowns surcomptait). Vérifié live : « VICTOIRE · Chen · Dragon Shire · 3/17/2 · KP 67% ».
    Fond transparent OK pour OBS. `kills/deaths/takedowns` ajoutés à `/api/matches`.
  **Décision scope (opérateur 2026-06-13)** : **on abandonne l'extension Twitch cloud / Azure**.
  Le seul overlay sera le **widget OBS local** (`/widget`, déjà servi par storm-codex, zéro Azure).
  `azure.rs` reste en dormance (non câblé). → plus de dépendance à des identifiants EBS.
  **Reste (opérateur)** :
  - **S'abonner au topic ntfy** `jarvis-62a7e8eba161` pour *recevoir* les briefs (0 abonné ;
    publication prouvée). Option alternative/complément : injecter la phrase Jarvis dans le widget.
  - Ajouter le widget dans OBS (browser source, URL ci-dessus) + valider sur une **vraie partie**.
  - **Décommission du Node** (runbook, après validation partie réelle).

  ⚠️ **Piège déploiement** : `rsync --delete` vers `~/apps/storm-codex` **supprime le `.env` du box**
  (gitignoré, absent côté source). TOUJOURS `--exclude .env`. Secrets récupérables depuis le
  conteneur vivant (`docker inspect storm-codex-server --format '{{range .Config.Env}}…'`) —
  surtout `POSTGRES_PASSWORD` (le volume est initialisé avec).
- **Jalon 6 : prêt à publier** — voir ci-dessous.

## D3 — ligues + export JSON (2026-06-13)
- **Export JSON** des matchs (suit les filtres) ✅.
- **Ligues** : regroupement au-dessus des équipes (colonne `teams.league`, migration 0004) ;
  `GET /api/teams` renvoie la ligue, `PUT /api/teams/{id}` (ré)assigne ; page **Leagues** (nav)
  groupe les équipes par ligue ; création + assignation dans l'Admin. ✅
- **Reste** : `dim_talents` (réf. talents depuis HotsPatchNotes → affichage des talents par joueur
  dans le détail de match) — prochain item.

## Orpheus (musique) — configuré, OAuth en attente (opérateur)
Creds Spotify en place (`.env` box), Orpheus redémarré, `/api/auth/login` redirige bien vers
Spotify (PKCE, redirect 127.0.0.1:3010). Reste : l'opérateur autorise via tunnel SSH
`-L 3010:127.0.0.1:3010` → `http://127.0.0.1:3010/api/auth/login` → Agree. Puis le widget musique
(intégré au `/queue` + page `/now-playing`) affiche la piste.

## Scène OBS « entre les games » (2026-06-13, vérifié live)
- **`/queue`** (browser source, fond transparent, EN) : TONIGHT'S SESSION (W-L, streak, WR),
  sparkline WR, recent games (badges mode + portraits + W/L + KDA), heroes tonight (W-L),
  best game, phrase Jarvis. Session = parties du même jour calendaire que la dernière.
- **`/now-playing`** (source persistante, EN) : widget musique lisant **Orpheus** via proxy
  `/api/now-playing` (config `ORPHEUS_URL`). Affiche « Music — off » tant que Spotify dormant.
- **Widget `/widget`** passé en anglais (VICTORY/DEFEAT) — cohérent. Voix Jarvis reste FR (persona).
- Maquette : `docs/specs/2026-06-13-scene-obs-entre-games-mockup.html` (panneau gauche, cam+jeu
  modestes à droite, musique bas-droite persistante).
- **Langue : tout en anglais** (décision opérateur — option B). Toutes les pages + nav + Admin +
  match detail traduits ; la **voix de Jarvis reste FR** (persona). Vérifié live.
- **Orpheus (musique)** : pour que `/now-playing` affiche la piste, il reste à configurer Spotify
  côté Orpheus (Premium + app dev + `cargo`/auth) — voir le récap dans le chat / `Orpheus/SETUP.md`.

## D1 — identité opérateur multi-comptes (2026-06-13, vérifié live)
Réglage **operator_names** (liste, multi-comptes) source unique de « qui suis-je » :
- Serveur : table `app_settings` (migration 0003), `GET /api/settings`, `PUT /api/admin/settings`.
- Front : `useSettings` + `pickOperator`/`matchOperator` → **widget par défaut sans `?me=`**,
  lignes Matchs (V/D + ton héros), **Session = TES stats** (win rate 53,5 % réel vs 50 % global,
  mes parties, héros joués, main héros). UI Admin « Mon identité » pour éditer.
- Jarvis : `HOTS_PLAYER_NAME` accepte une **liste** (le brief matche n'importe lequel de tes noms).
- Export **JSON** des matchs ajouté (suit les filtres), à côté du CSV. ✅
**Reste de D** : D2 phrase Jarvis dans le widget (round-trip) ; ligues (au-dessus des équipes) ;
`dim_talents` détaillés.

## Images : portraits héros + fonds de carte (2026-06-13, vérifié live)
- **Vendorisation déterministe** : au démarrage, `dim::vendor_images` télécharge les portraits
  héros (90) + images de cartes standard (15) depuis HotsPatchNotes dans `/data/images`
  (idempotent), servis par storm-codex sur `/images`. **Auto-suffisant** : aucune dépendance
  runtime à HotsPatchNotes (le téléchargement échoue en silence → fallback front). `IMAGES_DIR`.
- **Avatar = portrait réel** (cerclé couleur d'univers), fallback initiales si inconnu/erreur.
  `heroIcon()` lit `dim_heroes.icon`. ⚠️ le widget appelle `useDimHeroes()` lui-même (il est hors
  Layout, sinon DIM vide → initiales + anneau par défaut).
- **Fond de carte** sur les lignes Matchs + le widget : `mapImage()` (slug = nom min., sans
  apostrophe, espaces→tirets) en `background-image` voilé (gradient → texte lisible). Cartes ARAM
  sans image (Silver City, Industrial District, Lost Cavern, Braxis Outpost) → fond uni (fallback
  CSS silencieux, déterministe).

## Widget + front : passe du 2026-06-13 (vérifié live)
- **Widget OBS = perspective opérateur** : `?me=<nom>` (ex. `/widget?me=matella`) → V/D depuis
  ta team, KDA k/a/d, KP, mode, durée (avant : `players[0]` + « équipe X gagne » générique).
  KDA ajouté à `/api/matches`. Conforme maquette écran 10.
- **Codes de mode corrigés** (bug réel) : la table front était décalée (Storm League affichait
  « HL », QM « — »). Vérifié sur l'archive : **50091=SL, 50101=ARAM, -1=Custom** dominent.
  Plus de badge « — ».
- **Filtres façon SotS** sur Matchs : pills mode + dropdowns **carte** (20) + **héros** (91),
  tri `played_at DESC` (déjà en place). Export CSV suit les filtres.
- **Fix cache SPA (cause racine)** : `index.html` était servi **sans `no-cache`** → après chaque
  redeploy le navigateur restait sur un bundle périmé = **page blanche**. Corrigé (ServeDir ne
  sert plus l'index ; handler SPA en `no-cache`). Même classe de bug que HotsPatchNotes.
  ⚠️ Process : un échec `tsc` faisait échouer `npm run build` **silencieusement** (l'ancien
  bundle restait) — toujours vérifier `✓ built` au déploiement.
- **Reverse proxy + ntfy** : runbook `docs/runbooks/2026-06-13-reverse-proxy-et-ntfy.md`
  (le box utilise **Nginx Proxy Manager**, pas nginx-fichiers ; ntfy `:8093`). Reste opérateur :
  s'abonner au topic ntfy pour recevoir les briefs.
- **Connu / refinements** : identité opérateur globale (Session/Matchs/Héros montrent encore
  `players[0]`/agrégat, pas toi — le widget a `?me=`) ; phrase Jarvis dans le widget ; deltas vs
  moyenne. + résiduels jalon 4 (ligues, export JSON, dim_talents).

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

## Compat uploader client-rs — FAIT (2026-06-12)
Le client `client-rs` (Hots-Overlay v1.1.2) poste sur **`/api/upload-raw`** (octets bruts,
header `X-Filename`, `Bearer`). storm-codex n'exposait que `/api/upload` → **alias ajouté**
(`/api/upload-raw` → même handler). Bout-en-bout prouvé sur le box : token créé via admin,
upload octets → auth → archive → parse → statut classé, 401 sans token, santé admin OK. Le
client marche donc en re-pointant juste `SERVER_URL` (pas de changement de code client).
Token d'upload **matella-pc** créé (valeur donnée en chat ; recréable via `POST /api/admin/tokens`
avec le champ `name` + `ADMIN_TOKEN` du `.env` box). Base + archive remises à vide.

## Deux correctifs (2026-06-12, retours opérateur)
- **Backfill réellement complet** (repo Hots-Overlay, `client-rs`) : le commit PC d3efb77 annonçait
  « scan complet » mais `candidates.truncate(INITIAL_UPLOAD_LIMIT=10)` était resté → seuls les 10
  plus récents partaient. Corrigé : `scan_and_upload` envoie **tout** le non-uploadé (oldest-first,
  historique chronologique), idempotent (set persisté + 409 serveur). Cap supprimé.
- **Apostrophe dans le nom de carte** (Blackheart's Bay) : le nom **parsé** en base était déjà sûr
  (sqlx bind, jamais d'interpolation ; front via URLSearchParams + échappement React). Restait le
  header `X-Filename` stocké percent-encodé (`Blackheart%27s Bay`) → **décodé côté serveur** + test ;
  vérifié de bout en bout sur le box (`… Blackheart's Bay.StormReplay` stocké proprement).

## Ce qui reste (état réel au 2026-06-13)
Le développement est essentiellement terminé (jalons 0→5 livrés + vérifiés ; backfill prod fait :
2007 matchs ; Redis Jarvis câblé + brief E2E ; images vendorisées ; widget/filtres). Reste :

**A. Actions opérateur (pas du code)**
- **S'abonner au topic ntfy** `jarvis-62a7e8eba161` (appli ntfy, serveur `http://192.168.129.85:8093`)
  pour *recevoir* les briefs (publication déjà prouvée ; 0 abonné aujourd'hui).
- **Ajouter le widget dans OBS** : browser source `http://192.168.129.85:5102/widget?me=matella`.
- (optionnel) **Reverse proxy NPM** pour un nom propre + TLS — runbook
  `docs/runbooks/2026-06-13-reverse-proxy-et-ntfy.md`.

**B. Bascule finale jalon 5 (ensemble, le soir, sur une vraie partie)**
- Jouer une partie → valider widget OBS + page < 5 s en conditions réelles (le reste est prouvé).
- **Décommissionner le serveur Node** de l'overlay — runbook
  `docs/runbooks/2026-06-12-bascule-decommission-node.md`. (Azure/extension Twitch **abandonné** —
  décision opérateur : overlay local uniquement.)

**C. Jalon 6 — publication (action crates.io opérateur)**
- `cargo publish` storm-replay puis storm-stats ; repos GitHub publics. `--dry-run` validé, MIT en place.

**D. Refinements — ✅ LIVRÉS (2026-06-13)**
- ✅ Identité opérateur globale (réglage `operator_names` multi-comptes + `pickOperator`/`matchOperator`
  partout : Session, Matches, widget, brief Jarvis).
- ✅ Phrase Jarvis dans le widget OBS (répertoire déterministe FR, voix majordome).
- ✅ Résiduels jalon 4 : **ligues** (page /leagues + assignation Admin), **export CSV/JSON**,
  **`dim_talents`** (référentiel 1918 talents synchronisé depuis HotsPatchNotes → builds nommés
  par joueur dans la fiche de match ; icônes non servies par HotsPatchNotes → texte en V1).
- ✅ Portraits héros + fonds de carte vendorisés ; UI **entièrement en anglais**.
- ✅ Scène OBS `/queue` (1920×1080, panneau session + slots cam/game encadrés + musique intégrée) ;
  widget musique `/now-playing` (proxy Orpheus, pochette d'album, forme imbriquée gérée).

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
