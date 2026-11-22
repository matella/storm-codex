# Remove Jarvis/Redis event emission (2026-11-21)

## Decision

Storm Codex is going fully local. Drop the optional Jarvis (Redis pub/sub) event emitter entirely — it was already opt-in (`REDIS_URL` unset = no emission) and never shipped in the standalone desktop build, but it's dead weight now: no operator box depends on it going forward. Redis has no other use in this codebase (confirmed: the `redis` crate dependency exists solely for `jarvis.rs`).

## Scope

### Remove
- `crates/storm-codex-server/src/jarvis.rs` (module), its `mod jarvis;` in `main.rs`, and the `redis = "1"` dependency in `crates/storm-codex-server/Cargo.toml`.
- `crate::jarvis::match_completed_event` / `publish` call site in `upload.rs` (the `tokio::spawn` best-effort emission after a parsed match).
- `AppConfig::redis_url` / `AppConfig::jarvis_channel` fields and their `env::var` parsing in `config.rs` (both test fixtures in `main.rs` and `standalone/tests.rs` that set `jarvis_channel: "test".into()`).
- `REDIS_URL` / `JARVIS_CHANNEL` from `.env.example`, `crates/storm-codex-server/.env.example`, `docker-compose.yml` (drop the `jarvis` external network block and its env wiring on the server service — keep the rest of compose working standalone).
- `crates/storm-codex-server/test_jarvis.py` (manual test harness, no longer applicable).
- The `JARVIS_CHANNEL` entry in the desktop shell's env-leak allowlist test (`desktop/src/supervise.rs`) — since the var no longer exists server-side, there's nothing to assert doesn't leak; drop it from that list (keep `ADMIN_TOKEN`/`AZURE_PUSH_TOKEN` leak checks, those remain real).

### Keep (cosmetic, unrelated)
- The "Jarvis" **butler brief line** (`web-rs/core/src/session.rs::jarvis_phrase`, rendered in the OBS session widget and `web-rs/sources/src/sessions.rs`) is a deterministic, locally-generated French flavor string — it never called out to Jarvis, "Jarvis" is just its persona name. Not in scope; this spec is about the Redis event *integration*, not this cosmetic widget text. (Separate call if the operator also wants that renamed — not assumed here.)

### Docs (Rule 0, same commit)
- `docs/spec/01-architecture.md`, `04-serveur.md` (drop the `REDIS_URL`/`JARVIS_CHANNEL` env-var row and the Jarvis arrow in the architecture diagram), `07-evenements.md` (this doc's whole "Jarvis (Redis pub/sub)" section goes — keep the WS `/ws` contract, drop the Jarvis half, update the title if it was e.g. "Events — WebSocket `/ws` and Jarvis contract"), `09-operations.md` (drop the Jarvis Redis network row in the "what's optional" table).
- `docs/STATUS.md`: dated entry noting the removal and why.
- Leave `docs/STATUS-historique.md`, `docs/runbooks/*`, `docs/research/*`, `docs/plans/2026-06-12-jalon5-stream-jarvis.md`, and `docs/specs/2026-06-*` design docs untouched — they're historical record of a milestone that happened, not live specs; don't rewrite history.

## Verification
- `cargo test --workspace`, native + wasm clippy `-D warnings`, `cargo fmt --check`.
- Confirm no remaining reference to `redis`/`Redis`/`JARVIS_CHANNEL`/`REDIS_URL` in `crates/`, `desktop/`, `docker-compose.yml`, `.env.example` files (the butler-phrase cosmetic text is the one intentional exception, grep for `jarvis_phrase`/`Jarvis\"` in session widgets to confirm it's the only surviving hit outside historical docs).
- `trunk build --release` + bundle budget (unaffected, server-only change, but build must still succeed).
- Desktop: `child_process_inherits_no_unlisted_variable` and sibling leak tests in `supervise.rs` still pass with the updated allowlist.

**Status: APPROVED 2026-11-21** (operator: "Could we remove those actually? Those are useless nowadays", following the live audit of Jarvis/token remnants in chat).
