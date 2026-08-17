#!/usr/bin/env python3
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import build_est_ru_df as b  # noqa: E402


def article(
    word: str,
    *,
    o: str | None = None,
    sl: str | None = None,
    gloss: str | None = None,
    est: str | None = None,
    see: str | None = None,
) -> str:
    o_attr = f' x:O="{o}"' if o is not None else ""
    sl_el = f"<x:sl>{sl}</x:sl>" if sl else ""
    parts = [f"<x:A><x:P><x:mg><x:m{o_attr}>{word}</x:m>{sl_el}</x:mg></x:P><x:S>"]
    if see:
        parts.append(f"<x:tp><x:tvt>{see}</x:tvt></x:tp>")
    if gloss or est:
        parts.append("<x:tp><x:tg>")
        if est:
            parts.append(f"<x:dg><x:d>{est}</x:d></x:dg>")
        if gloss:
            parts.append(f"<x:xp><x:xg><x:x>{gloss}</x:x></x:xg></x:xp>")
        parts.append("</x:tg></x:tp>")
    parts.append("</x:S></x:A>")
    return "".join(parts)


class HeadwordTests(unittest.TestCase):
    def test_uses_printed_text_not_sort_key(self) -> None:
        item = b.parse_article(article("24/7", o="A", gloss="круглосуточно"))
        self.assertIsNotNone(item)
        self.assertEqual(item["headwords"], ["24/7"])

    def test_homograph_drops_numeric_sort_suffix(self) -> None:
        item = b.parse_article(article("aga", o="aga1", sl="konj", gloss="но"))
        self.assertEqual(item["headwords"], ["aga"])

    def test_hyphenated_word_keeps_hyphens_and_compact_variant(self) -> None:
        item = b.parse_article(
            article("aeg-ajalt", o="aegajalt", sl="adv", gloss="временами")
        )
        self.assertEqual(item["headwords"], ["aeg-ajalt", "aegajalt"])


class XmlWrapTests(unittest.TestCase):
    def test_article_with_braces_parses(self) -> None:
        item = b.parse_article(article("foo{bar}", gloss="gloss"))
        self.assertIsNotNone(item)
        self.assertEqual(item["headwords"], ["foo{bar}"])


class InflectionTests(unittest.TestCase):
    def test_short_lemma_gets_no_inflections(self) -> None:
        entry = {
            "headwords": ["a"],
            "pos": ["s"],
            "senses": [{"glosses": ["буква"], "examples": []}],
            "phrases": [],
        }
        variants = b.collect_variants(entry, {"a": ["aga", "as", "al"]})
        self.assertEqual(variants, [])

    def test_longer_lemma_keeps_inflections(self) -> None:
        entry = {
            "headwords": ["aabe"],
            "pos": ["s"],
            "senses": [{"glosses": ["буква"], "examples": []}],
            "phrases": [],
        }
        variants = b.collect_variants(entry, {"aabe": ["aape", "aabet"]})
        self.assertEqual(variants, ["aape", "aabet"])

    def test_inflection_lookup_is_casefold_and_unhyphenated(self) -> None:
        entry = {
            "headwords": ["Aeg-ajalt"],
            "pos": ["adv"],
            "senses": [{"glosses": ["временами"], "examples": []}],
            "phrases": [],
        }
        variants = b.collect_variants(entry, {"aegajalt": ["aegajaldagi"]})
        self.assertEqual(variants, ["aegajaldagi"])


class FallbackTests(unittest.TestCase):
    def test_estonian_definition_when_no_russian(self) -> None:
        item = b.parse_article(article("abaasi", sl="adjg", est="abasiini"))
        self.assertEqual(item["senses"][0]["glosses"], ["abasiini"])

    def test_russian_gloss_preferred_over_estonian(self) -> None:
        item = b.parse_article(
            article("aabe", sl="s", gloss="буква", est="kirjatäht")
        )
        self.assertEqual(item["senses"][0]["glosses"], ["буква"])

    def test_see_also_copies_target_senses(self) -> None:
        source = b.parse_article(
            article("aabitsa+teadmised", o="aabitsateadmised", sl="s", see="aabitsa+tarkus")
        )
        target = b.parse_article(
            article("aabitsa+tarkus", sl="s", gloss="азы")
        )
        entries = b.merge_parsed([source, target])
        b.resolve_see_also(entries)
        by_head = {entry["headwords"][0]: entry for entry in entries}
        self.assertEqual(by_head["aabitsateadmised"]["senses"][0]["glosses"], ["азы"])

    def test_missing_see_also_target_gets_placeholder(self) -> None:
        source = b.parse_article(article("abaasi", sl="adjg", see="abasiini"))
        entries = b.merge_parsed([source])
        b.resolve_see_also(entries)
        self.assertEqual(entries[0]["senses"][0]["glosses"], ["см. abasiini"])


class WriteDfTests(unittest.TestCase):
    def test_write_omits_short_lemma_variants(self) -> None:
        entries = [
            {
                "headwords": ["a"],
                "pos": ["s"],
                "senses": [{"glosses": ["буква"], "examples": []}],
                "phrases": [],
            }
        ]
        with tempfile.TemporaryDirectory() as tmp:
            dest = Path(tmp) / "out.df"
            b.write_df(entries, {"a": ["aga", "as"]}, dest)
            text = dest.read_text(encoding="utf-8")
        self.assertIn("@ a\n", text)
        self.assertNotIn("& aga\n", text)


if __name__ == "__main__":
    unittest.main()
