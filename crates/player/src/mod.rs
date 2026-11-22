//! Native Aurora player entry point.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Aurora game player")]
struct Options {
    /// Capture one rendered frame to a PNG and exit.
    #[arg(long, conflicts_with = "headless")]
    screenshot: Option<PathBuf>,
    /// Load a custom PNG into the demo scene.
    #[arg(long)]
    sprite: Option<PathBuf>,
    /// Directory for imported textures.
    #[arg(long, default_value = ".aurora/import")]
    asset_cache: PathBuf,
    /// Run fixed simulation ticks without opening a window and print scene state as JSON.
    #[arg(long)]
    headless: bool,
    /// Number of simulation ticks in headless mode (defaults to 600).
    #[arg(long, requires = "headless")]
    frames: Option<u64>,
}

fn main() -> anyhow::Result<()> {
    let options = Options::parse();
    let cache = Some(options.asset_cache);
    if options.headless {
        let state = engine_core::app::run_headless(
            options.sprite.as_deref(),
            cache,
            options.frames.unwrap_or(600),
        )?;
        println!("{state}");
        Ok(())
    } else {
        engine_core::app::run(options.sprite.as_deref(), cache, options.screenshot)
    }
}
