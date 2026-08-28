use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use kobo_et_ru::fetch_inflections::{self, FetchArgs};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[derive(Parser)]
#[command(about = "Rebuild an inflection TSV from the Ekilex API.")]
struct Args {
    #[arg(long, default_value_os_t = repo_root().join("data/est_inflected_forms.tsv"))]
    lemmas: PathBuf,
    #[arg(long)]
    words: Option<PathBuf>,
    #[arg(long, default_value_os_t = repo_root().join("dist/est_inflected_forms.ekilex.tsv"))]
    output: PathBuf,
    #[arg(long, default_value_os_t = repo_root().join("cache/ekilex_checkpoint.jsonl"))]
    checkpoint: PathBuf,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long, default_value_t = fetch_inflections::DEFAULT_BASE.to_string())]
    base: String,
    #[arg(long, default_value_t = fetch_inflections::DEFAULT_DELAY_SECS)]
    delay: f64,
    #[arg(long, default_value_t = 0)]
    limit: usize,
    #[arg(long)]
    export_only: bool,
    #[arg(long)]
    force: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match fetch_inflections::convert(FetchArgs {
        lemmas: args.lemmas,
        words: args.words,
        output: args.output,
        checkpoint: args.checkpoint,
        api_key: args.api_key,
        base: args.base,
        delay: args.delay,
        limit: args.limit,
        export_only: args.export_only,
        force: args.force,
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
