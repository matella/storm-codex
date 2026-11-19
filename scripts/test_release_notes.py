"""Tests for scripts/release-notes.py. Run: python3 -m unittest discover -s scripts -p 'test_release_notes.py'"""

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

_SPEC = importlib.util.spec_from_file_location("release_notes", Path(__file__).with_name("release-notes.py"))
assert _SPEC is not None and _SPEC.loader is not None
rn = importlib.util.module_from_spec(_SPEC)
sys.modules["release_notes"] = rn
_SPEC.loader.exec_module(rn)

REPO = "matella/storm-codex"

# Newest first, as `git log` lists them (real subjects from the repository's history, plus edge cases).
FIXTURE = [
    "feat(standalone): download reference data at runtime instead of bundling it (desktop 0.1.2) (#50)",
    "feat(site): GitHub Pages download page for the latest stable installer (#48)",
    "test(watcher): assert backfill order on the queue, not on request arrival (#49)",
    "chore(deps): bump pino (#43)",
    "build(deps): bump getrandom from 0.3.4 to 0.4.3 in /desktop (#47)",
    "fix(deps): bump dirs from 6.0.0 to 7.0.0 (#46)",
    "docs: refresh Storm Codex setup guidance",
    "ci: prebuilt tauri-cli in the release, Node 24 actions everywhere",
    "fix: keep standalone identity and replay import in sync (#38)",
    "Merge pull request #35 from matella/fix/desktop-verbatim-tool-paths",
    "fix(supervisor): give Postgres tools plain paths, not \\?\\ verbatim ones",  # real subject: not \?\ verbatim
    "perf(server): cache the <Path> lookups",
    "refactor(server): split the router",
    "feat!: drop the legacy snapshot format",
    "fix: keep standalone identity and replay import in sync",
    "random subject without a type",
]


def releases(*items):
    return [{"tag_name": t, "draft": d, "prerelease": p} for t, d, p in items]


class ParseTest(unittest.TestCase):
    def test_keeps_feat_fix_perf_strips_scope_and_pr(self):
        self.assertEqual(rn.parse_subject("feat(site): add a page (#48)"), ("feat", "Add a page", 48))
        self.assertEqual(rn.parse_subject("fix: keep sync"), ("fix", "Keep sync", None))
        self.assertEqual(rn.parse_subject("perf(server)!: faster"), ("perf", "Faster", None))
        self.assertEqual(rn.parse_subject("Feat(x): Mixed case"), ("feat", "Mixed case", None))

    def test_drops_other_types_deps_and_non_conventional(self):
        for s in ("chore: x", "ci: x", "docs: x", "test(a): x", "build(deps): x", "refactor: x",
                  "fix(deps): bump x", "chore(deps): bump x", "Merge pull request #35 from a/b",
                  "no type here", "feat:", "feat(x): (#12)"):
            self.assertIsNone(rn.parse_subject(s), s)

    def test_collect_groups_in_order_and_dedupes(self):
        g = rn.collect(FIXTURE)
        self.assertEqual([d for d, _ in g["feat"]], [
            "Download reference data at runtime instead of bundling it (desktop 0.1.2)",
            "GitHub Pages download page for the latest stable installer",
            "Drop the legacy snapshot format",
        ])
        self.assertEqual(g["fix"][0], ("Keep standalone identity and replay import in sync", 38))
        self.assertEqual(len(g["fix"]), 2)  # the duplicate without a PR number is dropped
        self.assertEqual(g["perf"], [("Cache the <Path> lookups", None)])


class PreviousTagTest(unittest.TestCase):
    def test_highest_published_below_current(self):
        rel = releases(("desktop-v0.1.0", False, False), ("desktop-v0.1.2", False, False),
                       ("desktop-v0.1.10", False, False), ("desktop-v0.2.0", False, False),
                       ("desktop-v0.1.11", True, False), ("desktop-v0.1.12", False, True),
                       ("v9.9.9", False, False), ("data-d6776d73", False, False))
        self.assertEqual(rn.previous_tag(rel, "0.1.13"), "desktop-v0.1.10")

    def test_drafts_only_means_none(self):
        rel = releases(("desktop-v0.1.0", True, False), ("desktop-v0.1.1", True, False))
        self.assertIsNone(rn.previous_tag(rel, "0.1.2"))

    def test_current_version_is_not_its_own_base(self):
        self.assertIsNone(rn.previous_tag(releases(("desktop-v0.1.2", False, False)), "0.1.2"))

    def test_parse_releases_array_and_lines(self):
        objs = [{"tag_name": "a"}, {"tag_name": "b"}]
        self.assertEqual(rn.parse_releases(json.dumps(objs)), objs)
        self.assertEqual(rn.parse_releases("\n".join(json.dumps(o) for o in objs) + "\n"), objs)
        self.assertEqual(rn.parse_releases(""), [])

    def test_bad_version(self):
        with self.assertRaises(rn.NotesError):
            rn.previous_tag([], "1.2")


class RenderTest(unittest.TestCase):
    def test_markdown(self):
        md = rn.render_markdown(rn.collect(FIXTURE), REPO, "https://example/compare", "")
        self.assertIn("### Features\n- Download reference data at runtime instead of bundling it (desktop 0.1.2) "
                      f"([#50](https://github.com/{REPO}/pull/50))", md)
        self.assertIn("### Fixes", md)
        self.assertIn("- Cache the &lt;Path&gt; lookups", md)
        self.assertIn("- Give Postgres tools plain paths, not \\\\?\\\\ verbatim ones", md)  # each backslash escaped
        self.assertNotIn("pino", md)
        self.assertNotIn("Merge", md)
        self.assertNotIn("(standalone)", md)
        self.assertTrue(md.rstrip().endswith("**Full changelog**: https://example/compare"))

    def test_markdown_caps_items_and_neutralises_mentions(self):
        many = [f"fix: thing {i} for @someone" for i in range(rn.MAX_BODY_ITEMS + 7)]
        md = rn.render_markdown(rn.collect(many), REPO, None, "since text")
        self.assertEqual(md.count("\n- "), rn.MAX_BODY_ITEMS)
        self.assertIn("…and 7 more changes.", md)
        self.assertNotIn("@someone", md)
        self.assertIn("_since text_", md)

    def test_empty(self):
        self.assertIn(rn.EMPTY, rn.render_markdown(rn.collect(["chore: x"]), REPO, None, ""))
        self.assertEqual(rn.render_plain(rn.collect(["docs: x"])), rn.EMPTY)

    def test_plain_is_short_and_has_no_links(self):
        notes = rn.render_plain(rn.collect(FIXTURE))
        self.assertTrue(notes.startswith("New:\n- Download reference data"))
        self.assertIn("Fixed:\n- Keep standalone identity and replay import in sync", notes)
        self.assertNotIn("#", notes)
        many = [f"feat: a fairly long feature description number {i} " + "x" * 60 for i in range(30)]
        notes = rn.render_plain(rn.collect(many))
        self.assertLessEqual(len(notes), rn.MAX_NOTES_CHARS)
        self.assertLessEqual(notes.count("\n- ") + 1, rn.MAX_NOTES_ITEMS)
        self.assertTrue(notes.endswith("more (see the release page)."))

    def test_plain_cuts_a_single_huge_subject(self):
        notes = rn.render_plain(rn.collect(["feat: " + "y" * 2000]))
        self.assertLessEqual(len(notes), rn.MAX_NOTES_CHARS)
        self.assertTrue(notes.endswith("…"))


class GitRangeTest(unittest.TestCase):
    """End to end against a throwaway repository."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.cwd = os.getcwd()
        os.chdir(self.dir)
        env = {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t",
               "GIT_COMMITTER_EMAIL": "t@t"}
        self.env = {**os.environ, **env}
        self.run_git("init", "-q", "-b", "main")

    def tearDown(self):
        os.chdir(self.cwd)
        self.tmp.cleanup()

    def run_git(self, *args):
        subprocess.run(["git", *args], check=True, env=self.env, capture_output=True)

    def commit(self, subject, path="f.txt"):
        p = self.dir / path
        p.parent.mkdir(parents=True, exist_ok=True)
        with p.open("a") as f:
            f.write(subject + "\n")
        self.run_git("add", "-A")
        self.run_git("commit", "-q", "-m", subject)

    def notes(self, rel, version="0.2.0"):
        (self.dir / "rel.json").write_text(json.dumps(rel))
        out_b, out_n = self.dir / "body.md", self.dir / "notes.txt"
        code = rn.main(["--repo", REPO, "--version", version, "--releases", str(self.dir / "rel.json"),
                        "--body-out", str(out_b), "--notes-out", str(out_n)])
        return code, (out_b.read_text() if out_b.exists() else ""), (out_n.read_text() if out_n.exists() else "")

    def test_range_starts_at_previous_published_tag(self):
        self.commit("feat: before desktop")
        self.commit("feat(desktop): shell", "desktop/a.txt")
        self.run_git("tag", "desktop-v0.1.0")
        self.commit("fix: after the release (#7)")
        self.commit("chore: noise")
        code, body, notes = self.notes(releases(("desktop-v0.1.0", False, False)))
        self.assertEqual(code, 0)
        self.assertIn("- After the release ([#7]", body)
        self.assertNotIn("Shell", body)
        self.assertIn(f"https://github.com/{REPO}/compare/desktop-v0.1.0...", body)
        self.assertEqual(notes, "Fixed:\n- After the release")

    def test_without_published_release_starts_at_first_desktop_commit(self):
        self.commit("feat: before desktop")
        self.commit("feat(desktop): shell", "desktop/a.txt")
        self.commit("fix: later")
        code, body, _ = self.notes(releases(("desktop-v0.1.0", True, False)))
        self.assertEqual(code, 0)
        self.assertIn("- Shell", body)
        self.assertIn("- Later", body)
        self.assertNotIn("Before desktop", body)
        self.assertIn("First published release", body)

    def test_missing_tag_fails(self):
        self.commit("feat(desktop): shell", "desktop/a.txt")
        code, _, _ = self.notes(releases(("desktop-v0.1.0", False, False)))
        self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
