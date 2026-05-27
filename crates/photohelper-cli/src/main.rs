//! photohelper command-line entrypoint.
//!
//! Bootstrap stub: real subcommands (`ingest`, `cull`, `develop`, `export`,
//! `run`, `models`, `camera`) land in session 01 per `SESSION-STATE.md`.

fn main() {
    println!(
        "photohelper {} (bootstrap stub — see SESSION-STATE.md for session 01 scope)",
        photohelper_core::version()
    );
}
