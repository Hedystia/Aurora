//! Native macOS host builds and `.app` packaging.
//!
//! Apple's software licence permits macOS virtualization only on Apple-branded
//! hardware, so this path never runs inside a Linux container image.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use engine_core::app::bundle::{self, ExportManifest};

use super::{
    BuildMetadata, BuildOutcome, BuildRequest, absolute_output, copy_tree, resolve_includes,
    resolve_project, write_build_metadata,
};

const APPLE_HOST_ERROR: &str = "macOS builds require an Apple-branded macOS host. Apple's software \
    licence permits macOS virtualization only on Apple hardware (up to two development VMs per \
    Mac), so Linux container images such as dockurr/macos are deliberately not supported. Run this \
    on a Mac, or use a hosted macOS CI runner.";

/// Builds and packages a project, including native scripts, into a macOS application bundle.
pub fn build(request: &BuildRequest) -> Result<BuildOutcome> {
    ensure!(cfg!(target_os = "macos"), "{APPLE_HOST_ERROR}");
    ensure!(
        request.target.id == "macos",
        "internal error: the macOS builder received another target"
    );
    ensure!(
        request.image.is_none(),
        "--image applies only to container targets"
    );
    ensure!(
        !request.keep_image,
        "--keep-image applies only to container targets"
    );
    if request.notarize.is_some() {
        ensure!(
            request
                .sign
                .as_deref()
                .is_some_and(|identity| identity != "-"),
            "--notarize needs a Developer ID --sign identity, not an ad-hoc '-' signature"
        );
    }
    let version = env!("CARGO_PKG_VERSION");
    let output = absolute_output(&request.output)?;
    ensure!(
        output
            .extension()
            .is_some_and(|extension| extension == "app"),
        "macOS build output must end in .app: {}",
        output.display()
    );
    ensure!(
        !output.try_exists()?,
        "Build output already exists: {}",
        output.display()
    );
    let parent = output
        .parent()
        .context("The output parent directory must exist")?;
    ensure!(
        parent.is_dir(),
        "The output parent directory must exist: {}",
        parent.display()
    );
    let (root, selected) = resolve_project(&request.project)?;
    let includes = resolve_includes(&root, &request.includes)?;
    let editor = editor_binary()?;
    let player = editor.with_file_name(format!("aurora-player{}", env::consts::EXE_SUFFIX));
    ensure!(
        player.is_file(),
        "The macOS player must be installed beside the editor: {}",
        player.display()
    );
    let (manifest_target, triple) = host_target()?;
    let bundle_name = output
        .file_stem()
        .and_then(|name| name.to_str())
        .context("The .app name must be valid UTF-8")?;

    if request.dry_run {
        eprintln!(
            "Dry run: macOS host build with {} for {manifest_target}",
            editor.display()
        );
        eprintln!(
            "  {} --project {} --export <staging>/export{}",
            editor.display(),
            selected.display(),
            includes
                .iter()
                .map(|include| format!(" --include-scene {}", include.display()))
                .collect::<String>()
        );
        return Ok(BuildOutcome {
            image: "host".to_owned(),
            image_id: None,
            output,
            source_bundle_sha256: None,
            dry_run: true,
        });
    }

    let staging = tempfile::Builder::new()
        .prefix(".aurora-macos-")
        .tempdir_in(parent)
        .context("Creating the macOS build staging directory")?;
    let export = staging.path().join("export");
    let mut command = Command::new(&editor);
    command.arg("--project").arg(&selected);
    command.arg("--export").arg(&export);
    for include in &includes {
        command.arg("--include-scene").arg(include);
    }
    let status = command
        .status()
        .with_context(|| format!("Running {}", editor.display()))?;
    ensure!(
        status.success(),
        "The macOS project export failed. Native scripts need clang and the scriptc toolchain \
         installed by tools/setup.sh."
    );
    let export_manifest = verify_export(&export, version, manifest_target)?;

    let app = staging.path().join("App.app");
    let spec = PackageSpec {
        app: &app,
        export: &export,
        player: &player,
        bundle_name,
        engine_version: version,
        manifest_target,
        triple,
        sign: request.sign.as_deref(),
    };
    let source_bundle_sha256 = package_app(&spec)?;
    if let Some(profile) = &request.notarize {
        notarize_app(&app, profile)?;
    }
    // The staging directory is inside the output parent, so this rename is atomic.
    fs::rename(&app, &output)
        .with_context(|| format!("Publishing macOS app to {}", output.display()))?;
    // Keep the original export digest available for diagnostics even though the
    // manifest changes when build.json is added.
    eprintln!(
        "Exported bundle {} then repacked it with build metadata",
        export_manifest.source_bundle_sha256
    );
    Ok(BuildOutcome {
        image: "host".to_owned(),
        image_id: None,
        output,
        source_bundle_sha256: Some(source_bundle_sha256),
        dry_run: false,
    })
}

fn editor_binary() -> Result<PathBuf> {
    if let Some(path) = env::var_os("AURORA_EDITOR") {
        let path = PathBuf::from(path);
        ensure!(
            path.is_file(),
            "AURORA_EDITOR is not a file: {}",
            path.display()
        );
        return Ok(path);
    }
    let sibling =
        env::current_exe()?.with_file_name(format!("aurora-editor{}", env::consts::EXE_SUFFIX));
    ensure!(
        sibling.is_file(),
        "aurora build needs aurora-editor beside it, or AURORA_EDITOR set to the editor binary"
    );
    Ok(sibling)
}

fn host_target() -> Result<(&'static str, &'static str)> {
    match env::consts::ARCH {
        "aarch64" => Ok(("macos-aarch64", "aarch64-apple-darwin")),
        "x86_64" => Ok(("macos-x86_64", "x86_64-apple-darwin")),
        other => bail!("Unsupported macOS architecture: {other}"),
    }
}

fn verify_export(
    export: &Path,
    engine_version: &str,
    manifest_target: &str,
) -> Result<ExportManifest> {
    let manifest_path = export.join(bundle::EXPORT_MANIFEST);
    let manifest: ExportManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("Reading {}", manifest_path.display()))?,
    )
    .context("The macOS export produced an invalid manifest")?;
    ensure!(
        manifest.engine_version == engine_version,
        "The macOS export used engine version {} but this CLI is {engine_version}",
        manifest.engine_version
    );
    ensure!(
        manifest.target == manifest_target,
        "The macOS export targets {} but this host must produce {manifest_target}",
        manifest.target
    );
    manifest
        .verify(export)
        .context("The macOS export failed its integrity check")?;
    Ok(manifest)
}

struct PackageSpec<'a> {
    app: &'a Path,
    export: &'a Path,
    player: &'a Path,
    bundle_name: &'a str,
    engine_version: &'a str,
    manifest_target: &'a str,
    triple: &'a str,
    sign: Option<&'a str>,
}

fn package_app(spec: &PackageSpec<'_>) -> Result<String> {
    let contents = spec.app.join("Contents");
    let macos = contents.join("MacOS");
    let game = contents.join("Resources").join("game");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&game)?;

    let executable = macos.join("aurora-player");
    fs::copy(spec.player, &executable)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
    }

    copy_tree(spec.export, &game)?;
    let export_manifest: ExportManifest =
        serde_json::from_slice(&fs::read(game.join(bundle::EXPORT_MANIFEST))?)?;
    export_manifest
        .verify(&game)
        .context("The copied macOS export failed its integrity check")?;
    // The app bundle already has Contents/MacOS/aurora-player, so the export's
    // own player and launcher are redundant. Removing them before regenerating
    // the manifest keeps the bundle smaller and avoids signing two players.
    for runtime in [
        "aurora-player",
        "aurora-player.exe",
        "run-game.sh",
        "run-game.cmd",
    ] {
        let path = game.join(runtime);
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }

    let metadata = BuildMetadata::host(
        spec.engine_version,
        spec.manifest_target,
        spec.triple,
        &export_manifest.source_bundle_sha256,
    );
    let manifest =
        write_build_metadata(&game, &metadata, spec.engine_version, spec.manifest_target)?;

    fs::write(
        contents.join("Info.plist"),
        info_plist(
            spec.bundle_name,
            &bundle_identifier(spec.bundle_name),
            spec.engine_version,
        ),
    )?;
    fs::write(contents.join("PkgInfo"), "APPL????")?;

    if let Some(identity) = spec.sign {
        // Apple silicon requires a signature on every native executable. The
        // script child lives in Resources, outside codesign's usual nested-code
        // locations, so sign it explicitly before sealing the app.
        let script = game.join("game");
        if script.is_file() {
            sign_binary(&script, identity)?;
        }
        sign_app(spec.app, identity)?;
    }
    Ok(manifest.source_bundle_sha256)
}

fn info_plist(name: &str, identifier: &str, version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>{name}</string>
    <key>CFBundleExecutable</key>
    <string>aurora-player</string>
    <key>CFBundleIdentifier</key>
    <string>{identifier}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>{name}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
"#,
        name = xml_escape(name),
        identifier = xml_escape(identifier),
        version = xml_escape(version)
    )
}

fn bundle_identifier(name: &str) -> String {
    let mut suffix = String::new();
    let mut separator = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            suffix.push(character.to_ascii_lowercase());
            separator = false;
        } else if !separator && !suffix.is_empty() {
            suffix.push('-');
            separator = true;
        }
    }
    while suffix.ends_with('-') {
        suffix.pop();
    }
    if suffix.is_empty() {
        suffix.push_str("game");
    }
    format!("dev.aurora.{suffix}")
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn sign_binary(path: &Path, identity: &str) -> Result<()> {
    codesign(path, identity)
}

fn sign_app(app: &Path, identity: &str) -> Result<()> {
    codesign(app, identity)?;
    let mut verify = Command::new("codesign");
    verify.args(["--verify", "--deep", "--strict"]).arg(app);
    run_tool(&mut verify, "codesign --verify")?;
    Ok(())
}

fn codesign(path: &Path, identity: &str) -> Result<()> {
    let mut command = Command::new("codesign");
    command.args(["--force", "--options", "runtime", "--sign", identity]);
    command.arg(if identity == "-" {
        "--timestamp=none"
    } else {
        "--timestamp"
    });
    command.arg(path);
    run_tool(&mut command, "codesign")
}

fn notarize_app(app: &Path, profile: &str) -> Result<()> {
    let archive = app.with_extension("zip");
    let mut ditto = Command::new("ditto");
    ditto
        .args(["-c", "-k", "--keepParent"])
        .arg(app)
        .arg(&archive);
    run_tool(&mut ditto, "ditto")?;
    let mut submit = Command::new("xcrun");
    submit.args(["notarytool", "submit"]).arg(&archive).args([
        "--keychain-profile",
        profile,
        "--wait",
    ]);
    let result = run_tool(&mut submit, "notarytool submit");
    let _ = fs::remove_file(&archive);
    result?;
    let mut staple = Command::new("xcrun");
    staple.args(["stapler", "staple"]).arg(app);
    run_tool(&mut staple, "stapler staple")?;
    let mut validate = Command::new("xcrun");
    validate.args(["stapler", "validate"]).arg(app);
    run_tool(&mut validate, "stapler validate")?;
    Ok(())
}

fn run_tool(command: &mut Command, action: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("Running {action}"))?;
    ensure!(status.success(), "{action} failed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERSION: &str = env!("CARGO_PKG_VERSION");

    fn export_fixture(root: &Path) -> Result<PathBuf> {
        let export = root.join("export");
        fs::create_dir(&export)?;
        fs::write(export.join("main.scene"), "scene")?;
        fs::write(export.join("aurora-player"), "bundled player")?;
        fs::write(export.join("run-game.sh"), "#!/bin/sh\n")?;
        let manifest = ExportManifest::create(&export, VERSION, "macos-aarch64")?;
        fs::write(
            export.join(bundle::EXPORT_MANIFEST),
            serde_json::to_vec(&manifest)?,
        )?;
        Ok(export)
    }

    fn player_fixture(root: &Path) -> Result<PathBuf> {
        let player = root.join("aurora-player");
        fs::write(&player, "player")?;
        Ok(player)
    }

    #[test]
    fn packages_a_verified_app_bundle() -> Result<()> {
        let root = tempfile::tempdir()?;
        let export = export_fixture(root.path())?;
        let player = player_fixture(root.path())?;
        let app = root.path().join("Game.app");
        let spec = PackageSpec {
            app: &app,
            export: &export,
            player: &player,
            bundle_name: "My & Game",
            engine_version: VERSION,
            manifest_target: "macos-aarch64",
            triple: "aarch64-apple-darwin",
            sign: None,
        };
        let bundle = package_app(&spec)?;

        let executable = app.join("Contents/MacOS/aurora-player");
        assert!(executable.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(
                fs::metadata(&executable)?.permissions().mode() & 0o111,
                0o111
            );
        }
        let game = app.join("Contents/Resources/game");
        assert!(!game.join("aurora-player").exists());
        assert!(!game.join("run-game.sh").exists());
        let manifest: ExportManifest =
            serde_json::from_slice(&fs::read(game.join(bundle::EXPORT_MANIFEST))?)?;
        manifest.verify(&game)?;
        assert_eq!(bundle, manifest.source_bundle_sha256);
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(game.join(super::super::BUILD_METADATA))?)?;
        assert_eq!(metadata["builder_kind"], "host");
        assert!(metadata["builder_image"].is_null());
        assert_eq!(metadata["target_triple"], "aarch64-apple-darwin");

        let plist = fs::read_to_string(app.join("Contents/Info.plist"))?;
        assert!(plist.contains("<string>My &amp; Game</string>"));
        assert!(plist.contains("dev.aurora.my-game"));
        assert_eq!(
            fs::read_to_string(app.join("Contents/PkgInfo"))?,
            "APPL????"
        );
        Ok(())
    }

    #[test]
    fn packages_a_native_script_program() -> Result<()> {
        let root = tempfile::tempdir()?;
        let export = export_fixture(root.path())?;
        fs::write(export.join("game"), "binary")?;
        let manifest = ExportManifest::create(&export, VERSION, "macos-aarch64")?;
        fs::write(
            export.join(bundle::EXPORT_MANIFEST),
            serde_json::to_vec(&manifest)?,
        )?;
        let player = player_fixture(root.path())?;
        let app = root.path().join("Game.app");
        package_app(&PackageSpec {
            app: &app,
            export: &export,
            player: &player,
            bundle_name: "Game",
            engine_version: VERSION,
            manifest_target: "macos-aarch64",
            triple: "aarch64-apple-darwin",
            sign: None,
        })?;
        let game = app.join("Contents/Resources/game");
        assert!(game.join("game").is_file());
        let written: ExportManifest =
            serde_json::from_slice(&fs::read(game.join(bundle::EXPORT_MANIFEST))?)?;
        written.verify(&game)?;
        Ok(())
    }

    #[test]
    fn identifiers_and_plist_values_are_safe() {
        assert_eq!(bundle_identifier("My  Game!"), "dev.aurora.my-game");
        assert_eq!(bundle_identifier("&&&"), "dev.aurora.game");
        assert_eq!(xml_escape("<A & \"B\">"), "&lt;A &amp; &quot;B&quot;&gt;");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn refuses_to_run_outside_an_apple_host() -> Result<()> {
        let root = tempfile::tempdir()?;
        let request = BuildRequest {
            project: root.path().to_path_buf(),
            target: crate::target::Target::parse("macos")?,
            output: root.path().join("Game.app"),
            includes: Vec::new(),
            image: None,
            keep_image: false,
            dry_run: false,
            sign: None,
            notarize: None,
        };
        let error = build(&request).unwrap_err().to_string();
        assert!(error.contains("Apple-branded macOS host"), "{error}");
        assert!(error.contains("dockurr/macos"), "{error}");
        Ok(())
    }
}
