//! Native Aurora player entry point.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Aurora game player")]
struct Options {
    /// Capture one rendered frame to a PNG and exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    engine_core::app::run(Options::parse().screenshot)
}
