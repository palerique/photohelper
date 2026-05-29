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
// here: this main.rs is the binary's top-level file; the transitive
// crate users (tracing / walkdir / rayon / etc.) live inside the
// `commands` module tree. The lint sees the bin compilation unit
// "not directly using" those crates and flags them. Same for dev-deps
// referenced only by tests/cli.rs. Per-target file-level allow is the
// idiomatic mitigation.
#![allow(unused_crate_dependencies)]

use std::process::ExitCode;

use clap::Parser;

mod commands;

use commands::ingest::run_ingest;

/// Cross-platform CLI for AI-powered Canon RAW processing.
#[derive(Parser, Debug)]
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

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Walk a directory + catalog RAW photos.
    Ingest(IngestArgs),
    /// AI culling (planned for v0.1; blocked on NIMA model license — see docs/analysis/ANL-002).
    Cull,
    /// Apply develop settings via XMP sidecars (planned for v0.1).
    Develop,
    /// Export to JPEG with resize + watermark (planned for v0.1).
    Export,
    /// Run ingest → cull → develop → export (planned for v0.1).
    Run,
    /// Manage AI model bundles (planned for v0.1).
    Models,
    /// Inspect / list known camera profiles (planned for v0.1).
    Camera,
}

#[derive(clap::Args, Debug)]
struct IngestArgs {
    /// Directory to walk.
    path: std::path::PathBuf,

    /// Recurse into subdirectories.
    #[arg(short, long, default_value_t = true)]
    recursive: bool,

    /// Treat unknown cameras or any per-photo error as fatal at end-of-run.
    #[arg(long, default_value_t = false)]
    strict: bool,
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
        Command::Ingest(args) => match run_ingest(&cli, args) {
            Ok(code) => ExitCode::from(code),
            Err(err) => {
                tracing::error!("{err:#}");
                ExitCode::from(exit_code_for_error(&err))
            }
        },
        Command::Cull => stub("cull"),
        Command::Develop => stub("develop"),
        Command::Export => stub("export"),
        Command::Run => stub("run"),
        Command::Models => stub("models"),
        Command::Camera => stub("camera"),
    }
}

fn stub(name: &str) -> ExitCode {
    eprintln!(
        "photohelper {name}: not yet implemented in v0.1 (ingest only); \
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
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

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
