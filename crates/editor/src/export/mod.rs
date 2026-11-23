//! Builds authored scenes and their native scripts into a portable data directory.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, ensure};
use engine_core::{
    app::bundle,
    save::{NodeFile, PrefabFile},
};

use crate::{
    preview::{binding_nodes, script_entry_for, script_prefabs_within},
    project::Project,
};

pub(super) fn export(project: &Project, includes: &[PathBuf], output: &Path) -> Result<()> {
    ensure!(
        !project.prefab && project.path.is_file(),
        "Export needs a saved scene"
    );
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let destination = parent
        .canonicalize()?
        .join(output.file_name().context("Missing export name")?);
    ensure!(
        !destination.try_exists()?,
        "Export destination already exists"
    );

    let mut projects = Vec::with_capacity(includes.len() + 1);
    projects.push(project.clone());
    for path in includes {
        let included = Project::open_existing(path)
            .with_context(|| format!("Opening included scene {}", path.display()))?;
        ensure!(
            !included.prefab && included.path.canonicalize()?.starts_with(&project.root),
            "Included scene must be a saved scene inside the project: {}",
            path.display()
        );
        projects.push(included);
    }

    let mut manifest = bundle::Manifest {
        version: bundle::VERSION,
        scenes: projects
            .iter()
            .map(|project| {
                let name = project
                    .path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .context("Exported scene filenames must have UTF-8 names")?;
                Ok(bundle::Member {
                    name: name.to_owned(),
                    bindings: Vec::new(),
                })
            })
            .collect::<Result<_>>()?,
    };
    manifest.validate()?;
    let names: Vec<_> = manifest
        .scenes
        .iter()
        .map(|scene| scene.name.clone())
        .collect();
    let build = tempfile::tempdir()?;
    let staged = tempfile::Builder::new()
        .prefix(".export-")
        .tempdir_in(parent)?;
    let engine = crate::engine_resources()?;
    let multiple = projects.len() > 1;

    for (index, (project_scene, member)) in projects.iter().zip(&mut manifest.scenes).enumerate() {
        let output = if multiple {
            staged.path().join(bundle::member_directory(index))
        } else {
            staged.path().to_path_buf()
        };
        fs::create_dir_all(&output)?;
        member.bindings = pack_scene(
            project_scene,
            &project.root,
            &output,
            build.path(),
            &engine,
            &names,
        )
        .with_context(|| format!("Exporting scene {}", project_scene.path.display()))?;
    }

    let (name, bytes) = if multiple {
        (bundle::MANIFEST, serde_json::to_vec(&manifest)?)
    } else {
        (
            "bindings.json",
            serde_json::to_vec(&manifest.scenes[0].bindings)?,
        )
    };
    ensure!(
        bytes.len() <= bundle::MAX_MANIFEST_BYTES,
        "Bundle manifest is too large"
    );
    fs::write(staged.path().join(name), bytes)?;

    // Claim the destination exclusively before publishing the complete package.
    fs::create_dir(&destination)?;
    if let Err(error) = fs::rename(staged.path(), &destination) {
        let _ = fs::remove_dir(&destination);
        return Err(error.into());
    }
    Ok(())
}

fn pack_scene(
    project: &Project,
    source_root: &Path,
    output: &Path,
    build: &Path,
    engine: &Path,
    names: &[String],
) -> Result<Vec<usize>> {
    let nodes = binding_nodes(project)?;
    let catalog = script_prefabs_within(project, &nodes, source_root)?;
    let mut document = project.document.clone();
    let mut assets = Assets::new(source_root, output);
    assets.rewrite_nodes(&mut document.nodes, &project.root, false)?;
    for instance in &mut document.instances {
        instance.prefab = assets.prefab(&project.root.join(&instance.prefab))?;
    }
    document.script_prefabs = catalog
        .values()
        .map(|path| assets.prefab(path))
        .collect::<Result<_>>()?;
    if let Some(gameplay) = &mut document.gameplay {
        for path in gameplay
            .sounds
            .iter_mut()
            .chain(gameplay.frames.iter_mut().flat_map(|bank| &mut bank.png))
        {
            *path = assets.file(&project.root.join(&*path))?;
        }
    }
    document.save(&output.join("main.scene"))?;

    let native = build.join("scripts/build/native.ts");
    let modules = build.join("project");
    let (entry, bindings) =
        script_entry_for(project, &nodes, &native, &catalog, names, |source| {
            let source = source.canonicalize()?;
            let relative = source
                .strip_prefix(source_root)
                .context("Script lies outside the project")?;
            Ok(modules.join(relative))
        })?;
    if bindings.is_empty() {
        return Ok(bindings);
    }

    if !build.join("tsconfig.json").is_file() {
        stage_sources(source_root, build, engine)?;
    }
    let entry_path = build.join("entry.ts");
    fs::write(&entry_path, entry)?;
    let compiler = Command::new("bash")
        .arg(engine.join("tools/compile-native.sh"))
        .arg(&entry_path)
        .arg(output.join("game"))
        .status()
        .context("Running the native script compiler")?;
    ensure!(compiler.success(), "Native scene script compilation failed");
    Ok(bindings)
}

fn stage_sources(source_root: &Path, build: &Path, engine: &Path) -> Result<()> {
    let protocol = build.join(".aurora/scripts/metadata");
    fs::create_dir_all(&protocol)?;
    fs::write(
        protocol.join("protocol.mts"),
        codegen::protocol_typescript()?,
    )?;
    copy_sources(
        &engine.join("scripts/aurora"),
        &build.join("scripts/aurora"),
    )?;
    copy_sources(&engine.join("scripts/build"), &build.join("scripts/build"))?;
    copy_sources(source_root, &build.join("project"))?;
    let sdk = "./scripts/aurora/index.ts";
    let mut paths = BTreeMap::new();
    paths.insert("aurora", [sdk]);
    paths.insert("aurora-cc", [sdk]);
    let config = TypeScriptConfig {
        compiler_options: TypeScriptOptions {
            strict: true,
            target: "ES2022",
            module: "NodeNext",
            module_resolution: "NodeNext",
            allow_importing_ts_extensions: true,
            no_emit: true,
            paths,
        },
    };
    fs::write(
        build.join("tsconfig.json"),
        serde_json::to_vec_pretty(&config)?,
    )?;
    Ok(())
}

/// TypeScript configuration staged next to exported script sources.
#[derive(serde::Serialize)]
struct TypeScriptConfig<'a> {
    #[serde(rename = "compilerOptions")]
    compiler_options: TypeScriptOptions<'a>,
}

#[derive(serde::Serialize)]
struct TypeScriptOptions<'a> {
    strict: bool,
    target: &'a str,
    module: &'a str,
    #[serde(rename = "moduleResolution")]
    module_resolution: &'a str,
    #[serde(rename = "allowImportingTsExtensions")]
    allow_importing_ts_extensions: bool,
    #[serde(rename = "noEmit")]
    no_emit: bool,
    paths: BTreeMap<&'a str, [&'a str; 1]>,
}

fn copy_sources(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "node_modules" || name == "target" || name.to_string_lossy().starts_with('.') {
            continue;
        }
        let target = destination.join(&name);
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_sources(&entry.path(), &target)?;
        } else if kind.is_file()
            && matches!(
                entry.path().extension().and_then(|value| value.to_str()),
                Some("ts" | "mts" | "cts")
            )
        {
            let source = fs::read_to_string(entry.path())?;
            let lowered = codegen::lower(&source)
                .with_context(|| format!("Lowering {}", entry.path().display()))?;
            fs::write(target, lowered)?;
        }
    }
    Ok(())
}

struct Assets<'a> {
    root: &'a Path,
    output: &'a Path,
    paths: BTreeMap<PathBuf, PathBuf>,
    written: BTreeSet<PathBuf>,
}

impl<'a> Assets<'a> {
    fn new(root: &'a Path, output: &'a Path) -> Self {
        Self {
            root,
            output,
            paths: BTreeMap::new(),
            written: BTreeSet::new(),
        }
    }

    fn reserve(&mut self, path: &Path) -> Result<(PathBuf, PathBuf)> {
        let source = path
            .canonicalize()
            .with_context(|| format!("Resolving asset {}", path.display()))?;
        ensure!(
            source.starts_with(self.root) && source.is_file(),
            "Asset lies outside the project: {}",
            path.display()
        );
        let index = self.paths.len();
        let target = self
            .paths
            .entry(source.clone())
            .or_insert_with(|| {
                let mut path = PathBuf::from("data").join(index.to_string());
                if let Some(extension) = source.extension() {
                    path.set_extension(extension);
                }
                path
            })
            .clone();
        fs::create_dir_all(self.output.join("data"))?;
        Ok((source, target))
    }

    fn file(&mut self, path: &Path) -> Result<PathBuf> {
        let (source, target) = self.reserve(path)?;
        if self.written.insert(target.clone()) {
            fs::copy(source, self.output.join(&target))?;
        }
        Ok(target)
    }

    fn prefab(&mut self, path: &Path) -> Result<PathBuf> {
        let (source, target) = self.reserve(path)?;
        if self.written.insert(target.clone()) {
            let mut prefab = PrefabFile::load(&source)?;
            let parent = source
                .parent()
                .context("Prefab has no containing directory")?;
            self.rewrite_nodes(&mut prefab.nodes, parent, true)?;
            prefab.save(&self.output.join(&target))?;
        }
        Ok(target)
    }

    fn rewrite_nodes(
        &mut self,
        nodes: &mut [NodeFile],
        directory: &Path,
        prefab: bool,
    ) -> Result<()> {
        for node in nodes {
            for path in node.sprite.iter_mut().map(|sprite| &mut sprite.png).chain(node.visual.as_mut().and_then(|visual| visual.asset_mut())) {
                let target = self.file(&directory.join(&*path))?;
                *path = if prefab {
                    PathBuf::from(target.file_name().context("Missing packaged asset name")?)
                } else {
                    target
                };
            }
        }
        Ok(())
    }
}
