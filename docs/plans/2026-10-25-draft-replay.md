# Draft depuis un replay — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Charger un replay Custom, préparer son draft caché, le dérouler phase par phase (manuel ou auto 30 s) sur l'overlay OBS existant, avec file de replays et fearless.

**Architecture:** Le moteur `DraftState` (crate `storm-codex-server`, module `draft`) gagne un `ReplayState` (file de parties chargées + script) et des opérations pures `reveal_phase`/`back_phase`. Un décodage « draft seulement » (storm-stats `process_replay`, rien écrit en base) produit un `LoadedGame`. Les routes `/api/draft/replay/*` mutent l'état, persistent (`draft_live`) et diffusent `draft.updated`. La page React `/draft/replay` est la télécommande ; `/draft/overlay` n'est pas modifié.

**Tech Stack:** Rust (axum, tokio, serde, sqlx), storm-stats, React + Vite + TS + TanStack Query. Consulter Context7 (axum, TanStack Query) avant d'utiliser leurs API.

**Spec:** `docs/specs/2026-10-25-draft-replay-design.md` (aussi lire `docs/specs/2026-06-19-draft-simulator-design.md`).

## Global Constraints

- Seules les parties **Custom** sont acceptées ; tout autre mode → 4xx avec message ; **rien n'est écrit dans `matches`/`uploads`**.
- Unité de lecture = **phase** (bloc d'étapes consécutives de même `Order`, `DraftState::current_phase_len`) ; Standard = 12 phases : ban, ban, ban, ban, 1 pick, 2 picks, 2 picks, ban, ban, 2 picks, 2 picks, 1 pick.
- Auto-play : **30 000 ms par phase** par défaut (`interval_ms`), réglable.
- Budget : chargement d'un replay < 500 ms (mesuré, pas estimé).
- Équipe 0 = Blue, équipe 1 = Red (à vérifier sur un replay réel — Task 2).
- Clippy strict, `unwrap()` interdit hors tests (`unwrap_used = "deny"`), erreurs typées `thiserror`, commits conventionnels.
- **Règle n° 0** : chaque tâche qui change un contrat met à jour `docs/spec/` dans le même commit.
- `SCHEMA_VERSION` du draft passe à 2 ; les anciens états persistés (sans `replay`) doivent toujours se charger (`#[serde(default)]`).

## Review Focus

- Replay ancien (build < 66292) ou ban absent/`null` : l'étape est **sautée** (« no ban »), la lecture ne se bloque pas (Task 1 + 2).
- Héros en double dans le script (Custom sans règle d'unicité) : la révélation ne doit **pas** échouer (`Unavailable`) (Task 1).
- Corps vide, fichier tronqué/corrompu, replay non Custom, fichier énorme : 4xx propre, jamais 500/panic (Task 4).
- Suivant à la fin, Retour au début, Suivant sans partie chargée : 409 sans panique ni changement d'état (Task 1 + 4).
- Redémarrage du serveur en pleine lecture auto : `playing` repasse à `false`, curseur conservé (Task 4).
- Suivant manuel simultané au tick du timer : une seule phase révélée par action (verrou d'écriture unique) (Task 4).

---

### Task 1: Moteur — types replay, révélation par phase

**Files:**
- Create: `crates/storm-codex-server/src/draft/replay.rs` (types + opérations pures)
- Modify: `crates/storm-codex-server/src/draft/mod.rs` (`mod replay; pub use replay::*;`, champ `replay`, `SCHEMA_VERSION = 2`)
- Test: tests unitaires dans `replay.rs`

**Interfaces:**
- Produces:
  - `pub struct LoadedGame { pub source: String, pub map: String, pub first_pick: Side, pub script: Vec<Option<String>> /*16, ordre des étapes Standard*/, pub pick_players: Vec<Option<String>> /*16, aligné*/, pub blue_players: [String;5], pub red_players: [String;5] }` (Serialize/Deserialize/Clone/Debug)
  - `pub struct ReplayState { pub queue: Vec<LoadedGame>, pub current: Option<usize>, pub fearless: bool, pub playing: bool, pub interval_ms: u32 /*défaut 30_000*/, pub next_at_ms: Option<i64> }` + `Default`
  - `DraftState.replay: ReplayState` (`#[serde(default)]`)
  - `impl DraftState { pub fn reveal_phase(&mut self) -> Result<(), DraftError>; pub fn back_phase(&mut self) -> Result<(), DraftError>; pub fn select_game(&mut self, index: usize) -> Result<(), DraftError>; pub fn public(&self) -> DraftState }`
  - `DraftError` gagne `NoGame` ; `DraftError::Complete`/`NothingToUndo` réutilisés.

- [ ] **Step 1: Tests (rouges)** dans `replay.rs` : `reveal_phase_reveals_whole_two_pick_phase` (script 16 héros distincts `h0..h15` ; 4 reveals → curseur 4 ; le 5e → 5 ; le 6e → curseur 7 et `assignments[5..7]` remplis), `twelve_reveals_complete_standard` (12 appels → `is_complete()`, 13e → `Err(Complete)`), `back_phase_undoes_whole_phase` (après 6 reveals, `back_phase` → curseur 5, `assignments[5..7]` = None), `back_phase_at_start_errors` (`NothingToUndo`), `reveal_without_game_errors` (`NoGame`), `none_step_is_skipped` (script avec `None` aux étapes 2,3 : reveal de ces phases avance le curseur, assignation `None`), `duplicate_hero_does_not_fail` (script avec `h0` aux étapes 0 et 5 : 6 reveals OK), `select_game_fearless_seeds_series_bans` (queue de 2 parties, `fearless=true`, `select_game(1)` → `series_bans` == picks (non `None`) de la partie 0, curseur 0, format `Fearless`, équipes/joueurs/map/first_pick de la partie 1), `select_game_non_fearless_has_no_series_bans`, `public_hides_scripts` (`public().replay.queue[i].script` vide, `pick_players` vide).
- [ ] **Step 2:** `cargo test -p storm-codex-server draft::replay` → échec de compilation/tests attendus.
- [ ] **Step 3: Implémenter** types et méthodes. `reveal_phase` : `len = current_phase_len()`, pour chacune des `len` étapes écrire `script[cursor]` directement dans `assignments` (sans passer par `apply`, donc sans contrôle d'unicité) et avancer ; `back_phase` : reculer jusqu'au début du bloc de phase précédent (calculer les bornes de phases depuis `steps`, pas depuis le curseur seul). `select_game` : `*self = DraftState::new(format, g.first_pick, g.map)` en conservant `score`, `bo`, `manual_unavailable`, `replay`, noms d'équipe ; remplir `blue.players`/`red.players` ; `series_bans` = union des picks de `queue[..index]` si `fearless`. `public()` : clone avec scripts/`pick_players` vidés.
- [ ] **Step 4:** mêmes tests → PASS ; `cargo clippy -p storm-codex-server --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** `feat(draft): moteur replay — révélation par phase, file, fearless`

### Task 2: Extraction du draft d'un replay Custom

**Files:**
- Modify: `crates/storm-codex-server/src/draft/replay.rs` (ajout), `crates/storm-codex-server/src/read.rs` (extraire `pub(crate) fn hero_attribute_map() -> HashMap<String, String>` de `dim_hero_attributes`, qui l'appelle)
- Create: `crates/storm-codex-server/src/draft/fixtures/custom_draft_match.json` (sortie `match_` + `players` réduite d'un vrai replay Custom de l'opérateur, via `fetch-corpus`/corpus local)
- Test: `replay.rs`

**Interfaces:**
- Consumes: `storm_stats::Output { status, match_, players }`, `hero_attribute_map()`.
- Produces: `pub fn loaded_game_from_output(out: &storm_stats::Output, source: &str, attrs: &HashMap<String,String>) -> Result<LoadedGame, ReplayError>` ; `#[derive(thiserror::Error)] pub enum ReplayError { NotCustom, NoDraft, Parse(i64) }`.

- [ ] **Step 1:** Sur un vrai replay Custom, dumper `match_.mode`, `bans`, `picks` (+`first`), et pour chaque joueur `team`, `hero`, `name`, `turn` ; **vérifier** la forme (picks = noms de héros ? bans = `{hero, order, absolute}` avec codes attribut ?) et la correspondance équipe 0 = Blue. Réduire en fixture JSON. Si une hypothèse du plan est fausse, corriger la spec avant de continuer.
- [ ] **Step 2: Tests (rouges)** : `custom_fixture_script_matches_turns` (le script reconstruit : pour chaque joueur, `script.iter().position(héros) == turn` du parser), `bans_resolved_to_canonical_names` (aucun code 4 lettres dans le script), `non_custom_rejected` (mode ≠ Custom → `NotCustom`), `old_build_has_none_steps` (fixture modifiée build 66000 : étapes 2 et 3 = `None`), `status_not_one_is_parse_error`, `pick_players_follow_pick_owner` (le pseudo en `pick_players[i]` est celui du joueur dont le héros est `script[i]`).
- [ ] **Step 3: Implémenter** l'ordre des 16 étapes en miroir exact de `selections` de `process_draft` (`crates/storm-stats/src/process.rs:2343-2380`) : First = équipe `picks.first` ; le mode Custom est testé avec `storm_stats::constants::rt_int("GameMode","Custom")`. Hero ban `null`/absent ou code inconnu → `None`. `first_pick` = Blue si équipe 0.
- [ ] **Step 4:** tests PASS + clippy.
- [ ] **Step 5: Commit** `feat(draft): extraction du draft d'un replay Custom (script d'étapes)`

### Task 3: Routes, timer d'auto-play, persistance

**Files:**
- Create: `crates/storm-codex-server/src/draft/replay_api.rs`
- Modify: `draft/mod.rs` (`pub mod replay_api;`), `draft/api.rs` (`get_draft` renvoie `public()`), `main.rs` (routes + `replay_timer: Arc<Mutex<Option<JoinHandle<()>>>>` dans `AppState` + remise à `playing=false` au chargement de l'état), `docs/spec/05-api.md`, `docs/spec/04-serveur.md`, `docs/spec/07-evenements.md` (note : `draft.updated` réutilisé)
- Test: tests axum dans `main.rs` (même style que ceux existants, `tower::ServiceExt::oneshot`)

**Interfaces:**
- Consumes: Task 1 (`reveal_phase`, `back_phase`, `select_game`, `public`), Task 2 (`loaded_game_from_output`), `store::save`, `persist`.
- Produces (toutes mutent sous un unique `write()`, puis `persist` + `draft.updated`) :
  - `POST /api/draft/replay/load` — corps = octets du `.StormReplay` (en-tête `X-Filename` optionnel, via `upload::filename`) ; écrit dans un fichier temporaire, `spawn_blocking(process_replay)`, ajoute à `queue`, sélectionne la partie si c'est la première ; 201 + `ReplayState` public ; 422 `{error:"not_custom"|"no_draft"|"parse_failed"}` ; 400 corps vide ; 413 au-delà de la limite existante du serveur.
  - `DELETE /api/draft/replay/queue/{i}`, `POST /api/draft/replay/queue/move` `{from,to}`, `POST /api/draft/replay/select` `{index}`, `POST /api/draft/replay/fearless` `{value}` (re-sélectionne la partie courante), `POST /api/draft/replay/next`, `/back`, `/reset`, `POST /api/draft/replay/play` `{playing: bool, interval_ms?: u32}` → 200 état ; 409 sur `DraftError`.
  - `GET /api/draft/replay` → `ReplayState` **complet** (scripts) pour la page replay.
  - Timer : `play(true)` annule l'éventuel timer, spawn une tâche qui dort `interval_ms`, révèle une phase (même `write()` + `persist`), met à jour `next_at_ms`, et s'arrête à la fin du draft (`playing=false`) ; `play(false)`, `reset`, `back`, `select`, `load` annulent le timer. La 1re phase est révélée **après** un intervalle complet (choix du plan ; « Suivant » donne la 1re immédiatement).

- [ ] **Step 1: Tests (rouges)** : `load_rejects_empty_body_400`, `load_rejects_non_custom_422_and_writes_nothing` (corps = `silver-city-aram.StormReplay` de `crates/storm-stats/tests/data` ; 422 et `SELECT count(*) FROM uploads` inchangé si DB de test dispo, sinon assert sur l'absence d'appel de projection), `next_without_game_409`, `next_at_end_409`, `back_at_start_409`, `get_draft_hides_scripts` (`/api/draft` ne contient aucun héros non révélé), `get_replay_returns_full_script`, `play_true_then_false_cancels_timer` (`interval_ms` = 20 ms avec `tokio::time::pause`/`advance` : après 1 tick → curseur 1, `play false`, nouveau délai → curseur inchangé), `manual_next_and_tick_never_double_reveal`, `startup_resets_playing` (état persisté avec `playing=true` → chargé `playing=false`, curseur conservé).
- [ ] **Step 2:** `cargo test -p storm-codex-server draft` → échecs attendus.
- [ ] **Step 3: Implémenter** les handlers (erreurs typées → statut) ; réutiliser `persist`. Mettre à jour les 3 docs spec (routes, état/persistance de la file, `draft.updated`).
- [ ] **Step 4:** tests PASS ; `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** `feat(server): routes /api/draft/replay/* — chargement, lecture manuelle et auto`

### Task 4: Page `/draft/replay`

**Files:**
- Create: `web/src/pages/DraftReplay.tsx`, `web/src/draftReplay.test.ts` (logique pure : libellés de phase)
- Modify: `web/src/api.ts` (fonctions + types `ReplayState`/`LoadedGame` à côté de `DraftState`), `web/src/App.tsx` (route `draft/replay` à côté de `draft`), `docs/spec/08-frontend.md`, `docs/STATUS.md`
- Test: `web/src/draftReplay.test.ts`

**Interfaces:**
- Consumes: routes de la Task 3 ; `useDraft()`/invalidation `["draft"]` existants (WS `draft.updated`).
- Produces: `export const draftReplayLoad = (file: File) => …`, `draftReplayNext/Back/Reset`, `draftReplayPlay(playing, intervalMs?)`, `draftReplaySelect(i)`, `draftReplayFearless(v)`, `draftReplayRemove(i)`, `fetchDraftReplay()` ; `export function phaseLabels(steps: Step[]): {action: "ban"|"pick"; side: "first"|"second"; count: number}[]` (12 éléments pour Standard).

- [ ] **Step 1: Test (rouge)** `phaseLabels` : Standard → 12 phases, comptes `[1,1,1,1,1,2,2,1,1,2,2,1]`, actions `ban×4, pick×3, ban×2, pick×3`.
- [ ] **Step 2:** `npm test` (vitest, comme `session.test.ts`) → échec.
- [ ] **Step 3: Implémenter** `phaseLabels` puis la page (tokens du thème existants, mêmes classes que `Draft.tsx`) : zone de dépôt (accepte `.StormReplay`, plusieurs fichiers → envoyés dans l'ordre), file (partie courante en surbrillance, suppression, sélection, ordre), bascule fearless, rail des 12 phases (numérotées, héros masqués jusqu'à révélation, phase courante marquée), boutons Suivant/Retour/Reset/Play-Pause + champ durée (secondes, 30 par défaut) + compte à rebours depuis `next_at_ms`, raccourcis Espace (Suivant), Retour arrière (Retour). Afficher le message d'erreur 422 lisible (« partie non personnalisée »).
- [ ] **Step 4:** `npm test`, `npm run build`, `npx tsc --noEmit` → verts.
- [ ] **Step 5: Vérification navigateur** (preview launch.json :8089) : charger un vrai Custom, vérifier overlay vide → révélation dans l'ordre → comparaison avec la fiche de match ; mesurer le chargement (< 500 ms) ; tester file de 3 replays en fearless, Retour/Reset, lecture auto (réduire l'intervalle à 2 s pour le test), rafraîchissement en pleine lecture, rejet d'un replay QM/ARAM. Consigner les mesures dans `docs/STATUS.md`.
- [ ] **Step 6: Commit** `feat(web): page /draft/replay — télécommande du draft depuis un replay`
