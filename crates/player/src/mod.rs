//! Native Aurora player entry point.

use std::path::PathBuf;

use clap::Parser;
use engine_core::{
    app::Scenario,
    input::{InputEvent, Key},
    scripting::Script,
};

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
    /// Run a scriptc game executable.
    #[arg(long, conflicts_with = "script_dev")]
    script: Option<PathBuf>,
    /// Run a generated development script launcher.
    #[arg(long)]
    script_dev: Option<PathBuf>,
    /// Play the M3 bird and pipes sample.
    #[arg(long, conflicts_with = "sprite")]
    bird: bool,
    /// Inject a Space key at a zero-based tick in a headless or screenshot run.
    #[arg(long, requires = "bird")]
    flap_at: Vec<u64>,
    /// Inject a touch press at a zero-based tick in a headless or screenshot run.
    #[arg(long, requires = "bird")]
    touch_at: Vec<u64>,
    /// Simulate this many ticks before capturing the frame.
    #[arg(long, requires = "screenshot")]
    capture_after: Option<u64>,
}

fn main() -> anyhow::Result<()> {
    let options = Options::parse();
    let cache = Some(options.asset_cache);
    let script = options
        .script
        .map(Script::Native)
        .or_else(|| options.script_dev.map(Script::Development));
    let input: Vec<_> = options
        .flap_at
        .into_iter()
        .map(|tick| (tick, InputEvent::KeyDown(Key::Space)))
        .chain(
            options
                .touch_at
                .into_iter()
                .map(|tick| (tick, InputEvent::TouchStart)),
        )
        .collect();
    anyhow::ensure!(
        input.is_empty() || options.headless || options.screenshot.is_some(),
        "Scheduled input requires --headless or --screenshot"
    );
    let scenario = Scenario {
        bird: options.bird,
        input: &input,
    };
    if options.headless {
        let state = engine_core::app::run_headless(
            options.sprite.as_deref(),
            cache,
            options.frames.unwrap_or(600),
            script.as_ref(),
            scenario,
        )?;
        println!("{state}");
        Ok(())
    } else {
        engine_core::app::run(
            options.sprite.as_deref(),
            cache,
            options.screenshot,
            script.as_ref(),
            scenario,
            options.capture_after.unwrap_or(0),
        )
    }
}
