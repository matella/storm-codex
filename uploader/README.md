# Uploader

The **replay uploader** for Heroes of the Storm: a small client that watches your replay folder and
uploads new `.StormReplay` files to your Storm Codex server, which parses them and powers stats,
match pages and OBS overlays. Migrated from the standalone `matella/Hots-Overlay` repo
(2026-11-23) — see `docs/spec/09-operations.md`'s "Uploader (gaming PC)" and "Releasing the
uploader" sections in the root of this repo.

Use this uploader to send replays from your gaming PC to a self-hosted server, or to feed the
server's in-progress `/live` view. The standalone Windows Storm Codex app already imports completed
local replays, so it doesn't need this uploader for that.

## Running it

It runs on your **gaming PC**, backfills your existing replays, then watches for new games. The
**server URL**, **upload token** and **replay folder** are all set at runtime (GUI Settings or
environment variables) — **nothing is baked into a publicly distributed binary**.

Two ways to run it:

- **Headless Docker** (`ghcr.io/matella/hots-uploader`) — for all-Docker setups. Configure via the
  `SERVER_URL`, `AUTH_TOKEN` and `REPLAY_DIR` environment variables (the `storm-codex-suite` compose
  has an `uploader` profile that wires these for you).
- **Native Windows app** (GUI, system tray, auto-start). Build it from this repository's root:
  ```powershell
  cargo build --release --manifest-path uploader/Cargo.toml  # output: uploader/target/release/
  # optional installer (needs Inno Setup `iscc` in PATH), with YOUR OWN server/token baked in for a
  # personal build (never do this for a build you intend to share publicly):
  .\uploader\build-uploader.ps1 -Server "http://<server-ip>:5102" -Token "<your-token>" -Installer
  ```
  Public releases are built by `.github/workflows/release-uploader.yml` on an `uploader-v*` tag,
  with no server/token set, so the published installer is safe to share.

### First run
1. Launch the app, open **Settings**.
2. Set the **Server URL** (e.g. `http://<server-ip>:5102`) and **upload token** (created in the
   server's *Admin → Upload tokens*).
3. Add your **replay folder** (see below) and **Save**. Settings persist locally.

It sits in the system tray and uploads new replays automatically; if the server goes offline,
uploads resume when it comes back.

### Live game feed (storm-codex `/live`)
While a game runs, the uploader also follows HotS's temporary replay folder
(`%TEMP%\Heroes of the Storm\TempWriteReplayP1\`) and streams it to storm-codex's live page. Nothing
is decoded on the PC: it pushes raw bytes and the server parses them. The steps:
- the battlelobby goes to `/api/lobby` as soon as the loading screen writes it;
- `replay.tracker.events` and `replay.game.events` are sent as they grow, in full 4 KB blocks only,
  to `/api/live/chunk`;
- the end of the game (`replay.details` + `replay.initData`) flushes the last partial blocks and
  calls `/api/live/end`.

If the server is unreachable, the uploader keeps its position in each file and catches up when the
server comes back. A game that was already over when the uploader started is never streamed. The
watched folder can be overridden with `LIVE_DIR`; for the Docker image, mount the Windows temp
folder there. Activity is logged to `%LOCALAPPDATA%\HotS Replay Client\client.log` (lines starting
`live:`).

### Where to find your replay folder
HotS saves replays to:
```
C:\Users\<YourName>\Documents\Heroes of the Storm\Accounts\<AccountNumber>\<ToonHandle>\Replays\Multiplayer
```
Point the uploader at `…\Heroes of the Storm\Accounts` and it discovers every account/toon under it.

## Tech stack

Rust, `eframe`/`egui` (GUI + system tray), `ureq` (HTTP), `notify` (filesystem watching). See
`src/live.rs` for the in-progress-game watcher and `src/{detector,watcher,uploader}.rs` for the
completed-replay backfill/watch path.
