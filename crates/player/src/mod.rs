//! Native Aurora player entry point.

mod bundle;

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use engine_core::{
    app::Scenario,
    input::{InputEvent, Key},
    physics3d::Backend,
    scripting::Script,
};

#[derive(Clone, Copy, Default, ValueEnum)]
enum PhysicsBackend {
    #[default]
    Rapier,
    Jolt,
}

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
    /// Run the native 3D cuboid drop test.
    #[arg(long = "3d", conflicts_with_all = ["bird", "scene", "sprite", "script", "script_dev", "flap_at", "touch_at", "pointer_at", "wheel_at"])]
    three_d: bool,
    /// Select the solver for the 3D drop test.
    #[arg(long, value_enum, requires = "three_d")]
    physics_backend: Option<PhysicsBackend>,
    /// Register an authored scene. The first path opens at startup.
    #[arg(long, conflicts_with_all = ["bird", "sprite"])]
    scene: Vec<PathBuf>,
    /// Run a native scene bundle exported by the editor.
    #[arg(long, conflicts_with_all = ["scene", "script", "script_dev", "script_node", "bird", "sprite", "three_d"])]
    bundle: Option<PathBuf>,
    /// Inject a Space key at a zero-based tick in a headless or screenshot run.
    #[arg(long)]
    flap_at: Vec<u64>,
    /// Inject a touch press at a zero-based tick in a headless or screenshot run.
    #[arg(long)]
    touch_at: Vec<u64>,
    /// Inject a pointer press, move and release over three ticks in a headless or screenshot run.
    #[arg(long)]
    pointer_at: Vec<u64>,
    /// Inject a mouse wheel step at the frame centre in a headless or screenshot run.
    #[arg(long)]
    wheel_at: Vec<u64>,
    /// Simulate this many ticks before capturing the frame.
    #[arg(long, requires = "screenshot")]
    capture_after: Option<u64>,
    /// Stream offscreen frames for an editor session.
    #[arg(long, conflicts_with_all = ["headless", "screenshot", "sprite"])]
    preview_dir: Option<PathBuf>,
    /// Bind a component to this authored node index. Repeat in script entry order.
    #[arg(long, requires = "scene", default_value = "0")]
    script_node: Vec<usize>,
    /// Persist an authored game's best score to this file.
    #[arg(long)]
    profile: Option<PathBuf>,
    /// Project config file exposed to scripts as readable and writable values.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Host-only secrets file exposed to scripts as read-only values.
    #[arg(long)]
    secrets: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let mut options = Options::parse();
    // A desktop app bundle opens its packaged game when it receives no explicit mode.
    if options.bundle.is_none()
        && options.scene.is_empty()
        && !options.bird
        && !options.three_d
        && options.sprite.is_none()
        && options.script.is_none()
        && options.script_dev.is_none()
        && options.preview_dir.is_none()
    {
        options.bundle = bundle::adjacent();
    }
    let mut scene_programs = Vec::new();
    if let Some(path) = &options.bundle {
        (options.scene, scene_programs) = bundle::load(path)?;
    }
    anyhow::ensure!(
        options.profile.is_none() || !options.scene.is_empty(),
        "--profile requires a scene or bundle"
    );
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
        .chain(options.pointer_at.into_iter().flat_map(|tick| {
            let pointer = 1;
            let viewport = [800, 480];
            [
                (
                    tick,
                    InputEvent::PointerStart {
                        position: [0.5, 0.5],
                        viewport,
                        id: pointer,
                    },
                ),
                (
                    tick + 1,
                    InputEvent::PointerMove {
                        position: [0.55, 0.5],
                        viewport,
                        id: pointer,
                    },
                ),
                (
                    tick + 2,
                    InputEvent::PointerEnd {
                        position: [0.55, 0.5],
                        viewport,
                        id: pointer,
                    },
                ),
            ]
        }))
        .chain(options.wheel_at.into_iter().map(|tick| {
            (
                tick,
                InputEvent::Wheel {
                    position: [0.5, 0.5],
                    delta: [0.0, 1.0],
                    viewport: [800, 480],
                },
            )
        }))
        .collect();
    anyhow::ensure!(
        input.is_empty() || options.headless || options.screenshot.is_some(),
        "Scheduled input requires --headless or --screenshot"
    );
    let scenario = Scenario {
        bird: options.bird,
        spatial: options
            .three_d
            .then(|| match options.physics_backend.unwrap_or_default() {
                PhysicsBackend::Rapier => Backend::Rapier,
                PhysicsBackend::Jolt => Backend::Jolt,
            }),
        scenes: &options.scene,
        script_nodes: &options.script_node,
        scene_programs: &scene_programs,
        profile: options.profile.as_deref(),
        config: options.config.as_deref(),
        secrets: options.secrets.as_deref(),
        input: &input,
    };
    if let Some(dir) = options.preview_dir {
        return engine_core::app::preview::run_preview(&dir, cache, script.as_ref(), scenario);
    }
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
