//! Manual smoke test: invoke `read_raw` against a Canon R8 (or other
//! LibRaw-supported) CR3 fixture passed on the command line. Prints
//! the decoded BayerPlane dimensions + first few pixels + every
//! companion type (CfaPattern, SensorLevels, WhiteBalance,
//! CamRgbToXyzD65Matrix).
//!
//! ```sh
//! cargo run --release --example smoke_read_raw -- path/to/photo.cr3
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use photohelper_raw::decode::read_raw;
use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args_os().nth(1) else {
        eprintln!("usage: smoke_read_raw <path/to/photo.cr3>");
        return ExitCode::from(64);
    };
    let p = Path::new(&path);
    match read_raw(p) {
        Ok(img) => {
            let pixels = img.pixels();
            println!("ok");
            println!("path:                   {}", p.display());
            println!("pixels.width:           {}", pixels.width());
            println!("pixels.height:          {}", pixels.height());
            println!("cfa_pattern:            {:?}", img.cfa_pattern());
            let levels = img.levels();
            println!(
                "sensor levels:          black={} white={} bit_depth={}",
                levels.black(),
                levels.white(),
                levels.bit_depth().get()
            );
            let wb = img.as_shot_white_balance();
            println!(
                "as_shot WB:             R={:.4} G1={:.4} B={:.4} G2={:.4}",
                wb.r(),
                wb.g1(),
                wb.b(),
                wb.g2()
            );
            let cm = img.color_matrix();
            let m = cm.as_array();
            println!(
                "color matrix row 0:     [{:.4}, {:.4}, {:.4}]",
                m[0][0], m[0][1], m[0][2]
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("err: {e}");
            ExitCode::from(1)
        }
    }
}
