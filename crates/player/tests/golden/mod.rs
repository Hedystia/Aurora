//! Deterministic 600-frame native Jumpy Bird playthrough.

use std::{env, path::Path, process::Command};

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires the scriptc game executable prepared by tools/golden_test.sh"]
#[allow(
    clippy::disallowed_methods,
    reason = "Insta expands an internal unwrap inside this test"
)]
fn jumpy_bird() -> Result<()> {
    let executable = env::var_os("AURORA_GOLDEN_SCRIPT").context("Run tools/golden_test.sh")?;
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/game-template/jumpy");
    let cache = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_aurora-player"))
        .arg("--scene")
        .arg(project.join("main.aurora-scene"))
        .args(["--script-node", "5", "--headless", "--frames", "600"])
        .args([
            "--touch-at",
            "0",
            "--flap-at",
            "52",
            "--flap-at",
            "104",
            "--flap-at",
            "156",
            "--flap-at",
            "208",
            "--flap-at",
            "260",
            "--flap-at",
            "312",
            "--flap-at",
            "364",
        ])
        .arg("--script")
        .arg(executable)
        .arg("--asset-cache")
        .arg(cache.path())
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()?;
    ensure!(
        output.status.success(),
        "Player failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let state: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(
        state["ticks"] == 600,
        "Expected exactly 600 simulation ticks"
    );
    let nodes = state["nodes"].as_array().context("No entity state")?;
    let by_name = |name: &str| {
        nodes
            .iter()
            .find(|node| node["name"] == name)
            .with_context(|| format!("Missing {name}"))
    };
    ensure!(
        by_name("Start screen")?["visible"] == false,
        "The input trace did not start the game"
    );
    ensure!(
        by_name("Game over screen")?["appearance"]["active"] == true,
        "The game did not end"
    );
    ensure!(
        by_name("Score digit 3")?["sprite"]["texture"]
            != by_name("Score digit 1")?["sprite"]["texture"],
        "The input trace did not score"
    );
    let digest = format!("{:x}", Sha256::digest(&output.stdout));
    println!("Jumpy Bird after 600 frames: {digest}");
    insta::assert_snapshot!("jumpy_bird", digest);
    Ok(())
}
