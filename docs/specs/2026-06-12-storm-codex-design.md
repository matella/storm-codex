# Storm Codex — rénovation complète de Stats of the Storm

**Date :** 2026-06-12 · **Statut :** design validé en session (brainstorm complet), spec à relire.
**Recherche source :** `docs/research/2026-06-12-stats-of-the-storm-renovation.md` (+ référence
format replay : `docs/research/hots-replay-data-reference.md`). À lire avant toute implémentation.
**Maquettes :** `docs/superpowers/specs/2026-06-12-storm-codex-mockup.html` (tous les écrans).

## Vision

Réécrire [stats-of-the-storm](https://github.com/ebshimizu/stats-of-the-storm) (tracker de stats
HotS, Electron/jQuery/NeDB 2018) en un service moderne : **cœur de parsing Rust open-source**,
serveur unique sur le box, front réutilisant le design system Nexus Codex, intégré à notre
écosystème (uploader client-rs existant, référentiel HotsPatchNotes, événements Jarvis, overlay
stream/Twitch). Parité fonctionnelle **totale** avec SotS en V1, plus : temps réel (fin de partie
→ page à jour < 5 s), widget stream post-game, backfill de plusieurs années de replays en minutes.

## Décisions actées (brainstorm 2026-06-12)

| # | Décision |
|---|---|
| 1 | **Moteur Rust** (perf + crates publiables ; aucun parseur HotS Rust n'existe → contribution réelle). Repli .NET (Heroes.StormReplayParser) si le spike échoue. |
| 2 | **PostgreSQL** (agrégations SQL, écritures concurrentes, multi-tenant V2-ready). |
| 3 | Front : **design system Nexus Codex** réutilisé (tokens, badges, anneaux d'univers). |
| 4 | Le nouveau serveur **remplace** le serveur Node/Mongo **local** de Hots-Overlay (décommission en fin de V1) ; overlay OBS re-pointé ; l'extension Twitch reste servie par l'EBS Azure existant, alimenté par push box→Azure (absorption EBS = V2). |
| 5 | **V1 = parité fonctionnelle complète** avec SotS, y compris ligues/équipes/collections. |
| 6 | Pas de mode pré-game (`.battlelobby`) en V1. |
| 7 | Scope **C** : V1 auto-hébergée (box), code open-source dès le départ, architecture prête pour un hébergement public V2 (sans l'activer). |
| 8 | Upload **B** : multi-tokens nominatifs dès la V1 (en pratique : un seul utilisateur au début). |
| 9 | Données : **tout conserver** via 3 étages (fichier brut = source de vérité ; Postgres = projection complète ; dump décodé à la demande avec cache LRU — PAS de pré-décodage intégral à l'upload). |
| 10 | Nom : **Storm Codex** — crates `storm-replay`, `storm-stats`, binaire `storm-codex-server`. |

## Architecture

```
PC de jeu (Windows)                      box 192.168.129.85
┌────────────────────┐  POST /upload   ┌─────────────────────────────────────┐
│ client-rs (existant│ ───────────────►│ storm-codex-server (Rust, axum)     │
│ re-pointé + mode   │     token       │ ├ pool de workers parse             │
│ backfill)          │                 │ │   storm-replay → storm-stats      │
└────────────────────┘                 │ ├ PostgreSQL (projections)          │
                                       │ ├ archive replays (volume/NAS)      │
   navigateur ◄── WS push ─────────────│ ├ REST + WebSocket                  │
   (SPA React, pages parité SotS)      │ ├ sert le front buildé (1 binaire)  │
                                       │ ├ endpoints overlay/Twitch (remplace│
   OBS (widget stream) ◄── WS ─────────│ │  le serveur Node)                 │
                                       │ └ events → Redis ─► Jarvis          │
                                       └─────────────────────────────────────┘
```

## Composants

### Crate `storm-replay` (open-source, crates.io)
Décodage `.StormReplay` : lecteur MPQ + décodeur bit-packed/versionné (machinerie éprouvée par
s2protocol-rs côté SC2 — même moteur de sérialisation Blizzard) + **tables de protocole générées
automatiquement** depuis [Blizzard/heroprotocol](https://github.com/Blizzard/heroprotocol)
(maintenu, release fév. 2026) par un script `protocol-gen` (commité, relançable à chaque patch HotS).
- API : `Replay::open(path)` → accès typé à `header`, `details`, `initdata`, `attributes`,
  `messages`, `trackerevents`, `gameevents` (décodage paresseux par stream).
- Builds inconnus : fallback « dernier protocole connu » + warning (comportement heroprotocol).
- Tests : corpus de replays de référence multi-builds (2023→2026) hébergé sur le NAS/box avec un
  script `fetch-corpus` (pas de Git LFS — les repos publics restent légers ; un mini-corpus de
  2-3 replays libres est commité pour le CI public).

### Crate `storm-stats` (open-source, crates.io)
Port complet de la logique [hots-parser](https://github.com/ebshimizu/hots-parser) (3 360 lignes
JS analysées — voir dossier de recherche) : replay décodé → `MatchStats` typé.
- Périmètre : picks/bans + ordre de draft ; score screen intégral (~80 stats/joueur) ; awards ;
  takedowns enrichis (positions, participants, vengeances) ; **timeline d'objectifs spécifique aux
  16 cartes** (immortels+durées, dragon, araignées, tributs, autels, crânes, navires, punishers,
  Braxis avec force de vague, temples, trônes, mines, graines, gemmes, protecteur) ; team fights ;
  XP/niveaux périodiques + level advantage/uptime ; first objective/fort/keep ; taunts/BM (bsteps,
  danses, sprays, voicelines + contexte) ; messages/pings ; votes ; globes ; camps.
- **Validation de parité** : harnais qui exécute hots-parser (Node) et storm-stats sur le même
  corpus et diff les sorties champ par champ (tolérances documentées). Aucun jalon stats n'est
  « fini » sans ce diff vert.

### `storm-codex-server` (binaire Rust, axum, Docker sur le box)
- **Upload** : `POST /api/upload` (Bearer token nominal) → fingerprint anti-doublon (même formule
  MD5 BlizzIDs+random que l'existant/HeroesProfile) → enregistrement fichier dans l'archive →
  job de parse en pool (jamais sur le thread HTTP) → transaction Postgres → événement.
  **Cycle de vie d'un échec de parse** : le fichier est TOUJOURS archivé d'abord ; un échec
  (fichier corrompu, panic isolé du worker, erreur de logique stats) marque la ligne `uploads`
  en `failed` avec une classe d'erreur typée + message, visible dans Admin/santé, et
  re-tentable via le re-process (idempotent, piloté par `parser_version`). Aucun échec ne bloque
  la file. **Sémantique de réponse** : la requête attend le résultat du parse jusqu'à 2 s
  (typique < 0,5 s) et renvoie le statut final (`parsed` / `duplicate` / `parse_failed`) ; si le
  pool est saturé (backfill), elle renvoie immédiatement `202 {status:"accepted"}` et l'issue
  est observable via WS/Admin. Le client n'a jamais rien à refaire.
- **Lecture** : endpoints REST pour toutes les pages (matchs, match, joueur, héros, trends,
  classements, équipes/ligues, collections, cartes, sessions, exports CSV/JSON).
- **Temps réel** : WebSocket `/ws` — push `match.parsed` (et progression backfill) à tous les
  clients (site, overlay OBS, extension Twitch).
- **Dump intégral** : `GET /api/matches/{id}/raw?stream=…` — décode le fichier archivé à la volée,
  cache disque compressé **LRU borné** (config `RAW_CACHE_MAX_BYTES`, défaut 5 GB) ; jamais de
  pré-décodage massif.
- **Overlay/Twitch** : API **nouvelle et propre** (pas de compatibilité route-à-route avec le
  serveur Node) ; on possède tous les clients, donc la page overlay OBS et la config de
  l'extension Twitch sont re-pointées/adaptées lors du jalon 5. Capacités reprises : résumé
  dernière partie/session, lookup matchs récents, panneau patch-digest (le contrat de push
  HotsPatchNotes → serveur est conservé tel quel, même token/format).
  **Twitch en V1** : l'extension tourne dans le navigateur des viewers (hors LAN) et reste servie
  par l'instance overlay **Azure** existante ; le box lui POUSSE les résumés post-game via le même
  mécanisme authentifié que le patch-digest (push sortant box→Azure, rien d'entrant). La
  validation JWT Twitch reste donc dans l'EBS Azure ; son absorption par storm-codex-server est
  un chantier V2 (hébergement public). Seul le serveur Node **local** est décommissionné en V1.
- **Jarvis** : publie `hots.match.completed` (payload : résultat, héros, KDA, carte, durée,
  awards, écarts vs moyennes) sur le Redis du box — l'événement respecte les invariants Jarvis :
  `schema_version`, `correlation_id`/`causation_id`, `occurred_at`/`recorded_at`,
  type `entity.verb` au passé.
- **Admin** : gestion tokens, re-process (tout ou par filtre, idempotent, suit `parser_version`),
  état backfill, santé.
- Auth V1 : tokens d'upload nominatifs ; UI en lecture sur LAN/Tailscale (pas de comptes web).
  V2-ready : `uploader_id` partout, serveur stateless, config par env.

### Migration & décommission (décision explicite)
**Aucune migration de données.** L'historique de matchs se reconstruit intégralement par
backfill depuis les replays bruts (supérieur à toute migration : parser moderne, stats
complètes). Les définitions non dérivables des replays (équipes/ligues/collections SotS dans
NeDB, éventuels matchs de test dans le Mongo de l'overlay — vide à ce jour : 0 replays uploadés)
ne sont **pas** importées : elles se recréent à la main dans la nouvelle UI (opérateur unique,
volume négligeable). Le Mongo est inspecté une dernière fois avant arrêt, puis supprimé.

### Référentiel héros/talents (source unique)
Le front et storm-stats ont besoin des métadonnées héros (noms, rôles, univers, portraits,
talents par palier + icônes). Source unique : **l'API HotsPatchNotes** (:5001, même box), qui
synchronise déjà heroes-talents + images locales. storm-codex-server les réplique à son
démarrage (et sur demande admin) dans des tables de dimension Postgres (`dim_heroes`,
`dim_talents`) — pas d'appel inter-service sur le chemin chaud, pas de double pipeline de sync.

### Front (SPA React + Vite + TS, servie par le binaire)
- TanStack Query (cache + invalidation par WS), TanStack Table + Virtual (les « big tables » de
  SotS sans DataTables), uPlot (timelines XP/objectifs — le plus rapide), design system Nexus
  Codex (tokens, `.bdg`, kickers, anneaux d'univers, hairlines).
- Pages (parité SotS 1:1 + ajouts) : Tableau de bord/session · Matchs (liste filtrable temps
  réel) · Détail de match (score 10 joueurs, draft ordonné, XP/objectifs/team fights timeline,
  taunts/BM, chat, votes) · Joueur (résumé, progression, hero pool, détail héros, talent builds,
  duos avec/contre, awards, big tables) · Héros/collection · Trends par patch (lien Nexus Codex)
  · Classements joueur/équipe · Équipes & Ligues (rosters, collections) · Cartes · Admin/Import ·
  Exports CSV/JSON. Détail visuel : voir le fichier de maquettes.

### client-rs (évolutions légères de l'existant)
- Re-pointage URL + token vers storm-codex-server (config existante).
- **Mode backfill** : scan complet de l'archive (pas seulement les nouveaux fichiers), upload en
  rafale throttlée, reprise sur interruption, barre de progression dans l'app tray.
- Latence live : abaisser la stabilisation fichier (~5 s aujourd'hui → ~1 s).

## Modèle de données (3 étages, « tout garder »)

1. **Archive brute** : chaque `.StormReplay` conservé tel quel (volume box, sauvegardable NAS).
   Source de vérité — toute stat future est récupérable par re-process (< 150 ms/replay).
2. **Postgres (projection complète)** — chaque match porte `parser_version` :
   `matches` (build, mode, carte, durée, date, équipes, winner, firsts, fingerprint, patch Codex)
   · `match_players` (10/match : héros, score screen intégral, awards, niveau, déco)
   · `talents` (joueur×palier + timestamp) · `draft` (picks/bans ordonnés)
   · `timeline_events` (JSONB typé : takedowns, objectifs, structures, camps, XP périodique,
   team fights, taunts, messages/pings, votes)
   · `players` (ToonHandle, alias/tags) · `teams`/`leagues` (rosters) · `collections`
   · `uploads`/`upload_tokens` (traçabilité par uploader).
   Index sur les axes de filtre (carte, héros, joueur, date, patch, collection). Vues/maté-vues
   pour les agrégats chauds (hero stats, trends par patch, duos, classements).
3. **Dump décodé à la demande** : endpoint `…/raw` + cache LRU (cf. serveur). Analyses de masse
   éventuelles (heatmaps game events) : export Parquet ponctuel + DuckDB, hors chemin chaud.

## Budgets de performance (vérifiés, pas déclarés)

| Étape | Budget |
|---|---|
| Détection fichier → upload reçu (LAN) | < 2 s (stabilisation ~1 s + transfert ~0,3 s) |
| Parse complet (decode + stats) | **< 150 ms** /replay (1 cœur) — go/no-go spike : < 500 ms |
| Écriture Postgres + push WS | < 200 ms |
| **Fin de partie → page à jour (sans F5)** | **< 5 s** |
| Backfill archive 3 ans (~3 000 replays, 2–3 GB) | < 5 min de bout en bout (workers = nb cœurs) |
| Pages : requête API p95 | < 100 ms (agrégats indexés/maté-vues) |

## Risques & parades

- **Dérive de protocole** (nouveaux builds HotS) : `protocol-gen` relançable depuis le repo
  Blizzard ; fallback dernier protocole ; le corpus de tests inclut chaque nouveau build.
- **Parité stats** : le harnais de diff vs hots-parser est un **livrable du jalon 2**, pas une
  option. Les écarts assumés (bugs hots-parser corrigés) sont documentés.
- **Spike no-go** : repli .NET (Heroes.StormReplayParser + port stats C#), architecture identique.
- **Box éteint la journée** : acceptable en V1 (usage perso le soir) ; V2 public = hébergement
  always-on (hors scope V1, l'architecture stateless le permet).
- **Gros volumes V2** : fingerprint + `uploader_id` + Postgres → mutualisation déjà prévue.

## Jalons V1 (chacun : livrable + critère d'acceptation)

> **Cadrage plans :** ce document est le design *programme*. Chaque jalon ci-dessous fait
> l'objet de **son propre plan d'implémentation** (writing-plans), dans l'ordre — pas un plan
> unique pour l'ensemble.

0. **Spike go/no-go (1–2 j)** — décoder 3 replays réels (builds ≥ 2024) en Rust ; bench 50
   replays Rust vs .NET vs Python. *Accept : décode complet < 500 ms/replay, champs nécessaires
   présents.* Sinon : bascule plan B documentée.
1. **storm-replay** — décodage des 7 streams, protocol-gen, tests corpus multi-builds.
   *Accept : 100 % du corpus décodé, bench < 150 ms hors stats.*
2. **storm-stats** — port hots-parser complet + harnais de parité.
   *Accept : diff vs hots-parser vert sur le corpus (tolérances documentées).*
3. **Serveur + DB + backfill** — upload/fingerprint/workers/Postgres/WS/raw+LRU/admin/tokens ;
   mode backfill client-rs. *Accept : 100 % de l'archive archivée et tentée ; ≥ 99 % parsée
   (échecs listés et classés dans Admin — les builds alpha pré-2015 peuvent légitimement
   échouer) ; fin de partie → page < 5 s.*
4. **Front parité** — toutes les pages, design Codex, temps réel.
   *Accept : chaque fonctionnalité SotS retrouvable (checklist par page), p95 API < 100 ms.*
5. **Stream + Jarvis + bascule** — widget OBS, `hots.match.completed` → Redis, connecteur Jarvis,
   push post-game box→Azure pour l'extension Twitch, **décommission du serveur Node/Mongo local**
   (l'EBS Azure reste, cf. § Overlay/Twitch). *Accept : partie jouée → widget OBS + brief Jarvis
   + extension Twitch à jour ; serveur Node local arrêté sans perte de fonction.*
6. **Publication** — crates.io + READMEs + docs (repos GitHub publics).

## Hors scope V1
Pré-game/battlelobby · comptes web/quotas (V2) · hébergement cloud public (V2) · MMR calculé
(HeroesProfile reste la référence ; forward-upload optionnel à décider plus tard) · heatmaps
game-events (export Parquet ponctuel si besoin).
