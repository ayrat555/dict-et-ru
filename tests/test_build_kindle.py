#!/usr/bin/env python3
from __future__ import annotations

import gzip
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import build_kindle as k  # noqa: E402

SAMPLE_HTML = """<html><w><p><a name="aabe" /><b>aabe</b> сущ.</p><var><variant name="aape"/><variant name="aabet"/></var>
<p><i>aabe aape aabet</i></p>
<p><b>буква</b></p></w><w><p><a name="24/7" /><b>24/7</b></p><var></var>
<p><b>круглосуточно</b></p></w></html>
"""


class ParseKoboTests(unittest.TestCase):
    def test_reads_headword_pos_variants_and_body(self) -> None:
        entries = k.parse_kobo_html(SAMPLE_HTML)
        self.assertEqual(len(entries), 2)
        self.assertEqual(entries[0]["headword"], "aabe")
        self.assertEqual(entries[0]["pos"], "сущ.")
        self.assertEqual(entries[0]["variants"], ["aape", "aabet"])
        self.assertIn("буква", entries[0]["body"])
        self.assertEqual(entries[1]["headword"], "24/7")
        self.assertEqual(entries[1]["variants"], [])

    def test_dedupes_copied_variant_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            zip_path = Path(tmp) / "dicthtml.zip"
            with zipfile.ZipFile(zip_path, "w") as archive:
                archive.writestr("aa.html", gzip.compress(SAMPLE_HTML.encode("utf-8")))
                archive.writestr("aa.html".replace("aa", "ap"), gzip.compress(SAMPLE_HTML.encode("utf-8")))
            entries = k.parse_kobo_zip(zip_path)
        self.assertEqual([item["headword"] for item in entries], ["aabe", "24/7"])
        self.assertEqual(entries[0]["variants"], ["aape", "aabet"])

    def test_merges_homograph_bodies(self) -> None:
        html = (
            '<html><w><p><a name="sai" /><b>sai</b> сущ.</p><var></var>'
            "<p><b>булка</b></p></w>"
            '<w><p><a name="sai" /><b>sai</b> гл.</p><var></var>'
            "<p><i>saama</i></p><p><b>получать</b></p></w></html>"
        )
        with tempfile.TemporaryDirectory() as tmp:
            zip_path = Path(tmp) / "dicthtml.zip"
            with zipfile.ZipFile(zip_path, "w") as archive:
                archive.writestr("sa.html", gzip.compress(html.encode("utf-8")))
            entries = k.parse_kobo_zip(zip_path)
        self.assertEqual(len(entries), 1)
        self.assertEqual(entries[0]["headword"], "sai")
        self.assertEqual(entries[0]["pos"], "сущ., гл.")
        self.assertIn("булка", entries[0]["body"])
        self.assertIn("получать", entries[0]["body"])
        self.assertIn("saama", entries[0]["body"])


class KindleSourceTests(unittest.TestCase):
    def test_entry_has_orth_and_inflections(self) -> None:
        html = k.render_entry(
            {
                "headword": "aabe",
                "pos": "сущ.",
                "variants": ["aape"],
                "body": "<p><b>буква</b></p>",
            }
        )
        self.assertIn('<idx:orth value="aabe">', html)
        self.assertIn('<idx:iform value="aape"/>', html)
        self.assertIn("<p><b>aabe сущ.</b></p>", html)

    def test_writes_opf_and_html(self) -> None:
        entries = [
            {
                "headword": "aabe",
                "pos": "сущ.",
                "variants": ["aape"],
                "body": "<p><b>буква</b></p>",
            }
        ]
        with tempfile.TemporaryDirectory() as tmp:
            dest = Path(tmp)
            opf = k.write_kindle_source(entries, dest)
            html = (dest / "content-001.html").read_text(encoding="utf-8")
            text = opf.read_text(encoding="utf-8")
            self.assertTrue((dest / "cover.png").is_file())
        self.assertIn("DictionaryInLanguage>et<", text)
        self.assertIn("DictionaryOutLanguage>ru<", text)
        self.assertIn("DefaultLookupIndex>default<", text)
        self.assertIn('name="cover"', text)
        self.assertIn('<idx:entry name="default"', html)
        self.assertIn("<mbp:frameset>", html)


if __name__ == "__main__":
    unittest.main()
