#!/usr/bin/env python3
"""Release notes of a desktop release, generated from conventional-commit subjects.

Range: from the previous *published* desktop release (highest `desktop-vX.Y.Z` below --version that is neither a
draft nor a prerelease) to --to (default HEAD). Without one: from the first commit touching desktop/, capped at the
last MAX_FALLBACK_COMMITS commits. Merge commits are skipped.

Kept: feat, fix, perf (scope stripped, `(#N)` turned into a PR link). Dropped: every other type (chore, ci, docs,
test, build, refactor, style, revert...), any `(deps)` scope, and subjects that are not conventional commits.

Outputs:
- --body-out: Markdown for the GitHub Release body (grouped lists, PR links, compare link), capped;
- --notes-out: short plain text for the updater manifest's `notes` (latest.json), capped.

Usage: release-notes.py --repo OWNER/NAME --version X.Y.Z --releases FILE [--to REF]
                        [--body-out FILE] [--notes-out FILE]
--releases: the repository's releases as a JSON array or JSON lines of objects with `tag_name`, `draft`,
`prerelease` (e.g. `gh api --paginate repos/OWNER/NAME/releases --jq '.[]'`). Without --body-out/--notes-out both
renderings are printed. Python standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

TAG_RE = re.compile(r"^desktop-v(\d+)\.(\d+)\.(\d+)$")
VERSION_RE = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")
SUBJECT_RE = re.compile(r"^(?P<type>[A-Za-z]+)(?:\((?P<scope>[^)]*)\))?(?P<bang>!)?:\s*(?P<desc>\S.*)$")
PR_SUFFIX_RE = re.compile(r"\s*\(#(\d+)\)\s*$")
GROUPS = (("feat", "Features", "New"), ("fix", "Fixes", "Fixed"), ("perf", "Performance", "Faster"))
MAX_FALLBACK_COMMITS = 50
MAX_BODY_ITEMS = 25
MAX_NOTES_ITEMS = 8
MAX_NOTES_CHARS = 600
EMPTY = "Maintenance release (no user-facing changes)."


class NotesError(Exception):
    """A failure that must stop the release (never replaced by empty notes)."""


def parse_releases(text: str) -> list[dict]:
    """A JSON array, or JSON lines (one object per line, `gh api --paginate --jq '.[]'`)."""
    text = text.strip()
    if not text:
        return []
    if text.startswith("["):
        data = json.loads(text)
    else:
        data = [json.loads(line) for line in text.splitlines() if line.strip()]
    if not all(isinstance(r, dict) for r in data):
        raise NotesError("releases must be JSON objects")
    return data


def parse_version(version: str) -> tuple[int, int, int]:
    m = VERSION_RE.match(version)
    if not m:
        raise NotesError(f"version {version!r} is not X.Y.Z")
    return int(m.group(1)), int(m.group(2)), int(m.group(3))


def previous_tag(releases: list[dict], version: str) -> str | None:
    """The tag of the highest published (non-draft, non-prerelease) desktop release below `version`."""
    current = parse_version(version)
    best: tuple[tuple[int, int, int], str] | None = None
    for r in releases:
        if r.get("draft") or r.get("prerelease"):
            continue
        tag = str(r.get("tag_name", ""))
        m = TAG_RE.match(tag)
        if not m:
            continue
        v = (int(m.group(1)), int(m.group(2)), int(m.group(3)))
        if v < current and (best is None or v > best[0]):
            best = (v, tag)
    return best[1] if best else None


def parse_subject(subject: str) -> tuple[str, str, int | None] | None:
    """(type, description, PR number) of a kept commit subject, None for a dropped one."""
    m = SUBJECT_RE.match(subject.strip())
    if not m:
        return None
    kind = m.group("type").lower()
    scope = (m.group("scope") or "").strip().lower()
    if kind not in {g[0] for g in GROUPS} or scope == "deps":
        return None
    desc = m.group("desc").strip()
    pr = None
    pm = PR_SUFFIX_RE.search(desc)
    if pm:
        pr = int(pm.group(1))
        desc = desc[: pm.start()].rstrip()
    if not desc:
        return None
    return kind, desc[0].upper() + desc[1:], pr


def collect(subjects: list[str]) -> dict[str, list[tuple[str, int | None]]]:
    """Kept entries per type, in the given (newest-first) order, without duplicate descriptions."""
    groups: dict[str, list[tuple[str, int | None]]] = {g[0]: [] for g in GROUPS}
    seen: set[str] = set()
    for s in subjects:
        parsed = parse_subject(s)
        if parsed is None:
            continue
        kind, desc, pr = parsed
        if desc.lower() in seen:
            continue
        seen.add(desc.lower())
        groups[kind].append((desc, pr))
    return groups


def _md_escape(text: str) -> str:
    """Keeps `<Path>`-like text visible (GitHub would treat it as an HTML tag) and blocks @-mentions."""
    return text.replace("\\", "\\\\").replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("@", "@\u200b")


def render_markdown(groups: dict, repo: str, compare_url: str | None, since: str) -> str:
    total = sum(len(v) for v in groups.values())
    lines: list[str] = []
    if total == 0:
        lines.append(EMPTY)
    budget = MAX_BODY_ITEMS
    for kind, title, _ in GROUPS:
        entries = groups[kind][:budget] if budget > 0 else []
        budget -= len(entries)
        if not entries:
            continue
        if lines:
            lines.append("")
        lines.append(f"### {title}")
        for desc, pr in entries:
            link = f" ([#{pr}](https://github.com/{repo}/pull/{pr}))" if pr else ""
            lines.append(f"- {_md_escape(desc)}{link}")
    hidden = total - min(total, MAX_BODY_ITEMS)
    if hidden:
        lines.append("")
        lines.append(f"…and {hidden} more change{'s' if hidden != 1 else ''}.")
    lines.append("")
    if compare_url:
        lines.append(f"**Full changelog**: {compare_url}")
    else:
        lines.append(f"_{since}_")
    return "\n".join(lines) + "\n"


def render_plain(groups: dict) -> str:
    """Short plain text: at most MAX_NOTES_ITEMS entries and MAX_NOTES_CHARS characters, no links."""
    total = sum(len(v) for v in groups.values())
    if total == 0:
        return EMPTY
    lines: list[str] = []
    shown = 0
    for kind, _, label in GROUPS:
        header_done = False
        for desc, _ in groups[kind]:
            if shown >= MAX_NOTES_ITEMS:
                break
            candidate = ([] if header_done else [f"{label}:"]) + [f"- {desc}"]
            # keep room for the "...more" line
            if len("\n".join(lines + candidate)) > MAX_NOTES_CHARS - 40:
                break
            lines.extend(candidate)
            header_done = True
            shown += 1
    if shown == 0:  # a single huge subject: cut it
        first = next(desc for kind, _, _ in GROUPS for desc, _ in groups[kind])
        lines = [first[: MAX_NOTES_CHARS - 41].rstrip() + "…"]
        shown = 1
    if total > shown:
        lines.append(f"+ {total - shown} more (see the release page).")
    return "\n".join(lines)


def git(*args: str) -> str:
    try:
        return subprocess.run(["git", *args], check=True, capture_output=True, text=True, encoding="utf-8").stdout
    except subprocess.CalledProcessError as e:
        raise NotesError(f"git {' '.join(args)} failed: {e.stderr.strip()}") from e


def commit_subjects(base: str | None, to: str, max_count: int | None = None) -> list[str]:
    args = ["log", "--no-merges", "--format=%s"]
    if max_count:
        args.append(f"--max-count={max_count}")
    args.append(f"{base}..{to}" if base else to)
    return [s for s in git(*args).splitlines() if s.strip()]


def resolve_range(prev: str | None, to: str) -> tuple[list[str], str]:
    """Subjects of the release, and a sentence describing the range when there is no compare link."""
    if prev:
        try:
            git("rev-parse", "--verify", "--quiet", f"{prev}^{{commit}}")
        except NotesError as e:
            raise NotesError(f"previous release tag {prev} is not in the clone (fetch tags, full history)") from e
        return commit_subjects(prev, to), ""
    first = git("log", "--reverse", "--format=%H", to, "--", "desktop").split()
    base = None
    if first:
        parents = git("rev-list", "--parents", "-n", "1", first[0]).split()
        base = parents[1] if len(parents) > 1 else None
    since = f"First published release: up to the last {MAX_FALLBACK_COMMITS} commits of the desktop app's history."
    return commit_subjects(base, to, MAX_FALLBACK_COMMITS), since


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repo", required=True)
    ap.add_argument("--version", required=True)
    ap.add_argument("--releases", required=True, type=Path)
    ap.add_argument("--to", default="HEAD")
    ap.add_argument("--body-out", type=Path)
    ap.add_argument("--notes-out", type=Path)
    a = ap.parse_args(argv)
    try:
        parse_version(a.version)
        prev = previous_tag(parse_releases(a.releases.read_text(encoding="utf-8")), a.version)
        subjects, since = resolve_range(prev, a.to)
        sha = git("rev-parse", "--verify", f"{a.to}^{{commit}}").strip()
    except (NotesError, OSError, json.JSONDecodeError) as e:
        print(f"release-notes: {e}", file=sys.stderr)
        return 1
    groups = collect(subjects)
    compare = f"https://github.com/{a.repo}/compare/{prev}...{sha}" if prev else None
    body = render_markdown(groups, a.repo, compare, since)
    notes = render_plain(groups)
    print(f"release-notes: {len(subjects)} commits since {prev or 'the fallback bound'}, "
          f"{sum(len(v) for v in groups.values())} kept", file=sys.stderr)
    if a.body_out:
        a.body_out.write_text(body, encoding="utf-8")
    if a.notes_out:
        a.notes_out.write_text(notes, encoding="utf-8")
    if not a.body_out and not a.notes_out:
        print(body)
        print("---- latest.json notes ----")
        print(notes)
    return 0


if __name__ == "__main__":
    sys.exit(main())
