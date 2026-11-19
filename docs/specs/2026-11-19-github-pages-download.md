# GitHub Pages download page

Date: 2026-11-19. Issue #39. Status: approved by the operator in chat (2026-11-19).

## Goal

One public page, `https://matella.github.io/storm-codex/`, that says what Storm Codex does and offers the latest
**stable** Windows installer. The installer stays on GitHub Releases; the page only links to it.

## Design

- **Source**: `site/` holds a hand-written static template (`index.html`, `style.css`) using the Nexus Codex
  palette (`web-rs/app/theme.css`) and the app icon (`desktop/icons/128x128.png`, copied at build time). No
  JavaScript, no web fonts, no analytics, no third-party requests: the page works without scripts and leaks nothing
  to anyone but GitHub.
- **Generator**: `scripts/build-pages.py` (Python stdlib only, no new dependency) reads
  `GET /repos/{repo}/releases/latest`. GitHub defines that endpoint as the most recent published release that is
  neither a draft nor a prerelease, so drafts (the dry runs) and prereleases are excluded by construction. The script
  accepts the release only if its tag is `desktop-vX.Y.Z` and it has exactly one asset named
  `StormCodex_X.Y.Z_x64-setup.exe` with the same version (the name `release-desktop.yml` publishes). It renders the
  version, publication date, installer size, a **Download for Windows (x64)** button pointing at the asset's
  `browser_download_url`, and links to the release notes and the repository. All values are HTML-escaped.
- **No stable release** (404, wrong tag, missing or ambiguous asset): the page says no stable installer is published
  yet and links to the releases page; no download button is rendered. API errors other than 404 fail the build so a
  transient outage never replaces a good page with the empty state.
- **Deployment**: `.github/workflows/pages.yml`, Pages source "GitHub Actions" (no `gh-pages` branch). Triggers:
  push to `main` touching `site/`, the generator or the workflow; `workflow_run` when `release-desktop` completes
  (a release created with `GITHUB_TOKEN` emits no `release` event); `release` published/edited/deleted (manual
  edits in the UI); manual run. Permissions are `contents: read`, `pages: write`, `id-token: write`; every action is
  pinned to a full commit SHA; checkout does not persist credentials; one deployment at a time.
- **Tests**: `scripts/test_build_pages.py` (stdlib `unittest`) covers a stable release, no release (404), a
  release without or with a mismatched installer, escaping, and a non-404 API error. It runs in the `checks` job of
  `ci.yml`.

## Acceptance

- Deployed page loads at the URL above with the correct state; layout checked at desktop and mobile widths.
- With a stable release, the button URL equals the installer asset URL and resolves (HTTP 200 after redirect).
- Today only drafts exist, so the deployed page must show the "no stable release" state. The stable path is proven
  by the unit tests and by rendering against a fixture; the first real stable release must then refresh the page
  without a manual edit (checked at that release).
- `docs/spec/09-operations.md` documents deployment and update; README links the page.

## Out of scope

Publishing a release to populate the page, updater changes, custom domain, translations, changelog rendering.
