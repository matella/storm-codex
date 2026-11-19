# Security policy

## Report a vulnerability privately

Use [GitHub private vulnerability reporting](https://github.com/matella/storm-codex/security/advisories/new). Do not open a public issue for a suspected vulnerability. Include the affected version or commit, whether it is the Windows app or self-hosted server, the impact, and the minimum steps or sanitized sample needed to reproduce it. Remove credentials, personal replay data, chat logs, and private hostnames. Do not attach your entire archive or database.

The maintainer reviews reports privately and coordinates disclosure through GitHub Security Advisories. The project prioritizes the latest stable desktop release, when available, and current `main`. Older builds and draft installers are not maintained release lines.

## Deployment boundary

The standalone Windows app binds its server to loopback. The self-hosted server is intended for trusted networks: do not expose an open Admin or upload configuration to the internet. If remote access is required, use a trusted VPN or an authenticated HTTPS reverse proxy, configure the server's admin and upload credentials, and keep PostgreSQL private. See the [operations guide](docs/spec/09-operations.md).

Download public installers from [GitHub Releases](https://github.com/matella/storm-codex/releases). The built-in updater verifies its update signature. That is distinct from Windows Authenticode signing; don't assume a draft is a trusted public release.
