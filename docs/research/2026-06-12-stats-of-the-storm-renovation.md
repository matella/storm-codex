# Dossier de recherche — rénovation complète de « Stats of the Storm »

**Date :** 2026-06-12 · **Statut :** recherche terminée, brainstorm en cours (rien d'implémenté).
**Objectif :** réécrire https://github.com/ebshimizu/stats-of-the-storm (tracker de stats HotS,
Electron 2018) en stack moderne et performante, intégré à notre écosystème (Hots-Overlay +
uploader Rust, HotsPatchNotes/Nexus Codex, Jarvis), avec un widget stream « Jarvis » (résultat,
KDA, résumé de perf) et un pipeline replay → page web **instantané**.

---

## 1 · Anatomie de Stats of the Storm (l'existant à égaler)

**Stack d'origine (tout est 2018, tout est mort ou gelé) :**
Electron 4 · jQuery 3 + Semantic UI 2 + Handlebars (templates HTML côté client) · Chart.js 2 ·
DataTables 1.10 (+7 plugins) · moment.js · NeDB 1.8 / LinvoDB3 + medea/medeadown (stores
abandonnés ~2016-2018) · `request` (déprécié) · vis.js 4 · floatThead/tablesort (plugins jQuery).
Process : Electron main + un `background.html` caché qui fait le parsing pour ne pas geler l'UI.

**Modèle de données (4 collections LinvoDB, schemaless) :**
- `matches` — 1 doc par partie : version, mode, carte, date, durée, équipes, picks/bans + ordre
  de draft, niveaux/XP timeline, takedowns détaillés (positions, participants), structures
  détruites, **timeline d'objectifs par carte**, messages/pings, votes fin de partie, stats d'équipe.
- `heroData` — 1 doc par joueur×partie : héros, talents par palier, ~80 stats individuelles
  (gameStats du score screen + stats calculées), awards, taunts/BM (bsteps, danses, sprays…).
- `players` — profil cumulé par ToonHandle (cache des agrégats + alias/tags).
- `settings` — réglages + définition des « collections » (sous-ensembles de matchs, ex. ligues).
- Concepts transverses : **collections** (filtres persistants de matchs), **équipes** (rosters
  nommés), focus « My ID », import sets (dossier → collection).

**Pages / fonctionnalités (templates + js/) :**
- *Matches* : liste filtrable (carte/mode/héros/joueur/date/patch), drill-down.
- *Match detail* (le plus riche, 3 084 lignes) : score complet 10 joueurs, draft avec ordre,
  graphes XP (vis/chart), timeline objectifs + takedowns, stats d'équipe, taunt/BM table, chat,
  export/print par sections.
- *Player* : résumé, progression, hero pool, détail par héros, talent builds + pick rates,
  duos (« with » / « against »), award tracker, big tables.
- *Hero collection* : stats globales par héros (picks, win rate, talents, compos).
- *Trends* : par patch — picks/bans/win rates, compos.
- *Player ranking / Team ranking / Teams* : classements internes, ligues (rosters), MVP ratios.
- *Maps* : win rates par carte/side, first objective.
- *Settings/Parser* : watch folder, import par date, import → collection, upload HotsAPI/HotsLogs
  (tous deux morts aujourd'hui — l'équivalent vivant est HeroesProfile).
- Exports CSV (hero-csv, hero-draft-csv) + bases « externes » partageables.

**Agrégations (js/database/summarize-*.js, ~2 000 lignes au total) :** hero/map/match/player/
talent/team/trend — tout est fait **en JS en mémoire** sur les résultats de requêtes NeDB.
C'est le 2ᵉ goulot de perf après le parsing (le 1ᵉʳ chargement d'un gros historique se compte
en dizaines de secondes, la RAM monte vite, compaction manuelle).

**Awards :** mapping complet `EndOfMatchAward*Boolean` → nom/icône dans `js/game-data/awards.js`
(MVP, Dominator, Avenger, … ~40 entrées). À conserver tel quel (données, pas du code).

---

## 2 · hots-parser (le joyau à porter — c'est LUI le vrai actif)

Repo : ebshimizu/hots-parser (npm `hots-parser`, v7.55.x — **déjà utilisé par notre Hots-Overlay**).
3 360 lignes de `parser.js` + 446 de `constants.js`. Entrée : `.StormReplay` ; sortie :
`{match, players}` prêts pour la DB. S'appuie sur `heroprotocol` (port JS de GaryIrick/nydus).

**Ce qu'il calcule (au-delà du score screen) — la valeur différenciante :**
- Picks/bans avec ordre de draft, first pick, équipes, MMR-less.
- Takedowns enrichis : positions, participants, chain-kills, **vengeances**.
- **Objectifs par carte** : logique spécifique pour CHAQUE battleground (immortels + durée,
  dragon, araignées, autels, tributs, crânes, navires fantômes, punishers, Braxis avec **force
  de vague calculée** (`braxisWaveStrength`), temples, trônes, tours…) → timeline normalisée.
- Team fights (définition interne), stats « team fight * » par joueur.
- Timeline de niveaux/XP des 2 équipes (PeriodicXPBreakdown), level diff, **time at level adv**.
- Uptime/« hero advantage » (analyzeUptime), first fort/keep/objective.
- Taunts/BM : bsteps (détection par séquence de frames), danses, sprays, voice lines + contexte
  (près d'un ennemi, suivi d'un kill…).
- Globes ramassés, camps capturés, structures, chat, votes.
- `getHeader` (peek rapide) et `getBattletags` sans parse complet.

**Limites actuelles :** `MAX_SUPPORTED_BUILD = 87774` + option `overrideVerifiedBuild` (notre
overlay parse les builds 2026 avec override) ; synchrone (bloque l'event loop Node ~1-3 s par
replay) ; dépend du port JS de heroprotocol (protocoles par build, maintenance communautaire).

**Doc précieuse :** `docs/hots-replay-data.md` du repo SotS = **référence complète de 1 090
lignes** sur l'emplacement de chaque donnée dans le replay (game loops 16/s, offset -610,
fixedData /4096, PlayerIDs réservés 11/12, quêtes NON stockées dans le replay, etc.).
À garder comme bible pour tout nouveau parseur.

---

## 3 · Verdict dépendance par dépendance

| Dépendance 2018 | Rôle | Verdict 2026 |
|---|---|---|
| Electron 4 | shell desktop | **Supprimer** — devient un service web sur le box (notre modèle) |
| heroprotocol JS (GaryIrick) | décodage replay | **Remplacer** (voir §4) |
| hots-parser | replay → stats | **Porter** la logique (c'est la spec fonctionnelle), pas le code |
| NeDB / LinvoDB3 / medea | DB embarquée | **Remplacer** par une vraie DB (SQLite/Postgres/DuckDB) |
| jQuery / Semantic UI / Handlebars | UI | **Remplacer** (front moderne) |
| DataTables (+7 plugins) | tables | **Remplacer** (TanStack Table ou tables virtualisées maison) |
| Chart.js 2 / vis.js 4 | graphes | **Remplacer** (uPlot/ECharts/observable-plot — uPlot = le plus rapide) |
| moment.js | dates | **Remplacer** (Intl natif / date-fns / dayjs) |
| heroes-talents (heroespatchnotes) | données talents/icônes | **Garder** — on le synchronise DÉJÀ dans HotsPatchNotes |
| electron-settings/-updater/-window-state, extract-zip, fs-extra, request, node-watch, xregexp | plomberie Electron | **Disparaissent** avec Electron |
| Upload HotsAPI/HotsLogs | partage | Morts → **HeroesProfile** est l'équivalent vivant (optionnel) |

---

## 4 · Moteurs de parsing candidats (le choix structurant n°1)

| Option | État | Pour | Contre |
|---|---|---|---|
| **Blizzard/heroprotocol** (Python, officiel) | **Vivant** — release 2.55.15.96477, MAJ fév. 2026 | référence exacte, suit les patchs, protocole par build auto-généré | Python (lent, ~s/replay), juste un décodeur (aucune stat) |
| **Heroes.StormReplayParser** (HeroesToolChest, .NET) | v2.2.1 juin 2024, NuGet, MIT | **parseur natif .NET haute perf** (Span-based, projet Benchmarks), API propre (`StormReplay.Parse`), score results + awards + draft + tracker events + **parse du `.battlelobby`** (pré-game !), pas de protocole par build à régénérer ; **s'aligne sur notre stack .NET 10 (HotsPatchNotes)** | dernier release 2024 → à valider sur replays 2026 (à benchmarker sur nos fichiers réels) ; les stats dérivées (objectifs par carte, team fights, BM) restent à porter depuis hots-parser |
| **hots-parser JS** (statu quo) | utilisé par notre overlay aujourd'hui (override build) | zéro travail, sémantique connue | les perfs/maintenance qu'on fuit ; synchrone ; Node |
| **Rust from scratch** | **aucun crate HotS existant** (s2protocol-rs = SC2 uniquement) | perf max, fun | il faut réécrire MPQ + décodeur bit-packed + versionnage protocole + TOUTE la logique stats : de loin le plus long, pour un gain marginal vs .NET sur 1 replay/15 min |
| **Hybride** : décodeur existant + **moteur de stats à nous** | — | le décodage brut n'est PAS le différenciateur ; la logique hots-parser (objectifs/team fights/BM) l'est | — |

**Lecture honnête du besoin perf :** on parse 1 replay toutes les ~15 min en live + un backfill
unique de quelques milliers de replays. Même 1 s/replay suffit en live ; le backfill veut du
parallélisme (8 cœurs × 1 s ≈ 7 500 replays/15 min). Le « instantané » perçu vient surtout de
l'**architecture push** (watcher → upload dès fermeture du fichier → parse → WS → UI), pas du
langage. Candidat par défaut : **Heroes.StormReplayParser (.NET 10)** + port de la logique
hots-parser en C# (typée, testée sur replays de référence) — à confirmer par un benchmark réel
sur ~50 replays de la machine de jeu avant d'écrire la spec.

---

## 5 · Nos actifs (à réutiliser, pas à réécrire)

**Hots-Overlay** (box, Node :8086 + Mongo + extension Twitch) :
- **client-rs** : uploader Rust (egui + tray + notify + ureq, installeur Windows `installer.iss`,
  auto-update `updater.rs`) qui détecte `Documents/Heroes of the Storm/Accounts/*/*/Replays/
  Multiplayer`, watch et POST `/upload` (auth token, fingerprint anti-doublon MD5 des BlizzIDs
  + random value — même formule que HeroesProfile). **C'est exactement l'uploader demandé** ;
  il suffit de le pointer (aussi) vers le nouveau backend, ou de mettre le nouveau backend
  derrière la même route.
- Serveur : chokidar watcher (`awaitWriteFinish` 5 s — latence à raboter), parse hots-parser
  **synchrone** (à remplacer), 1 doc Mongo/match (10 joueurs + events embarqués), routes riches
  (today/sessions/recent/matches/lookup/modes/players, download du replay brut), WS overlay,
  panneau patch-digest (poussé par HotsPatchNotes), EBS Twitch (JWT).
- `docs/plans/2026-03-14-xp-graph-*.md` : travail XP graph déjà fait côté overlay.

**HotsPatchNotes / Nexus Codex** (box, .NET 10 + SQLite + Blazor WASM :5100) :
- Référentiel héros/talents/capacités (sync heroes-talents) + portraits locaux + couleurs
  d'univers + historique de patchs classifiés (BUFF/NERF/…) + design system codex complet.
- Le nouveau stat-tracker peut **consommer ce référentiel** (icônes, talents par palier,
  métadonnées héros) au lieu de re-embarquer heroes-talents, et lier chaque match au patch
  Codex correspondant (« ce match était sur le patch du 11 mai → voir les notes »).

**Jarvis** (box, FastAPI + Postgres/pgvector + Redis + voix) :
- Connecteurs `hots.py` / `hots_overlay.py` déjà en place (cartes console).
- Spine événementiel : un événement `match.completed` → brief vocal post-game, résumé stream,
  mémoire long-terme (« 3ᵉ défaite avec X sur Y aujourd'hui »).
- Le widget stream « Jarvis » = un panneau overlay (même mécanique que le patch-digest panel)
  alimenté par un endpoint du nouveau backend + (optionnel) une phrase générée par Jarvis.

**Écosystème externe :**
- **HeroesProfile** : vivant, API publique + upload (fingerprint compatible), source MMR.
  Intégration optionnelle « forward upload » comme SotS le faisait vers HotsAPI.
- **HeroesDataParser / heroes-talents** : données de jeu à jour.
- **HeroesMatchTracker** (.NET, équivalent moderne de SotS) : **archivé juil. 2023** — le
  créneau « tracker local riche » est vacant, notre projet comble un vrai vide.

---

## 6 · Axes d'architecture proposés (pour le brainstorm)

**Cible pressentie** (à challenger ensemble) :
```
PC de jeu                         box (192.168.129.85)
┌─────────────────┐   POST /upload   ┌──────────────────────────────┐
│ client-rs (tel  │ ───────────────► │ stats-api (.NET 10)          │
│ quel, re-pointé)│  + battlelobby?  │  ├ parse (StormReplayParser) │
└─────────────────┘                  │  ├ moteur stats (port hots-  │
                                     │  │  parser : objectifs, TF,  │
        WS push « match parsed »     │  │  BM, XP, uptime…)         │
┌─────────────────┐ ◄─────────────── │  ├ SQLite (1 fichier, WAL)   │
│ front stats     │                  │  │  ou Postgres du box       │
│ (SPA rapide)    │   GET /api/...   │  └ events → Redis (Jarvis)   │
└─────────────────┘                  └──────────────┬───────────────┘
┌─────────────────┐                                 │
│ overlay stream  │ ◄── widget « Jarvis post-game » ┘
│ + ext. Twitch   │      (résultat, KDA, résumé)
└─────────────────┘
```
- **Budget perf cible** : replay fermé → visible dans l'UI < 5 s (watch ~1 s + upload LAN ~0,3 s
  + parse < 1 s + agrégats SQL < 0,2 s + push WS). Backfill : parallèle par cœur.
- **DB** : agrégations = SQL (fini les summarize JS en RAM). SQLite (cohérent avec HotsPatchNotes,
  un fichier, sauvegardable) vs Postgres (déjà sur le box pour Jarvis, fenêtres analytiques plus
  riches). Schéma relationnel : matches / match_players / talents / events(timeline) / teams /
  collections — les vues = requêtes, plus de caches à invalider.
- **Front** : SPA légère (Svelte/SolidJS/React+Vite — à trancher), tables virtualisées, uPlot
  pour les timelines, design system **Nexus Codex réutilisé** (cohérence visuelle avec :5100).
- **Live/pré-game** : le `.battlelobby` est parsable (StormReplayPregame) → le widget stream
  peut afficher la compo AVANT la fin du match ; le client-rs peut watcher le fichier temp.
- **Jarvis** : événement `hots.match.completed` (Redis) → carte console, brief vocal, phrase
  stream ; ses connecteurs lisent la nouvelle API au lieu de Mongo.
- **Devenir de l'overlay actuel** : option A — le nouveau backend REMPLACE le serveur Node
  (l'overlay/Twitch ext deviennent des clients du nouveau service) ; option B — cohabitation
  (overlay garde Mongo, nouveau service à côté). À trancher (A = moins de doublons, plus de chantier).

## 7 · Questions ouvertes pour le brainstorm
1. **Moteur** : valider .NET + port de hots-parser après benchmark sur replays réels ? (vs Rust
   intégral, plus long pour un gain non nécessaire au volume « 1 joueur »)
2. **DB** : SQLite ou le Postgres du box ?
3. **Front** : quelle techno SPA, et reprend-on le design Nexus Codex tel quel ?
4. **Overlay** : remplacement (A) ou cohabitation (B) avec le serveur Node actuel ?
5. **Périmètre v1** : solo-first (tes stats) d'abord, ligues/équipes (teams/collections de SotS)
   en v2 ? Les fonctions « ligue » de SotS sont riches mais servent surtout les organisateurs.
6. **Multi-joueurs ?** un seul compte (toi) ou multi-comptes/famille ?
7. **HeroesProfile forward-upload** : oui/non ?
8. **Backfill** : taille réelle de ton archive de replays sur le PC de jeu ? (dimensionne le
   premier import et le benchmark)
9. **Widget stream** : quelle granularité ? (résultat+KDA+awards à chaud, ou aussi des phrases
   Jarvis générées, comparaisons à ta moyenne, série de victoires…)

## Références
- SotS : https://github.com/ebshimizu/stats-of-the-storm (clone local : /tmp/sots)
- hots-parser : https://github.com/ebshimizu/hots-parser (clone local : /tmp/hots-parser)
- Réf. données replay : /tmp/sots/docs/hots-replay-data.md (1 090 lignes, à archiver avec la spec)
- heroprotocol officiel : https://github.com/Blizzard/heroprotocol (2.55.15.96477, fév. 2026)
- Heroes.StormReplayParser : https://github.com/HeroesToolChest/Heroes.StormReplayParser (v2.2.1)
- HeroesDecode / HeroesDataParser : https://github.com/HeroesToolChest
- HeroesMatchTracker (archivé 2023) : https://github.com/HeroesToolChest/HeroesMatchTracker
- HeroesProfile API/upload : https://api.heroesprofile.com/ · https://github.com/Heroes-Profile
- heroes-talents : https://github.com/heroespatchnotes/heroes-talents
