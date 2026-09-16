//! Standalone headless CLI. No Tauri, no GTK — build with
//! `cargo build -p mustardy-core --bin mustardy-cli`.
//!
//! The packaged desktop app exposes the same commands from its own binary
//! (see `src-tauri/src/main.rs`); this target is for CI, servers, and agents
//! that only need the trim step.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(mustardy_core::cli::run(&args));
}
