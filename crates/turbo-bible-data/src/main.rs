mod audit;
mod build;
mod compress;
mod labels;
mod manifest_source;
mod osis;
mod schema;
mod xrefs;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "turbo-bible-data",
    about = "Offline data pipeline for turbo-bible.",
    long_about = "Audits scrollmapper licenses, builds per-translation SQLite \
                  files from scrollmapper JSON exports, and compresses them \
                  for distribution."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Walk a scrollmapper checkout and emit a CSV of license info per translation.
    AuditLicenses(AuditLicensesArgs),
    /// Build per-translation `SQLite` files into dist/build/.
    Build(BuildArgs),
    /// Compress dist/build/*.db into dist/translations/*.db.zst and emit manifest.json.
    Compress(CompressArgs),
}

#[derive(Args, Debug)]
struct AuditLicensesArgs {
    /// Path to a local `scrollmapper/bible_databases` checkout.
    #[arg(long)]
    scrollmapper: PathBuf,
    /// CSV output path. Defaults to stdout if omitted.
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct BuildArgs {
    /// Path to a local `scrollmapper/bible_databases` checkout.
    #[arg(long)]
    scrollmapper: PathBuf,
    /// Curated translation manifest (`data/manifest_source.toml`).
    #[arg(long)]
    manifest: PathBuf,
    /// Output directory for the built per-translation .db files.
    #[arg(long, default_value = "dist/build")]
    out: PathBuf,
    /// Comma-separated translation codes to build (default: all in the manifest).
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
}

#[derive(Args, Debug)]
struct CompressArgs {
    /// Directory containing built .db files (output of `build`).
    #[arg(long, default_value = "dist/build")]
    r#in: PathBuf,
    /// Output directory for compressed .db.zst files and manifest.json.
    #[arg(long, default_value = "dist/translations")]
    out: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::AuditLicenses(args) => audit::run(&args.scrollmapper, args.out.as_deref()),
        Command::Build(args) => {
            build::run(&args.scrollmapper, &args.manifest, &args.out, &args.only)
        }
        Command::Compress(args) => compress::run(&args.r#in, &args.out),
    }
}
