"""Tests for scripts/build-pages.py. Run: python3 -m unittest discover -s scripts -p 'test_build_pages.py'"""

import importlib.util
import sys
import tempfile
import unittest
import urllib.error
from email.message import Message
from pathlib import Path
from unittest import mock

_SPEC = importlib.util.spec_from_file_location("build_pages", Path(__file__).with_name("build-pages.py"))
assert _SPEC is not None and _SPEC.loader is not None
bp = importlib.util.module_from_spec(_SPEC)
sys.modules["build_pages"] = bp  # dataclasses resolve annotations through sys.modules
_SPEC.loader.exec_module(bp)

REPO = "matella/storm-codex"
URL = "https://github.com/matella/storm-codex/releases/download/desktop-v0.1.2/StormCodex_0.1.2_x64-setup.exe"


def release(**over):
    r = {
        "tag_name": "desktop-v0.1.2",
        "draft": False,
        "prerelease": False,
        "published_at": "2026-11-20T10:00:00Z",
        "html_url": "https://github.com/matella/storm-codex/releases/tag/desktop-v0.1.2",
        "assets": [
            {"name": "StormCodex_0.1.2_x64-setup.exe", "size": 52_400_000, "browser_download_url": URL},
            {"name": "StormCodex_0.1.2_x64-setup.exe.sig", "size": 400, "browser_download_url": URL + ".sig"},
            {"name": "latest.json", "size": 600, "browser_download_url": "https://github.com/x/latest.json"},
        ],
    }
    r.update(over)
    return r


class BuildPagesTest(unittest.TestCase):
    def build(self, rel):
        with tempfile.TemporaryDirectory() as d:
            msg = bp.build(REPO, Path(d), rel)
            page = (Path(d) / "index.html").read_text(encoding="utf-8")
            files = sorted(p.name for p in Path(d).iterdir())
        return msg, page, files

    def test_stable_release_offers_the_installer(self):
        msg, page, files = self.build(release())
        self.assertIn("offering 0.1.2", msg)
        self.assertIn(f'class="download" href="{URL}"', page)
        self.assertIn("Version 0.1.2 · 2026-11-20 · 52 MB", page)
        self.assertIn("releases/tag/desktop-v0.1.2", page)
        self.assertNotIn("{{", page)
        self.assertEqual(files, [".nojekyll", "icon.png", "index.html", "style.css"])

    def test_no_release_renders_no_button(self):
        msg, page, _ = self.build(None)
        self.assertIn("no published release", msg)
        self.assertIn("No stable Windows installer is published yet.", page)
        self.assertNotIn('class="download"', page)

    def test_rejected_releases_render_no_button(self):
        cases = {
            "draft": release(draft=True),
            "prerelease": release(prerelease=True),
            "server tag": release(tag_name="v1.2.0"),
            "missing installer": release(assets=[]),
            "version mismatch": release(tag_name="desktop-v0.1.3"),
            "two installers": release(assets=release()["assets"][:1] * 2),
            "foreign url": release(
                assets=[{"name": "StormCodex_0.1.2_x64-setup.exe", "size": 1, "browser_download_url": "https://evil.example/x.exe"}]
            ),
            "bad date": release(published_at="soon"),
        }
        for name, rel in cases.items():
            with self.subTest(name):
                _, page, _ = self.build(rel)
                self.assertNotIn('class="download"', page)
                self.assertIn("No stable Windows installer", page)

    def test_values_are_escaped(self):
        stable = bp.Stable("0.1.2", URL + '?a="><script>', 1, "2026-11-20", "https://github.com/x/\"y")
        page = bp.render("{{DOWNLOAD}}", REPO, stable)
        self.assertNotIn("<script>", page)
        self.assertIn("&quot;&gt;&lt;script&gt;", page)

    def test_api_404_means_no_release(self):
        err = urllib.error.HTTPError("u", 404, "Not Found", Message(), None)
        with mock.patch.object(bp.urllib.request, "urlopen", side_effect=err):
            self.assertIsNone(bp.fetch_latest(REPO, None))

    def test_other_api_errors_fail_the_build(self):
        for err in (urllib.error.HTTPError("u", 503, "Unavailable", Message(), None), urllib.error.URLError("down")):
            with self.subTest(str(err)), mock.patch.object(bp.urllib.request, "urlopen", side_effect=err):
                with self.assertRaises(bp.BuildError):
                    bp.fetch_latest(REPO, None)


if __name__ == "__main__":
    unittest.main()
