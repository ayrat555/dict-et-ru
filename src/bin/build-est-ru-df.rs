use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use kobo_et_ru::est_ru_df;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[derive(Parser)]
#[command(about = "Convert EKI Estonian–Russian Dictionary XML into a dictutil dictfile.")]
struct Args {
    #[arg(long, default_value_os_t = repo_root().join("cache/evs_EKI_CCBY40.xml"))]
    evs: PathBuf,
    #[arg(long, default_value_os_t = repo_root().join("data/est_inflected_forms.tsv"))]
    inflections: PathBuf,
    #[arg(long, default_value_os_t = repo_root().join("dist/est-ru.df"))]
    output: PathBuf,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match est_ru_df::convert(&args.evs, &args.inflections, &args.output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
