# hotsdash.com gap analysis (2026-11-17)

Issue: #34. Research only: no code, no spec. Ideas are reimplemented, never copied (no code or assets from hotsdash.com).

## Method

- hotsdash.com (v1.127.0) browsed signed-out on 2026-11-17: home, example player page, one match (all 8 tabs),
  heroes, meta, maps, records, leaderboards, compare, live, streamers, eSports, guilds, feed, Discord bot,
  stream overlay, upload, about.
- storm-codex side: `docs/spec/03-storm-stats.md`, `docs/spec/05-api.md`, `docs/spec/08-frontend.md`, and a
  keyword grep of `web-rs/` and `crates/`. Items marked **verify** were not confirmed either way.
- Signed-in pages (live game check, "ways to improve", guild pages) were not visible; their content is
  taken from the site's own descriptions.

## Context

hotsdash.com is a multi-user web service (Google sign-in, community uploads). storm-codex is single-operator,
local-first and self-hosted. Features that only make sense with many users are listed as out of scope.

## What storm-codex already covers

Match page (draft, scoreboard, talents, level advantage, XP, timeline, chat, pings, APM), 2D replay viewer,
player pages with aliases and linked accounts, heroes, maps, synergies, trends per patch, patch notes,
live in-game feed, scouting reports, draft simulator, cast control room and OBS sources, CSV/JSON export,
standalone Windows installer.

## Gaps

Effort: S < 1 day, M a few days, L a week or more. Data: what storm-stats already gives us.

| # | Feature on hotsdash | What it does | Data | Effort | Value for us |
|---|---|---|---|---|---|
| 1 | Records | Best single games per stat (kills, damage, healing, XP, camps...) | Score screen, have it | S | Medium: fun, cast material |
| 2 | Early leads | Win % for first blood, first to 10, first fort, first to 20 | Have it (takedowns, levels, structures) | S | Medium |
| 3 | Season filter | Filter every page by ranked season | Season calendar to add (static table) | S | Medium |
| 4 | Team fights table | Fights grouped by time and place, who lost what, "then N buildings fell" | Takedowns have time and position; fight detection is new (storm-stats only has end-of-game teamfight totals) | S | High |
| 5 | Camps tab | Camps per team and side, why taken (before an objective, after a won fight, quiet moment) | Camp times have it; classification is new logic | S–M | Medium |
| 6 | Team read | Per game: who carried or struggled against a typical game on that hero, level gap, deaths spread | Score screen; needs per-hero baselines from our own games (small sample) | M | High |
| 7 | Team shape | Team strengths/weak spot (damage, sustain, CC, waveclear, siege, soak) against random line-ups | Per-hero averages a minute; same baseline as #6 | M | Medium |
| 8 | Personal insights | Nemesis (who kills you most), cursed map, heroes to play more, % of deaths alone, deaths for a globe, thrown leads (lost after +2 levels), hearths cancelled | Takedowns, XP periodic, globes have it; hearths not parsed; "alone" needs positions | M | High |
| 9 | Head to head | Any two players side by side: records, habits, games shared | Have it; Synergies covers the operator only | S–M | Medium |
| 10 | Calls analysis | Each ping marked followed, partly or ignored, deaths after an unheeded Retreat | Pings have it; "followed" needs hero positions over time (replay2d data) | M–L | Medium |
| 11 | Map habits / heatmap | Background built from where heroes went across many games, bushes and lanes layers, follow a hero | replay2d positions; aggregation is new | M | Medium |
| 12 | Replay path filling | Moves heroes along routes around walls between the ~15 s samples instead of straight lines | Map wall data needed | L | Low–Medium |
| 13 | Hearths, B-steps, sprays per game | Counts and % cancelled per player | Sprays and B-steps partly shown; hearths not parsed | S | Low |
| 14 | Chat mood | Mood score per line and over the game | Needs chat text and sentiment scoring; conflicts with our "never the text" rule on player chat | M | Low; skip unless the rule changes |
| 15 | Live lobby record | While a game loads: who you've met before, your record with and against them, nemesis alert | storm-lobby gives the lobby; history join is new (**verify** what the companion shows today) | M | High (streaming) |
| 16 | Personal draft helper | What to ban and pick on this map from your own record against each hero | Have it; plugs into the draft simulator | S–M | Medium |
| 17 | Twitch chat commands and panels | `!hotsdash`, `!heromap` answers in Twitch chat; Twitch panels | New Twitch integration (R2: auth, dependency) | M | Medium (streaming) |
| 18 | Discord bot | `/hots me`, `/hots last`, `vs`, server leaderboard, scout, game posts, weekly recap | New Discord integration (R2) | L | Low for a single operator |
| 19 | Year in review | Yearly recap page | Have it | M | Low–Medium |
| 20 | Share picture | "Save picture" of a match card | We already have HTML export; PNG is new | S | Low |
| 21 | Ask | Free-text questions ("when did we last win with a Butcher?") | Would need a local LLM (Jarvis) over the DB | L | Low–Medium |
| 22 | Skill rating | Own rating per account, "most improved" | We record the Storm League rank read from the end-of-game screen (`rank.rs`); no computed rating | M | Low with one team's games |

## Out of scope (multi-user service features)

Google sign-in and accounts, community leaderboards (overall, heroes, maps, votes), guilds, public feed,
streamers directory with live Twitch status, eSports hub (leagues and tournaments; our cast control room
already covers our own casting), Linux/macOS uploader, browser folder sync, analytics.

## Recommendation

1. Quick wins, one small spec: Records (#1), Early leads (#2), Season filter (#3), Team fights on the match
   page (#4).
2. Coaching batch: Team read (#6) and Personal insights (#8). Shared prerequisite: per-hero baselines from
   our own games, with a minimum sample size and a visible "small sample" warning.
3. Streaming batch: Live lobby record (#15), then Twitch commands (#17) if wanted.
4. Later, as a separate design: Calls analysis (#10) and Map habits (#11), both built on replay2d positions.

Each approved item needs a dated spec in `docs/specs/` before any code (`gads/PROJECT.md`).

## Open questions for the operator

- Which batch first?
- Hearths: parse them (needed by #8 and #13)?
- Twitch and Discord integrations (R2): wanted at all?
