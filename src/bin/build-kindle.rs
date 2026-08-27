use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use kobo_et_ru::kindle;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[derive(Parser)]
#[command(about = "Convert a Kobo dicthtml zip into Kindle dictionary source (OPF + XHTML).")]
struct Args {
    #[arg(long, default_value_os_t = repo_root().join("out/dicthtml-et-ru.zip"))]
    kobo: PathBuf,
    #[arg(long, default_value_os_t = repo_root().join("dist/kindle"))]
    outdir: PathBuf,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match kindle::convert(&args.kobo, &args.outdir) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
