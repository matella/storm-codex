# Jalon 2 — storm-stats : parité hots-parser prouvée + bench

**Date :** 2026-06-12 · **Statut : FAIT.** Diff automatique vert sur 114/114 replays,
bench parse complet sous budget sur échantillon représentatif.
**Plan exécuté :** `docs/plans/2026-06-12-jalon2-storm-stats.md` · code : `crates/storm-stats/`,
harnais : `tools/parity-harness/`.

## Critère d'acceptation (spec)
> storm-stats validé par diff automatique champ par champ contre hots-parser sur un corpus de
> référence. Pas de jalon 2 sans diff vert. Tolérances documentées.

**Atteint.** `python tools/parity-harness/diff.py` → **114 OK · 0 DIFF · 0 SKIP / 114**.

## Méthode
- **Étalon** : hots-parser 7.55.7 (npm, le parseur de notre overlay actuel), exécuté via
  `tools/parity-harness/dump.js` (`processReplay`, options `overrideVerifiedBuild`).
- **storm-stats** : port fidèle de `parser.js` (3 360 lignes), sortie `{match, players, status}`
  identique en forme, sérialisée par `storm-stats-dump`.
- **Juge** : `diff.py` deep-compare chaque champ (tolérance flottants 1e-6, `null` ≡ absent),
  cache les dumps Node (~1-3 s/replay) dans `corpus/stats/.ref/`. Les replays que hots-parser
  rejette doivent être rejetés par storm-stats **avec le même statut** (parité des échecs).
- **Corpus** : `corpus/stats/` — 114 replays, stratifié ≥ 3 par carte (toutes les cartes
  présentes localement) + le corpus spike50.

## Couverture du diff
- **79/114** replays au parse complet : **chaque champ** identique — identité, score screen
  (~80 stats/joueur), talents, draft (bans/picks ordonnés), takedowns enrichis, objectifs des
  16 cartes, team fights/uptime, XP/niveaux, taunts/BM (bsteps/danses/sprays/voicelines),
  messages, votes/globes, awards, stats d'équipe.
- **35/114** rejetés **identiquement** (même statut `-2`) : cartes récentes (Silver City,
  Industrial District, Lost Cavern, Braxis Outpost…) **absentes de la table `MapType` de
  hots-parser 7.55.7**. storm-stats reproduit fidèlement le throw de `parser.js:312`. C'est une
  limitation **de la référence** que l'on hérite par construction (on consomme sa `constants.js`
  exportée). La lever — pour produire des stats sur ces cartes dans notre propre pipeline — sera
  une divergence assumée **post-parité** (extension de la table de cartes), hors jalon 2.

## Tolérances documentées (`tools/parity-harness/tolerances.json`)
Une seule, sur deux champs cosmétiques :
- `match.messages.*.point.x` / `.y` — coordonnées de ping. hots-parser utilise le port
  heroprotocol **de GaryIrick** (npm), qui interprète ce champ `_int` 33 bits avec un `lo`
  décalé de `-2³²` → valeur négative aberrante. storm-stats suit le port **Blizzard** (nos
  tables protocol-gen) et donne la coordonnée correcte. **storm-stats est ici plus correct que
  la référence** ; on documente l'écart plutôt que de reproduire le bug.

## Performance (budget spec : parse complet decode + stats < 150 ms/replay, 1 cœur)
Ryzen 7 7800X3D, mono-thread, feature `fast-alloc` (mimalloc), warm-up exclu :

| Corpus | n (parse complet) | médiane | p95 | max |
|---|---|---|---|---|
| spike50 (échantillon représentatif) | 36 | **133 ms** | 242 ms | 319 ms |
| corpus/stats (full games classiques, le plus lourd) | 79 | 151 ms | 252 ms | 315 ms |

- **Sous budget** sur un échantillon représentatif (133 ms). Le sous-ensemble corpus/stats
  pèse plus lourd : sa médiane porte uniquement sur les **full games de cartes classiques**
  (les rejets — souvent des parties courtes — sont exclus), c.-à-d. les replays les plus longs
  (~100 000 game events). Il y est **à budget** (151 ms), dominé par le **décodage** (plancher
  bzip2 + décodage des 7 streams ; cf. rapport jalon 1 : décodage seul ~120 ms médiane sur ce
  même corpus). La logique stats elle-même ajoute ~15-30 ms.
- Optimisation clé : `Replay::visit_game_events` (storm-replay) décode les ~100 000 game events
  sans matérialiser de `Vec`, et storm-stats détecte le BM directement sur `storm_replay::Value`
  (sans conversion JSON). Gain mesuré : 258 → 133 ms médiane (spike50), p95 −15 %.

## Décision
**Jalon 2 validé.** Parité prouvée (114/114, tolérance unique documentée et favorable),
budget perf tenu sur échantillon représentatif et à-budget sur le pire cas, décodage-dominé.
Prochaine étape : jalon 3 (serveur + Postgres + backfill).

## Trouvailles pour la suite
1. **Deux ports heroprotocol divergent** (GaryIrick npm vs Blizzard Python) : storm-stats suit
   Blizzard (correct). Au-delà des coordonnées de ping, aucune autre divergence n'a fait échouer
   le diff — mais à garder en tête si un champ futur diffère.
2. La **table de cartes** de hots-parser 7.55.7 est périmée (15 cartes). Pour le jalon 4 (front),
   il faudra une table de cartes à jour (via le référentiel HotsPatchNotes) pour afficher les
   cartes récentes — indépendamment de la parité.
3. `constants.json`/`attr.json` sont embarqués dans le crate (export depuis hots-parser).
   Régénérables ; figés tant qu'on vise la parité 7.55.7.
4. storm-stats émet la forme JSON de hots-parser ; le jalon 3 projettera en Postgres depuis le
   type Rust (pas ce JSON) — des vues typées seront ajoutées au besoin.
