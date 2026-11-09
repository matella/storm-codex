# Plan — player search + linked accounts (smurfs)

> **For the implementer:** use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans. `- [ ]` boxes = tracking. First read CLAUDE.md and `docs/spec/10-developpement.md`.

**Objective:** player search box (current name, old names, fragment), an
"Identity" tab (dated history + guessed accounts + confirm/dismiss), without ever merging profiles.

**Architecture:** new server module `src/players/` — read SQL (`names.rs`, `search.rs`),
**pure scoring core tested without a DB** (`similarity.rs`), loading of the signals + bounded
pre-selection (`identity.rs`), routes (`api.rs`). One migration (`player_links`). Front: `/players` page,
Identity tab in the existing `player/:toon` page, tested pure helpers (`players.ts`).

**Stack :** Rust/axum/sqlx (Postgres), React + Vite + TS + TanStack Query, vitest.

**Spec:** `docs/specs/2026-10-25-recherche-joueurs-design.md` (approved on 2026-10-25) — **amended by
task 0** (see "Corrections").

## Corrections to the spec (found by reading the code and the real dev corpus)

1. **"Same uploader" does not work as written**: each replay contains all 10 players, so all 10
   "share the uploader". Dev corpus: token 4 = 2,719 of the 2,723 games. The signal becomes
   **"token owner"**: an account present in ≥ 50% of a token's games (≥ 50 games).
   Two accounts owning the **same** token = strong signal (the operator's smurf).
2. **The recurring teammate is biased by the operator** (`matella` is in 2,128 games): everyone
   would be "linked" through them. **Token owners and `operator_names`** are excluded from the
   teammates signal **and** from the candidate pre-selection.
3. **`players.last_name` is not the current name** (UPSERT in processing order, backfill out of
   order) and `names` is not chronological. Current name, "was …" and "3 most recent" come
   from the **dated history** (`match_players.data->>'name'` + `matches.played_at`). `played_at` NULL =
   oldest. `players.names` is still used as a *set* (search matching).
4. **Routes**: the existing front route `player/:toon` is **kept** (other pages link to it);
   we add `players` for the search (the spec said `/players/:toon`).
5. **No `CREATE EXTENSION`** (pg_trgm/unaccent): migrations run at startup on the box's shared
   Postgres; a privilege failure would prevent the server from starting. `lower()` + `ILIKE`
   first (allowed by the spec: "if the measurement requires it"). Accent insensitivity: out of v1, to be flagged.
6. **Coverage** (consequence of the existing code, not re-decided): scouting replays are isolated by
   construction (migration 0010) and never reach `players`: a player seen **only** in
   scouting is not searchable.

## Global constraints

- Rule no. 0: **each task commits its `docs/spec/` diff with its code** (05-api, 06-modele-donnees,
  08-frontend). No final "docs" task.
- `git add` with **explicit paths** (the repo contains an untracked file `e399b92…` and
  a non-ignored `.superpowers/` — do not commit them). Conventional commits, French or English like
  the rest of the history; footer `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Rust: strict clippy, `thiserror`, no `unwrap()` outside tests. **Context7 before any
  axum / sqlx / TanStack Query API** (`resolve-library-id` then `query-docs`); flag it if it is missing.
- **Integration tests**: they skip without `DATABASE_URL` ("ignored" = empty green). Always run
  with `DATABASE_URL=postgres://storm:storm@localhost:5433/storm_codex` and **check that the "ignored"
  line is absent**. Database shared between tests: fixtures with unique toon/name (`uuid`), *containment*
  assertions, never exact global counts.
- Budgets: `GET /api/players/search` p95 < 100 ms; bounded identity (≤ 30 detailed candidates, no
  pairwise comparison of all players). Measure on the dev corpus (2,723 games, 7,618 players).
- `PUT …/links` writes: guarded by `crate::manage::is_admin` (open mode if `ADMIN_TOKEN` is empty).
- Reading: visible to everyone; every estimate is labelled "estimate" in the UI.

## Review Focus (cases the spec implies without testing them)

1. Name containing `_` or `%` (e.g. `Kerrigan_Main`): the search treats them **literally** (task 2).
2. Query < 2 characters / empty / spaces: empty list, no heavy SQL query (task 2).
3. Two accounts that share **only** the operator or the same token: **no link** (tasks 3, 4).
4. Accounts with very few games: hero overlap can **never** reach HIGH (task 3).
5. `PUT …/links/{self}` → 400; pair stored ordered (`CHECK toon_a < toon_b`); idempotent
   and symmetric PUT (A→B ≡ B→A) (tasks 1, 5).
6. `played_at` NULL or a history with a single name: no crash, stable order (task 1).

---

## Task 0 — Amend the spec (docs only)

**Files:** modify `docs/specs/2026-10-25-recherche-joueurs-design.md`.

- [ ] Carry the 6 corrections above into the spec (sections Signals, Data, API/Routes,
  Out of scope) and set its status to "approved, amended on 2026-10-25".
- [ ] Commit: `docs(spec): player search — uploader signal redefined, dated current name, routes`
  (`git add docs/specs/2026-10-25-recherche-joueurs-design.md docs/plans/2026-10-25-recherche-joueurs.md`).
- [ ] The operator is told that 1 and 2 correct the text they approved (in the final message).

## Task 1 — `player_links` migration + dated name history

**Files:** create `crates/storm-codex-server/migrations/0011_player_links.sql`,
`src/players/mod.rs`, `src/players/names.rs`; modify `src/main.rs` (`mod players;`), `src/read.rs`
(`get_player`, l. ~135-160); docs `06-modele-donnees.md`, `05-api.md`.

**Interfaces — produit :**
```rust
// src/players/names.rs
pub struct NameSpan { pub name: String, pub first_seen: Option<DateTime<Utc>>,
                      pub last_seen: Option<DateTime<Utc>>, pub games: i64 }
/// Sorted from most recent to oldest (last_seen DESC, NULL last, then name ASC: stable order).
/// Empty if there is no row, dated or not; the caller then falls back to `players.names`.
pub async fn name_history(db: &PgPool, toon: &str) -> Result<Vec<NameSpan>, sqlx::Error>;
/// Current name = first of `name_history`, otherwise `players.last_name`, otherwise None.
pub async fn current_name(db: &PgPool, toon: &str) -> Result<Option<String>, sqlx::Error>;
```

- [ ] **Migration**: `player_links(toon_a TEXT, toon_b TEXT, status TEXT NOT NULL, decided_at
  TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (toon_a, toon_b), CHECK (toon_a < toon_b), CHECK
  (status IN ('confirmed','dismissed')))`. **No `CREATE EXTENSION`.** Index on `toon_b`.
- [ ] **Test (fails first)** `names::tests::historique_ordonne_et_current_name` (integration, `uuid`
  toon): insert an old match (name "Old"), a recent one (name "New"), one with
  `played_at NULL` (name "Undated"); expect `[New, Old, Undated]`, `current_name == "New"`,
  correct `first_seen/last_seen`, correct `games`. Second test: a single name → a single `NameSpan`.
- [ ] Implement `name_history` (a `GROUP BY data->>'name'` on `match_players` JOIN `matches`);
  `current_name`.
- [ ] `get_player`: `name` = `current_name`, `names` = names from `name_history` (recent first),
  fallback to the old behaviour if empty. The JSON shape does not change (the existing front keeps working).
  Non-regression test: the existing fields of `/api/players/{toon}` are still present.
- [ ] Run with `DATABASE_URL`; check that "ignored" is absent; `cargo clippy --workspace -- -D warnings`.
- [ ] Docs: `06` (table `player_links`, 0011; rule "current name = dated history"), `05`
  (`/api/players/{toon}`: `name`/`names` semantics). Commit:
  `feat(server): dated name history + player_links table`.

## Task 2 — Search `GET /api/players/search`

**Files:** create `src/players/search.rs`; modify `src/main.rs` (route, **before** `{toon}` when reading
the router — check with Context7 axum's static/parameterised priority); docs `05`.

**Interfaces — produit :**
```rust
pub fn escape_like(q: &str) -> String;           // escapes \ % _; pure
pub struct SearchHit { pub toon: String, pub name: Option<String>, pub matched_name: String,
    pub matched_old: bool, pub games: i64, pub wins: i64, pub roles: Vec<(String, i64)>,
    pub last_seen: Option<DateTime<Utc>> }        // roles: first 2, via dim_heroes (hero→role)
pub async fn search(db: &PgPool, q: &str, limit: i64) -> Result<Vec<SearchHit>, sqlx::Error>;
```
Route: `GET /api/players/search?q=&limit=` (default 20, max 50) → `{ "hits": [...] }`.

- [ ] **Pure tests (fail first)**: `escape_like("Kerrigan_Main") == "Kerrigan\\_Main"`, `%` and `\`
  likewise; `q` trimmed + `chars().count() < 2` → `search` returns `[]` without touching the DB (test with a lazy
  pool/`connect_lazy` or a pure function `normalize_query(&str) -> Option<String>`).
- [ ] **Integration test**: uuid fixtures — player A (current name `Zed{u}Knight`, old
  `Zed{u}Night`), player B (`Zed{u}_Main`). `q="zed{u}night"` finds A with `matched_old=true`;
  `q="Zed{u}_M"` finds B and **not** A (the `_` is literal: a wildcard `_` would match `Zed{u}XM…` — add
  a player C `Zed{u}XMain` who must NOT come out); sort exact > prefix > substring.
- [ ] **Router test** (`api_tests`): `GET /api/players/search?q=ab` does not fall into `get_player`
  (response `{hits}` and not `{toon:"search"}`).
- [ ] Implement: filter on `players` (`lower(name) ILIKE … ESCAPE '\'` on `jsonb_array_elements_text(names)`),
  sort/limit **first**, then aggregates (games, wins, roles, last_seen, current name) **only
  for the ≤ limit rows kept** (p95 budget).
- [ ] Measurement: 20 queries on the dev corpus, record the p95 in the commit message (target < 100 ms).
- [ ] Docs `05`. Commit: `feat(server): GET /api/players/search (current and old names)`.

## Task 3 — Pure scoring core `similarity.rs`

**Files:** create `src/players/similarity.rs` (no DB/axum dependency).

**Interfaces — produit :**
```rust
pub struct PairFeatures {
    pub games_a: u32, pub games_b: u32,
    pub hero_cosine: f32,                  // 0..1, share of games per hero
    pub shared_teammates: u32,             // common teammates excluding operator/token owners
    pub shared_teammate_games: u32,        // cumulative games with them (bounds the strength)
    pub talent_agreement: Option<f32>,     // 0..1 over heroes played ≥ 3 times by both, None if none
    pub hour_cosine: f32,                  // histogramme 24 h UTC
    pub overlapping_games: bool,           // both accounts in the same game / overlapping games
    pub same_token_owner: bool,
}
pub enum Level { High, Low }
pub struct Reason { pub kind: &'static str, pub text: String }   // kind: "token"|"teammates"|"heroes"|"talents"|"hours"
pub struct Suggestion { pub level: Level, pub score: u8, pub reasons: Vec<Reason> }
/// None = no suggestion (score too low, or time exclusion).
pub fn score(f: &PairFeatures) -> Option<Suggestion>;
pub const MIN_GAMES_FOR_HIGH: u32 = 15;   // starting point, to tune in task 8
```
Rules (spec "Signals"): `overlapping_games` → `None`; `same_token_owner` = strong signal; teammates
and talents = medium; heroes = weak-medium; hours = weak; **HIGH requires ≥ 2 independent signals, one of
them strong/medium** and `min(games_a, games_b) ≥ MIN_GAMES_FOR_HIGH`; heroes/hours alone → at best LOW;
each suggestion carries its readable `reasons`.

- [ ] **Tests (fail first)**, one per behaviour: `chevauchement_exclut`,
  `heroes_seuls_jamais_haut` (cosine 0.99, nothing else), `peu_de_parties_jamais_haut` (strong signals but
  `games_a = 3`), `token_plus_heures_donne_haut`, `teammates_plus_talents_donne_haut`,
  `aucun_signal_none`, `deux_comptes_partageant_seulement_loperateur_aucun_lien`
  (`shared_teammates = 0`, `same_token_owner = false`, cosine moyen → `None`), `raisons_non_vides`.
- [ ] Implement `score` (free weighting, named constants at the top of the file, one-line doc
  each). Test `score_monotone`: increasing a signal never lowers the score.
- [ ] `cargo test -p storm-codex-server similarity` green; clippy.
- [ ] Docs: `05`/`06` not concerned; the spec's "Signals" section points to `similarity.rs`
  (the commit includes that pointer in the spec). Commit: `feat(server): pure scoring of linked accounts`.

## Task 4 — Signals and bounded pre-selection `identity.rs`

**Files:** create `src/players/identity.rs`.

**Interfaces :**
- Consumes: `names::name_history/current_name` (t.1), `similarity::{PairFeatures, score, Suggestion}` (t.3).
- Produces:
```rust
/// Operator (`app_settings.operator_names` → toons with a matching name) ∪ token owners
/// (present in ≥ 50% of the games of a token with ≥ 50 games, via uploads.token_id/match_id).
pub async fn excluded_toons(db: &PgPool) -> Result<HashSet<String>, sqlx::Error>;
/// Owner accounts of each token: token_id → toons (for `same_token_owner`).
pub async fn token_owners(db: &PgPool) -> Result<HashMap<i64, HashSet<String>>, sqlx::Error>;
pub struct Candidate { pub toon: String, pub name: Option<String>, pub suggestion: Suggestion }
/// ≤ 30 detailed candidates, sorted by decreasing score. Excludes `target` itself, the `excluded_toons`
/// (except for the token signal), and pairs already decided in `player_links`.
pub async fn suggest(db: &PgPool, target: &str) -> Result<Vec<Candidate>, sqlx::Error>;
```
Pre-selection: (a) hero vectors of all players ≥ `MIN_GAMES` in **one** query, cosine computed in
Rust, top 30; (b) ∪ players sharing ≥ 3 team games with the target, excluded ones aside; then detail
(talents, hours, overlap) for ≤ 30 only. Talents: `match_players.data->'talents'`
(`Tier1Choice`…`Tier7Choice`, object seen in the dev corpus).

- [ ] **Integration tests (fail first)**, `uuid` fixtures, helper `insert_match(db, played_at,
  players: &[(toon, name, team, hero, talents)])`:
  `smurf_evident_est_suggere` (two accounts: same heroes/talents, same non-operator teammates,
  never in the same game → HIGH suggestion); `lien_via_loperateur_seul_ignore` (two accounts
  sharing only the operator → absent); `comptes_dans_la_meme_partie_exclus`; `paire_ecartee_nest_plus_proposee`
  (`dismissed` row inserted); `deux_proprietaires_du_meme_token_fort` (uploads + fake `token_id` ≥ 50 games);
  `borne_30_candidats`.
- [ ] Implement; EXPLAIN/timing on the dev corpus: `suggest` for a player with 300 games < 1 s (budget
  "bounded", to be recorded; no contractual p95).
- [ ] Commit: `feat(server): pre-selection and signals for linked accounts`.

## Task 5 — Identity routes + decision

**Files:** create `src/players/api.rs`; modify `src/main.rs` (routes); docs `05`.

**Interfaces — produit :**
- `GET /api/players/{toon}/identity` →
  `{ toon, name, names:[{name, first_seen, last_seen, games}], suggestions:[{toon, name, level:"high"|"low",
  score, reasons:[{kind,text}]}], confirmed:[{toon, name, decided_at}] }` (404 if the toon is unknown).
- `PUT /api/players/{toon}/links/{other}` 🔒 corps `{ "status": "confirmed" | "dismissed" }` → 200
  `{status}`; `toon == other` → 400; pair normalised (min,max); **idempotent upsert**.

- [ ] **Router tests (fail first)**: identity of an unknown toon → 404; PUT self → 400;
  PUT with `ADMIN_TOKEN` set and no Bearer → 401, with Bearer → 200; PUT A→B then reading from B
  shows the confirmed link (symmetry); PUT twice = a single row; invalid status → 400/422;
  `dismissed` removes the suggestion from the next response.
- [ ] Implement (`is_admin` guard, `scouting/api.rs` pattern); Context7 for the `Json`/`Path` extractor.
- [ ] Docs `05` (3 routes). Commit: `feat(server): GET identity + PUT links (linked accounts)`.

## Task 6 — Front data layer + pure helpers

**Files:** modify `web/src/api.ts`; create `web/src/players.ts`, `web/src/players.test.ts`.

**Interfaces — produit :**
```ts
export interface PlayerHit { toon: string; name: string | null; matched_name: string; matched_old: boolean;
  games: number; wins: number; roles: [string, number][]; last_seen: string | null }
export interface NameSpan { name: string; first_seen: string | null; last_seen: string | null; games: number }
export interface LinkSuggestion { toon: string; name: string | null; level: "high" | "low"; score: number;
  reasons: { kind: string; text: string }[] }
export interface PlayerIdentity { toon: string; name: string | null; names: NameSpan[];
  suggestions: LinkSuggestion[]; confirmed: { toon: string; name: string | null; decided_at: string }[] }
export const searchPlayers = (q: string) => Promise<PlayerHit[]>;
export const fetchIdentity = (toon: string) => Promise<PlayerIdentity>;
export const putLink = (toon: string, other: string, status: "confirmed" | "dismissed") => Promise<void>; // via the authenticated write helper (api.ts ~l.682)
// players.ts (purs)
export function highlightSegments(name: string, q: string): { text: string; hit: boolean }[];
export function recentNames(h: NameSpan[], n?: number): { shown: NameSpan[]; hidden: number };   // n = 3
export function seenRange(s: NameSpan): string;   // "Jan–Aug 2025", "Jan 2025", "unknown date"
export function likelyAlias(i: PlayerIdentity): { toon: string; name: string | null } | null; // confirmed first, otherwise 1st HIGH, never LOW
```
- [ ] **vitest tests (fail first)**: `highlightSegments` case-insensitive, empty `q` → a single
  non-highlighted segment, `q` containing regex characters (`_`, `.`, `(`) treated literally;
  `recentNames` 14 names → 3 + hidden 11; ≤ 3 → hidden 0; `seenRange` with null dates; `likelyAlias`
  **never** returns a LOW suggestion, prefers a confirmed link.
- [ ] Implement; `npm test` + `npx tsc --noEmit`. Docs `08` (types added). Commit:
  `feat(web): player search/identity types and helpers`.

## Task 7 — Pages: search, Identity tab, hint line

**Files:** create `web/src/pages/Players.tsx`, `web/src/components/PlayerIdentity.tsx`; modify
`web/src/App.tsx` (route `players`), `web/src/components/Layout.tsx` (nav entry "Players"),
`web/src/pages/Player.tsx` (Stats | Identity tabs + hint line); docs `08-frontend.md`.

Approved mockup: `.superpowers/brainstorm/171-1792937326/content/search-and-profile-nexus.html`
(variant **B** + hint line). `ds-` classes (`ds.css`), Nexus Codex tokens.

- [ ] Context7: TanStack Query v5 (`useQuery` with a debounced key, `useMutation` + `invalidateQueries`).
- [ ] `Players.tsx`: field (debounce ~250 ms, `q` in the URL `?q=`), < 2 characters = explanatory empty state,
  "medallion" rows (name highlighted via `highlightSegments`, "was …" label if `matched_old`, games,
  win rate, roles, last game), link to `/player/:toon`. Loading / no-result states.
- [ ] `PlayerIdentity.tsx`: recent names (`recentNames`) + "show all (N)" expanding the dated list
  (`seenRange`), scrollable; gold dotted "Estimate" box with guessed accounts (HIGH/LOW badge +
  score + reasons); **Confirm** / **Not the same person** buttons (`putLink`, on a 401 the message
  "admin token required" pointing to `/admin`); confirmed links listed separately. **No merge button.**
- [ ] `Player.tsx`: tabs; "Also plays as X →" line (Identity tab) only if
  `likelyAlias(identity)`; the identity loads separately (the summary stays fast).
- [ ] `npm run build` without errors; `npm test` green. Docs `08` (route `/players`, Identity tab, hint
  line). Commit: `feat(web): Players page + Identity tab`.

## Task 8 — Evidence, threshold tuning, STATUS

- [ ] `DATABASE_URL=… cargo test --workspace` (no "ignored"), `cargo clippy --workspace -- -D warnings`,
  `npm test`, `npm run build`: outputs pasted in the final message.
- [ ] **Tuning on real data**: on the dev corpus, run `identity` for 5 players with several
  names (`jsonb_array_length(names) > 1`, 95 exist) and the top 3 accounts by games; review the
  HIGH suggestions by hand with the operator; adjust **only** the constants of `similarity.rs`
  (task 3 tests stay green); record the reviewed cases in `docs/STATUS.md`.
- [ ] Measurements: p95 of `search` (target < 100 ms) and duration of `identity` for a heavy player, taken on
  the release server + dev Postgres; if p95 > 100 ms, add the trigram index **in a separate migration,
  outside blocking startup, after the operator's agreement**.
- [ ] Browser check (preview_start): search for an old name, open the profile, Identity
  tab, confirm then dismiss a link, screenshot.
- [ ] `docs/STATUS.md`: state + next step. Commit: `docs(status): player search shipped`.

## Out of plan (deliberately)

- Undo a decision (no `DELETE …/links`): absent from the spec, to propose to the operator if needed.
- Accent-insensitive search, trigram extension, players seen only in scouting.
- Operator's time zone for the hour ranges (UTC for both accounts, so comparable).
