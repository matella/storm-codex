# Local insights: records, early leads, team fights, time windows, camps, head to head, personal insights

Date: 2026-11-17 · Status: **DRAFT, awaiting operator approval** · Issue #34 · Research:
`docs/research/2026-11-17-hotsdash-gap-analysis.md` · Branch `feat/local-insights`

## Goal

Add the easy, fully local features found in the hotsdash.com review, plus damage-per-time-window as the
operator asked. Everything runs the same on the box and in the standalone app.

## Constraints (standalone fit)

- Read-only, computed from data already stored (`matches.data`, `match_players.data`, storm-stats JSON).
- No migration, no new dependency, no network call, no account, no Twitch or Discord.
- New read endpoints live in the existing axum server, so the standalone app gets them for free.
- web-rs app stays within its 1 MB brotli budget (`scripts/check-web-rs-size.sh`).
- Pure logic (clustering, classification, window maths) lives in `web-rs/core` or a server module with
  unit tests; endpoints get integration tests like the existing read endpoints.

## Data available (checked on the box DB, 3,480 games)

- `match.takedowns[]`: `time` (s), `x`/`y` (fixed point), `victim{player,hero}`, `killers[]`.
- `match.levelTimes[team][level].time`, `match.XPBreakdown[]` (per team, **every 60 s**), `match.firstFort`,
  `firstFortWin`, `firstKeep`, `levelAdvTimeline`.
- `match.structures{}`: `team`, `name`, `destroyed` (s) when destroyed.
- `match.mercs.captures[]`: `time`, `team`, `type` (**no position**).
- `match.objective`: per-map objective events with times.
- Players: `gameStats` (end-of-game score screen), `deaths[]`, `takedowns[]`, `globes`, `bsteps`, `sprays`.

### Damage per time window: not in the replay

The replay stores hero, siege and healing totals **once, at the end of the game** (`SScoreResultEvent`,
tracker event 11). No event gives damage over time, so "damage from time A to time B" cannot be computed
from a `.StormReplay`. The window view (F4) therefore shows what *is* timestamped, and shows the end-of-game
`TeamfightHeroDamage` / `TeamfightDamageTaken` / `TeamfightHealingDone` totals as whole-game context,
labelled as such.

## Features

### F1 Records (`/records`, `GET /api/records`)
Top 5 single games per stat: kills, takedowns, assists, deaths, hero damage, siege damage, healing,
damage taken, XP contribution, mercenary camps, self-healing, time CC'ing heroes. Filters: mode, map, hero,
"my accounts only" toggle (`operator_names`). Each row: value, player, hero, map, date, link to the match.
Operator rows highlighted.

### F2 Early leads (block on `/maps`, field `early_leads` in `GET /api/maps`)
Win % of the team that got first blood, first to level 10, first fort, first to level 20, over the filtered
games and per map. Also the operator's own rate when their team got there first. Rows with no data (e.g.
ARAM forts) are hidden.

### F3 Team fights (match page section)
Deaths clustered by time and place: a death joins the current fight if it happens ≤ 15 s after the previous
death of that fight and ≤ 30 map units from the fight's centre; a fight needs ≥ 2 deaths. Per fight: start
and end time, heroes lost per side, result (side that lost fewer heroes; equal = even), and what followed
within 30 s (structures destroyed, camps taken, objective events). Summary line: "Your team won N of M".
Clicking a fight selects it as the F4 window.

### F4 Time window (match page section)
Pick a window: a fight from F3, or a custom range (two time inputs, mm:ss). Shown for the window:
- per player: kills, assists (takedown participation), deaths;
- per team: levels reached, XP gained (from the 60 s samples, so the window is widened to whole minutes and
  the UI says so), structures destroyed, camps taken, objective events;
- whole-game teamfight totals per player as context, labelled "whole game, not this window".
Window state is in the URL (`?from=&to=`) so it can be shared.

### F5 Camps (match page section)
Camps per team and per type, each capture classified by timing: before an objective (≤ 60 s before the
next objective event), after a won fight (≤ 30 s after an F3 fight the team won), traded for an objective
(the enemy took an objective within ± 20 s), or quiet moment. No map side (captures have no position).

### F6 Head to head (`/compare?a=<toon>&b=<toon>`, `GET /api/compare?a=&b=`)
Two players side by side: games, win rate, K/D/A per game, top heroes, roles; games together (same team,
record) and against each other (record each way), with links to those games. Player pickers reuse
`/api/players/search`. Entry points: Player page "Compare" button.

### F7 Personal insights (Player page "Insights" tab, `GET /api/players/{toon}/insights`)
- Nemesis: the account that killed this player most (kills, games, most-used hero against them).
- Thrown leads: % of losses after leading by ≥ 2 levels at some point; comebacks: % of wins after
  trailing by ≥ 2 levels.
- Deaths alone: % of deaths that are not part of an F3 fight.
- Best and worst map, heroes to play more / less (≥ 10 games, from existing aggregates).
Small-sample warning under 20 games, like the scouting reports.

## Out of scope (deferred, need their own spec)

Season filter (needs a verified season calendar), team read against per-hero baselines, ping follow-up
analysis, position heatmaps, personal draft helper, lobby "met before" record, parsing hearths.

## Contracts touched (Rule 0)

- `docs/spec/05-api.md`: `GET /api/records`, `GET /api/compare`, `GET /api/players/{toon}/insights`,
  `early_leads` in `GET /api/maps`.
- `docs/spec/08-frontend.md`: routes `/records`, `/compare`; match page sections F3–F5; Player "Insights"
  tab; Maps early-leads block.
- No change to `06-modele-donnees.md` (no migration).

## Verification

- Unit tests for fight clustering, camp classification, window maths, thrown-lead detection.
- Endpoint tests on fixture replays; numbers cross-checked by hand on 2 real games.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `scripts/check-web-rs-size.sh`.
- Endpoint latency measured on the box DB (3,480 games); target < 300 ms each.
- Standalone: Windows acceptance run on mh-home, can't be done from matelab.

## Delivery order

F1, F2 → F3, F4 → F5 → F6 → F7, one commit per feature (code + spec docs together), one PR.
