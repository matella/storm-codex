# Storm Codex

A ground-up renovation of [Stats of the Storm](https://github.com/ebshimizu/stats-of-the-storm) —
a Heroes of the Storm replay stat tracker — rebuilt as:

- **`storm-replay`** — a Rust crate decoding `.StormReplay` files (MPQ + versioned bit-packed
  protocol, tables generated from [Blizzard/heroprotocol](https://github.com/Blizzard/heroprotocol)).
- **`storm-stats`** — a Rust crate turning decoded replays into rich match stats: per-map
  objective timelines, team fights, XP/level advantage, taunts/BM detection, enriched takedowns,
  awards — a full port of [hots-parser](https://github.com/ebshimizu/hots-parser)'s logic.
- **`storm-codex-server`** — a single-binary axum server: authenticated replay upload, parallel
  parse workers, PostgreSQL projections, WebSocket push (end of game → page updated in < 5 s),
  full-archive backfill, raw-stream decode on demand.
- **A fast SPA** (React + Vite) with complete Stats-of-the-Storm feature parity — matches, match
  detail, players, heroes, talent builds, compositions, trends by patch, teams & leagues,
  collections, maps, rankings — plus an OBS stream widget for post-game summaries.

Status: **design phase complete** — implementation starts with a go/no-go decode spike (jalon 0).

## Documentation (start here)

| Doc | What |
|---|---|
| [`docs/STATUS.md`](docs/STATUS.md) | Where the project is, what's next — read first |
| [`docs/specs/2026-06-12-storm-codex-design.md`](docs/specs/2026-06-12-storm-codex-design.md) | The validated program design (architecture, data model, perf budgets, milestones) |
| [`docs/specs/2026-06-12-storm-codex-mockup.html`](docs/specs/2026-06-12-storm-codex-mockup.html) | Visual reference — all 14 screens (open in a browser) |
| [`docs/research/2026-06-12-stats-of-the-storm-renovation.md`](docs/research/2026-06-12-stats-of-the-storm-renovation.md) | Research dossier: SotS anatomy, dependency verdicts, parser-engine comparison, ecosystem |
| [`docs/research/hots-replay-data-reference.md`](docs/research/hots-replay-data-reference.md) | Complete `.StormReplay` data location reference (from SotS docs) |

## Acknowledgements

Stats of the Storm and hots-parser by [@ebshimizu](https://github.com/ebshimizu) (MIT) — this
project is a re-architecture of those ideas, not a fork. Replay format reference courtesy of the
same project. Heroes of the Storm™ is a trademark of Blizzard Entertainment, Inc. This project is
not affiliated with Blizzard.
