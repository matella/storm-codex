# OCR reading of the end-of-game rank + "recent games" overlay card — design

**Status**: approved by the operator (2026-10-31). Decisions 1 to 5 retained as written (digits read with templates, not generic OCR). Implementation not started: the raw captures from the gaming PC are still to be provided.
**Date**: 2026-10-31
**Areas**: Aegis (`aegis/crates/aegis-uploader`, capture + OCR), storm-codex (`crates/storm-codex-server/src/rank/`, `web/src/pages/`), spec `docs/spec/05-api.md` and `docs/spec/07-evenements.md` to be completed at implementation.

## Context

MMR is not in the replay. The ranking (rank, rank points gained or lost, total, points required for promotion) only appears on screen. Captures from a Twitch VOD (2026-10-31) show the end-of-game flow:

1. **Loot chest** (not always present): no ranking points.
2. **Summary, before loading**: "Rank Points" counter at `0`, rank shown (Diamond 1). A value never to be read.
3. **Hovering the rank**: "Rank Points Change" tooltip (*Match Points*, *Total Rank Points*). The counter is still at `0`.
4. **Animation**: the counter rises (`227` observed) and the ring fills. Interruptible.
5. **Play page, Ranked**: rank, **total points** (535 observed, shown only when hovering the rank), **points required for promotion** (465 observed). 535 + 465 = 1000.

In a placement game, no rank is shown: nothing is read and nothing is published.

## Goal

1. **Aegis** reads the rank after each ranked game and sends a **validated** or **unavailable** result to storm-codex.
2. **storm-codex** keeps the result with the game and displays it in an OBS overlay: a persistent compact card, which expands at the end of each game. It shows the account's last ten games, then the KDA and the points gained or lost when they arrive.

## Out of scope

- No mouse control or clicking in the game. Hovering the rank to show the tooltip is **not** automated: we only read what is already displayed.
- No Twitch extension or Azure (abandoned, see `docs/STATUS.md`).
- No reading of placement games (no rank shown).
- No change to the Hots-Overlay uploader (`client-rs`): it is replaced by Aegis.

## Architecture

### Aegis: the reader

- Trigger: appearance of `replay.details` in `TempWriteReplayP1` (already detected by `aegis-uploader/src/live/`). To be checked: whether this file arrives **before** or **after** the on-screen summary. If after, the OCR waits for the summary; otherwise, it waits for the game window.
- Capture: the Heroes of the Storm window only, via `Windows.Graphics.Capture` (Windows' capture API, not the whole desktop). On an HDR screen, capture the scRGB image and convert it back: the 8-bit capture comes out over-saturated (Heroes Profile's finding). Frequency: one frame per second is enough for the end screen, which stays displayed for several seconds.
- Reading the digits: **digit templates**, not generic OCR. The total and required digits are in a fixed font; they are compared to templates built from labelled samples, versioned in the binary, with a test that rebuilds them. `Windows.Media.Ocr` remains possible for text (VICTORY/DEFEAT title), not for digits.
- State machine (pure, testable on recorded images):
  - `Waiting` → `Summary` ("VICTORY" or "DEFEAT" title read) → `Stable` (same total value read on **N = 3 consecutive frames**, N to be calibrated) → `Published`.
  - Possible exit at any time: `Unavailable` after a 60 s delay without a stable reading.
- Consistency check before publication: **total + required for promotion = 1000** read on the Play page. If the check fails, `unavailable` is published with the reason, never an estimated value.
- Kept in memory until the end of the game: the readings (and sample images) are **not** sent until the game mode is known. Only Storm League (or ranked) mode triggers sending; other modes are discarded (idea taken from Heroes Profile).
- Sending: `POST /api/rank/result` with the `X-Session` header = SHA-256 of the `battlelobby` (same identifier as the live feed), so storm-codex attaches the result to the game even if the OCR finishes before the parse.
- Samples (opt-in, disabled by default): Aegis can send a few labelled captures to calibrate the templates. Never automatic.

### storm-codex: the server

- New table `ranked_results` (next migration): `session_id`, `match_id` (filled in when the game is parsed), `rank_name`, `total_points`, `points_delta` (nullable), `promotion_required`, `status` (`valide` | `indisponible` — French database values meaning valid | unavailable, without accents), `reason`, `read_at`. A single reading per session: the latest **valid** reading wins; an `indisponible` reading never overwrites a `valide` one.
- Routes:
  - `POST /api/rank/result` (Aegis, box token);
  - `GET /api/rank/recent?player=<toon>&limit=10`: the account's last ten games with KDA and rank result when known.
- Attachment: if the game is not parsed yet, the result stays pending on `session_id`, then is linked to the `match_id` when the replay arrives. Attachment is done by **game fingerprint** (`crate::upload::game_fingerprint`, already used for deduplication) rather than by `session_id` alone. Pending: expires after 6 h, like Heroes Profile's `PendingRanks`.
- Events (compliant with `docs/spec/07-evenements.md`, `schema_version`, `correlation_id`, `occurred_at` / `recorded_at`, past-tense `entity.verb` form):
  - `hots.match_finished` (as soon as parsed: KDA, result, mode) → the card expands without waiting for the OCR;
  - `hots.rank_read` (validated or unavailable) → the card's ring fills or shows "rank unknown".

### Overlay: the card

- Same model as `/now-playing` (`?mini`, `?reveal`): a persistent source, then an enlarged announcement at each end of game. Transparent background, top-right anchoring.
- Proposed route: `/ranked` (OBS source), `?me=<toon>` parameter like `/widget`.
- Contents:
  - **Mini**: last ten games (win/loss pips) and current rank;
  - **Announcement** (end of game): KDA and hero as soon as `hots.match_finished`; points gained or lost and progress ring as soon as `hots.rank_read`; otherwise "points pending" or "rank unknown", without blocking the announcement.
- The announcement never depends on the OCR: it is displayed with the server's data, and the OCR completes it when it arrives.

## Validation rules

- A read value is only published after **stability** (N identical frames) **and** the 1000 check.
- An unavailable result is published as such. No value is inferred (for example from the previous total).
- Numbers are read on the Play page after the animation, or on the summary once the counter is stable, never during the animation.
- Recognition must distinguish VICTORY and DEFEAT in the summary title to check the direction of the gain.

## Tests

- **Aegis state machine**: unit tests on a sequence of described frames (no real image): stability, interruption, unavailable after 60 s, rejection if total + required ≠ 1000, no publication in placement.
- **OCR**: set of raw captures from the gaming PC (not committed if they contain player names), with the expected reading annotated. Criterion: 100 % correct readings on the validated captures, zero wrong publications.
- **storm-codex**: attachment of a result that arrived before the parse; `GET /api/rank/recent` without a rank result; well-formed events.
- **Overlay**: rendering of `?mini` and of the announcement; checked for real in OBS on a real game.

## Reference: Heroes Profile's RankCapture

The [HeroesProfile.Uploader](https://github.com/Heroes-Profile/HeroesProfile.Uploader) project already reads ranks
in game (C#, Windows/macOS/Linux). What we take from it, and what we do not:

- **Taken (ideas)**: capturing the game window only, HDR handling, game mode known before sending,
  results kept pending while the replay arrives, glyph templates rather than OCR, opt-in samples
  for calibration.
- **Different**: they read the **loading** screen (league and division of each of the ten players, not the
  points). Points gained or lost only appear on the end screen, so our reading stays at the end of
  the game. Loading remains a later option (starting rank of the ten players), not a V1.
- **Licence**: the repository shows no recognised licence (`NOASSERTION`). We copy no code: we
  re-implement the ideas. Any code reuse first requires checking the licence.

## Data needed (to be provided)

- **Raw** captures (not a compressed VOD) of the complete sequence, in a ranked game, with the game window in fullscreen and windowed.
- A placement game, to check the "no rank" case.
- A loss and a win, to check the direction of the gain.

## Decisions to approve

1. **Aegis** handles capture and OCR (proposed), storm-codex only stores and displays.
2. **Reading digits**: templates built from labelled samples, not generic OCR (proposed, see Architecture). `Windows.Media.Ocr` only for text.
3. **Overlay**: new `/ranked` route modelled on `/now-playing`, and not an extension of `/widget` (proposed). `/widget` stays the last-game card.
4. **Stability**: N = 3 consecutive identical frames before publication (proposed, to be calibrated on the captures).
5. **Games without a reading**: published as `indisponible` after 60 s (proposed), never estimated.
