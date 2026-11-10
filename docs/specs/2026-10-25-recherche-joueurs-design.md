# Player search + linked accounts (smurfs) — design

> Status: **approved by the operator on 2026-10-25, amended the same day** (see "Amendments").
> From a brainstorm of 2026-10-25. Points marked **[my choice]** are
> default decisions, not approved: review them first.

## Intent

The operator wants to find a specific player and know what they play and what they do well. On HotS
many players have smurfs: they also want to be able to spot that an account "looks like" another one.

A smurf is another Blizzard account (another `toon_handle`): **no replay links two accounts
directly**. The link can only be a probabilistic estimate, always presented as such.

## Goals

1. **Search**: a search box finds a player by current name, **any former
   name**, or name fragment. A player may have a long list of renames.
2. **Player profile**: heroes, roles, winrate, strong builds/talents, maps, recent games.
3. **Linked accounts**: "Identity" tab with the dated name history and a guessed list
   of other accounts, each with a score and its reasons.
4. **Confirm / dismiss** a guessed link; the choice is remembered.

## Out of scope / anti-goals

- **No profile merge.** A confirmed link is a simple "same person" note: each account
  keeps its own stats. No merged winrate or hero pool.
- Never present a guess as a fact; no score without readable reasons.
- No player absent from our replays: coverage = the uploaded replays. No call to an external
  Blizzard API.
- No inferred link between an account and a real person (no real-world identity).

## Behaviour

### Search (`/players`)
- One box; matching ignores case and accents, by substring, on the current
  name **and** all former names. Result: the name that matched (marked "was …" if former),
  number of games, winrate, main roles, last game seen. Order: exact > prefix > substring,
  then by number of games.
- Empty / too short (< 2 characters): no request.

### Profile (`/players/:toon`)
- Current name, **3 most recent former names** + "see all (N)" which opens the complete list
  (potentially very long list: collapsed by default, scrolling, never silently truncated).
- If a **high-confidence** linked account exists (or a confirmed link): an "Also plays as
  X →" line to the Identity tab. Low-confidence links are not shown here.
- Tabs: Stats · Heroes · Games · Identity.

### Identity tab
- Name history, **dated**: "StormyNight, seen Jan–Aug 2025".
- Guessed linked accounts: name, level (HIGH/LOW) + score, detailed reasons (see signals).
- Actions per suggestion: **Confirm** ("same person" note) · **Not the same person** (dismisses it and
  it is no longer suggested). Confirmed links listed separately.

## Similarity signals (account A ↔ account B)

Data available today (checked in the code): `match_players` (hero, team, talents, win),
`matches.played_at`, `players.names` (flat list, **without dates**), `uploads.token_id` (uploader).
**No group/premade information in the data**: "same friends" is approximated by
recurring teammates.

| Signal | Strength | Note |
|---|---|---|
| **Owner of the same token**: a token's main account (present in ≥ 50 % of its ≥ 50 games) and any other account present in ≥ 10 % of those games **without ever sharing a game with the main one or with another owner already retained** (processed from largest to smallest: a friend who only duos with the smurf is not an owner) (a person does not play two accounts in the same game; frequent friends play with the main one, they do not count) | strong | raw "same uploader" is useless: every replay contains all 10 players. Does not prove it on its own (lent PC) |
| Common recurring teammates | medium | approximation of the group of friends; **excluding the operator (`operator_names`) and token owners**, otherwise everyone is linked through them |
| Hero pool overlap | weak-medium | on its own, never enough for "HIGH" |
| Talent habits on the same heroes | medium | |
| Playing time slots | weak | never on its own |
| Time exclusion | negative | two accounts playing **at the same time** in the same game or in overlapping games ≠ same person |

HIGH level: requires at least **two independent signals**, one of which is "strong or medium". The exact
weights and thresholds are left to the implementation plan, to be tuned on real cases (no
arbitrary value frozen here).

## Amendments (2026-10-25)

1. "Same uploader" signal redefined as "token owner" (definition refined at implementation: main ≥ 50 % + other accounts ≥ 10 % never in a game with it, because two accounts of the same person share the games, they cannot each exceed 50 %); operator and token owners excluded from teammates and from the preselection (finding on the dev corpus: one token = 2,719 of the 2,723 games).
2. **Current name, "was …" and "3 most recent" come from the dated history** (`match_players` + `matches.played_at`, NULL = oldest), not from `players.last_name` (overwritten in processing order). `players.names` is only used as a set for matching.
3. Front-end routes: `player/:toon` (existing) kept; `/players` = search. Replaces `/players/:toon`.
4. No `CREATE EXTENSION`: search with `lower()` + `ILIKE`; trigram only if measurement requires it, via a separate approved migration. Accent insensitivity outside v1.
5. Coverage: a player seen **only** in a scouting report (isolated tables, migration 0010) is not searchable.
6. Scoring is a pure function: `crates/storm-codex-server/src/players/similarity.rs`.

## Data

- `players.names` (flat JSONB) is kept as is. The **dated history** is derived from
  `match_players` + `matches.played_at` (first/last day seen per name) — derivable, so no
  additional stored data to re-process.
- New table for the decisions: `player_links (toon_a, toon_b, status ∈ confirmed/dismissed,
  decided_at)`, pair stored ordered. The suggestions themselves are **computed**, not stored.
- One migration (`0011_player_links.sql`), `parser_version` not affected. No extension (see Amendments).

## API (to be documented in `docs/spec/05-api.md` at implementation)

- `GET /api/players/search?q=` — search results.
- `GET /api/players/{toon}` — existing; `name`/`names` come from the dated history.
- `GET /api/players/{toon}/identity` — dated history + guessed linked accounts + decided links.
- `PUT /api/players/{toon}/links/{other}` 🔒 `{status}` — confirm / dismiss.

## Visibility and rights

- **Everyone sees** the Identity tab and the "same person" notes (operator decision).
- **[my choice]** Confirm/dismiss is an admin write: guarded by `ADMIN_TOKEN` like the other
  writes (open mode if it is not set), so that a visitor cannot rewrite the links.
- Accepted risk: the guesses are visible to everyone; hence the permanent "estimate" label
  and no link displayed at low confidence on the main profile.

## Constraints

- Perf budget: `GET /api/players/search` p95 < 100 ms (the programme spec's API budget); computing
  linked accounts may be slower but must stay bounded (preselected candidates, no
  pairwise comparison of all players); to be measured on the real corpus.
- Front-end: React + TanStack, Nexus Codex tokens (`ds.css` / `theme.css`), mockup approved visually
  (`.superpowers/brainstorm/…/search-and-profile-nexus.html`).
- Docs: update `docs/spec/05-api.md`, `06-modele-donnees.md`, `08-frontend.md` in the same
  commit as the code (rule no. 0).

## Left to the implementer

- Signal weights/thresholds; candidate preselection algorithm; trigram vs `ILIKE`;
  exact components; format of the displayed reasons.

## Open questions for the operator

1. Do you approve that **confirm/dismiss** is reserved to the admin (`ADMIN_TOKEN`)?
2. The length of the "seen Jan–Aug" history depends on the replays: is it acceptable that renames
   prior to the replays are invisible?
