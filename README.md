**English** · [Français](README.fr.md)

# Storm Codex

Storm Codex turns your Heroes of the Storm replays into a match history you can browse and analyze. Review a fight on the map, compare two players, or see how your results vary by hero and battleground. The Windows app stores your replays and stats on your PC, with no Storm Codex account required.

## Run it on Windows

Check [GitHub Releases](https://github.com/matella/storm-codex/releases) for a published `StormCodex_<version>_x64-setup.exe`. If no installer is listed, there is no public Windows build yet. The installer bundles PostgreSQL and manages it for you.

1. Launch Storm Codex. It looks for the game's standard replay folders and imports completed `.StormReplay` files.
2. If you keep replays somewhere else, add the folder under **Admin → This PC**. Check **My identity** if the app hasn't picked up your account.
3. Open **Matches** to browse the import. Leave the app running to pick up new replays after each game.

The match page includes a 2D replay, takedowns, objectives, camp captures, and detected fights. You can select a time range to review events, but replays only provide damage and healing totals for the whole match.

## Self-host with Docker

Use [storm-codex-suite](https://github.com/matella/storm-codex-suite) for the Docker Compose setup. See the [operations guide](docs/spec/09-operations.md) for server configuration, OBS sources, and desktop release instructions. The desktop replay watcher imports **completed** games; it does not provide an in-progress game feed.

## Source and documentation

- [`storm-replay`](crates/storm-replay) decodes `.StormReplay` files using tables generated from [Blizzard's heroprotocol](https://github.com/Blizzard/heroprotocol).
- [`storm-stats`](crates/storm-stats) turns decoded events into match stats.
- [`storm-codex-server`](crates/storm-codex-server) provides replay ingestion and the API using Axum and PostgreSQL.
- [`web-rs`](web-rs) contains the Leptos web app and OBS sources. [`desktop`](desktop) contains the Tauri Windows shell and installer configuration.

Start with the [technical documentation](docs/spec/README.md), [development workflow](docs/spec/10-developpement.md), or [Windows acceptance checklist](docs/standalone-acceptance.md). See [project status](docs/STATUS.md) for work in progress.

## Credits

Storm Codex is a Rust rewrite inspired by [Stats of the Storm](https://github.com/ebshimizu/stats-of-the-storm) and [hots-parser](https://github.com/ebshimizu/hots-parser) by [@ebshimizu](https://github.com/ebshimizu). Heroes of the Storm is a trademark of Blizzard Entertainment, Inc. This project is not affiliated with Blizzard.
