//! Writes `include/tarnish.h`: `cargo run -p tarnish-c --example header --features headers`.

fn main() -> std::io::Result<()> {
    tarnish_c::write_header(concat!(env!("CARGO_MANIFEST_DIR"), "/include/tarnish.h"))
}
