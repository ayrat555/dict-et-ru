#!/usr/bin/env python3
"""Convert EKI Estonian–Russian Dictionary XML into a dictutil dictfile."""

from __future__ import annotations

import argparse
import html
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

NS_URI = "http://www.eki.ee/dict/ev2"
ARTICLE_WRAP = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    f'<root xmlns:x="{NS_URI}">{{article}}</root>'
)

POS_LABELS = {
    "s": "сущ.",
    "v": "гл.",
    "adj": "прил.",
    "adv": "нар.",
    "pron": "мест.",
    "num": "числ.",
    "konj": "союз",
    "prep": "предл.",
    "postp": "послелог",
    "interj": "межд.",
    "prop": "имя собств.",
    "adjg": "прил.",
    "adjid": "прил.",
    "vrm": "гл.",
}

VOWELS = set("аеёиоуыэюяАЕЁИОУЫЭЮЯaeiouyäöüõAEIOUYÄÖÜÕ")
MAX_EXAMPLES_PER_SENSE = 6
MAX_PHRASES = 4
MAX_VARIANTS = 120
MIN_INFLECTION_HEADWORD_LEN = 3
COMPACT_STRIP = str.maketrans("", "", "+-. ")


def local_name(tag: str) -> str:
    if tag.startswith("{"):
        return tag.rsplit("}", 1)[-1]
    if ":" in tag:
        return tag.split(":", 1)[1]
    return tag


def children(el: ET.Element, name: str):
    return [child for child in list(el) if local_name(child.tag) == name]


def first(el: ET.Element | None, name: str) -> ET.Element | None:
    if el is None:
        return None
    for child in children(el, name):
        return child
    return None


def text_of(el: ET.Element | None) -> str:
    if el is None:
        return ""
    chunks: list[str] = []

    def walk(node: ET.Element) -> None:
        if node.text:
            chunks.append(node.text)
        for child in list(node):
            walk(child)
            tail = child.tail
            if not tail:
                continue
            if (
                chunks
                and not chunks[-1][-1].isspace()
                and not tail[0].isspace()
                and tail[0] not in ".,;:!?)]}"
            ):
                chunks.append(" ")
            chunks.append(tail)

    walk(el)
    return " ".join("".join(chunks).split())


def apply_stress(text: str) -> str:
    out: list[str] = []
    i = 0
    while i < len(text):
        if text[i] == '"' and i + 1 < len(text) and text[i + 1] in VOWELS:
            out.append(text[i + 1])
            out.append("\u0301")
            i += 2
            continue
        out.append(text[i])
        i += 1
    return "".join(out)


def clean_text(text: str) -> str:
    text = html.unescape(text)
    text = text.replace("&v;", " / ")
    text = " ".join(text.split())
    text = apply_stress(text)
    return text.strip(" /")


def clean_headword(text: str) -> str:
    text = text.replace("+", "").replace('"', "").strip()
    return " ".join(text.split())


def compound_prefix(raw_headword: str) -> str:
    if "+" not in raw_headword:
        return ""
    return clean_headword(raw_headword.rsplit("+", 1)[0])


def clean_paradigm(raw: str, prefix: str = "") -> str:
    text = html.unescape(raw)
    text = text.replace("_&_", " / ").replace("&", " / ")
    text = text.replace("'", "")
    parts: list[str] = []
    for token in text.split():
        token = token.replace("[", "").replace("/", "")
        token = token.lstrip("+")
        if not token:
            continue
        if prefix and any(ch.isalpha() for ch in token) and not token.startswith(prefix):
            token = prefix + token
        parts.append(token)
    return " ".join(parts)


def compact_spelling(text: str) -> str:
    return text.translate(COMPACT_STRIP).casefold()


def is_compact_variant(sort_key: str, word: str) -> bool:
    return bool(sort_key) and sort_key != word and compact_spelling(sort_key) == compact_spelling(word)


def attr(el: ET.Element, name: str) -> str:
    return el.get(f"{{{NS_URI}}}{name}") or el.get(name) or ""


def inflection_keys(word: str) -> list[str]:
    compact = word.replace("-", "").replace(" ", "")
    keys: list[str] = []
    seen: set[str] = set()
    for key in (word, word.casefold(), compact, compact.casefold()):
        if key and key not in seen:
            seen.add(key)
            keys.append(key)
    return keys


def inflection_forms(word: str, inflections: dict[str, list[str]]) -> list[str]:
    if len(word) < MIN_INFLECTION_HEADWORD_LEN:
        return []
    for key in inflection_keys(word):
        forms = inflections.get(key)
        if forms:
            return forms
    return []


def collect_variants(entry: dict, inflections: dict[str, list[str]]) -> list[str]:
    headword = entry["headwords"][0]
    variants: list[str] = []
    seen = {headword}
    for extra in entry["headwords"][1:]:
        if extra not in seen:
            variants.append(extra)
            seen.add(extra)
    for key in entry["headwords"]:
        for form in inflection_forms(key, inflections):
            if form not in seen:
                variants.append(form)
                seen.add(form)
    return variants[:MAX_VARIANTS]


def iter_articles(path: Path):
    buf = ""
    with path.open(encoding="utf-8") as handle:
        while True:
            chunk = handle.read(1 << 20)
            if not chunk:
                break
            buf += chunk
            while True:
                start = buf.find("<x:A")
                end = buf.find("</x:A>")
                if start == -1 or end == -1 or end < start:
                    if start > 0:
                        buf = buf[start:]
                    elif len(buf) > 2_000_000:
                        buf = buf[-4096:]
                    break
                yield buf[start : end + 6]
                buf = buf[end + 6 :]


def parse_article_xml(article: str) -> ET.Element:
    return ET.fromstring(ARTICLE_WRAP.replace("{article}", article))


def gloss_from_xg(xg: ET.Element) -> str | None:
    gloss = clean_text(text_of(first(xg, "x")))
    if not gloss:
        return None
    vg = first(xg, "vg")
    extras: list[str] = []
    if vg is not None:
        extras.extend(
            clean_text(text_of(gender))
            for gender in children(vg, "vgsugu")
            if clean_text(text_of(gender))
        )
    if extras:
        gloss = f"{gloss} ({', '.join(extras)})"
    return gloss


def parse_article(article: str) -> dict | None:
    root = parse_article_xml(article)
    entry = first(root, "A")
    if entry is None:
        return None

    head = first(entry, "P")
    senses_el = first(entry, "S")
    phrases_el = first(entry, "F")
    if head is None:
        return None

    headwords: list[str] = []
    pos_codes: list[str] = []
    prefix = ""
    paradigm = ""
    for mg in children(head, "mg"):
        for m in children(mg, "m"):
            raw_m = text_of(m)
            if not prefix:
                prefix = compound_prefix(raw_m)
            word = clean_headword(raw_m or attr(m, "O"))
            if word and word not in headwords:
                headwords.append(word)
            sort_key = clean_headword(attr(m, "O"))
            if (
                sort_key
                and sort_key not in headwords
                and is_compact_variant(sort_key, word)
            ):
                headwords.append(sort_key)
        for sl in children(mg, "sl"):
            code = clean_text(text_of(sl))
            if code and code not in pos_codes:
                pos_codes.append(code)
        for grg in children(mg, "grg"):
            mv = first(grg, "mv")
            if mv is None or paradigm:
                continue
            paradigm = clean_paradigm(text_of(mv), prefix)

    if not headwords:
        return None

    senses: list[dict] = []
    see: list[str] = []
    if senses_el is not None:
        for tp in children(senses_el, "tp"):
            for tvt in children(tp, "tvt"):
                target = clean_headword(text_of(tvt))
                if target and target not in see:
                    see.append(target)
            glosses: list[str] = []
            examples: list[tuple[str, str]] = []
            for tg in children(tp, "tg"):
                xp = first(tg, "xp")
                if xp is None:
                    continue
                for xg in children(xp, "xg"):
                    gloss = gloss_from_xg(xg)
                    if gloss and gloss not in glosses:
                        glosses.append(gloss)
            if not glosses:
                for tg in children(tp, "tg"):
                    dg = first(tg, "dg")
                    if dg is None:
                        continue
                    for definition in children(dg, "d"):
                        est = clean_text(text_of(definition))
                        if est and est not in glosses:
                            glosses.append(est)
            np = first(tp, "np")
            if np is not None:
                for ng in children(np, "ng"):
                    et = clean_text(text_of(first(ng, "n")))
                    qnp = first(ng, "qnp")
                    rus = [
                        clean_text(text_of(qn))
                        for qng in children(qnp if qnp is not None else ng, "qng")
                        for qn in children(qng, "qn")
                    ]
                    rus = [item for item in rus if item]
                    if et and rus and len(examples) < MAX_EXAMPLES_PER_SENSE:
                        examples.append((et, " / ".join(rus)))
            if glosses:
                senses.append({"glosses": glosses, "examples": examples})

    phrases: list[tuple[str, str]] = []
    if phrases_el is not None:
        for fg in children(phrases_el, "fg"):
            et = clean_text(text_of(first(fg, "f")))
            rus = [
                clean_text(text_of(qf))
                for fqnp in children(fg, "fqnp")
                for fqng in children(fqnp, "fqng")
                for qf in children(fqng, "qf")
            ]
            rus = [item for item in rus if item]
            if et and rus and len(phrases) < MAX_PHRASES:
                phrases.append((et, " / ".join(rus)))

    if not senses and not phrases and not see:
        return None

    return {
        "headwords": headwords,
        "pos": pos_codes,
        "senses": senses,
        "phrases": phrases,
        "see": see,
        "paradigm": paradigm,
    }


def load_inflections(path: Path) -> dict[str, list[str]]:
    mapping: dict[str, list[str]] = {}
    with path.open(encoding="utf-8") as handle:
        header = handle.readline()
        if "\t" not in header:
            handle.seek(0)
        for line in handle:
            if "\t" not in line:
                continue
            word, forms = line.rstrip("\n").split("\t", 1)
            unique: list[str] = []
            seen = {word}
            for form in forms.split(","):
                form = form.strip()
                if not form or form in seen:
                    continue
                seen.add(form)
                unique.append(form)
                if len(unique) >= MAX_VARIANTS:
                    break
            if unique:
                mapping[word] = unique
    return mapping


def pos_header(codes: list[str]) -> str:
    labels = [POS_LABELS.get(code, code) for code in codes]
    return ", ".join(dict.fromkeys(labels))


def html_escape(text: str) -> str:
    return html.escape(text, quote=False)


def render_definition(entry: dict) -> str:
    parts = ["<html>"]
    if entry.get("paradigm"):
        parts.append(f"<p><i>{html_escape(entry['paradigm'])}</i></p>")
    for index, sense in enumerate(entry["senses"], start=1):
        gloss = "; ".join(sense["glosses"])
        prefix = f"{index}. " if len(entry["senses"]) > 1 else ""
        parts.append(f"<p><b>{prefix}{html_escape(gloss)}</b></p>")
        for et, ru in sense["examples"]:
            parts.append(
                f"<p><i>{html_escape(et)}</i> — {html_escape(ru)}</p>"
            )
    if entry["phrases"]:
        parts.append("<p><i>Фразеологизмы</i></p>")
        for et, ru in entry["phrases"]:
            parts.append(f"<p><i>{html_escape(et)}</i> — {html_escape(ru)}</p>")
    return "\n".join(parts)


def write_df(entries: list[dict], inflections: dict[str, list[str]], dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    with dest.open("w", encoding="utf-8") as handle:
        for entry in entries:
            headword = entry["headwords"][0]
            handle.write(f"@ {headword}\n")
            header = pos_header(entry["pos"])
            if header:
                handle.write(f": {header}\n")
            for variant in collect_variants(entry, inflections):
                handle.write(f"& {variant}\n")
            handle.write(render_definition(entry))
            handle.write("\n\n")


def merge_parsed(parsed: list[dict]) -> list[dict]:
    grouped: dict[str, dict] = {}
    order: list[str] = []
    for item in parsed:
        key = item["headwords"][0]
        if key not in grouped:
            grouped[key] = {
                "headwords": list(item["headwords"]),
                "pos": list(item["pos"]),
                "senses": list(item["senses"]),
                "phrases": list(item["phrases"]),
                "see": list(item.get("see", [])),
                "paradigm": item.get("paradigm") or "",
            }
            order.append(key)
            continue
        dest = grouped[key]
        for word in item["headwords"]:
            if word not in dest["headwords"]:
                dest["headwords"].append(word)
        for code in item["pos"]:
            if code not in dest["pos"]:
                dest["pos"].append(code)
        dest["senses"].extend(item["senses"])
        dest["phrases"].extend(item["phrases"][: max(0, MAX_PHRASES - len(dest["phrases"]))])
        for target in item.get("see", []):
            if target not in dest["see"]:
                dest["see"].append(target)
        if not dest.get("paradigm") and item.get("paradigm"):
            dest["paradigm"] = item["paradigm"]
    return [grouped[key] for key in order]


def resolve_see_also(entries: list[dict]) -> None:
    by_head: dict[str, dict] = {}
    for entry in entries:
        for word in entry["headwords"]:
            by_head.setdefault(word, entry)

    for entry in entries:
        if entry["senses"] or entry["phrases"]:
            continue
        resolved = False
        for target in entry.get("see", []):
            dest = by_head.get(target)
            if dest is None or dest is entry:
                continue
            if dest["senses"] or dest["phrases"]:
                entry["senses"] = [dict(sense) for sense in dest["senses"]]
                room = max(0, MAX_PHRASES - len(entry["phrases"]))
                entry["phrases"].extend(dest["phrases"][:room])
                if not entry.get("paradigm") and dest.get("paradigm"):
                    entry["paradigm"] = dest["paradigm"]
                resolved = True
                break
        if not resolved and entry.get("see"):
            target = entry["see"][0]
            entry["senses"] = [{"glosses": [f"см. {target}"], "examples": []}]


def main() -> int:
    here = Path(__file__).resolve()
    repo = here.parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--evs",
        type=Path,
        default=repo / "cache/evs_EKI_CCBY40.xml",
    )
    parser.add_argument(
        "--inflections",
        type=Path,
        default=repo / "data/est_inflected_forms.tsv",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=repo / "dist/est-ru.df",
    )
    args = parser.parse_args()

    if not args.evs.is_file():
        print(f"EVS XML not found: {args.evs}", file=sys.stderr)
        return 1

    print(f"Parsing {args.evs} ...", flush=True)
    parsed: list[dict] = []
    errors = 0
    for index, article in enumerate(iter_articles(args.evs), start=1):
        try:
            item = parse_article(article)
        except ET.ParseError:
            errors += 1
            continue
        if item:
            parsed.append(item)
        if index % 10000 == 0:
            print(f"  {index} articles, {len(parsed)} usable", flush=True)

    entries = merge_parsed(parsed)
    resolve_see_also(entries)
    print(f"Parsed {len(parsed)} articles into {len(entries)} headwords ({errors} XML errors)")

    inflections: dict[str, list[str]] = {}
    if args.inflections.is_file():
        print(f"Loading inflections from {args.inflections} ...", flush=True)
        inflections = load_inflections(args.inflections)
        matched = sum(
            1
            for entry in entries
            if any(inflection_forms(key, inflections) for key in entry["headwords"])
        )
        print(f"  inflection rows {len(inflections)}, matched headwords {matched}")

    print(f"Writing {args.output} ...", flush=True)
    write_df(entries, inflections, args.output)
    print(f"Wrote {args.output} ({args.output.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
