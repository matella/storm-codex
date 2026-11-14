# Standalone installer — acceptance

Spec: `docs/specs/2026-11-12-standalone-installer-design.md` ("Acceptance"). Plan:
`docs/plans/2026-11-12-standalone-installer.md`, Task 10. Operations contract:
`docs/spec/09-operations.md` ("Standalone desktop app").

**State on 2026-11-14:** the code is complete on `feat/standalone-installer`, but **v1 is not
accepted yet**. What can run on the operator's PC without an installer was run (below). The
installer, the clean-VM run and the real update need the operator: the checklist at the end is
**not run**.

| Spec acceptance item | State |
|---|---|
| 1. Clean Windows 11 profile (VM): install, start, real replay folder imported with no manual step | **Not run** (no installer built, no VM). Without an installer: passed on the operator's PC (dev run below) |
| 2. A replay played afterwards appears without restarting | **Passed in the dev run** (2.6 s); not run on an installed build |
| 3. All six OBS sources render from `http://localhost:<port>` | **HTTP 200 on all six in the dev run** (after the port fix below); not checked in OBS itself |
| 4. Update N → N+1 keeps data, migrations run; kill mid-import loses nothing and resumes | Kill + resume: **passed in the dev run**. Update N → N+1: **not run** (needs two signed builds) |
| 5. Perf budgets on the operator's PC; first start and installer size recorded | Parse, API p95, backfill and first start (dev build) **measured**; installer size **not measurable** (not built) |

## Dev run on the operator's PC (2026-11-14)

**Setup.** `cargo build --release -p storm-codex-server`, `cargo build --manifest-path
desktop/Cargo.toml` (**debug** shell: the only build where `STORM_CODEX_RESOURCE_DIR` and
`STORM_CODEX_DATA_ROOT` work). Resource folder in a scratch directory outside the repository: the
server exe, `postgres/` (Postgres 17.12 from `scripts/fetch-postgres.ps1`), `web/` (=
`web-rs/dist`), `minimaps/`, and a **tiny test `referential.tar.gz`** (built against a fake API,
631 bytes: the real snapshot does not exist yet). Data roots in the same scratch directory. The
replay folders are found through the Documents known-folder API, which cannot be redirected per
process: the app read the operator's real `Documents\Heroes of the Storm` folders **in place,
read only** (the watcher only reads replays; its ledger lives in the data root). Nothing was
copied into or written to those folders; files used for the live test were copied into a scratch
extra folder. Port 8088 was held by another program on `[::]` (qBittorrent's web UI), which is
how the port finding below showed up. Everything was stopped and the scratch folders deleted
afterwards.

| Check | Result | Evidence |
|---|---|---|
| Data root with spaces and accents (`…\Mätt é Test\data`) | **Pass** | initdb, Postgres, server, import all fine; `dataDir` in the status shows the path intact |
| Data root with Cyrillic (`…\Пользователь é\data`) | **Fail** (known, see Known limitations) | `desktop.log`: "startup failed: Postgres: `initdb` failed … could not open file "…\???????????? �\data\pg-password"" (ANSI code page 1252); error dialog, nothing left running |
| First launch imports with no manual step, progress shown | **Pass** | status `importing`, total 3,427, done rising (1,190 after ~40 s); the Welcome page (read over WebView2 CDP) shows "100% · 2127 of 2127 replays … 17 ALREADY KNOWN 0 REJECTED 0 FAILED" at the end |
| Hard kill of the shell mid-import (`Stop-Process -Force`, at 1,315 of 3,427) | **Pass** | server died with the shell (job object); Postgres and its `postmaster.pid` stayed (by design). DB: 1,319 uploads, ledger 1,300 entries, 3 `pending` |
| Relaunch after the kill: no leftover postmaster, import resumes, no data loss | **Pass** | `postgres.log`: fast shutdown of the orphan, then a new start (port 54329 → 49728, see L4); `server.log`: "3 pending upload(s) resumed at startup"; import `ready` 100 s later. Final: 3,425 uploads = **3,425 distinct source files** (SHA-256 of all 3,427; 2 byte-identical copies), 3,345 matches, 80 `parse_failed` (see L5), ledger 3,427 entries, 3,425 archive files, 0 `pending` |
| New replay copied in afterwards appears without restart | **Pass** | extra folder `…\Extra Пользователь é` added with `PUT /api/standalone/folders` (Cyrillic **replay** path works: only the Postgres data dir is affected); one game removed from the scratch DB, its replay copied in under a new name → new match in `matches` after **2.63 s**, visible in `GET /api/matches` |
| Six OBS routes on the running port | **Pass after fix** | before: 200 on `127.0.0.1:8088`, but `http://localhost:8088/...` → 404 from qBittorrent (`::1` first). After the `pick_port` fix: port 51042, all six (`/cast/overlay`, `/draft/overlay`, `/ticker`, `/now-playing`, `/widget`, `/queue`) 200 on both `127.0.0.1` and `localhost` |
| Second launch | **Pass** | second process exited with code 0 in 0.19 s; still 1 shell / 1 server; first window visible, not minimised, foreground |
| Clean stop | **Pass** | `WM_QUERYENDSESSION` + `WM_ENDSESSION` to the session watcher: `desktop.log` "stopped (Windows session end, SessionEnd budget)", no server or Postgres left, no `postmaster.pid` |
| Saved extra folders survive a restart | **Pass** | after a relaunch the status lists the saved Cyrillic extra folder |

Not covered by the dev run: the tray menu clicked with the mouse, a real Windows logoff/shutdown,
a machine without HotS (covered only by `detector_returns_empty_when_no_install`), the installer,
the uninstaller, the updater.

## Measurements (no cap except the existing budgets)

| Measure | Value | Method |
|---|---|---|
| Parse (decode + stats), budget < 150 ms | median **140.3 / 138.5 / 140.7 ms** (3 runs; a first cold run gave 179.9 ms); p95 254–274 ms | `storm-stats-dump --bench corpus/spike50` (release, built at HEAD), 50 replays, single-threaded, nothing else running |
| API p95, budget < 100 ms | **p95 39.3 ms**, median 27.4 ms, max 47.6 ms | 260 GETs (20 rounds × `/api/matches?limit=50`, `/api/heroes`, `/api/players?limit=50`, 10 × `/api/matches/{id}`) on the standalone server with the operator's 3,345 matches |
| Backfill, budget < 5 min for ~3 years | **≈ 2.7 min** for 3,427 files / 3.1 GB / 2024-08 → 2026-11 (≈ 2.3 years) | upload `created_at` windows: 63 s (killed at 1,319) + 100 s (2,105 more); excludes the 3.5 s restart in between |
| First start, empty data root → first page | **7.7 s, 8.8 s** | dev run, **debug** shell + release server; process start → `GET /` 200; includes initdb, migrations and a tiny snapshot (a real snapshot with images will add its unpack time) |
| Restart with data → first page | 3.5 s, 3.6 s | same |
| Installer size | **not measured** | no installer built (no `cargo tauri build` here; the first dry run records it) |

## Operator acceptance checklist (not run)

Do these on a **clean Windows 11 VM** (fresh local user) unless said otherwise. Record the
result and date next to each step, and copy the measurements into `docs/spec/09-operations.md`.

**Before (one time, see 09-operations "Releasing the desktop app"):** generate the updater key
pair, set `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, replace
`plugins.updater.pubkey`, build/upload/pin the reference snapshot, run the release workflow as a
**dry run** (draft) and download the installer. Record the installer size.

1. **Install (acceptance 1).** Copy a real `Documents\Heroes of the Storm\Accounts\…` tree (or
   install HotS and log in) into the VM user's Documents. Run the installer (SmartScreen: More
   info → Run anyway). *Expected:* no admin prompt; app starts; loading window, then the Welcome
   page with a progress bar; import reaches `ready` with no click; the page shows where the data
   lives (`%LOCALAPPDATA%\com.matella.stormcodex`). Time install-finished → window and → `ready`.
2. **Live game (acceptance 2).** With the app running, play a game (or copy a replay that is not
   in the folder yet, with a new file name). *Expected:* the match is in Matches within ~5 s, no
   restart.
3. **OBS (acceptance 3).** In OBS, add browser sources `http://localhost:<port>/cast/overlay`,
   `/draft/overlay`, `/ticker`, `/now-playing`, `/widget`, `/queue` (port = the one in the app's
   OBS URLs). *Expected:* each renders (no 404, no blank page). Repeat with another program
   listening on 8088 (e.g. `python -m http.server 8088 --bind ::`) before starting the app:
   the app takes another port and `localhost` still reaches it.
4. **Kill mid-import (acceptance 4).** Start from an empty data root (uninstall with "Delete the
   application data" ticked, reinstall), and during the import end `storm-codex-desktop.exe` in
   Task Manager. Relaunch. *Expected:* import resumes, ends `ready`; the number of matches equals
   a clean import's; Task Manager shows one Postgres group only.
5. **Power cut mid-import.** Same as 4 but reset the VM (hard power off). *Expected:* same as 4
   (Postgres crash recovery in `logs/postgres.log`).
6. **Update N → N+1 (acceptance 4).** Two signed builds (dry runs), N installed and populated.
   Publish N+1's `latest.json` (or point a test build at a local one, see 09-operations "Updater
   manual E2E"). *Expected:* banner and tray item "Restart to update"; click → app restarts on
   N+1; all matches still there; `server.log` shows the migrations. Repeat with the click during
   a backfill: resumes, nothing uploaded twice. Also run N+1's installer by hand over N: data kept.
7. **Installer hooks.** Run the installer while the app runs: "Quit it first" message, app left
   running. Kill the app in Task Manager (Postgres keeps running), run the installer: it stops
   that Postgres (`postgres.log` fast shutdown) and replaces `postgres\`.
8. **Uninstall.** Uninstall with the box unticked: `%LOCALAPPDATA%\com.matella.stormcodex` kept;
   reinstall: data back. Uninstall with the box ticked: folder gone.
9. **Profile name with spaces and accents** (e.g. user `Mätt Test`). *Expected:* works (passed in
   the dev run with a data root of that shape).
10. **Profile name in Cyrillic** (e.g. `Пользователь`) on an English/French Windows. *Expected
    today:* **fails** at first start with an initdb error (known limitation L1). Also try on a
    Russian-locale Windows (ANSI code page 1251), where it may work.
11. **No HotS install.** VM user without a `Heroes of the Storm` folder. *Expected:* phase
    `no-folders`, the Welcome page asks for a folder; adding one in settings starts the import.
12. **Export / import.** Export from the operator's PC, import on the VM. *Expected:* same
    matches; the replay archive restored.
13. **Perf on the operator's PC with the installed build:** first start → window and → `ready`
    on the real replays; API p95 with the installed build.

## Known limitations (v1, accepted or deferred)

Findings of the 2026-11-14 run (severity for a v1 user):
- **L1 — High: data root not representable in the ANSI code page.** Postgres' Windows tools take
  `char*` paths; a Windows profile named in Cyrillic (or any script outside the system's ANSI code
  page) puts `%LOCALAPPDATA%` outside it and initdb fails: the app cannot start (clear error
  dialog, nothing left running). Ruled at Task 2 as a Task 10 acceptance test; reproduced. Fix
  directions (not done): 8.3 short paths (not guaranteed on every volume), or a data root under
  an ASCII-only location when the profile path is not representable.
- **L2 — fixed: port held on `[::]` by another program.** `pick_port` now also refuses a port
  that accepts connections on `[::1]` (`localhost` → `::1` first); test
  `pick_port_falls_back_when_taken_on_ipv6_loopback_only`.
- **L3 — fixed: `extraFolders` empty while seeding.** Saved extras are loaded before the server
  listens; DB-backed tests added (see 04-serveur).
- **L4 — Low:** after a hard kill, the next start picks the Postgres port before stopping the
  orphan Postgres, so the port moves (54329 → another). Harmless (the URL is rebuilt each start).
- **L5 — Low/Medium:** replays the server stores as `parse_failed` (on the operator's PC: 37 too
  old, 28 stats failures, 14 unsupported modes, 1 with computer players) are counted as imported:
  the Welcome page shows "0 rejected". The spec wants corrupt replays "listed once at the end";
  today only HTTP-level rejections are listed (the server never sends them for bad content).
  The 28 `stats_failure` replays deserve a parser look (outside this project).
- **L6 — Low:** after a resume, progress shows only the files left ("2127 of 2127"), not the
  whole folder.
- **L7 — Low:** the ledger is keyed by file name: a replay whose name is already recorded from
  another folder is skipped without being counted.

Deferred minors carried from the task reviews (ledger
`.superpowers/sdd/2026-11-12-standalone-installer/progress.md`), still open:
- Supervisor: `pick_port` check-then-use race; an interrupted first initdb is not self-healing;
  `start()` restarts a running cluster; ownership check fails open if the process' exe/cmdline
  is unreadable; `pg-password` ACL inherits the data root's; crash mid password write leaves a
  partial file; a superuser (not a dedicated role) runs the app database.
- Watcher/server: live `Retryable` uploads are retried only at the next start or rescan; no
  circuit breaker on repeated failures; no ledger-error counter in the status; the watcher does
  not re-arm when a folder disappears; status shows no "unknown" when free space cannot be
  measured; repeated rescans restart the backfill.
- Web UI: standalone users skip onboarding step 1 (in-game names); Import tab stale after an
  import; Rescan has no busy guard; duplicate folder detection is exact-string; `busy` stays on
  after a successful `install_update` until reload.
- Shell: tray menu clicked with the mouse and a real logoff/shutdown never exercised; a panic
  inside `stack.stop` leaves `stopping` set; Quit ignored after a startup-failure dialog; session
  end during startup waits for the start; relaunch after an update keeps `--minimized`; the
  update is held in RAM; leftover `storm_import` role if the staging DB is gone; placeholder icons.
- Release: NSIS hooks never compiled; release workflow never run; `ci.yml` actions on movable
  refs; updater pubkey and referential pin are placeholders.

Out of scope (spec follow-ups): a code-signing certificate (SmartScreen warning on first
install), runtime refresh of the reference snapshot, the HotsPatchNotes snapshot publication
(matella/HeroesOfTheStormPatchNotes#1), SQLite instead of bundled Postgres (matella/storm-codex#31).
