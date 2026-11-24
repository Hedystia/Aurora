//! Command-line interface for generating Aurora's script metadata.

use std::{env, fs, path::Path};

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [mode, directory] if mode == "--protocol-only" => write_protocol(Path::new(directory)),
        [mode, input, output] if mode == "--lower" => {
            lower_file(Path::new(input), Path::new(output), false)
        }
        [mode, input, output] if mode == "--lower-runtime" => {
            lower_file(Path::new(input), Path::new(output), true)
        }
        [input, directory] => generate(Path::new(input), Path::new(directory)),
        _ => bail!(
            "Usage: aurora-codegen INPUT.ts OUTPUT_DIR | \
             aurora-codegen --lower INPUT.ts OUTPUT.ts | \
             aurora-codegen --lower-runtime INPUT.ts OUTPUT.ts | \
             aurora-codegen --protocol-only OUTPUT_DIR"
        ),
    }
}

fn generate(input: &Path, directory: &Path) -> Result<()> {
    let source = fs::read_to_string(input)
        .with_context(|| format!("Reading TypeScript source {}", input.display()))?;
    let components = codegen::extract(&source)
        .with_context(|| format!("Extracting component metadata from {}", input.display()))?;
    fs::create_dir_all(directory)
        .with_context(|| format!("Creating metadata directory {}", directory.display()))?;
    fs::write(
        directory.join("components.json"),
        serde_json::to_vec_pretty(&components)?,
    )
    .context("Writing component metadata")?;
    write_sdk(directory)?;
    write_protocol(directory)
}

fn write_sdk(directory: &Path) -> Result<()> {
    fs::write(
        directory.join("index.d.ts"),
        codegen::SDK_DECLARATIONS.as_bytes(),
    )
    .context("Writing SDK declarations")?;
    let modules = directory.join("sdk");
    fs::create_dir_all(&modules)
        .with_context(|| format!("Creating SDK declaration directory {}", modules.display()))?;
    for (name, contents) in codegen::SDK_MODULES {
        fs::write(modules.join(name), contents).with_context(|| {
            format!("Writing SDK declarations {}", modules.join(name).display())
        })?;
    }
    Ok(())
}

fn lower_file(input: &Path, output: &Path, runtime: bool) -> Result<()> {
    let source = fs::read_to_string(input)
        .with_context(|| format!("Reading TypeScript source {}", input.display()))?;
    let lowered = if runtime {
        codegen::lower_runtime(&source)
    } else {
        codegen::lower(&source)
    }
    .with_context(|| format!("Lowering TypeScript source {}", input.display()))?;
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("Creating output directory {}", parent.display()))?;
    }
    fs::write(output, lowered)
        .with_context(|| format!("Writing lowered source {}", output.display()))
}

fn write_protocol(directory: &Path) -> Result<()> {
    fs::create_dir_all(directory)
        .with_context(|| format!("Creating metadata directory {}", directory.display()))?;
    let protocol = codegen::protocol_typescript()?;
    fs::write(directory.join("protocol.mts"), protocol)
        .with_context(|| format!("Writing protocol to {}", directory.display()))
}
