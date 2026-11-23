//! Generate SDK declarations and inspector metadata for statically declared components.

use std::{env, fs, path::Path};

use anyhow::{Context, Result, ensure};
use codegen::extract;

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = args
        .next()
        .context("Usage: aurora-codegen INPUT.ts|--protocol-only OUTPUT_DIRECTORY")?;
    if source == "--lower" {
        let input = args.next().context("Missing TypeScript source")?;
        let output = args.next().context("Missing lowered TypeScript output")?;
        ensure!(args.next().is_none(), "Too many arguments");
        let output = Path::new(&output);
        if let Some(directory) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(directory)?;
        }
        fs::write(output, codegen::lower(&fs::read_to_string(input)?)?)?;
        return Ok(());
    }
    let output = args.next().context("Missing output directory")?;
    ensure!(args.next().is_none(), "Too many arguments");
    let output = Path::new(&output);
    fs::create_dir_all(output)?;
    if source != "--protocol-only" {
        let metadata = extract(&fs::read_to_string(source)?)?;
        fs::write(
            output.join("components.json"),
            serde_json::to_vec_pretty(&metadata)?,
        )?;
        fs::write(output.join("index.d.ts"), codegen::SDK_DECLARATIONS)?;
    }
    fs::write(output.join("protocol.mts"), codegen::protocol_typescript()?)?;
    Ok(())
}
