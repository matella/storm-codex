# Dependabot alert #1 — glib `Iterator` unsoundness (GHSA-wrw7-89jp-8q8g / RUSTSEC-2024-0429)

Status: **blocked upstream, no action taken**. Decision point for the operator below.

## Alert

- Advisory: GHSA-wrw7-89jp-8q8g / RUSTSEC-2024-0429, "Unsoundness in `Iterator` and
  `DoubleEndedIterator` impls for `glib::VariantStrIter`".
- Severity: medium. Vulnerable range `>=0.15.0, <0.20.0`. First patched version: `0.20.0`.
- Location: `desktop/Cargo.lock`, transitive via `tauri` → `tauri-runtime-wry` → `wry` →
  `gtk` 0.18 / `webkit2gtk` 2.0 → `glib` 0.18.5 (Linux only). Not a direct dependency of
  `storm-codex-desktop`.

## Investigation

Current pins (before this session): `glib` 0.18.5, `gtk` 0.18.2, `webkit2gtk` 2.0.2, `wry`
0.57.2, `tauri` 2.12.5.

1. Checked the current Tauri 2.x line: `tauri` 2.12.7, `tauri-runtime-wry` 2.12.5, `wry`
   0.57.3 are the latest released versions as of 2026-11-19 (checked crates.io/tauri.app
   release notes).
2. `wry`'s own `Cargo.toml` (every 0.57.x release, including 0.57.3) pins
   `webkit2gtk = "=2.0.2"` and `gtk = "0.18"` **exactly** (not a range) for Linux. `wry` has
   no release that references a newer `webkit2gtk-rs` fork version.
3. `tauri-apps/tauri#12048` ("Tauri 2.1.1 & 1.8.1 is affected by glib-rs 0.15 security
   vulnerability") was closed **"not planned"**. The blocker cited there: gtk3-rs (the
   `gtk`/`glib` 0.18 line used by `webkit2gtk-rs`) is EOL upstream; moving past glib 0.18
   needs `webkit2gtk-rs` (Tauri's own fork) to publish on `gtk` 0.19+, which in turn needs a
   GTK4 migration that has not happened.
4. Ran `cargo update` (no `Cargo.toml` edits) inside `storm-dev:latest` against
   `desktop/Cargo.lock`: it moved `tauri` 2.12.5→2.12.7, `tauri-runtime-wry` 2.12.3→2.12.5,
   `wry` 0.57.2→0.57.3, and several unrelated transitive crates, but **left `glib` at
   0.18.5 and `webkit2gtk` at 2.0.2 unchanged** — confirming point 2 empirically, not just
   from the manifest text.
5. `cargo-audit` (0.23.0, in `storm-dev:latest` via the `storm-cargo-bin` volume) against
   the unmodified `desktop/Cargo.lock` reports the advisory directly:
   `Crate: glib, Version: 0.18.5, ID: RUSTSEC-2024-0429, Solution: Upgrade to >=0.20.0`.
   No fixed version is resolvable from the registry today.
6. Noted in passing: the `webkit2gtk` crate on crates.io got a new `3.0.0` release on
   2026-11-19 (same day) requiring `glib ^0.22`/`gtk ^0.19`. This is brand new — no `wry` or
   `tauri` release depends on it yet, and adopting it would be a breaking GTK3→newer-gtk-rs
   jump for Tauri, not a drop-in bump. Worth re-checking in a future session, but it is not
   something this repo can consume today.

## Conclusion (R2 decision point)

No compatible bump exists upstream yet. `wry`/`tauri`'s Linux WebView backend is hard-pinned
to `webkit2gtk` 2.0.2 / `gtk` 0.18 / `glib` 0.18.5, and the maintainers have explicitly
declined to fix this in the gtk3-rs line (issue closed not-planned). Forcing a resolution by
vendor-patching `glib`, `[patch]`-overriding the crate, or dropping the Linux webview feature
would dodge the advisory rather than fix it, which was explicitly out of scope for this
session.

**No dependency, lockfile, or source change was made in this session.** The benign
`cargo update` performed during investigation (patch bumps within the already-declared `"2"`
semver range, e.g. `tauri` 2.12.5→2.12.7) was reverted and not committed, since it does not
affect the advisory and is outside this task's scope.

## What would unblock this

Any one of:
- `wry` releasing a version that depends on a `webkit2gtk-rs` fork built on `gtk` 0.19+/GTK4
  (not released as of 2026-11-19; `webkit2gtk-rs` itself has shown no activity toward this).
- A new `webkit2gtk` crate release (the `3.0.0` published today requires `glib ^0.22`) being
  adopted by `wry`'s Linux backend — needs `wry` to migrate off `gtk` 0.18 entirely.
- Tauri reopening/accepting a fix for `tauri-apps/tauri#12048`.

Re-check by rerunning `cargo update` in `desktop/` and `cargo audit` next time a `tauri`/`wry`
Dependabot PR lands; if `glib` finally moves off `0.18.x`, the advisory closes itself.

## Evidence / commands run

- `cargo update` in `desktop/` inside `storm-dev:latest` (registry volume
  `storm-cargo-registry`), reverted afterwards (`git checkout -- Cargo.lock`).
- `cargo audit` (0.23.0, `storm-cargo-bin` volume) against the unmodified
  `desktop/Cargo.lock`.
- crates.io dependency pages for `wry` 0.57.2/0.57.3 and `webkit2gtk`; `tauri.app` release
  notes for the `tauri` 2.12.x line; `tauri-apps/tauri#12048`.
