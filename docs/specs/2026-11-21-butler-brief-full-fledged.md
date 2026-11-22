# Butler brief, full-fledged (2026-11-21)

## Current state

`web-rs/core/src/session.rs::jarvis_phrase(won, hero, deaths, takedowns, streak)` returns one of 2-3
French variants per bucket (win streak ≥3, no-death win, ≥15-takedown win, plain win, loss streak ≤-3,
high-death loss, plain loss). It is pure, deterministic (`(takedowns+deaths) % variants.len()` as the
pick seed — no RNG, same inputs always produce the same line, so it's testable and replayable), no I/O.
Called once per render in `web-rs/sources/src/sessions.rs::queue_content` for the most recent game, shown
in the `/queue` OBS source next to the session scoreboard. Always-on, French only, no setting to disable.

## What's already computed locally that the brief doesn't use yet

No new data collection needed — everything below is already available where `jarvis_phrase` is called,
just not threaded through:
- **MVP award** (`MatchPlayer.award`, parsed by `award_label`) — `/queue`'s `QueueRow.award` already
  reads it per row; the brief ignores it.
- **The whole roster, both teams** (`MatchSummary.players`) — the opponent team is `players.filter(team
  != me.team)`, already how `nemesis.rs::enemy_toons` resolves opponents for the `/nemesis` lobby badge.
  Cross-referencing "did I just beat my tracked nemesis" is a pure toon-set intersection, no new fetch.
- **Session totals so far** (`session_totals`: kills/assists/deaths/takedowns/kda/avg length) and the
  full `games` list for the session — "best KDA of the session tonight" is a comparison against the
  other games already in that `Vec`, not a new stat.
- **Map and mode** (`MatchSummary.map`/`mode_badge`) — currently unused by the brief entirely.

## Options considered

1. **Keep as is.** Zero cost, zero gain — not what was asked.
2. **Call a local LLM (e.g. Ollama) for a generated line.** Rejected: adds a runtime dependency, non-determinism (can't unit test exact output), latency on a widget that must render instantly, and cuts against "fully local, no moving parts" — this app just removed its *one* external dependency (Jarvis/Redis) for that exact reason. A hand-written deterministic engine with more trigger categories gets the "feels aware of the game" effect without any of that cost.
3. **Expand the deterministic trigger engine + make it configurable.** Chosen — detailed below.

## Decision: expand the trigger set, add a setting

### New trigger categories (priority-ordered; first match wins, each with 2-3 phrase variants like today so the existing anti-repetition seed still works)

Priority, highest first (a game can only match one bucket, so order encodes "most noteworthy wins"):
1. **MVP this game** (win or loss — MVP in a loss is still worth a line: "MVP malgré la défaite, mes hommages, monsieur.").
2. **Beat the tracked nemesis** (win, and the opponent roster intersects the current nemesis toon from `nemesis.rs` — needs the nemesis toon passed in alongside the match, available wherever `/nemesis` already resolves it).
3. **Win streak ≥3** (existing).
4. **Session-best KDA tonight** (win, and this game's personal KDA for the session ≥ every earlier game's — computed from `games`, the data `queue_content` already holds).
5. **No-death win** (existing).
6. **≥15-takedown win** (existing).
7. **Plain win** (existing, 3 variants).
8. **Loss streak ≤-3** (existing).
9. **High-death loss (≥8)** (existing).
10. **Plain loss** (existing, 3 variants).

Map/mode flavor: *not* a separate trigger tier (16 maps × 10 categories would dilute the phrase bank for
little payoff); instead, splice the map name into roughly a third of each bucket's variants where it
reads naturally (already done for the hero name in several buckets today) — keeps it simple, still feels
specific.

### Configurability
Add `brief_voice: BriefVoice` to the existing settings (`operator_names`'s neighbor in `AppSettings` /
`PUT /api/admin/settings`), enum `Fr | En | Off`, **default `Fr`** (zero behavior change for the existing
install until the operator changes it). `En` is a straight English phrase bank, same trigger structure,
written fresh (not translated 1:1 — idiom, not a literal translation) so a non-French streamer gets the
same feature. `Off` makes `jarvis_phrase` return `None` and `/queue` simply omits the line (layout already
handles `jarvis: Option<String>`, see `sessions.rs:453-457` — the spacer/conditional already exists, no
layout work needed for `Off`).

### Signature
`jarvis_phrase(won, hero, deaths, takedowns, streak, mvp: bool, beat_nemesis: bool, session_best_kda:
bool, map: Option<&str>, voice: BriefVoice) -> Option<String>` — still pure, still no I/O, still unit
tested per bucket. Call site (`queue_content`) gains: `g.me.award` MVP check (already parsed), nemesis
toon threaded in (new parameter to `queue_content`, sourced the same way the `/nemesis` source already
gets it — check whether `/queue` already receives lobby/nemesis context or needs a new one; if not
already wired, the nemesis check degrades to always-false rather than adding a new live subscription —
don't make `/queue` depend on `/lobby` being open just for this one line), session-best-KDA computed from
`games` already in scope.

## Out of scope (not assumed approved here)
- Any outbound call (local LLM, TTS) — rejected above.
- A full persona rename/rebrand — "Jarvis"/butler stays as today, this is the existing feature's name.
- Localization beyond Fr/En (more languages are an easy follow-up to the same structure, not blocking v1).
- Showing the line anywhere other than `/queue` (e.g. `/widget`, `/ticker`) — those sources don't currently render it; adding it there is a one-line follow-up once this lands, not required for v1.

## Verification
- Unit tests per new trigger bucket (MVP win, MVP loss, nemesis beaten, session-best-KDA, each `BriefVoice`
  including `Off` returning `None`), plus the existing bucket tests still passing un-regressed.
- `cargo test --workspace`, native+wasm clippy, `trunk build --release`, bundle budget (two phrase banks
  instead of one adds a modest, bounded amount of WASM text — must stay under the Brotli cap).
- Browser: `/queue` renders the FR line (default, unchanged), renders the EN line with the setting
  switched, and renders no line with `Off`.

**Status: APPROVED 2026-11-21** (operator: "look into jarvis butler brief flavor... I want a full
fledge func when you are done" — this doc is the full detail asked for; build it as approved, same
session).
