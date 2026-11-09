# Chat habits — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show, per account, chat measures (volume, recipient, timing, recurring words) on the player page and in the scouting "Their players" cards, without ever exposing the text or judging toxicity.

**Architecture:** The chat is already in `matches.data.messages` (type 0 = chat, `recipient` 0 = all / 1 = team, `player` = toon, `text`, `time` in seconds). A pure module `players/chat.rs` computes the measures from `ChatRow` rows; a route `GET /api/players/{toon}/chat` loads the rows of a single account (no mass pre-computation) and calls that module. The front shows it via React Query. No migration, no `PARSER_VERSION` bump.

**Tech Stack :** Rust (axum, sqlx, serde_json), React + TanStack Query + vitest.

**Spec:** `docs/specs/2026-11-08-signaux-replay-supplementaires-design.md` (section C, idea 6); context `docs/specs/2026-10-25-recherche-joueurs-design.md`.

## Global Constraints

- Measures only: **no toxicity score, no ranking**; the text of a message is never returned by the API.
- API budget: p95 < 100 ms (hard rule 4 of `gads/PROJECT.md`); computed on read for **a single** account, at most the 5,000 most recent messages.
- Rust: `thiserror`, no `unwrap()` outside tests, `cargo clippy --workspace --all-targets -- -D warnings`.
- Rule no. 0: `docs/spec/05-api.md`, `docs/spec/08-frontend.md`, `docs/STATUS.md` updated in the commits that change the contract.
- Web: UI text in English like the rest of the page ("Chat", "not enough games").

## Review Focus

- Player with no messages at all: all counters at 0, **no NaN and no division by zero** (`per_game`, shares).
- Unknown player: `404 {"error":"joueur inconnu"}` like `/identity`.
- Messages from **other** players in the same game: never counted.
- Message without `text` (null), or with `time` or game length missing: ignored for timing, counted for volume.
- Non-Latin words (Cyrillic, CJK): kept; a word said only once or twice is **never** shown (no text leak).
- Small sample (< 5 games): the screen says "not enough games" instead of showing percentages.

---

### Task 1: Pure computation of chat habits

**Files:**
- Create: `crates/storm-codex-server/src/players/chat.rs`
- Modify: `crates/storm-codex-server/src/players/mod.rs` (ajouter `pub mod chat;`)

**Interfaces:**
- Produces:
  - `pub struct ChatRow { pub text: Option<String>, pub recipient: Option<i64>, pub time: Option<f64>, pub length: Option<f64> }` (`length` = game length in seconds).
  - `pub struct ChatHabits { pub games: i64, pub messages: i64, pub per_game: f64, pub all_share: f64, pub team_share: f64, pub timing: Timing, pub top_words: Vec<WordCount>, pub enough: bool }`, `Serialize`.
  - `pub struct Timing { pub early: f64, pub mid: f64, pub late: f64 }` (shares of timed messages: `time/length` < 0.25 / 0.25–0.75 / > 0.75; 0 if no timed message).
  - `pub struct WordCount { pub word: String, pub count: i64 }`.
  - `pub const MIN_GAMES: i64 = 5;` `pub const MIN_WORD_COUNT: i64 = 3;` `pub const MAX_WORDS: usize = 8;`
  - `pub fn habits(games: i64, rows: &[ChatRow]) -> ChatHabits`.

- [ ] **Step 1: Write the tests** in `chat.rs` (`#[cfg(test)]`, no DB):
  - `aucun_message_donne_des_zeros_sans_nan`: `habits(10, &[])` → `messages == 0`, `per_game == 0.0`, `all_share == 0.0`, `team_share == 0.0`, `timing == Timing{0,0,0}`, `top_words.is_empty()`, `enough == true`; `habits(0, &[])` → `per_game == 0.0` and `enough == false`.
  - `parts_destinataire_et_moment`: 4 messages (2 `recipient` 0, 2 `recipient` 1; `time/length` = 0.1, 0.5, 0.5, 0.9) → `all_share == 0.5`, `team_share == 0.5`, `timing == {0.25, 0.5, 0.25}`, `per_game == 4.0 / games`.
  - `mot_rare_jamais_affiche`: "gg" ×3, "bonjour" ×2 → `top_words == [("gg",3)]`.
  - `mots_non_latins_et_vides_ignores`: "Привет привет привет" → `("привет",3)`; words < 3 characters, bare numbers and stop words (`the`, `les`, `and`…) absent.
  - `message_sans_texte_ou_temps_compte_au_volume_seulement`: `text: None, time: None` → `messages == 1`, `timing` at zero, no panic; `length: Some(0.0)` does not divide by zero.

- [ ] **Step 2: Lancer** `cargo test -p storm-codex-server players::chat` → FAIL (module absent).

- [ ] **Step 3: Implement `habits`**: lowercase, split on any non-alphanumeric Unicode character, words ≥ 3 characters, neither numeric nor in the FR/EN stop-word list (constant `STOP_WORDS`), count in a `HashMap`, sort by decreasing count then increasing word (stable order), filter `count >= MIN_WORD_COUNT`, `take(MAX_WORDS)`. Shares computed over the messages that have `recipient` / `time`+`length > 0`; `enough = games >= MIN_GAMES`.

- [ ] **Step 4: Run** the same tests → PASS, then `cargo clippy --workspace --all-targets -- -D warnings` → clean.

- [ ] **Step 5: Commit** `feat(players): chat habits computation (volume, recipient, timing, recurring words)`.

---

### Task 2: Route `GET /api/players/{toon}/chat`

**Files:**
- Modify: `crates/storm-codex-server/src/players/chat.rs` (loading), `crates/storm-codex-server/src/players/api.rs` (handler), `crates/storm-codex-server/src/main.rs` (route, near line 277), `docs/spec/05-api.md`
- Test: `crates/storm-codex-server/src/main.rs` (module `api_tests`, same harness as `identity_historique_suggestions_et_decision`)

**Interfaces:**
- Consumes: `chat::habits`, `ChatRow`, `ChatHabits` (Task 1); `known(&db, &toon)` and `db_err` from `players/api.rs`.
- Produces:
  - `pub async fn load_rows(db: &PgPool, toon: &str) -> Result<(i64, Vec<ChatRow>), sqlx::Error>`: `(the account's games, ≤ 5,000 most recent messages of the account)`. SQL: `match_players` of the toon joined to `matches`, `jsonb_array_elements(m.data -> 'messages')` filtered on `x->>'type' = '0' AND x->>'player' = $1`, `ORDER BY m.played_at DESC NULLS LAST LIMIT 5000`; length = `matches.length`.
  - `pub async fn chat(State, Path<String>) -> Resp` in `api.rs`: `200` with `ChatHabits` as JSON, `404 {"error":"joueur inconnu"}` if unknown.
  - Route `GET /api/players/{toon}/chat`.

- [ ] **Step 1: Write the integration test** `chat_habitudes_comptees_pour_le_bon_joueur` (ignored without `DATABASE_URL`): insert a game with `testutil::insert_match` for two accounts A and B, then `UPDATE matches SET data = jsonb_build_object('messages', …), length = 600` with 3 × "gg" from A (type 0, recipient 1, time 60), 1 message from B, 1 type-4 message from A. Assertions: `GET /chat` of A → `messages == 3`, `top_words == [{"word":"gg","count":3}]`, no `text` key in the response; of B → `messages == 1`; unknown toon → 404.

- [ ] **Step 2: Run** `cargo test -p storm-codex-server chat_habitudes` with `DATABASE_URL=postgres://storm:storm@localhost:5433/storm_codex` → FAIL (route missing).

- [ ] **Step 3: Implement** `load_rows`, the handler and the route as described in Interfaces.

- [ ] **Step 4: Run** the test → PASS; measure `GET /api/players/<toon>/chat` on matelab after deployment: < 100 ms for a large account (probe of 2026-11-08: 2.3 ms for an average account).

- [ ] **Step 5: Document** in `docs/spec/05-api.md` the route's row (shape of `ChatHabits`, no text, 5,000-message cap); **commit** `feat(players): GET /api/players/{toon}/chat`.

---

### Task 3: Display on the player page and in scouting

**Files:**
- Modify: `web/src/api.ts` (type `ChatHabits`, `fetchChat`), `web/src/players.ts` (`chatSummary`), `web/src/pages/Player.tsx` ("Chat" section of the Stats tab), `web/src/pages/ScoutingReport.tsx` (row in `Players`), `docs/spec/08-frontend.md`
- Create: `web/src/components/PlayerChat.tsx`
- Test: `web/src/players.test.ts`

**Interfaces:**
- Consumes: `GET /api/players/{toon}/chat` (Task 2).
- Produces:
  - `export interface ChatHabits { games: number; messages: number; per_game: number; all_share: number; team_share: number; timing: { early: number; mid: number; late: number }; top_words: { word: string; count: number }[]; enough: boolean }` and `export const fetchChat = (toon: string) => get<ChatHabits>(...)` in `api.ts`.
  - `export function chatSummary(c: ChatHabits): string | null` in `players.ts`: `null` if `!c.enough`, `"quiet"` if `per_game < 0.5`, otherwise `"{per_game, 1 decimal} msg/game · {team_share in %} team chat"`.
  - `<PlayerChat toon={string} compact?: boolean />`: full version (volume, all/team split, early/mid/late bar, recurring words) or `compact` (one line = `chatSummary`). Renders "not enough games" if `!enough`, nothing on a 404 error (account missing from the corpus).

- [ ] **Step 1: Write the tests** in `players.test.ts`: `chatSummary` returns `null` if `enough: false`; `"quiet"` for `per_game: 0.2`; `"2.5 msg/game · 80% team chat"` for `per_game: 2.5, team_share: 0.8`.

- [ ] **Step 2: Run** `npm test` in `web/` → FAIL (`chatSummary` missing).

- [ ] **Step 3: Implement** `chatSummary`, the type and `fetchChat`, the `PlayerChat` component (existing `ds-` classes: `ds-panel`, `ds-chips`, `ds-chip`, `SecHead`), the "Chat" section in the Stats tab of `Player.tsx`, and `<PlayerChat toon={p.toon} compact />` under `ds-pstats` in `Players` of `ScoutingReport.tsx`. The scouting HTML export is **not** changed (out of scope, facts frozen on the server side).

- [ ] **Step 4: Check** `npx tsc --noEmit`, `npm test`, `npm run build` → green; open a player page with chat and one without (browser preview): no console error, "not enough games" for a small sample.

- [ ] **Step 5: Document** in `docs/spec/08-frontend.md` (Chat section of the page, compact scouting row); **commit** `feat(web): chat habits on the player page and in scouting`.

---

### Task 4: Wrap-up

**Files:**
- Modify: `docs/STATUS.md`, `docs/specs/2026-11-08-signaux-replay-supplementaires-design.md` (status of section C: approved, plan written)

- [ ] **Step 1:** Run the local CI: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (with `DATABASE_URL`), `npm ci && npm test && npm run build` in `web/` → all green.
- [ ] **Step 2:** Update `docs/STATUS.md` (chat shipped; remaining: B deserters, A cosmetics, E level, D keyboard); **commit** `docs: STATUS — chat habits`.
- [ ] **Step 3:** Open the PR to `main`.
