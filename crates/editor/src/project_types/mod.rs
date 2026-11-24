//! Installs the editor's versioned TypeScript SDK in a project.

use std::{
    fs,
    io::{ErrorKind, Write},
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};

use crate::strings;

const GENERATED: &str = ".aurora";
const DECLARATIONS: &str = ".aurora/index.d.ts";
const LEGACY_DECLARATIONS: &str = ".aurora/types/aurora-cc.d.ts";
const GENERATED_CONFIG: &str = ".aurora/tsconfig.json";
const STAMP: &str = "// Aurora SDK ";
const EDITOR_VERSION: &str = env!("CARGO_PKG_VERSION");
const ROOT_CONFIG: &str = "{\n  \"extends\": \"./.aurora/tsconfig.json\"\n}\n";
const BASE_CONFIG: &str = r#"{
  "compilerOptions": {
    "strict": true,
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "allowImportingTsExtensions": true,
    "noEmit": true,
    "experimentalDecorators": true,
    "paths": { "aurora": ["./index.d.ts"], "aurora-cc": ["./index.d.ts"] }
  }
}
"#;

pub(super) enum Status {
    Missing,
    Current,
    Different(String),
}

pub(super) fn root(document_dir: &Path) -> &Path {
    document_dir
        .ancestors()
        .find(|dir| crate::project::main_scene(dir).is_file())
        .unwrap_or(document_dir)
}

fn declarations_at(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn status(root: &Path) -> Result<Status> {
    let contents = match declarations_at(&root.join(DECLARATIONS))? {
        Some(contents) => Some(contents),
        None => declarations_at(&root.join(LEGACY_DECLARATIONS))?,
    };
    let Some(contents) = contents else {
        return Ok(Status::Missing);
    };
    match contents
        .lines()
        .next()
        .and_then(|line| line.strip_prefix(STAMP))
    {
        Some(version) if version == EDITOR_VERSION => Ok(Status::Current),
        Some(version) => Ok(Status::Different(version.into())),
        None => bail!("{}", strings::TYPES_UNMANAGED),
    }
}

pub(super) fn refresh(root: &Path, approved_version: Option<&str>) -> Result<()> {
    let generated = root.join(GENERATED);
    fs::create_dir_all(&generated)?;
    // Recheck while holding the lock in case two editors open the project together.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(generated.join("typescript.lock"))?;
    lock.lock()?;
    if let Status::Different(version) = status(root)? {
        ensure!(
            approved_version == Some(&version),
            "{}",
            strings::TYPES_REOPEN
        );
    }
    let legacy = root.join(LEGACY_DECLARATIONS);
    // Publish the version stamp last so an interrupted upgrade is retried.
    // Existing hand-written tsconfigs may still map the old generated file.
    let legacy_version = declarations_at(&legacy)?.and_then(|contents| {
        contents
            .lines()
            .next()
            .and_then(|line| line.strip_prefix(STAMP))
            .map(str::to_owned)
    });
    if let Some(version) = &legacy_version {
        replace_generated(
            &legacy,
            &format!("{STAMP}{version}\nexport * from \"../index.js\";\n"),
        )?;
    }
    replace_generated(&root.join(GENERATED_CONFIG), BASE_CONFIG)?;
    install_root_config(root)?;
    let sdk = generated.join("sdk");
    fs::create_dir_all(&sdk)?;
    for (name, contents) in codegen::SDK_MODULES {
        replace_generated(&sdk.join(name), contents)?;
    }
    replace_generated(
        &root.join(DECLARATIONS),
        &format!("{STAMP}{EDITOR_VERSION}\n{}", codegen::SDK_DECLARATIONS),
    )?;
    if legacy_version.is_some() {
        replace_generated(
            &legacy,
            &format!("{STAMP}{EDITOR_VERSION}\nexport * from \"../index.js\";\n"),
        )?;
    }
    Ok(())
}

pub(super) fn clean_incomplete_creation(root: &Path) {
    for file in [
        DECLARATIONS,
        GENERATED_CONFIG,
        ".aurora/typescript.lock",
        "tsconfig.json",
    ] {
        let _ = fs::remove_file(root.join(file));
    }
    let sdk = root.join(GENERATED).join("sdk");
    for (name, _) in codegen::SDK_MODULES {
        let _ = fs::remove_file(sdk.join(name));
    }
    let _ = fs::remove_dir(sdk);
    let _ = fs::remove_dir(root.join(GENERATED));
}

fn replace_generated(path: &Path, contents: &str) -> Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == contents.as_bytes()) {
        return Ok(());
    }
    let parent = path
        .parent()
        .context("Missing generated TypeScript directory")?;
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(contents.as_bytes())?;
    staged.persist(path)?;
    Ok(())
}

fn install_root_config(root: &Path) -> Result<()> {
    let path = root.join("tsconfig.json");
    if path.try_exists()? {
        return Ok(());
    }
    let mut staged = tempfile::NamedTempFile::new_in(root)?;
    staged.write_all(ROOT_CONFIG.as_bytes())?;
    match staged.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_mapping_keeps_working_after_confirmed_update() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let legacy = root.join(LEGACY_DECLARATIONS);
        fs::create_dir_all(legacy.parent().context("Missing legacy SDK directory")?)?;
        fs::write(&legacy, format!("{STAMP}0.0.1\nold declaration"))?;
        assert!(matches!(status(root)?, Status::Different(version) if version == "0.0.1"));
        assert!(refresh(root, None).is_err());
        assert!(!root.join(DECLARATIONS).exists());
        let custom =
            r#"{"compilerOptions":{"paths":{"aurora-cc":["./.aurora/types/aurora-cc.d.ts"]}}}"#;
        fs::write(root.join("tsconfig.json"), custom)?;
        refresh(root, Some("0.0.1"))?;
        assert!(fs::read_to_string(&legacy)?.contains("../index.js"));
        assert!(fs::read_to_string(root.join(DECLARATIONS))?.ends_with(codegen::SDK_DECLARATIONS));
        assert_eq!(fs::read_to_string(root.join("tsconfig.json"))?, custom);
        Ok(())
    }

    #[test]
    fn only_replaces_managed_files_and_requires_version_approval() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        assert!(matches!(status(root)?, Status::Missing));
        refresh(root, None)?;
        assert!(fs::read_to_string(root.join("tsconfig.json"))?.contains(GENERATED_CONFIG));
        assert!(fs::read_to_string(root.join(DECLARATIONS))?.ends_with(codegen::SDK_DECLARATIONS));

        let custom = "{ /* custom config */ }";
        fs::write(root.join("tsconfig.json"), custom)?;
        let old_version = "0.0.1";
        let old = format!("{STAMP}{old_version}\nold sdk");
        fs::write(root.join(DECLARATIONS), &old)?;
        assert!(matches!(status(root)?, Status::Different(version) if version == old_version));
        assert!(refresh(root, None).is_err());
        assert!(refresh(root, Some("0.0.2")).is_err());
        assert_eq!(fs::read_to_string(root.join(DECLARATIONS))?, old);
        refresh(root, Some(old_version))?;
        assert!(matches!(status(root)?, Status::Current));
        assert_eq!(fs::read_to_string(root.join("tsconfig.json"))?, custom);

        fs::write(
            root.join(DECLARATIONS),
            format!("{STAMP}{EDITOR_VERSION}\nstale sdk"),
        )?;
        refresh(root, None)?;
        assert!(fs::read_to_string(root.join(DECLARATIONS))?.ends_with(codegen::SDK_DECLARATIONS));
        assert!(
            fs::read_to_string(root.join(".aurora/sdk/index.d.ts"))?
                .contains("export declare function property")
        );
        Ok(())
    }
}
