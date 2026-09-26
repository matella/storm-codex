# Rapports de scouting — plan d'implémentation

> Spec : `docs/specs/2026-09-26-rapports-scouting-design.md` (validée le 2026-09-26, volet A
> confirmé par l'opérateur : drafts de 3 replays perso conformes).
> Steps en cases à cocher (`- [ ]`).

**Goal :** l'opérateur dépose les replays d'une équipe adverse, storm-codex calcule des faits
chiffrés, produit un pack LLM (`.md` + `.xlsx`), et réimporte une analyse JSON à format imposé
dont chaque affirmation est adossée aux faits.

**Architecture :** module serveur `crates/storm-codex-server/src/scouting/` — cœur **pur** (zéro
I/O, testé unitairement comme le moteur de draft) + une couche `api.rs` mince (sqlx/axum) :

| Fichier | Rôle | Pur ? |
|---|---|---|
| `summary.rs` | `Output` storm-stats → `GameSummary` compact (équipes, héros, bans, picks, stats, niveaux, objectifs) | oui |
| `side.rs` | candidats roster, ambiguïté, côté cible (roster ≥ 3 → ancre → manuel) | oui |
| `facts.rs` | `GameSummary[]` + côtés + roster → `Facts` (sections + index plat `id → valeur`) | oui |
| `pack.rs` | `Facts` → Markdown (prompt + définitions + faits + format de réponse) | oui |
| `xlsx.rs` | `Facts` → classeur `.xlsx` (rust_xlsxwriter, `save_to_buffer`) | oui |
| `analysis.rs` | texte LLM → JSON extrait → validé (serde tolérant) → preuves résolues | oui |
| `api.rs` | routes, persistance, recalcul des faits à chaque mutation | non |
| `prompt.md`, `analysis.schema.json` | embarqués (`include_str!`) | — |

**Écart assumé vs la spec (documenté dans la spec au même commit) :** `scouting_games` reçoit une
colonne `summary JSONB` en plus de `data`. Le recalcul des faits à chaque ajout de replay ne lit que
les résumés (quelques Ko) au lieu de recharger N objets `match` complets (dizaines de Mo cumulés sur
un dépôt de 30 fichiers) ; `data` reste la projection complète (re-process, volet F).

**Tech :** axum 0.8, sqlx 0.8, serde ; `rust_xlsxwriter` 0.93 (doc Context7 : `Workbook::new`,
`add_worksheet().set_name`, `write_with_format`, `save_to_buffer`) ; front React 18 + TanStack v5,
vitest (env node).

## Contraintes globales

- **Isolation** : aucune requête existante sur `matches`/`match_players` ne change ; aucune écriture
  du module scouting dans ces tables.
- Rôle d'un héros : `dim_heroes.role` (le `gameStats.Role` du parser vaut 0 pour tous les joueurs
  sur les parties perso — vérifié sur les 3 replays), chargé par `api.rs` et passé au cœur pur.
- Codes de ban → nom : `storm_stats::constants::hero_attribute()`.
- Rythme des faits : taux toujours `k/n` + %, `low = n < 3`.
- Ids de faits : `[a-z0-9_.]`, slug des noms (`Braxis Holdout` → `braxis_holdout`,
  `Anub'arak` → `anubarak`).
- Pas de `unwrap()` hors tests, clippy strict, erreurs typées ; UI en anglais, commentaires en
  français.
- Tests DB : ignorés sans `DATABASE_URL` (patron `api_tests`), lancés contre le Postgres Docker dev.
- Les 3 replays de l'opérateur (parties de tiers) **ne sont pas committés** : tests unitaires sur
  données synthétiques + replay committé `Industrial District` ; vérification réelle en dev.

## Tâche 1 — Migration + résumé de partie (volet B)

- [ ] `migrations/0010_scouting.sql` : tables de la spec + `anchors JSONB`, `target_source TEXT`,
      `summary JSONB NOT NULL` ; index `(report_id)`.
- [ ] `summary.rs` : `GameSummary { date, map, build, length_s, winner, teams: [[Player;5];2],
      bans: [Vec<Ban{hero, phase}>;2], picks: [Vec<String>;2], first_pick, first_fort,
      first_objective, level10_time: [Option<f64>;2], level_diff: Vec<(start,end,diff)> }` avec
      `Player { toon, name, hero, stats }` (K/D/A, HeroDamage, SiegeDamage, Healing, SelfHealing,
      DamageTaken, ExperienceContribution, TimeSpentDead, MercCampCaptures, KillParticipation).
      Tests : replay committé (forme, 10 joueurs, champs présents) ; ban `Crus` → `Johanna`.
- [ ] `06-modele-donnees.md` mis à jour ; migration appliquée sur le Postgres dev.

## Tâche 2 — Côté cible (volet B)

- [ ] `side.rs` : `candidates(games, anchors)` (≥ 50 % des parties, ou des parties de l'ancre, du
      côté de l'ancre), `ambiguous` = des candidats se sont affrontés dans une même partie (lot de
      séries contre une même équipe → l'ancre est requise), `detect(teams, roster, anchors,
      manual)` → `Option<(side, Source)>`.
- [ ] Tests : ≥ 3 du roster ; repli ancre ; manuel conservé ; ancre des deux côtés → `None` ;
      série BO3 contre la même équipe sans ancre → ambigu ; avec ancre → roster = 5 de l'ancre.

## Tâche 3 — Faits (volet C)

- [ ] `facts.rs` : sections `overview`, `players` (`p1…pN`, cœur puis remplaçants, héros, rôles,
      moyennes /min), `maps` (bilan + Wilson 95 %, picks, bans faits/subis), `draft` (taux de first
      pick, bans 1re phase / milieu, bans subis, premier pick de l'équipe, rôle du premier pick),
      `flow` (premier à 10, premier fort, premier objectif, écart de niveau moyen à 10/15/20 min,
      tranches de durée, remontées/écroulements à ±2 niveaux), `games` (une ligne par partie) +
      `index: BTreeMap<id, {label, text, k?, n, low}>`.
- [ ] Tests : bilans, Wilson (valeurs de référence), exclusion des parties sans côté, remplaçants,
      `n` partout, ids uniques et bien formés.

## Tâche 4 — API + dépôt (volet B/C)

- [ ] `api.rs` : routes de la spec (+ `PATCH /api/scouting/{id}/replays/{gid}` pour le côté
      manuel) ; dépôt = archive `ARCHIVE_DIR/scouting/<sha256>` → parse (pool `parse_sem`,
      `spawn_blocking`) → résumé → insert → recalcul ; toute mutation → `recompute(report_id)`
      (`facts`, `facts_version + 1`, `target_team` des parties non manuelles).
- [ ] `POST /api/admin/reprocess` couvre `scouting_games` (`parser_version` périmé) puis recalcule.
- [ ] Tests d'intégration : cycle création → dépôt → faits → suppression ; **isolation** :
      `/api/heroes`, `/api/maps` identiques avant/après dépôt ; doublon → statut `duplicate`.
- [ ] `05-api.md`, `04-serveur.md`.

## Tâche 5 — Pack (volet D)

- [ ] `prompt.md` (anglais), `analysis.schema.json` ; `pack.rs` : prompt + définitions + table
      `pN` → nom + faits en tableaux Markdown (colonne id) + schéma + exemple + `report_id` /
      `facts_version`.
- [ ] `xlsx.rs` : onglets README, Overview, Players, Heroes, Maps, Draft, Flow, Games.
- [ ] Routes `pack.md` / `pack.xlsx` (Content-Disposition). Tests : le Markdown contient chaque id
      de l'index ; le classeur se génère (octets non vides, signature ZIP).

## Tâche 6 — Import de l'analyse (volet E)

- [ ] `analysis.rs` : extraction (bloc ```json, sinon premier objet équilibré, en ignorant les
      accolades dans les chaînes), validation (`format_version == 1`, `report_id`), structures serde
      tolérantes, `resolve(analysis, index)` → preuves `{id, text}` + `unsupported`.
- [ ] `PUT/DELETE /api/scouting/{id}/analysis` ; `GET /api/scouting/{id}` renvoie l'analyse avec
      preuves résolues contre les faits **courants** + statut dérivé (`empty/ready/analyzed/stale`).
- [ ] Tests : fenced, prose autour, JSON cassé → erreur et rien d'écrasé, `report_id` faux → refus,
      id inconnu → `unsupported`, `facts_version` ancien → avertissement.

## Tâche 7 — Front (volet E)

- [ ] `api.ts` : types + hooks `useScoutingList`, `useScoutingReport` ; helpers purs
      `web/src/scouting.ts` (`fmtRate`, `statusLabel`, `claimEvidence`) + tests vitest.
- [ ] `pages/Scouting.tsx` (liste + création) ; `pages/ScoutingReport.tsx` (en-tête titre éditable,
      actions pack/import, onglets Overview / Players / Draft / Maps / Games / Roster, puces de
      preuve, dépôt séquentiel avec statut par fichier, fenêtre d'import avec aperçu et
      confirmation d'écrasement). Onglet nav « Scouting ».
- [ ] `08-frontend.md`. `npm test` + `npm run build` verts.

## Tâche 8 — Vérification réelle + mesure

- [ ] Serveur dev + front : rapport avec les 3 replays de l'opérateur, ancre `Razhag` → roster des
      5 attendus, 3 parties côté cible, faits cohérents avec les drafts confirmés.
- [ ] Pack copié → réponse JSON (rédigée par Claude dans la session à partir du pack réel, l'étape
      LLM étant manuelle) → import → preuves affichées ; ré-import → écrasement.
- [ ] `GET /api/scouting/{id}` : p95 mesuré (20 requêtes) → `01-architecture.md`.
- [ ] `STATUS.md`, commit, push.

Critères d'acceptation D (Claude **et** ChatGPT sur le pack réel) et B (roster confirmé sur un vrai
lot adverse plus large) : **exigent l'opérateur** — listés comme restant dus dans `STATUS.md`.
