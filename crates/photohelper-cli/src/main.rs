//! photohelper command-line entrypoint.
//!
//! See `docs/plans/session-01.md` §Deliverables 1.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
// Workspace `unused_crate_dependencies` lint produces false positives
// here: `dev-dependencies` (like `assert_cmd`) are provided during `cargo test --bin photohelper`
// but are strictly used in the separate `tests/cli.rs` integration crate. Per-target file-level
// allow is the idiomatic mitigation.
#![allow(unused_crate_dependencies)]

use std::process::ExitCode;

use clap::Parser;

mod commands;
mod heartbeat;

use photohelper_ai::{CLIP_MODEL_MANIFEST_NAME, MODEL_MANIFEST_NAME, VerifiedModelBytes};

use commands::cull::{CullArgs, run_cull};
use commands::dedup::{DedupeArgs, run_dedup};
use commands::develop::{DevelopArgs, run_develop};
use commands::export::{ExportArgs, run_export};
use commands::ingest::run_ingest;
use commands::rename::{RenameArgs, run_rename};
use commands::run::{RunArgs, run_pipeline};
use commands::watermark::{WatermarkArgs, run_watermark};

/// Cross-platform CLI for AI-powered Canon RAW processing.
#[derive(Parser, Debug, Clone)]
#[command(name = "photohelper", version, about)]
struct Cli {
    /// Increase verbosity (repeat: -v INFO, -vv DEBUG, -vvv TRACE).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Quiet mode: mute tracing below ERROR (summary still prints).
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    /// Worker thread count (default: number of CPUs).
    #[arg(long, global = true, value_parser = clap::value_parser!(u32).range(1..=1024))]
    threads: Option<u32>,

    /// Catalog DB path (default: <input>/.photohelper/catalog.db).
    #[arg(long, global = true)]
    catalog: Option<std::path::PathBuf>,

    /// File-lock retry budget in seconds for catalog open.
    #[arg(long, global = true, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=3600))]
    catalog_lock_timeout_seconds: u32,

    /// Disable colored output.
    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Command {
    /// Walk a directory + catalog RAW photos.
    Ingest(IngestArgs),
    /// AI aesthetic culling via the NIMA model (scores photos in `[1, 10]`).
    Cull(CullArgs),
    /// Duplicate detection via CLIP ViT-B/32 embeddings + cosine-similarity clustering.
    Dedup(DedupeArgs),
    /// Apply develop settings via XMP sidecars (Lightroom-compatible).
    Develop(DevelopArgs),
    /// Export to JPEG with resize + watermark.
    Export(ExportArgs),
    /// Run ingest → cull → dedup → develop → export.
    Run(RunArgs),
    /// Apply shadow gradient + dual corner marks to a directory of images → JPEG.
    Watermark(WatermarkArgs),
    /// Copy RAW+XMP into --output under catalog-driven Cluster-X_Cull-Y-Name.ext filenames.
    Rename(RenameArgs),
    /// Manage AI model bundles (planned for v0.1).
    Models,
    /// Inspect / list known camera profiles (planned for v0.1).
    Camera,
}

/// Arguments for the `ingest` subcommand.
#[derive(clap::Args, Debug, Clone)]
pub struct IngestArgs {
    /// Directory to walk.
    pub(crate) path: std::path::PathBuf,

    /// Recurse into subdirectories.
    #[arg(short, long, default_value_t = true)]
    pub(crate) recursive: bool,

    /// Treat unknown cameras or any per-photo error as fatal at end-of-run.
    #[arg(long, default_value_t = false)]
    pub(crate) strict: bool,
}

/// `sysexits.h` codes used by this binary.
mod exit_code {
    /// Stub subcommands.
    pub const EX_UNAVAILABLE: u8 = 69;
    /// Likely-wrong-directory (walked > 0 but nothing RAW).
    pub const EX_USAGE: u8 = 64;
    /// Fatal IO / catalog / config errors.
    pub const EX_IOERR: u8 = 74;
    /// Catalog lock held by another process (retry later).
    pub const EX_TEMPFAIL: u8 = 75;
    /// Permission denied (read-only filesystem or missing write access).
    pub const EX_NOPERM: u8 = 77;
    /// `--strict` escalation (POSIX generic failure).
    pub const EX_STRICT_FAIL: u8 = 1;
    /// Partial failure in bulk operations.
    pub const EX_PARTIAL_FAIL: u8 = 2;
}

/// Map a fatal `anyhow::Error` (which wraps a `photohelper_core::Error` on the
/// ingest path) to the appropriate POSIX exit code.
fn exit_code_for_error(err: &anyhow::Error) -> u8 {
    use photohelper_core::Error;
    err.downcast_ref::<Error>()
        .map_or(exit_code::EX_IOERR, |e| match e {
            Error::CatalogLockHeld { .. } => exit_code::EX_TEMPFAIL,
            Error::Io { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied => {
                exit_code::EX_NOPERM
            }
            _ => exit_code::EX_IOERR,
        })
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose, cli.quiet, cli.no_color);

    match &cli.command {
        Command::Ingest(args) => handle_pipeline_result(run_ingest(&cli, args)),
        Command::Cull(args) => {
            // model_dir: PHOTOHELPER_MODEL_DIR env var if set, else binary-adjacent models/.
            // current_exe() failure silently falls back to relative "models/"; EX_IOERR
            // is returned later if from_manifest then fails at that path.
            let model_dir = resolve_model_dir();
            match VerifiedModelBytes::from_manifest(&model_dir, MODEL_MANIFEST_NAME) {
                Ok(model) => {
                    let model_path = model_dir.join(format!("{MODEL_MANIFEST_NAME}.onnx"));
                    handle_pipeline_result(run_cull(&cli, args, &model, model_path))
                }
                Err(e) => {
                    tracing::error!("{e:#}");
                    ExitCode::from(exit_code::EX_IOERR)
                }
            }
        }
        Command::Dedup(args) => {
            let model_dir = resolve_model_dir();
            match VerifiedModelBytes::from_manifest(&model_dir, CLIP_MODEL_MANIFEST_NAME) {
                Ok(model) => handle_pipeline_result(run_dedup(&cli, args, &model)),
                Err(e) => {
                    tracing::error!("{e:#}");
                    ExitCode::from(exit_code::EX_UNAVAILABLE)
                }
            }
        }
        Command::Develop(args) => handle_pipeline_result(run_develop(&cli, args)),
        Command::Export(args) => handle_pipeline_result(run_export(&cli, args)),
        Command::Watermark(args) => handle_pipeline_result(run_watermark(&cli, args)),
        Command::Rename(args) => handle_pipeline_result(run_rename(&cli, args)),
        Command::Run(args) => {
            let model_dir = resolve_model_dir();

            // Try to load NIMA
            let nima_model =
                match VerifiedModelBytes::from_manifest(&model_dir, MODEL_MANIFEST_NAME) {
                    Ok(model) => model,
                    Err(e) => {
                        tracing::error!("{e:#}");
                        return ExitCode::from(exit_code::EX_UNAVAILABLE);
                    }
                };
            let nima_model_path = model_dir.join(format!("{MODEL_MANIFEST_NAME}.onnx"));

            // Try to load CLIP
            let clip_model =
                match VerifiedModelBytes::from_manifest(&model_dir, CLIP_MODEL_MANIFEST_NAME) {
                    Ok(model) => model,
                    Err(e) => {
                        tracing::error!("{e:#}");
                        return ExitCode::from(exit_code::EX_UNAVAILABLE);
                    }
                };

            handle_pipeline_result(run_pipeline(
                &cli,
                args,
                &nima_model,
                nima_model_path,
                &clip_model,
            ))
        }
        Command::Models => stub("models"),
        Command::Camera => stub("camera"),
    }
}

fn resolve_model_dir() -> std::path::PathBuf {
    std::env::var("PHOTOHELPER_MODEL_DIR").map_or_else(
        |_| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("models")))
                .unwrap_or_else(|| std::path::PathBuf::from("models"))
        },
        std::path::PathBuf::from,
    )
}

fn handle_pipeline_result(res: anyhow::Result<u8>) -> ExitCode {
    match res {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            tracing::error!("{err:#}");
            ExitCode::from(exit_code_for_error(&err))
        }
    }
}

fn stub(name: &str) -> ExitCode {
    eprintln!(
        "photohelper {name}: not yet implemented in v0.1; \
         see README.md for the current scope."
    );
    ExitCode::from(exit_code::EX_UNAVAILABLE)
}

fn init_tracing(verbose: u8, quiet: bool, no_color: bool) {
    use tracing_subscriber::EnvFilter;

    let default_level = if quiet {
        "error"
    } else {
        match verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|e| {
        if std::env::var("RUST_LOG").is_ok() {
            eprintln!("Warning: invalid RUST_LOG filter: {e}");
        }
        EnvFilter::new(default_level)
    });

    let builder = tracing_subscriber::fmt()
        .compact()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);

    if no_color {
        builder.with_ansi(false).init();
    } else {
        builder.init();
    }
}
