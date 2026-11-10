# Hotkey profile in replays → grouping the accounts of the same person — design

> Status: **approved by the operator on 2026-11-08** (source = replay, not the uploader; `0_default` and
> names ≤ 2 characters ignored, shared profile = HIGH on its own, generic threshold 5 accounts). Threshold 5 to
> be tuned on a probe of the real corpus. Complements `2026-10-25-recherche-joueurs-design.md` (linked accounts).

## Intent

Replays never link two accounts directly, but each client writes **the name of the player's
keyboard hotkey profile** into the replay. That name is chosen by hand (`Kure1`, `munky`…): finding
it on two accounts is a very strong hint of "same person", and it names the person.

## Findings (checked on 2026-11-08 on a real replay)

- The game event stream (`--stream game`) contains, at `_gameloop` 0, one
  `NNet.Game.SUserOptionsEvent` event **per player** with `m_hotkeyProfile`. Values seen in a single
  replay: `PrizraK`, `samuro`, `0_default`, a Cyrillic name. So it is the **active profile of every
  player in the game**, not only the operator's.
- Attachment to an account: `_userid.m_userId` → `toon_handle` through the `playerLobbyID` mapping that
  `storm-stats` already builds (`process/mod.rs`, around line 481).
- Nothing is extracted today: no occurrence of `hotkeyProfile` in `crates/` or `web/`.
- Consequence: **no need for the uploader or for access to the PC**. The whole already-archived corpus (idempotent
  re-process via `parser_version`) yields the signal, for all players met.
- On the operator's PC, `Variables.txt` (`hotkeyprofile=`) is `English` everywhere: useless here, we
  do not use it.

## Decisions

1. **Extraction**: when processing a replay, read `SUserOptionsEvent.m_hotkeyProfile` for each
   player (excluding observers) and store it per `(match, toon)`. No new data source.
2. **Names ignored by default**: `Standard`, `Standard for Lefties`, language names (English and
   native form: English, Français, Deutsch, Español, Русский, 한국어, 中文…), and `0_default` (actual
   value seen in replays). Comparison ignores case and surrounding spaces. The list
   can be overridden via `app_settings` (`ignored_profile_names`). **[my choice]** names ≤ 2 characters
   are ignored too.
3. **Automatic generic name**: a name carried by too many distinct accounts no longer identifies
   anyone (e.g. `samuro`, a hero name). **[my choice]** initial threshold: ≥ 5 distinct accounts, **to be
   tuned on a probe of the real corpus** before freezing it (same approach as `sonde_identite_corpus_reel`).
4. **"Shared profile" signal**: two accounts that have each carried, in at least one game, the same
   non-ignored name. Strength: **strong**. **[my choice]** it is enough on its own to reach the HIGH level, while
   remaining subject to the time exclusion (two accounts in the same game ≠ same person).
5. **Profile history per account**: profile names are kept per account with dates
   (first/last game), like the name history. A profile change without a new
   name stays readable.
6. **Display**: in the Identity tab, a "Shared profile `munky`" reason in the suggestions and
   a "Profiles used" line on the page. A link remains a simple note (no stats merge).

## Name history of an account (related request)

The `toon_handle` is stable, the name changes. **Already in place**: `players::names::name_history`
(dated names, most recent first) exposed by `GET /api/players/{toon}/identity` and shown in
the Identity tab (`PlayerIdentity.tsx`, "show all N"). Nothing new to build on the data side;
if the operator wants to see it elsewhere (profile header, search results), that is a display
addition to specify separately.

## Data and API (to be detailed in the plan)

- Storage of the profile per `(match_id, toon_handle)`: dedicated column on `match_players` or a field of
  `data`; **choice left to the plan** after measuring the cost of the grouping query.
- `parser_version` incremented to trigger the re-process of the corpus.
- `similarity.rs`: `profile` signal; `identity.rs`: preselection adds accounts sharing a rare
  name (priority path along with token co-owners).
- Docs to update in the same commit: `docs/spec/05-api.md`, the schema/projection spec,
  `08-frontend.md`.

## Out of scope

- Content of the hotkeys (not present in the replay, and useless).
- Any real-world identity: the profile name is a nickname chosen by the player.
- Changing `client-rs` (now unnecessary).

## Open questions for the operator

1. Additional ignored names: `0_default` (seen in replays) and names ≤ 2 characters?
2. Does a shared profile alone give HIGH, or is a second signal needed?
3. Generic name threshold: 5 accounts, to be confirmed by a probe on the corpus.
