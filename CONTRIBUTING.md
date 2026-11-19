# Contributing

Use [GitHub Issues](https://github.com/matella/storm-codex/issues) for bug reports and proposed changes. Report security issues [privately](SECURITY.md). Read the [code of conduct](CODE_OF_CONDUCT.md) before participating.

1. Start your branch from `main`. Read the [developer workflow](docs/spec/10-developpement.md) and [project rules](gads/PROJECT.md). Discuss changes to architecture, APIs, or user-facing contracts before writing code.
2. Add a focused test where practical and update the matching living spec in the same commit. Never edit migrations that have already been applied. Use committed replay fixtures or sanitized synthetic data, not your personal archive.
3. Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`. Database-backed tests need a disposable PostgreSQL instance and `DATABASE_URL`; never point tests at a real archive. The developer workflow lists separate web and Windows checks.
4. Open a small PR with the problem, the proposed change, and the checks you ran. Call out anything you could not verify, particularly the installed Windows app.
