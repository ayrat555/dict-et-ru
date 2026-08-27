use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use indexmap::IndexMap;
use roxmltree::{Document, Node};

use crate::html_util;

pub const NS_URI: &str = "http://www.eki.ee/dict/ev2";
pub const MAX_EXAMPLES_PER_SENSE: usize = 6;
pub const MAX_PHRASES: usize = 4;
pub const MAX_VARIANTS: usize = 120;
pub const MIN_INFLECTION_HEADWORD_LEN: usize = 3;
pub const MIN_ALIAS_HEADWORD_LEN: usize = 3;

const VOWELS: &str = "аеёиоуыэюяАЕЁИОУЫЭЮЯaeiouyäöüõAEIOUYÄÖÜÕ";

fn pos_label(code: &str) -> &str {
    match code {
        "s" => "сущ.",
        "v" => "гл.",
        "adj" => "прил.",
        "adv" => "нар.",
        "pron" => "мест.",
        "num" => "числ.",
        "konj" => "союз",
        "prep" => "предл.",
        "postp" => "послелог",
        "interj" => "межд.",
        "prop" => "имя собств.",
        "adjg" => "прил.",
        "adjid" => "прил.",
        "vrm" => "гл.",
        other => other,
    }
}

fn is_verb_pos(code: &str) -> bool {
    matches!(code, "v" | "vrm")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sense {
    pub glosses: Vec<String>,
    pub examples: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub headwords: Vec<String>,
    pub pos: Vec<String>,
    pub senses: Vec<Sense>,
    pub phrases: Vec<(String, String)>,
    pub see: Vec<String>,
    pub paradigm: String,
}

fn local_name<'a, 'input>(node: Node<'a, 'input>) -> &'a str {
    node.tag_name().name()
}

fn children<'a, 'input>(el: Node<'a, 'input>, name: &str) -> Vec<Node<'a, 'input>> {
    el.children()
        .filter(|child| child.is_element() && local_name(*child) == name)
        .collect()
}

fn first_child<'a, 'input>(el: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    children(el, name).into_iter().next()
}

fn attr(el: Node<'_, '_>, name: &str) -> String {
    el.attribute((NS_URI, name))
        .or_else(|| el.attribute(name))
        .unwrap_or("")
        .to_string()
}

fn walk_text(node: Node<'_, '_>, chunks: &mut Vec<String>) {
    let kids: Vec<_> = node.children().collect();
    let mut i = 0;
    let mut leading = String::new();
    while i < kids.len() && !kids[i].is_element() {
        if kids[i].is_text() {
            leading.push_str(kids[i].text().unwrap_or(""));
        }
        i += 1;
    }
    if !leading.is_empty() {
        chunks.push(leading);
    }
    while i < kids.len() {
        if !kids[i].is_element() {
            i += 1;
            continue;
        }
        walk_text(kids[i], chunks);
        i += 1;
        let mut tail = String::new();
        while i < kids.len() && !kids[i].is_element() {
            if kids[i].is_text() {
                tail.push_str(kids[i].text().unwrap_or(""));
            }
            i += 1;
        }
        if tail.is_empty() {
            continue;
        }
        if let Some(last) = chunks.last() {
            if let (Some(prev), Some(next)) = (last.chars().last(), tail.chars().next()) {
                if !prev.is_whitespace() && !next.is_whitespace() && !",.;:!?)]}".contains(next) {
                    chunks.push(" ".to_string());
                }
            }
        }
        chunks.push(tail);
    }
}

pub fn text_of(el: Option<Node<'_, '_>>) -> String {
    let Some(node) = el else {
        return String::new();
    };
    let mut chunks = Vec::new();
    walk_text(node, &mut chunks);
    chunks
        .concat()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn apply_stress(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' && i + 1 < chars.len() && VOWELS.contains(chars[i + 1]) {
            out.push(chars[i + 1]);
            out.push('\u{0301}');
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub fn clean_text(text: &str) -> String {
    let text = html_util::unescape(text).replace("&v;", " / ");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    apply_stress(&text).trim_matches(|ch| ch == ' ' || ch == '/').to_string()
}

pub fn clean_headword(text: &str) -> String {
    let text = text.replace('+', "").replace('"', "");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn compound_prefix(raw_headword: &str) -> String {
    if !raw_headword.contains('+') {
        return String::new();
    }
    let prefix = raw_headword.rsplit_once('+').map(|(left, _)| left).unwrap_or("");
    clean_headword(prefix)
}

pub fn clean_paradigm(raw: &str, prefix: &str) -> String {
    let text = html_util::unescape(raw)
        .replace("_&_", " / ")
        .replace('&', " / ")
        .replace('\'', "");
    let mut parts = Vec::new();
    for token in text.split_whitespace() {
        let mut token = token.replace('[', "").replace('/', "");
        token = token.trim_start_matches('+').to_string();
        if token.is_empty() {
            continue;
        }
        if !prefix.is_empty()
            && token.chars().any(|ch| ch.is_alphabetic())
            && !token.starts_with(prefix)
        {
            token = format!("{prefix}{token}");
        }
        parts.push(token);
    }
    parts.join(" ")
}

fn compact_spelling(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, '+' | '-' | '.' | ' '))
        .collect::<String>()
        .to_lowercase()
}

pub fn is_compact_variant(sort_key: &str, word: &str) -> bool {
    !sort_key.is_empty() && sort_key != word && compact_spelling(sort_key) == compact_spelling(word)
}

pub fn inflection_keys(word: &str) -> Vec<String> {
    let compact = word.replace(['-', ' '], "");
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    for key in [
        word.to_string(),
        word.to_lowercase(),
        compact.clone(),
        compact.to_lowercase(),
    ] {
        if !key.is_empty() && seen.insert(key.clone()) {
            keys.push(key);
        }
    }
    keys
}

pub fn inflection_forms<'a>(
    word: &str,
    inflections: &'a HashMap<String, Vec<String>>,
) -> &'a [String] {
    if word.chars().count() < MIN_INFLECTION_HEADWORD_LEN {
        return &[];
    }
    for key in inflection_keys(word) {
        if let Some(forms) = inflections.get(&key) {
            return forms;
        }
    }
    &[]
}

pub fn collect_variants(entry: &Entry, inflections: &HashMap<String, Vec<String>>) -> Vec<String> {
    let headword = &entry.headwords[0];
    let mut variants = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(headword.clone());
    for extra in entry.headwords.iter().skip(1) {
        if seen.insert(extra.clone()) {
            variants.push(extra.clone());
        }
    }
    for key in &entry.headwords {
        for form in inflection_forms(key, inflections) {
            if seen.insert(form.clone()) {
                variants.push(form.clone());
            }
        }
    }
    variants.truncate(MAX_VARIANTS);
    variants
}

pub fn iter_articles(xml: &str) -> Vec<&str> {
    let mut articles = Vec::new();
    let mut rest = xml;
    loop {
        let Some(start) = rest.find("<x:A") else {
            break;
        };
        let from = &rest[start..];
        let Some(end_rel) = from.find("</x:A>") else {
            break;
        };
        articles.push(&from[..end_rel + 6]);
        rest = &from[end_rel + 6..];
    }
    articles
}

fn wrap_article(article: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?><root xmlns:x="{NS_URI}">{article}</root>"#)
}

fn gloss_from_xg(xg: Node<'_, '_>) -> Option<String> {
    let gloss = clean_text(&text_of(first_child(xg, "x").into()));
    if gloss.is_empty() {
        return None;
    }
    let mut extras = Vec::new();
    if let Some(vg) = first_child(xg, "vg") {
        for gender in children(vg, "vgsugu") {
            let value = clean_text(&text_of(Some(gender)));
            if !value.is_empty() {
                extras.push(value);
            }
        }
    }
    if extras.is_empty() {
        Some(gloss)
    } else {
        Some(format!("{gloss} ({})", extras.join(", ")))
    }
}

pub fn parse_article(article: &str) -> Result<Option<Entry>, roxmltree::Error> {
    let wrapped = wrap_article(article);
    let doc = Document::parse(&wrapped)?;
    let root = doc.root_element();
    let Some(entry) = first_child(root, "A") else {
        return Ok(None);
    };
    let Some(head) = first_child(entry, "P") else {
        return Ok(None);
    };
    let senses_el = first_child(entry, "S");
    let phrases_el = first_child(entry, "F");

    let mut headwords = Vec::new();
    let mut pos_codes = Vec::new();
    let mut prefix = String::new();
    let mut paradigm = String::new();
    for mg in children(head, "mg") {
        for m in children(mg, "m") {
            let raw_m = text_of(Some(m));
            if prefix.is_empty() {
                prefix = compound_prefix(&raw_m);
            }
            let attr_o = attr(m, "O");
            let word = clean_headword(if raw_m.is_empty() { &attr_o } else { &raw_m });
            if !word.is_empty() && !headwords.contains(&word) {
                headwords.push(word.clone());
            }
            let sort_key = clean_headword(&attr_o);
            if !sort_key.is_empty()
                && !headwords.contains(&sort_key)
                && is_compact_variant(&sort_key, &word)
            {
                headwords.push(sort_key);
            }
        }
        for sl in children(mg, "sl") {
            let code = clean_text(&text_of(Some(sl)));
            if !code.is_empty() && !pos_codes.contains(&code) {
                pos_codes.push(code);
            }
        }
        for grg in children(mg, "grg") {
            if let Some(mv) = first_child(grg, "mv") {
                if paradigm.is_empty() {
                    paradigm = clean_paradigm(&text_of(Some(mv)), &prefix);
                }
            }
        }
    }
    if headwords.is_empty() {
        return Ok(None);
    }

    let mut senses = Vec::new();
    let mut see = Vec::new();
    if let Some(senses_el) = senses_el {
        for tp in children(senses_el, "tp") {
            for tvt in children(tp, "tvt") {
                let target = clean_headword(&text_of(Some(tvt)));
                if !target.is_empty() && !see.contains(&target) {
                    see.push(target);
                }
            }
            let mut glosses = Vec::new();
            let mut examples = Vec::new();
            for tg in children(tp, "tg") {
                if let Some(xp) = first_child(tg, "xp") {
                    for xg in children(xp, "xg") {
                        if let Some(gloss) = gloss_from_xg(xg) {
                            if !glosses.contains(&gloss) {
                                glosses.push(gloss);
                            }
                        }
                    }
                }
            }
            if glosses.is_empty() {
                for tg in children(tp, "tg") {
                    if let Some(dg) = first_child(tg, "dg") {
                        for definition in children(dg, "d") {
                            let est = clean_text(&text_of(Some(definition)));
                            if !est.is_empty() && !glosses.contains(&est) {
                                glosses.push(est);
                            }
                        }
                    }
                }
            }
            if let Some(np) = first_child(tp, "np") {
                for ng in children(np, "ng") {
                    let et = clean_text(&text_of(first_child(ng, "n").into()));
                    let qnp = first_child(ng, "qnp");
                    let rus_root = qnp.unwrap_or(ng);
                    let rus: Vec<String> = children(rus_root, "qng")
                        .into_iter()
                        .flat_map(|qng| children(qng, "qn"))
                        .map(|qn| clean_text(&text_of(Some(qn))))
                        .filter(|item| !item.is_empty())
                        .collect();
                    if !et.is_empty() && !rus.is_empty() && examples.len() < MAX_EXAMPLES_PER_SENSE {
                        examples.push((et, rus.join(" / ")));
                    }
                }
            }
            if !glosses.is_empty() {
                senses.push(Sense { glosses, examples });
            }
        }
    }

    let mut phrases = Vec::new();
    if let Some(phrases_el) = phrases_el {
        for fg in children(phrases_el, "fg") {
            let et = clean_text(&text_of(first_child(fg, "f").into()));
            let rus: Vec<String> = children(fg, "fqnp")
                .into_iter()
                .flat_map(|fqnp| children(fqnp, "fqng"))
                .flat_map(|fqng| children(fqng, "qf"))
                .map(|qf| clean_text(&text_of(Some(qf))))
                .filter(|item| !item.is_empty())
                .collect();
            if !et.is_empty() && !rus.is_empty() && phrases.len() < MAX_PHRASES {
                phrases.push((et, rus.join(" / ")));
            }
        }
    }

    if senses.is_empty() && phrases.is_empty() && see.is_empty() {
        return Ok(None);
    }

    Ok(Some(Entry {
        headwords,
        pos: pos_codes,
        senses,
        phrases,
        see,
        paradigm,
    }))
}

pub fn load_inflections(path: &Path) -> Result<HashMap<String, Vec<String>>, String> {
    let file = File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let mut mapping = HashMap::new();
    if let Some(header) = lines.next() {
        let header = header.map_err(|err| err.to_string())?;
        if !header.contains('\t') {
            // no header; treat as a data line
            ingest_inflection_line(&header, &mut mapping);
        }
    }
    for line in lines {
        let line = line.map_err(|err| err.to_string())?;
        ingest_inflection_line(&line, &mut mapping);
    }
    Ok(mapping)
}

fn ingest_inflection_line(line: &str, mapping: &mut HashMap<String, Vec<String>>) {
    if !line.contains('\t') {
        return;
    }
    let (word, forms) = line.split_once('\t').unwrap();
    let word = word.trim_end_matches(['\n', '\r']);
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(word.to_string());
    for form in forms.split(',') {
        let form = form.trim();
        if form.is_empty() || !seen.insert(form.to_string()) {
            continue;
        }
        unique.push(form.to_string());
        if unique.len() >= MAX_VARIANTS {
            break;
        }
    }
    if !unique.is_empty() {
        mapping.insert(word.to_string(), unique);
    }
}

pub fn pos_header(codes: &[String]) -> String {
    let mut labels = Vec::new();
    let mut seen = HashSet::new();
    for code in codes {
        let label = pos_label(code).to_string();
        if seen.insert(label.clone()) {
            labels.push(label);
        }
    }
    labels.join(", ")
}

pub fn is_alias_headword(variant: &str, parents: &[&Entry], headwords: &HashSet<String>) -> bool {
    if variant.chars().count() < MIN_ALIAS_HEADWORD_LEN {
        return false;
    }
    let verb_form = parents.iter().any(|parent| {
        parent.pos.iter().any(|code| is_verb_pos(code)) && parent.headwords[0] != variant
    });
    if verb_form {
        return true;
    }
    if headwords.contains(variant) {
        return false;
    }
    parents.len() > 1
}

pub fn render_definition(entry: &Entry, see_lemma: Option<&str>) -> String {
    let mut parts = vec!["<html>".to_string()];
    if let Some(lemma) = see_lemma {
        parts.push(format!("<p><i>{}</i></p>", html_util::escape(lemma, false)));
    }
    if !entry.paradigm.is_empty() {
        parts.push(format!(
            "<p><i>{}</i></p>",
            html_util::escape(&entry.paradigm, false)
        ));
    }
    for (index, sense) in entry.senses.iter().enumerate() {
        let gloss = sense.glosses.join("; ");
        let prefix = if entry.senses.len() > 1 {
            format!("{}. ", index + 1)
        } else {
            String::new()
        };
        parts.push(format!(
            "<p><b>{prefix}{}</b></p>",
            html_util::escape(&gloss, false)
        ));
        for (et, ru) in &sense.examples {
            parts.push(format!(
                "<p><i>{}</i> — {}</p>",
                html_util::escape(et, false),
                html_util::escape(ru, false)
            ));
        }
    }
    if !entry.phrases.is_empty() {
        parts.push("<p><i>Фразеологизмы</i></p>".to_string());
        for (et, ru) in &entry.phrases {
            parts.push(format!(
                "<p><i>{}</i> — {}</p>",
                html_util::escape(et, false),
                html_util::escape(ru, false)
            ));
        }
    }
    parts.join("\n")
}

pub fn write_df(
    entries: &[Entry],
    inflections: &HashMap<String, Vec<String>>,
    dest: &Path,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut headwords: HashSet<String> = entries
        .iter()
        .map(|entry| entry.headwords[0].clone())
        .collect();
    let mut owners: IndexMap<String, Vec<usize>> = IndexMap::new();
    let mut handle = BufWriter::new(File::create(dest).map_err(|err| err.to_string())?);
    for (index, entry) in entries.iter().enumerate() {
        let headword = &entry.headwords[0];
        let variants = collect_variants(entry, inflections);
        writeln!(handle, "@ {headword}").map_err(|err| err.to_string())?;
        let header = pos_header(&entry.pos);
        if !header.is_empty() {
            writeln!(handle, ": {header}").map_err(|err| err.to_string())?;
        }
        for variant in &variants {
            writeln!(handle, "& {variant}").map_err(|err| err.to_string())?;
            owners.entry(variant.clone()).or_default().push(index);
        }
        write!(handle, "{}", render_definition(entry, None)).map_err(|err| err.to_string())?;
        write!(handle, "\n\n").map_err(|err| err.to_string())?;
    }

    for (variant, parent_indexes) in &owners {
        let parents: Vec<&Entry> = parent_indexes.iter().map(|&i| &entries[i]).collect();
        if !is_alias_headword(variant, &parents, &headwords) {
            continue;
        }
        for parent in parents {
            writeln!(handle, "@ {variant}").map_err(|err| err.to_string())?;
            let header = pos_header(&parent.pos);
            if !header.is_empty() {
                writeln!(handle, ": {header}").map_err(|err| err.to_string())?;
            }
            write!(
                handle,
                "{}",
                render_definition(parent, Some(&parent.headwords[0]))
            )
            .map_err(|err| err.to_string())?;
            write!(handle, "\n\n").map_err(|err| err.to_string())?;
        }
        headwords.insert(variant.clone());
    }
    handle.flush().map_err(|err| err.to_string())?;
    Ok(())
}

pub fn merge_parsed(parsed: Vec<Entry>) -> Vec<Entry> {
    let mut grouped: IndexMap<String, Entry> = IndexMap::new();
    for item in parsed {
        let key = item.headwords[0].clone();
        if let Some(dest) = grouped.get_mut(&key) {
            for word in item.headwords {
                if !dest.headwords.contains(&word) {
                    dest.headwords.push(word);
                }
            }
            for code in item.pos {
                if !dest.pos.contains(&code) {
                    dest.pos.push(code);
                }
            }
            dest.senses.extend(item.senses);
            let room = MAX_PHRASES.saturating_sub(dest.phrases.len());
            dest.phrases.extend(item.phrases.into_iter().take(room));
            for target in item.see {
                if !dest.see.contains(&target) {
                    dest.see.push(target);
                }
            }
            if dest.paradigm.is_empty() && !item.paradigm.is_empty() {
                dest.paradigm = item.paradigm;
            }
        } else {
            grouped.insert(key, item);
        }
    }
    grouped.into_values().collect()
}

pub fn resolve_see_also(entries: &mut [Entry]) {
    let mut by_head: HashMap<String, usize> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        for word in &entry.headwords {
            by_head.entry(word.clone()).or_insert(index);
        }
    }
    for i in 0..entries.len() {
        if !entries[i].senses.is_empty() || !entries[i].phrases.is_empty() {
            continue;
        }
        let see = entries[i].see.clone();
        let mut resolved = false;
        for target in &see {
            let Some(&dest_i) = by_head.get(target) else {
                continue;
            };
            if dest_i == i {
                continue;
            }
            if !entries[dest_i].senses.is_empty() || !entries[dest_i].phrases.is_empty() {
                entries[i].senses = entries[dest_i].senses.clone();
                let room = MAX_PHRASES.saturating_sub(entries[i].phrases.len());
                let extra: Vec<_> = entries[dest_i].phrases.iter().take(room).cloned().collect();
                entries[i].phrases.extend(extra);
                if entries[i].paradigm.is_empty() && !entries[dest_i].paradigm.is_empty() {
                    entries[i].paradigm = entries[dest_i].paradigm.clone();
                }
                resolved = true;
                break;
            }
        }
        if !resolved {
            if let Some(target) = see.first() {
                entries[i].senses = vec![Sense {
                    glosses: vec![format!("см. {target}")],
                    examples: Vec::new(),
                }];
            }
        }
    }
}

pub fn convert(evs: &Path, inflections_path: &Path, output: &Path) -> Result<(), String> {
    if !evs.is_file() {
        return Err(format!("EVS XML not found: {}", evs.display()));
    }
    eprintln!("Parsing {} ...", evs.display());
    let xml = fs::read_to_string(evs).map_err(|err| format!("{}: {err}", evs.display()))?;
    let mut parsed = Vec::new();
    let mut errors = 0usize;
    for (index, article) in iter_articles(&xml).into_iter().enumerate() {
        match parse_article(article) {
            Ok(Some(item)) => parsed.push(item),
            Ok(None) => {}
            Err(_) => errors += 1,
        }
        if (index + 1) % 10000 == 0 {
            eprintln!("  {} articles, {} usable", index + 1, parsed.len());
        }
    }
    let mut entries = merge_parsed(parsed);
    let parsed_len = entries.len();
    resolve_see_also(&mut entries);
    eprintln!("Parsed articles into {parsed_len} headwords ({errors} XML errors)");

    let mut inflections = HashMap::new();
    if inflections_path.is_file() {
        eprintln!("Loading inflections from {} ...", inflections_path.display());
        inflections = load_inflections(inflections_path)?;
        let matched = entries
            .iter()
            .filter(|entry| {
                entry
                    .headwords
                    .iter()
                    .any(|key| !inflection_forms(key, &inflections).is_empty())
            })
            .count();
        eprintln!(
            "  inflection rows {}, matched headwords {matched}",
            inflections.len()
        );
    }

    eprintln!("Writing {} ...", output.display());
    write_df(&entries, &inflections, output)?;
    let size = fs::metadata(output).map_err(|err| err.to_string())?.len();
    eprintln!("Wrote {} ({size} bytes)", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn article(
        word: &str,
        o: Option<&str>,
        sl: Option<&str>,
        mv: Option<&str>,
        gloss: Option<&str>,
        est: Option<&str>,
        see: Option<&str>,
    ) -> String {
        let o_attr = o
            .map(|value| format!(" x:O=\"{value}\""))
            .unwrap_or_default();
        let sl_el = sl
            .map(|value| format!("<x:sl>{value}</x:sl>"))
            .unwrap_or_default();
        let mv_el = mv
            .map(|value| format!("<x:grg><x:mv>{value}</x:mv></x:grg>"))
            .unwrap_or_default();
        let mut parts = format!(
            "<x:A><x:P><x:mg><x:m{o_attr}>{word}</x:m>{sl_el}{mv_el}</x:mg></x:P><x:S>"
        );
        if let Some(see) = see {
            parts.push_str(&format!("<x:tp><x:tvt>{see}</x:tvt></x:tp>"));
        }
        if gloss.is_some() || est.is_some() {
            parts.push_str("<x:tp><x:tg>");
            if let Some(est) = est {
                parts.push_str(&format!("<x:dg><x:d>{est}</x:d></x:dg>"));
            }
            if let Some(gloss) = gloss {
                parts.push_str(&format!("<x:xp><x:xg><x:x>{gloss}</x:x></x:xg></x:xp>"));
            }
            parts.push_str("</x:tg></x:tp>");
        }
        parts.push_str("</x:S></x:A>");
        parts
    }

    fn parse(word: &str, o: Option<&str>, sl: Option<&str>, gloss: Option<&str>) -> Entry {
        parse_article(&article(word, o, sl, None, gloss, None, None))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn uses_printed_text_not_sort_key() {
        let item = parse("24/7", Some("A"), None, Some("круглосуточно"));
        assert_eq!(item.headwords, ["24/7"]);
    }

    #[test]
    fn homograph_drops_numeric_sort_suffix() {
        let item = parse("aga", Some("aga1"), Some("konj"), Some("но"));
        assert_eq!(item.headwords, ["aga"]);
    }

    #[test]
    fn hyphenated_word_keeps_hyphens_and_compact_variant() {
        let item = parse("aeg-ajalt", Some("aegajalt"), Some("adv"), Some("временами"));
        assert_eq!(item.headwords, ["aeg-ajalt", "aegajalt"]);
    }

    #[test]
    fn article_with_braces_parses() {
        let item = parse("foo{bar}", None, None, Some("gloss"));
        assert_eq!(item.headwords, ["foo{bar}"]);
    }

    #[test]
    fn short_lemma_gets_no_inflections() {
        let entry = Entry {
            headwords: vec!["a".into()],
            pos: vec!["s".into()],
            senses: vec![Sense {
                glosses: vec!["буква".into()],
                examples: Vec::new(),
            }],
            ..Entry::default()
        };
        let inflections = HashMap::from([(
            "a".into(),
            vec!["aga".into(), "as".into(), "al".into()],
        )]);
        assert!(collect_variants(&entry, &inflections).is_empty());
    }

    #[test]
    fn longer_lemma_keeps_inflections() {
        let entry = Entry {
            headwords: vec!["aabe".into()],
            pos: vec!["s".into()],
            senses: vec![Sense {
                glosses: vec!["буква".into()],
                examples: Vec::new(),
            }],
            ..Entry::default()
        };
        let inflections = HashMap::from([("aabe".into(), vec!["aape".into(), "aabet".into()])]);
        assert_eq!(collect_variants(&entry, &inflections), ["aape", "aabet"]);
    }

    #[test]
    fn inflection_lookup_is_casefold_and_unhyphenated() {
        let entry = Entry {
            headwords: vec!["Aeg-ajalt".into()],
            pos: vec!["adv".into()],
            senses: vec![Sense {
                glosses: vec!["временами".into()],
                examples: Vec::new(),
            }],
            ..Entry::default()
        };
        let inflections = HashMap::from([("aegajalt".into(), vec!["aegajaldagi".into()])]);
        assert_eq!(collect_variants(&entry, &inflections), ["aegajaldagi"]);
    }

    #[test]
    fn cleans_noun_stems() {
        assert_eq!(
            clean_paradigm("aabe 'aape aabe[t -, aabe[te 'aape[id", ""),
            "aabe aape aabet -, aabete aapeid"
        );
    }

    #[test]
    fn expands_compound_prefix() {
        assert_eq!(
            clean_paradigm("+k'ahvel k'ahvli k'ahvli[t", "aadama"),
            "aadamakahvel aadamakahvli aadamakahvlit"
        );
    }

    #[test]
    fn paradigm_shown_in_definition() {
        let item = parse_article(&article(
            "aabe",
            None,
            Some("s"),
            Some("aabe 'aape aabe[t -, aabe[te 'aape[id"),
            Some("буква"),
            None,
            None,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(item.paradigm, "aabe aape aabet -, aabete aapeid");
        assert!(render_definition(&item, None).contains("aabe aape aabet"));
    }

    #[test]
    fn estonian_definition_when_no_russian() {
        let item = parse_article(&article(
            "abaasi",
            None,
            Some("adjg"),
            None,
            None,
            Some("abasiini"),
            None,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(item.senses[0].glosses, ["abasiini"]);
    }

    #[test]
    fn russian_gloss_preferred_over_estonian() {
        let item = parse_article(&article(
            "aabe",
            None,
            Some("s"),
            None,
            Some("буква"),
            Some("kirjatäht"),
            None,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(item.senses[0].glosses, ["буква"]);
    }

    #[test]
    fn see_also_copies_target_senses() {
        let source = parse_article(&article(
            "aabitsa+teadmised",
            Some("aabitsateadmised"),
            Some("s"),
            None,
            None,
            None,
            Some("aabitsa+tarkus"),
        ))
        .unwrap()
        .unwrap();
        let target = parse("aabitsa+tarkus", None, Some("s"), Some("азы"));
        let mut entries = merge_parsed(vec![source, target]);
        resolve_see_also(&mut entries);
        let by_head: HashMap<_, _> = entries
            .iter()
            .map(|entry| (entry.headwords[0].as_str(), entry))
            .collect();
        assert_eq!(by_head["aabitsateadmised"].senses[0].glosses, ["азы"]);
    }

    #[test]
    fn missing_see_also_target_gets_placeholder() {
        let source = parse_article(&article(
            "abaasi",
            None,
            Some("adjg"),
            None,
            None,
            None,
            Some("abasiini"),
        ))
        .unwrap()
        .unwrap();
        let mut entries = merge_parsed(vec![source]);
        resolve_see_also(&mut entries);
        assert_eq!(entries[0].senses[0].glosses, ["см. abasiini"]);
    }

    fn tmp_df() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kobo-et-ru-df-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("out.df")
    }

    #[test]
    fn write_omits_short_lemma_variants() {
        let entries = [Entry {
            headwords: vec!["a".into()],
            pos: vec!["s".into()],
            senses: vec![Sense {
                glosses: vec!["буква".into()],
                examples: Vec::new(),
            }],
            ..Entry::default()
        }];
        let dest = tmp_df();
        write_df(
            &entries,
            &HashMap::from([("a".into(), vec!["aga".into(), "as".into()])]),
            &dest,
        )
        .unwrap();
        let text = fs::read_to_string(&dest).unwrap();
        assert!(text.contains("@ a\n"));
        assert!(!text.contains("& aga\n"));
    }

    #[test]
    fn verb_forms_become_searchable_headwords() {
        let entries = [
            Entry {
                headwords: vec!["olema".into()],
                pos: vec!["v".into()],
                senses: vec![Sense {
                    glosses: vec!["быть".into()],
                    examples: Vec::new(),
                }],
                ..Entry::default()
            },
            Entry {
                headwords: vec!["olnu".into()],
                pos: vec!["s".into()],
                senses: vec![Sense {
                    glosses: vec!["прошлое".into()],
                    examples: Vec::new(),
                }],
                ..Entry::default()
            },
            Entry {
                headwords: vec!["raamat".into()],
                pos: vec!["s".into()],
                senses: vec![Sense {
                    glosses: vec!["книга".into()],
                    examples: Vec::new(),
                }],
                ..Entry::default()
            },
        ];
        let inflections = HashMap::from([
            (
                "olema".into(),
                vec![
                    "olnud".into(),
                    "oldud".into(),
                    "olla".into(),
                    "olles".into(),
                    "oli".into(),
                ],
            ),
            ("olnu".into(), vec!["olnud".into(), "olnut".into()]),
            ("raamat".into(), vec!["raamatut".into(), "raamatud".into()]),
        ]);
        let dest = tmp_df();
        write_df(&entries, &inflections, &dest).unwrap();
        let text = fs::read_to_string(&dest).unwrap();
        assert!(text.matches("@ olnud\n").count() >= 2);
        assert!(text[text.find("@ olnud\n").unwrap()..].contains("быть"));
        assert!(text.contains("@ oldud\n"));
        assert!(text.contains("@ olla\n"));
        assert!(text.contains("@ olles\n"));
        assert!(text.contains("@ oli\n"));
        assert!(!text.contains("@ olnut\n"));
        assert!(!text.contains("@ raamatut\n"));
        assert!(!text.contains("@ raamatud\n"));
    }

    #[test]
    fn three_letter_verb_form_aliases_existing_noun() {
        let entries = [
            Entry {
                headwords: vec!["saama".into()],
                pos: vec!["v".into()],
                senses: vec![Sense {
                    glosses: vec!["получать".into()],
                    examples: Vec::new(),
                }],
                ..Entry::default()
            },
            Entry {
                headwords: vec!["sai".into()],
                pos: vec!["s".into()],
                senses: vec![Sense {
                    glosses: vec!["булка".into()],
                    examples: Vec::new(),
                }],
                ..Entry::default()
            },
        ];
        let inflections = HashMap::from([
            ("saama".into(), vec!["sai".into(), "on".into()]),
            ("sai".into(), vec!["saia".into()]),
        ]);
        let dest = tmp_df();
        write_df(&entries, &inflections, &dest).unwrap();
        let text = fs::read_to_string(&dest).unwrap();
        assert!(text.matches("@ sai\n").count() >= 2);
        assert!(text[text.find("@ sai\n").unwrap()..].contains("получать"));
        assert!(!text.contains("@ on\n"));
    }

    #[test]
    fn shared_noun_form_becomes_searchable_headword() {
        let entries = [
            Entry {
                headwords: vec!["kand".into()],
                pos: vec!["s".into()],
                senses: vec![Sense {
                    glosses: vec!["пятка".into()],
                    examples: Vec::new(),
                }],
                ..Entry::default()
            },
            Entry {
                headwords: vec!["kant".into()],
                pos: vec!["s".into()],
                senses: vec![Sense {
                    glosses: vec!["кант".into()],
                    examples: Vec::new(),
                }],
                ..Entry::default()
            },
        ];
        let inflections = HashMap::from([
            ("kand".into(), vec!["kanna".into(), "kanda".into()]),
            ("kant".into(), vec!["kandi".into(), "kanda".into()]),
        ]);
        let dest = tmp_df();
        write_df(&entries, &inflections, &dest).unwrap();
        let text = fs::read_to_string(&dest).unwrap();
        assert!(text.matches("@ kanda\n").count() >= 2);
        assert!(!text.contains("@ kanna\n"));
    }
}
