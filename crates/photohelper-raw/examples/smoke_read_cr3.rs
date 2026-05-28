//! Manual smoke test: invoke `read_cr3` against a Canon R8 (or other
//! LibRaw-supported) CR3 fixture passed on the command line.
//!
//! Used by the Deliverable 1a body commit to verify the FFI end-to-end
//! against the ANL-001 fixture set. Not a CI test — needs a real CR3
//! the contributor has access to.
//!
//! ```sh
//! cargo run --release --example smoke_read_cr3 -- path/to/photo.cr3
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use photohelper_raw::exif::read_cr3;
use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args_os().nth(1) else {
        eprintln!("usage: smoke_read_cr3 <path/to/photo.cr3>");
        return ExitCode::from(64);
    };
    let p = Path::new(&path);
    match read_cr3(p) {
        Ok(exif) => {
            println!("ok");
            println!("path:                   {}", p.display());
            println!("make:                   {}", exif.make());
            println!("model:                  {}", exif.model());
            println!("orientation:            {:?}", exif.orientation());
            println!(
                "capture_time_unix:      {:?}",
                exif.capture_time_unix_seconds()
            );
            println!("width:                  {}", exif.width());
            println!("height:                 {}", exif.height());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("err: {e}");
            ExitCode::from(1)
        }
    }
}
