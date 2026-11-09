# Runbook — publishing the crates (milestone 6)

**Operator** action (crates.io account). Publishes `storm-replay` then `storm-stats` to crates.io.
Dry-run validated (`storm-replay`: 54 files, 2.4 MiB, verification build OK).

## Prerequisites
- crates.io account (sign in via GitHub on https://crates.io).
- API token: crates.io → **Account Settings → API Tokens → New Token** (publish scope).
- `cargo login <token>` once on the machine that publishes (here the Mac: toolchain present).
- Check that the names `storm-replay` / `storm-stats` are free on crates.io (otherwise rename).

## Publication order (IMPORTANT)
`storm-stats` depends on `storm-replay` → publish `storm-replay` **first** (crates.io must
index it before `storm-stats` can reference `version = "0.1"`).

```bash
cd ~/Documents/GitHub/storm-codex          # repo root (workspace)
cargo publish -p storm-replay              # wait ~30-60s for the index to update
cargo publish -p storm-stats               # references storm-replay 0.1 (already published)
```
If `storm-stats` fails with "no matching package named storm-replay", wait 1 min
(index propagation) and retry.

## Notes
- **Permanent**: a published version cannot be deleted (only `cargo yank` to hide it).
  Publish when you are ready to freeze `0.1.0`.
- `repository` points to `github.com/matella/storm-codex` (monorepo, private today). For working
  public links: either make `storm-codex` public, or split the crates into their
  own public repos. Publishing to crates.io works in every case (the crate compiles
  on its own — dry-run OK); only the "Repository" link on crates.io would be dead while the repo is private.
- `storm-codex-server` is **not** published (application binary, not a lib) — implicit
  `publish = false` through lack of interest; only the two lib crates are published.

## After publication
- Update the crates' READMEs with the crates.io badge + a usage example.
- (optional) Announce it (the "HotS parser in Rust" niche is vacant).
