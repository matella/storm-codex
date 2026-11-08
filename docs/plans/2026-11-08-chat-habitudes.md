# Habitudes de chat — plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** Afficher, par compte, des mesures de chat (volume, destinataire, moment, mots récurrents) sur la fiche joueur et dans les cartes « Their players » du scouting, sans jamais exposer le texte ni juger de toxicité.

**Architecture :** Le chat est déjà dans `matches.data.messages` (type 0 = chat, `recipient` 0 = tous / 1 = équipe, `player` = toon, `text`, `time` en secondes). Un module pur `players/chat.rs` calcule les mesures à partir de lignes `ChatRow` ; une route `GET /api/players/{toon}/chat` charge les lignes d'un seul compte (pas de pré-calcul massif) et appelle ce module. Le front l'affiche via React Query. Aucune migration, aucun bump de `PARSER_VERSION`.

**Tech Stack :** Rust (axum, sqlx, serde_json), React + TanStack Query + vitest.

**Spec :** `docs/specs/2026-11-08-signaux-replay-supplementaires-design.md` (section C, idée 6) ; contexte `docs/specs/2026-10-25-recherche-joueurs-design.md`.

## Global Constraints

- Mesures uniquement : **aucun score de toxicité, aucun classement** ; le texte d'un message n'est jamais renvoyé par l'API.
- Budget API : p95 < 100 ms (règle dure 4 de `gads/PROJECT.md`) ; calcul à la lecture pour **un seul** compte, 5 000 messages les plus récents au maximum.
- Rust : `thiserror`, pas de `unwrap()` hors tests, `cargo clippy --workspace --all-targets -- -D warnings`.
- Règle n° 0 : `docs/spec/05-api.md`, `docs/spec/08-frontend.md`, `docs/STATUS.md` mis à jour dans les commits qui changent le contrat.
- Web : textes d'interface en anglais comme le reste de la fiche (« Chat », « not enough games »).

## Review Focus

- Joueur sans aucun message : tous les compteurs à 0, **pas de NaN ni de division par zéro** (`per_game`, parts).
- Joueur inconnu : `404 {"error":"joueur inconnu"}` comme `/identity`.
- Messages d'**autres** joueurs de la même partie : jamais comptés.
- Message sans `text` (null), `time` ou durée de partie absents : ignoré pour le moment, compté pour le volume.
- Mots non latins (cyrillique, CJK) : conservés ; un mot dit une seule ou deux fois n'est **jamais** affiché (pas de fuite de texte).
- Petit échantillon (< 5 parties) : l'écran dit « not enough games » au lieu de présenter des pourcentages.

---

### Task 1: Calcul pur des habitudes de chat

**Files:**
- Create: `crates/storm-codex-server/src/players/chat.rs`
- Modify: `crates/storm-codex-server/src/players/mod.rs` (ajouter `pub mod chat;`)

**Interfaces:**
- Produces:
  - `pub struct ChatRow { pub text: Option<String>, pub recipient: Option<i64>, pub time: Option<f64>, pub length: Option<f64> }` (`length` = durée de la partie en secondes).
  - `pub struct ChatHabits { pub games: i64, pub messages: i64, pub per_game: f64, pub all_share: f64, pub team_share: f64, pub timing: Timing, pub top_words: Vec<WordCount>, pub enough: bool }`, `Serialize`.
  - `pub struct Timing { pub early: f64, pub mid: f64, pub late: f64 }` (parts de messages datés : `time/length` < 0,25 / 0,25–0,75 / > 0,75 ; 0 si aucun message daté).
  - `pub struct WordCount { pub word: String, pub count: i64 }`.
  - `pub const MIN_GAMES: i64 = 5;` `pub const MIN_WORD_COUNT: i64 = 3;` `pub const MAX_WORDS: usize = 8;`
  - `pub fn habits(games: i64, rows: &[ChatRow]) -> ChatHabits`.

- [ ] **Step 1: Écrire les tests** dans `chat.rs` (`#[cfg(test)]`, sans DB) :
  - `aucun_message_donne_des_zeros_sans_nan` : `habits(10, &[])` → `messages == 0`, `per_game == 0.0`, `all_share == 0.0`, `team_share == 0.0`, `timing == Timing{0,0,0}`, `top_words.is_empty()`, `enough == true` ; `habits(0, &[])` → `per_game == 0.0` et `enough == false`.
  - `parts_destinataire_et_moment` : 4 messages (2 `recipient` 0, 2 `recipient` 1 ; `time/length` = 0.1, 0.5, 0.5, 0.9) → `all_share == 0.5`, `team_share == 0.5`, `timing == {0.25, 0.5, 0.25}`, `per_game == 4.0 / games`.
  - `mot_rare_jamais_affiche` : « gg » ×3, « bonjour » ×2 → `top_words == [("gg",3)]`.
  - `mots_non_latins_et_vides_ignores` : « Привет привет привет » → `("привет",3)` ; mots < 3 caractères, nombres seuls et mots vides (`the`, `les`, `and`…) absents.
  - `message_sans_texte_ou_temps_compte_au_volume_seulement` : `text: None, time: None` → `messages == 1`, `timing` à zéro, pas de panique ; `length: Some(0.0)` ne divise pas par zéro.

- [ ] **Step 2: Lancer** `cargo test -p storm-codex-server players::chat` → FAIL (module absent).

- [ ] **Step 3: Implémenter `habits`** : minuscules, découpe sur tout caractère non alphanumérique Unicode, mots ≥ 3 caractères, ni numériques ni dans la liste de mots vides FR/EN (constante `STOP_WORDS`), comptage dans un `HashMap`, tri par compte décroissant puis mot croissant (ordre stable), filtre `count >= MIN_WORD_COUNT`, `take(MAX_WORDS)`. Parts calculées sur les messages qui ont `recipient` / `time`+`length > 0` ; `enough = games >= MIN_GAMES`.

- [ ] **Step 4: Lancer** les mêmes tests → PASS, puis `cargo clippy --workspace --all-targets -- -D warnings` → propre.

- [ ] **Step 5: Commit** `feat(players): calcul des habitudes de chat (volume, destinataire, moment, mots récurrents)`.

---

### Task 2: Route `GET /api/players/{toon}/chat`

**Files:**
- Modify: `crates/storm-codex-server/src/players/chat.rs` (chargement), `crates/storm-codex-server/src/players/api.rs` (handler), `crates/storm-codex-server/src/main.rs` (route, près de la ligne 277), `docs/spec/05-api.md`
- Test: `crates/storm-codex-server/src/main.rs` (module `api_tests`, même harnais que `identity_historique_suggestions_et_decision`)

**Interfaces:**
- Consumes: `chat::habits`, `ChatRow`, `ChatHabits` (Task 1) ; `known(&db, &toon)` et `db_err` de `players/api.rs`.
- Produces:
  - `pub async fn load_rows(db: &PgPool, toon: &str) -> Result<(i64, Vec<ChatRow>), sqlx::Error>` : `(parties du compte, ≤ 5 000 messages du compte les plus récents)`. SQL : `match_players` du toon joint à `matches`, `jsonb_array_elements(m.data -> 'messages')` filtré sur `x->>'type' = '0' AND x->>'player' = $1`, `ORDER BY m.played_at DESC NULLS LAST LIMIT 5000` ; durée = `matches.length`.
  - `pub async fn chat(State, Path<String>) -> Resp` dans `api.rs` : `200` avec `ChatHabits` en JSON, `404 {"error":"joueur inconnu"}` si inconnu.
  - Route `GET /api/players/{toon}/chat`.

- [ ] **Step 1: Écrire le test d'intégration** `chat_habitudes_comptees_pour_le_bon_joueur` (ignoré sans `DATABASE_URL`) : insérer une partie avec `testutil::insert_match` pour deux comptes A et B, puis `UPDATE matches SET data = jsonb_build_object('messages', …), length = 600` avec 3 × « gg » de A (type 0, recipient 1, time 60), 1 message de B, 1 message type 4 de A. Assertions : `GET /chat` de A → `messages == 3`, `top_words == [{"word":"gg","count":3}]`, aucune clé `text` dans la réponse ; de B → `messages == 1` ; toon inconnu → 404.

- [ ] **Step 2: Lancer** `cargo test -p storm-codex-server chat_habitudes` avec `DATABASE_URL=postgres://storm:storm@localhost:5433/storm_codex` → FAIL (route absente).

- [ ] **Step 3: Implémenter** `load_rows`, le handler et la route comme décrit dans Interfaces.

- [ ] **Step 4: Lancer** le test → PASS ; mesurer `GET /api/players/<toon>/chat` sur matelab après déploiement : < 100 ms pour un gros compte (sonde du 2026-11-08 : 2,3 ms pour un compte moyen).

- [ ] **Step 5: Documenter** dans `docs/spec/05-api.md` la ligne de la route (forme de `ChatHabits`, pas de texte, plafond 5 000 messages) ; **commit** `feat(players): GET /api/players/{toon}/chat`.

---

### Task 3: Affichage sur la fiche joueur et dans le scouting

**Files:**
- Modify: `web/src/api.ts` (type `ChatHabits`, `fetchChat`), `web/src/players.ts` (`chatSummary`), `web/src/pages/Player.tsx` (section « Chat » de l'onglet Stats), `web/src/pages/ScoutingReport.tsx` (ligne dans `Players`), `docs/spec/08-frontend.md`
- Create: `web/src/components/PlayerChat.tsx`
- Test: `web/src/players.test.ts`

**Interfaces:**
- Consumes: `GET /api/players/{toon}/chat` (Task 2).
- Produces:
  - `export interface ChatHabits { games: number; messages: number; per_game: number; all_share: number; team_share: number; timing: { early: number; mid: number; late: number }; top_words: { word: string; count: number }[]; enough: boolean }` et `export const fetchChat = (toon: string) => get<ChatHabits>(...)` dans `api.ts`.
  - `export function chatSummary(c: ChatHabits): string | null` dans `players.ts` : `null` si `!c.enough`, `"quiet"` si `per_game < 0.5`, sinon `"{per_game, 1 décimale} msg/game · {team_share en %} team chat"`.
  - `<PlayerChat toon={string} compact?: boolean />` : version complète (volume, répartition tous/équipe, barre début/milieu/fin, mots récurrents) ou `compact` (une ligne = `chatSummary`). Rend « not enough games » si `!enough`, rien en cas d'erreur 404 (compte absent du corpus).

- [ ] **Step 1: Écrire les tests** dans `players.test.ts` : `chatSummary` renvoie `null` si `enough: false` ; `"quiet"` pour `per_game: 0.2` ; `"2.5 msg/game · 80% team chat"` pour `per_game: 2.5, team_share: 0.8`.

- [ ] **Step 2: Lancer** `npm test` dans `web/` → FAIL (`chatSummary` absent).

- [ ] **Step 3: Implémenter** `chatSummary`, le type et `fetchChat`, le composant `PlayerChat` (classes `ds-` existantes : `ds-panel`, `ds-chips`, `ds-chip`, `SecHead`), la section « Chat » dans l'onglet Stats de `Player.tsx`, et `<PlayerChat toon={p.toon} compact />` sous `ds-pstats` dans `Players` de `ScoutingReport.tsx`. L'export HTML du scouting n'est **pas** modifié (hors périmètre, faits figés côté serveur).

- [ ] **Step 4: Vérifier** `npx tsc --noEmit`, `npm test`, `npm run build` → verts ; ouvrir une fiche joueur avec chat et une sans (aperçu navigateur) : pas d'erreur console, « not enough games » pour un petit échantillon.

- [ ] **Step 5: Documenter** dans `docs/spec/08-frontend.md` (section Chat de la fiche, ligne compacte du scouting) ; **commit** `feat(web): habitudes de chat sur la fiche joueur et le scouting`.

---

### Task 4: Clôture

**Files:**
- Modify: `docs/STATUS.md`, `docs/specs/2026-11-08-signaux-replay-supplementaires-design.md` (statut de la section C : validée, plan écrit)

- [ ] **Step 1:** Lancer la CI locale : `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (avec `DATABASE_URL`), `npm ci && npm test && npm run build` dans `web/` → tout vert.
- [ ] **Step 2:** Mettre à jour `docs/STATUS.md` (chat livré ; reste B déserteurs, A cosmétiques, E niveau, D clavier) ; **commit** `docs: STATUS — habitudes de chat`.
- [ ] **Step 3:** Ouvrir la PR vers `main`.
