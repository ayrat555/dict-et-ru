use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_BASE: &str = "https://ekilex.ee/api";
pub const DEFAULT_DELAY_SECS: f64 = 0.1;
const MAX_BACKOFF_SECS: f64 = 10.0;
const MAX_CONSECUTIVE_ERRORS: u32 = 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointItem {
    pub word: String,
    pub ekilex_found: bool,
    pub word_id: Option<i64>,
    pub inflected_forms: Vec<String>,
    pub timestamp: String,
}

#[derive(Debug)]
pub enum FetchError {
    Message(String),
    Status(u16, String),
    RateLimited,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Message(msg) => write!(f, "{msg}"),
            FetchError::Status(code, msg) => write!(f, "HTTP {code}: {msg}"),
            FetchError::RateLimited => write!(f, "HTTP 429"),
        }
    }
}

impl std::error::Error for FetchError {}

pub trait Transport {
    fn get_json(&self, url: &str) -> Result<Value, FetchError>;
}

pub struct UreqTransport {
    pub api_key: String,
}

impl Transport for UreqTransport {
    fn get_json(&self, url: &str) -> Result<Value, FetchError> {
        let response = ureq::get(url)
            .set("ekilex-api-key", &self.api_key)
            .set("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .call();
        match response {
            Ok(resp) => resp
                .into_json()
                .map_err(|err| FetchError::Message(err.to_string())),
            Err(ureq::Error::Status(429, _)) => Err(FetchError::RateLimited),
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(FetchError::Status(code, body))
            }
            Err(err) => Err(FetchError::Message(err.to_string())),
        }
    }
}

pub struct EkilexClient<T: Transport> {
    transport: T,
    base: String,
    delay: Duration,
    last_request: Instant,
}

impl<T: Transport> EkilexClient<T> {
    pub fn new(transport: T, base: &str, delay_secs: f64) -> Self {
        Self {
            transport,
            base: base.trim_end_matches('/').to_string(),
            delay: Duration::from_secs_f64(delay_secs.max(0.0)),
            last_request: Instant::now()
                .checked_sub(Duration::from_secs(60))
                .unwrap_or_else(Instant::now),
        }
    }

    fn throttle(&mut self) {
        let wait = self
            .delay
            .saturating_sub(self.last_request.elapsed());
        if !wait.is_zero() {
            thread::sleep(wait);
        }
    }

    fn get(&mut self, path: &str) -> Result<Value, FetchError> {
        let url = format!("{}{path}", self.base);
        let mut backoff = 1.0_f64;
        loop {
            self.throttle();
            self.last_request = Instant::now();
            match self.transport.get_json(&url) {
                Err(FetchError::RateLimited) => {
                    let wait = backoff.min(MAX_BACKOFF_SECS);
                    log_line(format!("    rate limited on {path}, waiting {wait:.1}s"));
                    if !self.delay.is_zero() {
                        thread::sleep(Duration::from_secs_f64(wait));
                    }
                    backoff = (backoff * 2.0).min(MAX_BACKOFF_SECS);
                }
                other => return other,
            }
        }
    }

    pub fn fetch_forms(&mut self, word: &str) -> Result<CheckpointItem, FetchError> {
        let timestamp = utc_now();
        let mut ids = self.search_word_ids(word)?;
        let mut forms = Vec::new();
        let mut chosen_id = None;
        for word_id in &ids {
            forms = self.forms_for_word_id(*word_id)?;
            chosen_id = Some(*word_id);
            if forms.len() > 1 {
                break;
            }
        }
        if forms.len() <= 1 {
            if let Some(cap) = capitalized(word) {
                log_line(format!("    also searching {cap}"));
                for word_id in self.search_word_ids(&cap)? {
                    if ids.contains(&word_id) {
                        continue;
                    }
                    ids.push(word_id);
                    let candidate = self.forms_for_word_id(word_id)?;
                    if candidate.len() > forms.len() {
                        forms = candidate;
                        chosen_id = Some(word_id);
                    }
                    if forms.len() > 1 {
                        break;
                    }
                }
            }
        }
        let Some(word_id) = chosen_id else {
            return Ok(CheckpointItem {
                word: word.to_string(),
                ekilex_found: false,
                word_id: None,
                inflected_forms: Vec::new(),
                timestamp,
            });
        };
        Ok(CheckpointItem {
            word: word.to_string(),
            ekilex_found: true,
            word_id: Some(word_id),
            inflected_forms: forms,
            timestamp,
        })
    }

    fn search_word_ids(&mut self, word: &str) -> Result<Vec<i64>, FetchError> {
        log_line(format!("    searching {word}"));
        let payload = self.get(&format!("/word/search/{}", url_encode(word)))?;
        Ok(estonian_word_ids(&payload))
    }

    fn forms_for_word_id(&mut self, word_id: i64) -> Result<Vec<String>, FetchError> {
        log_line(format!("    word_id={word_id}, fetching paradigms"));
        let mut forms = Vec::new();
        if let Ok(paradigms) = self.get(&format!("/paradigm/details/{word_id}")) {
            forms = forms_from_details(&paradigms);
        }
        if forms.is_empty() {
            log_line("    no paradigm forms, trying word details");
            if let Ok(details) = self.get(&format!("/word/details/{word_id}")) {
                forms = forms_from_details(&details);
            }
        }
        Ok(forms)
    }
}

fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn utc_now() -> String {
    // RFC3339-ish UTC without extra deps; good enough for a checkpoint stamp.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn log_line(message: impl std::fmt::Display) {
    eprintln!("{message}");
    let _ = io::stderr().flush();
}

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{:.1}s", duration.as_secs_f64())
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn eta_after(started: Instant, done: usize, total: usize) -> String {
    if done == 0 || done >= total {
        return format_duration(started.elapsed());
    }
    let elapsed = started.elapsed();
    let remaining = elapsed.mul_f64((total - done) as f64 / done as f64);
    format!(
        "{} elapsed, ~{} left",
        format_duration(elapsed),
        format_duration(remaining)
    )
}

fn capitalized(word: &str) -> Option<String> {
    let mut chars = word.chars();
    let first = chars.next()?;
    let upper: String = first.to_uppercase().collect();
    if upper.starts_with(first) && upper.len() == first.len_utf8() {
        return None;
    }
    Some(format!("{upper}{}", chars.as_str()))
}

pub fn pick_estonian_hit(payload: &Value) -> Option<&Value> {
    estonian_hits(payload).into_iter().next()
}

fn estonian_hits(payload: &Value) -> Vec<&Value> {
    let items = match payload {
        Value::Array(items) => items.as_slice(),
        Value::Object(map) => map
            .get("words")
            .or_else(|| map.get("wordSearchResults"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        _ => return Vec::new(),
    };
    let est: Vec<&Value> = items
        .iter()
        .filter(|item| item.get("lang").and_then(Value::as_str) == Some("est"))
        .collect();
    if !est.is_empty() {
        return est;
    }
    items.iter().filter(|item| item.is_object()).collect()
}

fn estonian_word_ids(payload: &Value) -> Vec<i64> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for hit in estonian_hits(payload) {
        if let Some(id) = word_id_of(hit) {
            if seen.insert(id) {
                ids.push(id);
            }
        }
    }
    ids
}

pub fn word_id_of(hit: &Value) -> Option<i64> {
    for key in ["wordId", "word_id", "id"] {
        match hit.get(key) {
            Some(Value::Number(n)) => return n.as_i64(),
            Some(Value::String(s)) if s.chars().all(|ch| ch.is_ascii_digit()) => {
                return s.parse().ok();
            }
            _ => {}
        }
    }
    None
}

pub fn forms_from_details(payload: &Value) -> Vec<String> {
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    collect_forms(payload, &mut unique, &mut seen);
    unique
}

fn collect_forms(payload: &Value, unique: &mut Vec<String>, seen: &mut HashSet<String>) {
    match payload {
        Value::Array(items) => {
            for item in items {
                collect_forms(item, unique, seen);
            }
        }
        Value::Object(map) => {
            for key in ["forms", "paradigmForms"] {
                if let Some(forms) = map.get(key).and_then(Value::as_array) {
                    for form in forms {
                        push_form_value(form, unique, seen);
                    }
                }
            }
            if let Some(paradigms) = map.get("paradigms") {
                collect_forms(paradigms, unique, seen);
            }
            if let Some(word) = map.get("word") {
                collect_forms(word, unique, seen);
            }
        }
        _ => {}
    }
}

fn push_form_value(form: &Value, unique: &mut Vec<String>, seen: &mut HashSet<String>) {
    for key in ["value", "valuePrese"] {
        let Some(value) = form.get(key).and_then(Value::as_str) else {
            continue;
        };
        let Some(value) = clean_form_value(value) else {
            continue;
        };
        if !seen.insert(value.clone()) {
            continue;
        }
        unique.push(value);
        return;
    }
}

/// Plain lookup form. Drops Ekilex display markup such as
/// `aadelda<eki-form>nud</eki-form>` → `aadeldanud`, and skips hyphenated
/// stem+ending displays (`a-<eki-form>d</eki-form>`).
pub fn clean_form_value(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "-" {
        return None;
    }
    if raw.contains("-<") {
        return None;
    }
    let stripped = strip_tags(raw);
    if stripped.is_empty() || stripped == "-" || stripped.contains('<') {
        return None;
    }
    Some(stripped)
}

fn strip_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        match rest[start..].find('>') {
            Some(rel) => rest = &rest[start + rel + 1..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn needs_refetch(item: Option<&CheckpointItem>) -> bool {
    match item {
        None => true,
        Some(item) => item.ekilex_found && item.inflected_forms.is_empty(),
    }
}

pub fn lemmas_from_tsv(path: &Path) -> Result<Vec<String>, String> {
    if !path.is_file() {
        return Err(format!("Lemma TSV not found: {}", path.display()));
    }
    let file = File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let mut words = Vec::new();
    let mut seen = HashSet::new();
    if let Some(header) = lines.next() {
        let header = header.map_err(|err| err.to_string())?;
        if !header.contains('\t') {
            ingest_lemma_line(&header, &mut words, &mut seen);
        }
    }
    for line in lines {
        let line = line.map_err(|err| err.to_string())?;
        ingest_lemma_line(&line, &mut words, &mut seen);
    }
    Ok(words)
}

fn ingest_lemma_line(line: &str, words: &mut Vec<String>, seen: &mut HashSet<String>) {
    if !line.contains('\t') {
        return;
    }
    let word = line.split('\t').next().unwrap_or("").trim();
    if !word.is_empty() && seen.insert(word.to_string()) {
        words.push(word.to_string());
    }
}

pub fn words_from_lines(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut words = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let word = line.trim();
        if word.is_empty() || word.starts_with('#') || !seen.insert(word.to_string()) {
            continue;
        }
        words.push(word.to_string());
    }
    Ok(words)
}

pub fn load_checkpoint(path: &Path) -> Result<HashMap<String, CheckpointItem>, String> {
    let mut entries = HashMap::new();
    if !path.is_file() {
        return Ok(entries);
    }
    let file = File::open(path).map_err(|err| err.to_string())?;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|err| err.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let item: CheckpointItem =
            serde_json::from_str(line).map_err(|err| format!("checkpoint: {err}"))?;
        if !item.word.is_empty() {
            entries.insert(item.word.clone(), item);
        }
    }
    Ok(entries)
}

pub fn append_checkpoint(path: &Path, item: &CheckpointItem) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(item).map_err(|err| err.to_string())?
    )
    .map_err(|err| err.to_string())
}

pub fn mapping_from_checkpoint(entries: &HashMap<String, CheckpointItem>) -> HashMap<String, Vec<String>> {
    let mut mapping = HashMap::new();
    for (word, item) in entries {
        let mut forms = Vec::new();
        let mut seen = HashSet::new();
        for form in &item.inflected_forms {
            let Some(form) = clean_form_value(form) else {
                continue;
            };
            if seen.insert(form.clone()) {
                forms.push(form);
            }
        }
        if !forms.is_empty() {
            mapping.insert(word.clone(), forms);
        }
    }
    mapping
}

pub fn write_tsv(mapping: &HashMap<String, Vec<String>>, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut keys: Vec<&String> = mapping.keys().collect();
    keys.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    let mut file = File::create(dest).map_err(|err| err.to_string())?;
    writeln!(file, "word\tforms").map_err(|err| err.to_string())?;
    for word in keys {
        let forms = &mapping[word];
        if forms.is_empty() {
            continue;
        }
        writeln!(file, "{word}\t{}", forms.join(",")).map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub struct FetchArgs {
    pub lemmas: PathBuf,
    pub words: Option<PathBuf>,
    pub output: PathBuf,
    pub checkpoint: PathBuf,
    pub api_key: Option<String>,
    pub base: String,
    pub delay: f64,
    pub limit: usize,
    pub export_only: bool,
    pub force: bool,
}

pub fn collect_words(args: &FetchArgs) -> Result<Vec<String>, String> {
    if let Some(path) = &args.words {
        return words_from_lines(path);
    }
    lemmas_from_tsv(&args.lemmas)
}

pub fn run_fetch<T: Transport>(
    args: &FetchArgs,
    transport: T,
) -> Result<HashMap<String, CheckpointItem>, String> {
    let source = args
        .words
        .as_ref()
        .unwrap_or(&args.lemmas);
    log_line(format!("Loading lemmas from {} ...", source.display()));
    let mut words = collect_words(args)?;
    if args.limit > 0 {
        words.truncate(args.limit);
        log_line(format!("Limited to first {} lemmas", words.len()));
    }
    log_line(format!("Loading checkpoint {} ...", args.checkpoint.display()));
    let mut checkpoint = load_checkpoint(&args.checkpoint)?;
    let pending: Vec<String> = words
        .iter()
        .filter(|word| args.force || needs_refetch(checkpoint.get(*word)))
        .cloned()
        .collect();
    log_line(format!(
        "{} lemmas, {} in checkpoint, {} to fetch",
        words.len(),
        checkpoint.len(),
        pending.len()
    ));
    if args.export_only {
        log_line("--export-only: skipping fetch");
        return Ok(checkpoint);
    }
    if pending.is_empty() {
        log_line("Nothing to fetch");
        return Ok(checkpoint);
    }
    let api_key_missing = args
        .api_key
        .as_deref()
        .map(str::is_empty)
        .unwrap_or(true);
    if api_key_missing {
        return Err(
            "Ekilex API key missing. Set EKILEX_API_KEY or pass --api-key (https://ekilex.ee/userprofile)."
                .into(),
        );
    }

    log_line(format!(
        "Fetching from {} ({:.2}s delay between requests)",
        args.base, args.delay
    ));
    let mut client = EkilexClient::new(transport, &args.base, args.delay);
    let mut errors = 0u32;
    let mut skipped = 0usize;
    let mut found = 0usize;
    let mut missed = 0usize;
    let mut form_count = 0usize;
    let started = Instant::now();
    for (index, word) in pending.iter().enumerate() {
        let done = index + 1;
        log_line(format!("  {done}/{} fetching {word} ...", pending.len()));
        match client.fetch_forms(word) {
            Ok(item) => {
                errors = 0;
                if item.ekilex_found {
                    found += 1;
                    form_count += item.inflected_forms.len();
                    log_line(format!(
                        "  {done}/{} {word}  id={}  {} forms  | {found} found, {missed} miss, {form_count} forms  ({})",
                        pending.len(),
                        item.word_id.map(|id| id.to_string()).unwrap_or_else(|| "-".into()),
                        item.inflected_forms.len(),
                        eta_after(started, done, pending.len())
                    ));
                } else {
                    missed += 1;
                    log_line(format!(
                        "  {done}/{} {word}  not in Ekilex  | {found} found, {missed} miss, {form_count} forms  ({})",
                        pending.len(),
                        eta_after(started, done, pending.len())
                    ));
                }
                append_checkpoint(&args.checkpoint, &item)?;
                checkpoint.insert(word.clone(), item);
            }
            Err(err) => {
                errors += 1;
                skipped += 1;
                log_line(format!("  {done}/{} skip {word}: {err}", pending.len()));
                if errors >= MAX_CONSECUTIVE_ERRORS {
                    return Err(format!(
                        "{MAX_CONSECUTIVE_ERRORS} consecutive errors; re-run later to resume"
                    ));
                }
            }
        }
    }
    log_line(format!(
        "Fetch finished in {}: {found} found, {missed} miss, {skipped} skipped, {form_count} forms",
        format_duration(started.elapsed())
    ));
    Ok(checkpoint)
}

pub fn export_checkpoint(
    checkpoint: &HashMap<String, CheckpointItem>,
    dest: &Path,
) -> Result<usize, String> {
    log_line(format!("Writing {} ...", dest.display()));
    let mapping = mapping_from_checkpoint(checkpoint);
    let count = mapping.len();
    let forms = mapping.values().map(Vec::len).sum::<usize>();
    write_tsv(&mapping, dest)?;
    log_line(format!("Wrote {} ({count} lemmas, {forms} forms)", dest.display()));
    Ok(count)
}

pub fn convert(args: FetchArgs) -> Result<(), String> {
    let api_key = args
        .api_key
        .clone()
        .filter(|key| !key.is_empty())
        .or_else(|| std::env::var("EKILEX_API_KEY").ok().filter(|k| !k.is_empty()));
    let transport = UreqTransport {
        api_key: api_key.clone().unwrap_or_default(),
    };
    let mut fetch_args = args;
    fetch_args.api_key = api_key;
    let checkpoint = run_fetch(&fetch_args, transport)?;
    export_checkpoint(&checkpoint, &fetch_args.output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn search_payload() -> Value {
        serde_json::json!({
            "words": [
                {"wordId": 11, "wordValue": "tee", "lang": "rus"},
                {"wordId": 22, "wordValue": "tee", "lang": "est"}
            ]
        })
    }

    fn details_payload() -> Value {
        serde_json::json!({
            "paradigms": [{
                "forms": [
                    {"value": "olema"},
                    {"value": "olla"},
                    {"value": "-"},
                    {"value": "olla"},
                    {"value": "olnud"}
                ]
            }]
        })
    }

    #[test]
    fn prefers_estonian_search_hit() {
        let payload = search_payload();
        let hit = pick_estonian_hit(&payload).unwrap();
        assert_eq!(word_id_of(hit), Some(22));
    }

    #[test]
    fn collects_unique_forms() {
        assert_eq!(
            forms_from_details(&details_payload()),
            ["olema", "olla", "olnud"]
        );
        let nested = serde_json::json!({
            "word": { "paradigms": [{ "paradigmForms": [{"value": "aabe"}, {"value": "-"}] }] }
        });
        assert_eq!(forms_from_details(&nested), ["aabe"]);
        let array = serde_json::json!([{"forms": [{"value": "olla"}, {"valuePrese": "olnud"}]}]);
        assert_eq!(forms_from_details(&array), ["olla", "olnud"]);
        let marked = serde_json::json!([{
            "forms": [
                {"value": "aadeldanud"},
                {"value": "aadelda<eki-form>nud</eki-form>"},
                {"value": "a-<eki-form>d</eki-form>"},
                {"value": "aadrilaskmise<eki-form>id</eki-form>"}
            ]
        }]);
        assert_eq!(
            forms_from_details(&marked),
            ["aadeldanud", "aadrilaskmiseid"]
        );
    }

    #[test]
    fn strips_display_markup() {
        assert_eq!(
            clean_form_value("aadelda<eki-form>nud</eki-form>").as_deref(),
            Some("aadeldanud")
        );
        assert_eq!(clean_form_value("a-<eki-form>d</eki-form>"), None);
        assert_eq!(clean_form_value("-"), None);
    }

    fn tmp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kobo-et-ru-fetch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn tsv_roundtrip_sorted() {
        let dest = tmp_dir().join("forms.tsv");
        let mapping = HashMap::from([
            ("b".into(), vec!["b".into(), "bb".into()]),
            ("A".into(), vec!["A".into(), "aa".into()]),
        ]);
        write_tsv(&mapping, &dest).unwrap();
        let lemmas = lemmas_from_tsv(&dest).unwrap();
        assert_eq!(lemmas, ["A", "b"]);
    }

    #[test]
    fn checkpoint_append_and_reload() {
        let path = tmp_dir().join("ckpt.jsonl");
        append_checkpoint(
            &path,
            &CheckpointItem {
                word: "olema".into(),
                ekilex_found: true,
                word_id: Some(1),
                inflected_forms: vec!["olla".into()],
                timestamp: "1".into(),
            },
        )
        .unwrap();
        append_checkpoint(
            &path,
            &CheckpointItem {
                word: "olema".into(),
                ekilex_found: true,
                word_id: Some(1),
                inflected_forms: vec!["olla".into(), "olnud".into()],
                timestamp: "2".into(),
            },
        )
        .unwrap();
        let entries = load_checkpoint(&path).unwrap();
        assert_eq!(
            entries["olema"].inflected_forms,
            ["olla", "olnud"]
        );
    }

    struct ScriptedTransport {
        calls: RefCell<Vec<String>>,
        responses: RefCell<Vec<Result<Value, FetchError>>>,
    }

    impl Transport for ScriptedTransport {
        fn get_json(&self, url: &str) -> Result<Value, FetchError> {
            self.calls.borrow_mut().push(url.to_string());
            self.responses.borrow_mut().remove(0)
        }
    }

    #[test]
    fn fetch_forms_uses_estonian_id() {
        let transport = ScriptedTransport {
            calls: RefCell::new(Vec::new()),
            responses: RefCell::new(vec![Ok(search_payload()), Ok(details_payload())]),
        };
        let mut client = EkilexClient::new(transport, "https://ekilex.ee/api", 0.0);
        let item = client.fetch_forms("tee").unwrap();
        assert!(item.ekilex_found);
        assert_eq!(item.word_id, Some(22));
        assert_eq!(item.inflected_forms, ["olema", "olla", "olnud"]);
        let calls = client.transport.calls.borrow().clone();
        assert!(calls.iter().any(|url| url.contains("/paradigm/details/22")));
    }

    #[test]
    fn fetch_forms_tries_next_hit_when_first_has_no_forms() {
        let transport = ScriptedTransport {
            calls: RefCell::new(Vec::new()),
            responses: RefCell::new(vec![
                Ok(serde_json::json!({
                    "words": [
                        {"wordId": 11, "wordValue": "ingeri", "lang": "est"},
                        {"wordId": 22, "wordValue": "Ingeri", "lang": "est"}
                    ]
                })),
                Ok(serde_json::json!([])),
                Ok(serde_json::json!({"word": {}})),
                Ok(details_payload()),
            ]),
        };
        let mut client = EkilexClient::new(transport, "https://ekilex.ee/api", 0.0);
        let item = client.fetch_forms("ingeri").unwrap();
        assert_eq!(item.word_id, Some(22));
        assert_eq!(item.inflected_forms, ["olema", "olla", "olnud"]);
    }

    #[test]
    fn capitalizes_first_letter() {
        assert_eq!(capitalized("ingeri").as_deref(), Some("Ingeri"));
        assert_eq!(capitalized("Ingeri"), None);
        assert_eq!(capitalized("äike").as_deref(), Some("Äike"));
    }

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(format_duration(Duration::from_secs(75)), "1m 15s");
        assert_eq!(format_duration(Duration::from_secs(3723)), "1h 02m");
    }

    #[test]
    fn retries_rate_limit() {
        let transport = ScriptedTransport {
            calls: RefCell::new(Vec::new()),
            responses: RefCell::new(vec![
                Err(FetchError::RateLimited),
                Ok(serde_json::json!({"words": []})),
                Ok(serde_json::json!({"words": []})),
            ]),
        };
        let mut client = EkilexClient::new(transport, "https://ekilex.ee/api", 0.0);
        let item = client.fetch_forms("x").unwrap();
        assert!(!item.ekilex_found);
        assert!(client.transport.calls.borrow().len() >= 2);
    }
}
