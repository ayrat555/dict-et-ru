#!/usr/bin/env python3
"""Convert a Kobo dicthtml zip into Kindle dictionary source (OPF + XHTML)."""

from __future__ import annotations

import argparse
import gzip
import html
import re
import struct
import uuid
import zipfile
import zlib
from pathlib import Path

MAX_SECTION_BYTES = 20 * 1024 * 1024
IDX_NS = "https://kindlegen.s3.amazonaws.com/AmazonKindlePublishingGuidelines.pdf"
UID = uuid.uuid5(uuid.NAMESPACE_URL, "https://arhiiv.eki.ee/litsents/idkaart/evs/")


WORD_RE = re.compile(r"<w>(.*?)</w>", re.S)
NAME_RE = re.compile(r'<a name="([^"]*)"')
HEADER_RE = re.compile(
    r"<p>\s*<a name=\"[^\"]*\"\s*/>\s*<b>.*?</b>\s*([^<]*)</p>",
    re.S,
)
VARIANT_RE = re.compile(r'<variant name="([^"]*)"')


def parse_word_block(block: str) -> dict | None:
    name_match = NAME_RE.search(block)
    if name_match is None:
        return None
    headword = html.unescape(name_match.group(1))
    header_match = HEADER_RE.search(block)
    pos = header_match.group(1).strip() if header_match else ""
    variants: list[str] = []
    seen = {headword}
    for raw in VARIANT_RE.findall(block):
        variant = html.unescape(raw)
        if variant and variant not in seen:
            seen.add(variant)
            variants.append(variant)
    var_end = block.find("</var>")
    if var_end != -1:
        body = block[var_end + 6 :].strip()
    else:
        first_p = block.find("</p>")
        body = block[first_p + 4 :].strip() if first_p != -1 else ""
    return {
        "headword": headword,
        "pos": pos,
        "variants": variants,
        "body": body,
    }


def parse_kobo_html(text: str) -> list[dict]:
    return [
        entry
        for match in WORD_RE.finditer(text)
        if (entry := parse_word_block(match.group(1)))
    ]


def _read_kobo_html(raw: bytes) -> str:
    if raw[:2] == b"\x1f\x8b":
        raw = gzip.decompress(raw)
    return raw.decode("utf-8")


def parse_kobo_zip(path: Path) -> list[dict]:
    grouped: dict[str, dict] = {}
    order: list[str] = []
    with zipfile.ZipFile(path) as archive:
        for name in archive.namelist():
            if not name.endswith(".html"):
                continue
            for item in parse_kobo_html(_read_kobo_html(archive.read(name))):
                key = item["headword"]
                if key not in grouped:
                    grouped[key] = item
                    order.append(key)
                    continue
                dest = grouped[key]
                for variant in item["variants"]:
                    if variant not in dest["variants"]:
                        dest["variants"].append(variant)
                if not dest["body"] and item["body"]:
                    dest["body"] = item["body"]
                if not dest["pos"] and item["pos"]:
                    dest["pos"] = item["pos"]
    return [grouped[key] for key in order]


def xml_attr(text: str) -> str:
    return html.escape(text, quote=True)


def render_entry(entry: dict) -> str:
    head = entry["headword"]
    parts = [
        '<idx:entry name="default" scriptable="yes" spell="yes">',
        f'<idx:orth value="{xml_attr(head)}">{html.escape(head, quote=False)}',
    ]
    if entry["variants"]:
        parts.append("<idx:infl>")
        for variant in entry["variants"]:
            parts.append(f'<idx:iform value="{xml_attr(variant)}"/>')
        parts.append("</idx:infl>")
    parts.append("</idx:orth>")
    header = html.escape(head, quote=False)
    if entry["pos"]:
        header = f"{header} {html.escape(entry['pos'], quote=False)}"
    parts.append(f"<p><b>{header}</b></p>")
    if entry.get("body"):
        parts.append(entry["body"])
    parts.append("</idx:entry>")
    return "\n".join(parts)


def html_document(entries_html: str, title: str) -> str:
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        f'<html xmlns="http://www.w3.org/1999/xhtml" xmlns:idx="{IDX_NS}" '
        f'xmlns:mbp="{IDX_NS}">\n'
        "<head>\n"
        '<meta http-equiv="Content-Type" content="text/html; charset=utf-8"/>\n'
        f"<title>{html.escape(title, quote=False)}</title>\n"
        "</head>\n"
        "<body>\n"
        "<mbp:frameset>\n"
        f"{entries_html}\n"
        "</mbp:frameset>\n"
        "</body>\n"
        "</html>\n"
    )


def write_cover_png(path: Path, width: int = 600, height: int = 800) -> None:
    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(tag + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    rgb = (36, 64, 99)
    raw = b"".join(b"\x00" + bytes(rgb) * width for _ in range(height))
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def write_kindle_source(
    entries: list[dict],
    dest: Path,
    *,
    title: str = "Eesti-vene sõnaraamat",
) -> Path:
    dest.mkdir(parents=True, exist_ok=True)
    for old in dest.glob("content-*.html"):
        old.unlink()
    write_cover_png(dest / "cover.png")

    files: list[str] = []
    chunk: list[str] = []
    chunk_bytes = 0
    index = 1

    def flush_chunk() -> None:
        nonlocal chunk, chunk_bytes, index
        if not chunk:
            return
        name = f"content-{index:03d}.html"
        (dest / name).write_text(
            html_document("\n<hr/>\n".join(chunk), title),
            encoding="utf-8",
        )
        files.append(name)
        chunk = []
        chunk_bytes = 0
        index += 1

    for entry in entries:
        rendered = render_entry(entry)
        size = len(rendered.encode("utf-8"))
        if chunk and chunk_bytes + size > MAX_SECTION_BYTES:
            flush_chunk()
        chunk.append(rendered)
        chunk_bytes += size
    flush_chunk()

    manifest = "\n".join(
        f'    <item id="c{i}" href="{name}" media-type="application/xhtml+xml"/>'
        for i, name in enumerate(files, start=1)
    )
    spine = "\n".join(
        f'    <itemref idref="c{i}"/>' for i in range(1, len(files) + 1)
    )
    opf = dest / "dict.opf"
    opf.write_text(
        (
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<package unique-identifier="uid" xmlns="http://www.idpf.org/2007/opf">\n'
            '  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" '
            'xmlns:opf="http://www.idpf.org/2007/opf">\n'
            f"    <dc:title>{html.escape(title, quote=False)}</dc:title>\n"
            "    <dc:language>et</dc:language>\n"
            "    <dc:creator>Eesti Keele Instituut</dc:creator>\n"
            f'    <dc:identifier id="uid">urn:uuid:{UID}</dc:identifier>\n'
            '    <meta name="cover" content="cover-image"/>\n'
            "    <x-metadata>\n"
            "      <DictionaryInLanguage>et</DictionaryInLanguage>\n"
            "      <DictionaryOutLanguage>ru</DictionaryOutLanguage>\n"
            "      <DefaultLookupIndex>default</DefaultLookupIndex>\n"
            "    </x-metadata>\n"
            "  </metadata>\n"
            "  <manifest>\n"
            '    <item id="cover-image" href="cover.png" media-type="image/png"/>\n'
            f"{manifest}\n"
            "  </manifest>\n"
            "  <spine>\n"
            f"{spine}\n"
            "  </spine>\n"
            '  <guide>\n'
            f'    <reference type="index" title="Dictionary" href="{files[0] if files else "content-001.html"}"/>\n'
            "  </guide>\n"
            "</package>\n"
        ),
        encoding="utf-8",
    )
    return opf


def main() -> int:
    here = Path(__file__).resolve()
    repo = here.parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--kobo",
        type=Path,
        default=repo / "dicthtml-et-ru.zip",
        help="Kobo dicthtml zip (default: dicthtml-et-ru.zip)",
    )
    parser.add_argument("--outdir", type=Path, default=repo / "dist/kindle")
    args = parser.parse_args()
    if not args.kobo.is_file():
        raise SystemExit(f"Kobo dictionary not found: {args.kobo}")
    print(f"Reading {args.kobo} ...", flush=True)
    entries = parse_kobo_zip(args.kobo)
    print(f"  {len(entries)} entries", flush=True)
    opf = write_kindle_source(entries, args.outdir)
    print(f"Wrote Kindle source {opf}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
