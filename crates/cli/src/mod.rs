//! Aurora's local build command.
//!
//! `aurora build` produces a game package with a versioned Docker/Podman
//! builder image. Engine releases and builder images are distinct products:
//! this binary only orchestrates the local container lifecycle.

mod engine;
mod pipeline;
mod target;
mod template;

use std::{env, path::PathBuf, process::ExitCode};

use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use engine::{ContainerEngine, EngineChoice};
use pipeline::BuildRequest;
use target::{Status, Target};

#[derive(Parser)]
#[command(
    name = "aurora",
    version,
    about = "Build Aurora games inside local Docker or Podman builder images"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build a project for a target inside a versioned builder image.
    Build(BuildArgs),
    /// List build targets, image references, and blockers.
    Targets,
}

#[derive(clap::Args)]
struct BuildArgs {
    /// Project directory or authored scene file.
    #[arg(long)]
    project: PathBuf,
    /// Build target: linux, windows, android, or macos.
    #[arg(long, default_value = "linux")]
    target: String,
    /// Absent directory that receives the finished game package.
    #[arg(long)]
    output: PathBuf,
    /// Include another saved scene from the project. Repeat for switch order.
    #[arg(long = "include-scene", value_name = "PATH")]
    include_scene: Vec<PathBuf>,
    /// Container engine to use. Auto prefers Docker, then Podman.
    #[arg(long, value_enum, default_value = "auto")]
    engine: EngineChoice,
    /// Verified target template archive or URL used for the exported player.
    #[arg(long, value_name = "PATH|URL")]
    template: Option<String>,
    /// Override the builder image reference, for example a digest pin.
    #[arg(long)]
    image: Option<String>,
    /// Keep the pulled builder image after the build.
    #[arg(long)]
    keep_image: bool,
    /// Print the resolved container command without pulling or running it.
    #[arg(long)]
    dry_run: bool,
    /// macOS only: codesign identity, or "-" for an ad-hoc signature.
    #[arg(long)]
    sign: Option<String>,
    /// macOS only: notarytool keychain profile used after Developer ID signing.
    #[arg(long)]
    notarize: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build(args) => build(args),
        Command::Targets => {
            targets();
            Ok(())
        }
    }
}

fn build(args: BuildArgs) -> Result<()> {
    let target = Target::parse(&args.target)?;
    ensure!(
        args.template.is_none() || target.id != "macos",
        "--template applies to container targets; macOS builds use the host player"
    );
    let request = BuildRequest {
        project: args.project,
        target,
        output: args.output,
        includes: args.include_scene,
        template: args.template.clone(),
        image: args.image,
        keep_image: args.keep_image,
        dry_run: args.dry_run,
        sign: args.sign,
        notarize: args.notarize,
    };
    let outcome = if target.id == "macos" {
        pipeline::build_macos(&request)?
    } else {
        let choice = engine_choice(args.engine)?;
        let engine = ContainerEngine::detect(choice, None)?;
        eprintln!("Using {}", engine.version()?);
        pipeline::build(&engine, &request)?
    };
    if outcome.dry_run {
        return Ok(());
    }
    if target.id == "macos" {
        println!("Built target macos on the macOS host");
    } else {
        println!("Built target {} with {}", target.id, outcome.image);
    }
    if let Some(id) = &outcome.image_id {
        println!("Builder image: {id}");
    }
    if let Some(bundle) = &outcome.source_bundle_sha256 {
        println!("Package digest: {bundle}");
    }
    if let Some(template) = &args.template {
        println!("Template: {template}");
    }
    println!("Output: {}", outcome.output.display());
    if args.keep_image {
        println!("Kept builder image {}", outcome.image);
    }
    Ok(())
}

fn engine_choice(choice: EngineChoice) -> Result<EngineChoice> {
    if choice != EngineChoice::Auto {
        return Ok(choice);
    }
    let Ok(value) = env::var("AURORA_CONTAINER_ENGINE") else {
        return Ok(choice);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok(EngineChoice::Auto),
        "docker" => Ok(EngineChoice::Docker),
        "podman" => Ok(EngineChoice::Podman),
        other => Err(anyhow::anyhow!(
            "AURORA_CONTAINER_ENGINE must be docker, podman, or auto, not {other:?}"
        )),
    }
}

fn targets() {
    println!(
        "{:<10} {:<26} {:<24} STATUS",
        "TARGET", "TRIPLE", "IMAGE TAG"
    );
    for target in Target::ALL {
        let image = target.image(env!("CARGO_PKG_VERSION"));
        let tag = image.rsplit(':').next().unwrap_or(&image);
        let (status, image) = match target.status {
            Status::Supported => ("supported", tag),
            Status::HostOnly { .. } => ("host-only", "host"),
            Status::Blocked { .. } => ("blocked", tag),
        };
        println!(
            "{:<10} {:<26} {:<24} {status}",
            target.id, target.triple, image
        );
        match target.status {
            Status::HostOnly { note } => println!("  Note: {note}"),
            Status::Blocked { reason, next } => {
                println!("  Reason: {reason}");
                println!("  Next: {next}");
            }
            Status::Supported => {}
        }
    }
}
