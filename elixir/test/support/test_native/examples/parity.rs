//! tarnish-nif's parity tools on the tests' conversions, which CI runs to hold the tools to the
//! Node worker's protocol: `cargo run --release --example parity -- record requests.json`.

use std::path::Path;
use std::process::ExitCode;

use tarnish_nif::parity::{self, Parity};

fn main() -> ExitCode {
    parity::main(&Parity {
        conversions: tarnish_test_native::conversions(),
        module: Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../conversions.mjs")),
        made_up: |_, _, _, _| false,
        expected: |_, _, _| None,
    })
}
