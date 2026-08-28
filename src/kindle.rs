use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use flate2::read::GzDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use regex::Regex;
use zip::ZipArchive;

use crate::html_util;

pub const MAX_SECTION_BYTES: usize = 20 * 1024 * 1024;
pub const IDX_NS: &str = "https://kindlegen.s3.amazonaws.com/AmazonKindlePublishingGuidelines.pdf";
pub const UID: &str = "d00df2e5-502b-5858-bd67-f80d9cc89743";
pub const DEFAULT_TITLE: &str = "Eesti-vene sõnaraamat";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub headword: String,
    pub pos: String,
    pub variants: Vec<String>,
    pub body: String,
}

static WORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<w>(.*?)</w>").expect("word regex"));
static NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<a name="([^"]*)""#).expect("name regex"));
static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<p>\s*<a name="[^"]*"\s*/>\s*<b>.*?</b>\s*([^<]*)</p>"#)
        .expect("header regex")
});
static VARIANT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<variant name="([^"]*)""#).expect("variant regex"));

pub fn parse_word_block(block: &str) -> Option<Entry> {
    let name_match = NAME_RE.captures(block)?;
    let headword = html_util::unescape(&name_match[1]);
    let pos = HEADER_RE
        .captures(block)
        .map(|caps| caps[1].trim().to_string())
        .unwrap_or_default();
    let mut variants = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(headword.clone());
    for caps in VARIANT_RE.captures_iter(block) {
        let variant = html_util::unescape(&caps[1]);
        if !variant.is_empty() && seen.insert(variant.clone()) {
            variants.push(variant);
        }
    }
    let body = if let Some(var_end) = block.find("</var>") {
        block[var_end + 6..].trim().to_string()
    } else if let Some(first_p) = block.find("</p>") {
        block[first_p + 4..].trim().to_string()
    } else {
        String::new()
    };
    Some(Entry {
        headword,
        pos,
        variants,
        body,
    })
}

pub fn parse_kobo_html(text: &str) -> Vec<Entry> {
    WORD_RE
        .captures_iter(text)
        .filter_map(|caps| parse_word_block(&caps[1]))
        .collect()
}

fn read_kobo_html(raw: &[u8]) -> Result<String, String> {
    let decoded = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(raw);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|err| err.to_string())?;
        out
    } else {
        raw.to_vec()
    };
    String::from_utf8(decoded).map_err(|err| err.to_string())
}

pub fn parse_kobo_zip(path: &Path) -> Result<Vec<Entry>, String> {
    let bytes = fs::read(path).map_err(|err| format!("{path}: {err}", path = path.display()))?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|err| format!("zip: {err}"))?;
    let mut grouped: indexmap::IndexMap<String, Entry> = indexmap::IndexMap::new();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).map(|file| file.name().to_string()))
        .collect::<Result<_, _>>()
        .map_err(|err| format!("zip: {err}"))?;
    for name in names {
        if !name.ends_with(".html") {
            continue;
        }
        let mut file = archive
            .by_name(&name)
            .map_err(|err| format!("{name}: {err}"))?;
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)
            .map_err(|err| format!("{name}: {err}"))?;
        let text = read_kobo_html(&raw)?;
        for item in parse_kobo_html(&text) {
            if let Some(dest) = grouped.get_mut(&item.headword) {
                for variant in item.variants {
                    if !dest.variants.contains(&variant) {
                        dest.variants.push(variant);
                    }
                }
                if !item.body.is_empty() && !dest.body.contains(&item.body) {
                    dest.body = if dest.body.is_empty() {
                        item.body
                    } else {
                        format!("{}\n{}", dest.body, item.body)
                    };
                }
                if !item.pos.is_empty() && !dest.pos.contains(&item.pos) {
                    dest.pos = if dest.pos.is_empty() {
                        item.pos
                    } else {
                        format!("{}, {}", dest.pos, item.pos)
                    };
                }
            } else {
                grouped.insert(item.headword.clone(), item);
            }
        }
    }
    Ok(grouped.into_values().collect())
}

fn xml_attr(text: &str) -> String {
    html_util::escape(text, true)
}

pub fn render_entry(entry: &Entry) -> String {
    let head = &entry.headword;
    let mut parts = vec![
        "<idx:entry name=\"default\" scriptable=\"yes\" spell=\"yes\">".to_string(),
        format!(
            "<idx:orth value=\"{}\">{}",
            xml_attr(head),
            html_util::escape(head, false)
        ),
    ];
    if !entry.variants.is_empty() {
        parts.push("<idx:infl>".to_string());
        for variant in &entry.variants {
            parts.push(format!("<idx:iform value=\"{}\"/>", xml_attr(variant)));
        }
        parts.push("</idx:infl>".to_string());
    }
    parts.push("</idx:orth>".to_string());
    let mut header = html_util::escape(head, false);
    if !entry.pos.is_empty() {
        header = format!("{} {}", header, html_util::escape(&entry.pos, false));
    }
    parts.push(format!("<p><b>{header}</b></p>"));
    if !entry.body.is_empty() {
        parts.push(entry.body.clone());
    }
    parts.push("</idx:entry>".to_string());
    parts.join("\n")
}

pub fn html_document(entries_html: &str, title: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:idx=\"{IDX_NS}\" xmlns:mbp=\"{IDX_NS}\">\n\
<head>\n\
<meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\"/>\n\
<title>{title}</title>\n\
</head>\n\
<body>\n\
<mbp:frameset>\n\
{entries}\n\
</mbp:frameset>\n\
</body>\n\
</html>\n",
        title = html_util::escape(title, false),
        entries = entries_html
    )
}

fn png_chunk(tag: &[u8], data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(tag.len() + data.len());
    payload.extend_from_slice(tag);
    payload.extend_from_slice(data);
    let crc = crc32fast::hash(&payload);
    let mut out = Vec::with_capacity(12 + payload.len());
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

pub fn write_cover_png(path: &Path, width: u32, height: u32) -> Result<(), String> {
    let rgb = [36u8, 64, 99];
    let stride = 1 + width as usize * 3;
    let mut raw = vec![0u8; stride * height as usize];
    for row in raw.chunks_exact_mut(stride) {
        for pixel in row[1..].as_chunks_mut::<3>().0 {
            pixel.copy_from_slice(&rgb);
        }
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(9));
    encoder.write_all(&raw).map_err(|err| err.to_string())?;
    let compressed = encoder.finish().map_err(|err| err.to_string())?;
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
    png.extend_from_slice(&png_chunk(b"IDAT", &compressed));
    png.extend_from_slice(&png_chunk(b"IEND", b""));
    fs::write(path, png).map_err(|err| err.to_string())
}

pub fn write_kindle_source(entries: &[Entry], dest: &Path, title: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(dest).map_err(|err| err.to_string())?;
    if dest.is_dir() {
        for entry in fs::read_dir(dest).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("content-") && name.ends_with(".html") {
                fs::remove_file(entry.path()).map_err(|err| err.to_string())?;
            }
        }
    }
    write_cover_png(&dest.join("cover.png"), 600, 800)?;

    let mut files = Vec::new();
    let mut chunk: Vec<String> = Vec::new();
    let mut chunk_bytes = 0usize;
    let mut index = 1u32;

    let mut flush_chunk = |chunk: &mut Vec<String>, chunk_bytes: &mut usize, index: &mut u32| {
        if chunk.is_empty() {
            return Ok::<(), String>(());
        }
        let name = format!("content-{index:03}.html");
        fs::write(
            dest.join(&name),
            html_document(&chunk.join("\n<hr/>\n"), title),
        )
        .map_err(|err| err.to_string())?;
        files.push(name);
        chunk.clear();
        *chunk_bytes = 0;
        *index += 1;
        Ok(())
    };

    for entry in entries {
        let rendered = render_entry(entry);
        let size = rendered.len();
        if !chunk.is_empty() && chunk_bytes + size > MAX_SECTION_BYTES {
            flush_chunk(&mut chunk, &mut chunk_bytes, &mut index)?;
        }
        chunk.push(rendered);
        chunk_bytes += size;
    }
    flush_chunk(&mut chunk, &mut chunk_bytes, &mut index)?;

    let manifest = files
        .iter()
        .enumerate()
        .map(|(i, name)| {
            format!(
                "    <item id=\"c{}\" href=\"{name}\" media-type=\"application/xhtml+xml\"/>",
                i + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let spine = (1..=files.len())
        .map(|i| format!("    <itemref idref=\"c{i}\"/>"))
        .collect::<Vec<_>>()
        .join("\n");
    let first = files
        .first()
        .map(String::as_str)
        .unwrap_or("content-001.html");
    let opf = dest.join("dict.opf");
    fs::write(
        &opf,
        format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<package unique-identifier=\"uid\" xmlns=\"http://www.idpf.org/2007/opf\">\n",
                "  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\" ",
                "xmlns:opf=\"http://www.idpf.org/2007/opf\">\n",
                "    <dc:title>{title}</dc:title>\n",
                "    <dc:language>et</dc:language>\n",
                "    <dc:creator>Eesti Keele Instituut</dc:creator>\n",
                "    <dc:identifier id=\"uid\">urn:uuid:{uid}</dc:identifier>\n",
                "    <meta name=\"cover\" content=\"cover-image\"/>\n",
                "    <x-metadata>\n",
                "      <DictionaryInLanguage>et</DictionaryInLanguage>\n",
                "      <DictionaryOutLanguage>ru</DictionaryOutLanguage>\n",
                "      <DefaultLookupIndex>default</DefaultLookupIndex>\n",
                "    </x-metadata>\n",
                "  </metadata>\n",
                "  <manifest>\n",
                "    <item id=\"cover-image\" href=\"cover.png\" media-type=\"image/png\"/>\n",
                "{manifest}\n",
                "  </manifest>\n",
                "  <spine>\n",
                "{spine}\n",
                "  </spine>\n",
                "  <guide>\n",
                "    <reference type=\"index\" title=\"Dictionary\" href=\"{first}\"/>\n",
                "  </guide>\n",
                "</package>\n"
            ),
            title = html_util::escape(title, false),
            uid = UID,
            manifest = manifest,
            spine = spine,
            first = first,
        ),
    )
    .map_err(|err| err.to_string())?;
    Ok(opf)
}

pub fn convert(kobo: &Path, outdir: &Path) -> Result<PathBuf, String> {
    if !kobo.is_file() {
        return Err(format!("Kobo dictionary not found: {}", kobo.display()));
    }
    eprintln!("Reading {} ...", kobo.display());
    let entries = parse_kobo_zip(kobo)?;
    eprintln!("  {} entries", entries.len());
    let opf = write_kindle_source(&entries, outdir, DEFAULT_TITLE)?;
    eprintln!("Wrote Kindle source {}", opf.display());
    Ok(opf)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::write::GzEncoder;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    use super::*;

    const SAMPLE_HTML: &str = r#"<html><w><p><a name="aabe" /><b>aabe</b> сущ.</p><var><variant name="aape"/><variant name="aabet"/></var>
<p><i>aabe aape aabet</i></p>
<p><b>буква</b></p></w><w><p><a name="24/7" /><b>24/7</b></p><var></var>
<p><b>круглосуточно</b></p></w></html>
"#;

    fn gzip_bytes(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn write_zip(path: &Path, files: &[(&str, Vec<u8>)]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, data) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn reads_headword_pos_variants_and_body() {
        let entries = parse_kobo_html(SAMPLE_HTML);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].headword, "aabe");
        assert_eq!(entries[0].pos, "сущ.");
        assert_eq!(entries[0].variants, ["aape", "aabet"]);
        assert!(entries[0].body.contains("буква"));
        assert_eq!(entries[1].headword, "24/7");
        assert!(entries[1].variants.is_empty());
    }

    #[test]
    fn dedupes_copied_variant_files() {
        let dir = tempfile_dir();
        let zip_path = dir.join("dicthtml.zip");
        let html = gzip_bytes(SAMPLE_HTML.as_bytes());
        write_zip(&zip_path, &[("aa.html", html.clone()), ("ap.html", html)]);
        let entries = parse_kobo_zip(&zip_path).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|item| item.headword.as_str())
                .collect::<Vec<_>>(),
            ["aabe", "24/7"]
        );
        assert_eq!(entries[0].variants, ["aape", "aabet"]);
    }

    #[test]
    fn merges_homograph_bodies() {
        let html = concat!(
            r#"<html><w><p><a name="sai" /><b>sai</b> сущ.</p><var></var>"#,
            "<p><b>булка</b></p></w>",
            r#"<w><p><a name="sai" /><b>sai</b> гл.</p><var></var>"#,
            "<p><i>saama</i></p><p><b>получать</b></p></w></html>"
        );
        let dir = tempfile_dir();
        let zip_path = dir.join("dicthtml.zip");
        write_zip(&zip_path, &[("sa.html", gzip_bytes(html.as_bytes()))]);
        let entries = parse_kobo_zip(&zip_path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].headword, "sai");
        assert_eq!(entries[0].pos, "сущ., гл.");
        assert!(entries[0].body.contains("булка"));
        assert!(entries[0].body.contains("получать"));
        assert!(entries[0].body.contains("saama"));
    }

    #[test]
    fn entry_has_orth_and_inflections() {
        let html = render_entry(&Entry {
            headword: "aabe".into(),
            pos: "сущ.".into(),
            variants: vec!["aape".into()],
            body: "<p><b>буква</b></p>".into(),
        });
        assert!(html.contains(r#"<idx:orth value="aabe">"#));
        assert!(html.contains(r#"<idx:iform value="aape"/>"#));
        assert!(html.contains("<p><b>aabe сущ.</b></p>"));
    }

    #[test]
    fn writes_opf_and_html() {
        let entries = [Entry {
            headword: "aabe".into(),
            pos: "сущ.".into(),
            variants: vec!["aape".into()],
            body: "<p><b>буква</b></p>".into(),
        }];
        let dest = tempfile_dir();
        let opf = write_kindle_source(&entries, &dest, DEFAULT_TITLE).unwrap();
        let html = fs::read_to_string(dest.join("content-001.html")).unwrap();
        let text = fs::read_to_string(opf).unwrap();
        assert!(dest.join("cover.png").is_file());
        assert!(text.contains("DictionaryInLanguage>et<"));
        assert!(text.contains("DictionaryOutLanguage>ru<"));
        assert!(text.contains("DefaultLookupIndex>default<"));
        assert!(text.contains(r#"name="cover""#));
        assert!(html.contains(r#"<idx:entry name="default""#));
        assert!(html.contains("<mbp:frameset>"));
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kobo-et-ru-kindle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
