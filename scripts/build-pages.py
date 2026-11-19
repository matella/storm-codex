#!/usr/bin/env python3
"""Build the GitHub Pages download site (docs/specs/2026-11-19-github-pages-download.md).

Reads the repository's latest *stable* release (GitHub's /releases/latest never returns a draft or a
prerelease), checks that it is a desktop release with exactly one matching installer asset, and renders
site/index.html into the output directory. Without a valid stable release the page states that none is
published and renders no download button. Any API error other than 404 fails the build, so a transient
outage never replaces a good page with the empty state.

Usage: build-pages.py --repo OWNER/NAME --out DIR [--release-json FILE]
Env: GITHUB_TOKEN (optional, raises the API rate limit).
Python standard library only.
"""

from __future__ import annotations

import argparse
import html
import json
import os
import re
import shutil
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SITE_DIR = ROOT / "site"
ICON = ROOT / "desktop" / "icons" / "128x128.png"
TAG_RE = re.compile(r"^desktop-v(\d+\.\d+\.\d+)$")
API_TIMEOUT_S = 30


class BuildError(Exception):
    """A failure that must stop the deployment (never rendered as the empty state)."""


@dataclass(frozen=True)
class Stable:
    version: str
    url: str
    size_bytes: int
    published: str  # YYYY-MM-DD
    notes_url: str


def fetch_latest(repo: str, token: str | None) -> dict | None:
    """The latest stable release as JSON, or None when the repository has none (HTTP 404)."""
    req = urllib.request.Request(
        f"https://api.github.com/repos/{repo}/releases/latest",
        headers={"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"},
    )
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req, timeout=API_TIMEOUT_S) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None
        raise BuildError(f"GitHub API returned HTTP {e.code} for the latest release") from e
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as e:
        raise BuildError(f"cannot read the latest release: {e}") from e


def select_stable(release: dict | None) -> tuple[Stable | None, str]:
    """The installer to offer, or None with the reason it is not offered."""
    if release is None:
        return None, "no published release"
    if release.get("draft") or release.get("prerelease"):
        return None, "latest release is a draft or prerelease"
    m = TAG_RE.match(str(release.get("tag_name", "")))
    if not m:
        return None, f"latest release tag {release.get('tag_name')!r} is not desktop-vX.Y.Z"
    version = m.group(1)
    name = f"StormCodex_{version}_x64-setup.exe"
    assets = [a for a in release.get("assets") or [] if a.get("name") == name]
    if len(assets) != 1:
        return None, f"expected exactly one asset {name}, found {len(assets)}"
    asset = assets[0]
    url = str(asset.get("browser_download_url", ""))
    if not url.startswith("https://github.com/"):
        return None, f"unexpected installer URL {url!r}"
    published = str(release.get("published_at") or "")
    try:
        day = datetime.strptime(published[:10], "%Y-%m-%d").strftime("%Y-%m-%d")
    except ValueError:
        return None, f"invalid published_at {published!r}"
    return (
        Stable(
            version=version,
            url=url,
            size_bytes=int(asset.get("size") or 0),
            published=day,
            notes_url=str(release.get("html_url") or ""),
        ),
        "ok",
    )


def render(template: str, repo: str, stable: Stable | None) -> str:
    esc = html.escape
    repo_url = f"https://github.com/{repo}"
    releases_url = f"{repo_url}/releases"
    if stable is None:
        block = (
            '<p class="status">No stable Windows installer is published yet.</p>\n'
            f'      <p class="meta">Follow <a href="{esc(releases_url)}">GitHub Releases</a> '
            "for the first public build.</p>"
        )
    else:
        mb = stable.size_bytes / 1_000_000
        notes = stable.notes_url if stable.notes_url.startswith("https://github.com/") else releases_url
        block = (
            f'<a class="download" href="{esc(stable.url)}">Download for Windows (x64)</a>\n'
            f'      <p class="meta">Version {esc(stable.version)} · {esc(stable.published)} · {mb:.0f} MB · '
            f'<a href="{esc(notes)}">Release notes</a></p>'
        )
    return (
        template.replace("{{DOWNLOAD}}", block)
        .replace("{{REPO_URL}}", esc(repo_url))
        .replace("{{RELEASES_URL}}", esc(releases_url))
    )


def build(repo: str, out: Path, release: dict | None) -> str:
    stable, reason = select_stable(release)
    template = (SITE_DIR / "index.html").read_text(encoding="utf-8")
    if "{{DOWNLOAD}}" not in template:
        raise BuildError("site/index.html has no {{DOWNLOAD}} placeholder")
    out.mkdir(parents=True, exist_ok=True)
    (out / "index.html").write_text(render(template, repo, stable), encoding="utf-8")
    shutil.copyfile(SITE_DIR / "style.css", out / "style.css")
    shutil.copyfile(ICON, out / "icon.png")
    (out / ".nojekyll").write_text("", encoding="utf-8")
    return f"offering {stable.version}: {stable.url}" if stable else f"no download button: {reason}"


def main(argv: list[str]) -> int:
    p = argparse.ArgumentParser(description="Build the GitHub Pages download site.")
    p.add_argument("--repo", required=True)
    p.add_argument("--out", required=True, type=Path)
    p.add_argument("--release-json", type=Path, help="render from a saved API response (testing)")
    a = p.parse_args(argv)
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", a.repo):
        print(f"invalid --repo {a.repo!r}", file=sys.stderr)
        return 2
    try:
        if a.release_json:
            text = a.release_json.read_text(encoding="utf-8").strip()
            release = json.loads(text) if text and text != "null" else None
        else:
            release = fetch_latest(a.repo, os.environ.get("GITHUB_TOKEN"))
        print(build(a.repo, a.out, release))
    except BuildError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
