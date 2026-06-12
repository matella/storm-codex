# Jalon 2 — crate `storm-stats` : port de hots-parser + harnais de parité

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal :** un crate `crates/storm-stats` qui transforme un `storm_replay::Replay` en
`MatchStats` (= la sortie `{match, players}` de hots-parser), avec **diff automatique champ par
champ contre hots-parser 7.55.7 (Node)** sur un corpus couvrant les 16+ cartes.
**Accept (spec) : diff vert sur le corpus, tolérances documentées.**

**Architecture :** parser.js (3 360 lignes) est la **spec fonctionnelle** — on porte sa
sémantique phase par phase, bugs compris (un écart volontaire = tolérance documentée, jamais
silencieuse). La sortie Rust sérialise (serde) exactement la forme JSON de hots-parser pour
rendre le diff trivial ; la projection Postgres du jalon 3 se fera depuis le type Rust, pas
depuis ce JSON. Le harnais (`tools/parity-harness/`) : `dump.js` (réf. Node, **fait**, écrit
dans un fichier car pino pollue stdout) + `storm-stats-dump` (Rust) + `diff.py` (deep-compare,
liste d'exclusions = LES tolérances, cache des dumps de référence — Node met ~1-3 s/replay).

**Réfs :** `%TEMP%\hots-parser\parser.js` (lire la phase AVANT de la porter),
`constants.js` (446 lignes : unités par carte, awards, talents…, à transposer en
`constants.rs` généré ou écrit), `docs/research/hots-replay-data-reference.md` (bible du
format), sortie réelle : 33 clés `match`, ~40 clés `player` (relevées en session).

---

### Task 1 : corpus stats par carte + squelette crate + harnais diff

- `spike/sample_stats_corpus.ps1` : depuis l'archive locale, ≥ 3 replays par carte distincte
  (nom de carte = fin du nom de fichier) + le corpus spike50 → `corpus/stats/` (~100 fichiers).
- `crates/storm-stats` (workspace) : deps `storm-replay` (path), `serde`/`serde_json`,
  `thiserror` ; bin `storm-stats-dump <replay> <out.json>`.
- `tools/parity-harness/diff.py` : `--corpus <dir>` → pour chaque replay, dump Node (caché dans
  `corpus/stats/.ref/`) + dump Rust, deep-compare avec : tolérance floats 1e-6, `null` ≡ absent,
  exclusions lues dans `tools/parity-harness/tolerances.json` (chemins JSON + raison) ;
  `--only <préfixes>` pour le diff progressif pendant le port.
- Commit.

### Task 2 : identité du match + des joueurs (phase 1 de processReplay)

Porter : header/details/initdata/attributes → `match.{version,type,loopLength,filename,mode,
map,date,rawDate,region,playerIDs,heroes,loopGameStart,length}` + `players.{hero,name,uuid,
region,realm,ToonHandle,tag,team,build,mode,version,map,date,rawDate,length,win,
internalHeroName,heroLevel,skin,announcer,mount,silenced,voiceSilenced}`.
Attention : `attr.js` (mapping attributs → mode), date = winFileTimeToDate (offset -610 s,
cf. bible), tag depuis battletags (getBattletags lit l'initdata). Diff `--only` vert. Commit.

### Task 3 : score screen + talents + draft

- `processScoreArray` (SScoreResultEvent → `gameStats` ~80 stats + awards), talents par palier
  (game events SHeroTalentTreeSelectionEvent + noms via tracker), `globes`, `votes`.
- Draft : `match.bans` (SHeroBannedEvent), `match.picks` (SHeroPickedEvent ordonnés),
  `firstPickWin`, `turn`. Diff vert. Commit.

### Task 4 : pipeline d'unités tracker (le gros morceau)

Boucle UnitBorn/UnitDied/UnitOwnerChange… : takedowns enrichis (positions, participants,
chain kills, vengeances), `deaths`, `lifespan`, `mercs` (camps), `structures`,
`levelTimes`, `XPBreakdown` (PeriodicXPBreakdown), `computeLevelDiff`/`analyzeLevelAdv` →
`levelAdvTimeline`, `firstFort/firstKeep(+Win)`, `team0/1Takedowns`. Diff vert. Commit.

### Task 5 : objectifs par carte (16 branches)

Le switch par carte de processReplay → `match.objective` normalisé (immortels+durée, dragon,
araignées, tributs, autels, crânes, navires, punishers, **braxisWaveStrength**, temples,
trônes, mines, graines, gemmes, protecteur) + `getFirstObjectiveTeam` → `firstObjective(Win)`.
Chaque carte validée par diff sur ses replays du corpus. Commit (possiblement plusieurs).

### Task 6 : game events — taunts/BM, messages, with/against

`processTauntData` : bsteps (séquences de frames, BSTEP_FRAME_THRESHOLD=8), danses, sprays,
voicelines, taunts + contexte (près d'ennemi, kill proche) ; `match.messages` (chat/pings) ;
`with`/`against` ; `units`. Diff vert. Commit.

### Task 7 : stats d'équipe + uptime

`collectTeamStats` → `match.teams`, `analyzeUptime`/`analyzeTeamPlayerUptime`/
`timeWithHeroAdv`. Diff vert. Commit.

### Task 8 : diff intégral + bench + finitions

- `diff.py --corpus corpus/stats` **sans `--only`** : vert (exclusions = tolérances
  documentées dans tolerances.json avec raison ; les recopier dans le README du crate).
- Bench : decode + stats sur corpus/spike50, mono-thread — **budget spec < 150 ms/replay
  médiane** (decode seul = 102 ms ; stats doivent tenir dans ~45 ms).
- README crate, STATUS.md (jalon 2 fait), commit + **push**.

---

## Pièges connus

- hots-parser fige `version: 7` (PARSER_VERSION) et des champs legacy — reproduire tel quel.
- `undefined` JS → sérialisé `null` par dump.js : le diff traite null ≡ absent.
- Ordres d'itération JS (objets = insertion) : les tableaux doivent matcher exactement,
  les objets se comparent par clés.
- Floats : positions /4096 (fixedData), pourcentages — tolérance 1e-6, pas plus.
- ARAM/brawl : certaines cartes n'ont pas de logique objective dans hots-parser — la sortie
  doit être identiquement vide, pas absente.
- Node : toujours `dump.js` vers fichier (pino écrit sur stdout).
