//! Exports the scene-registered audio and sprite-frame catalog to script entries.

use std::fmt::Write as _;

use anyhow::Result;

use crate::project::Project;

pub(super) fn write_catalog(entry: &mut String, project: &Project) -> Result<()> {
    let mut audio = Vec::new();
    let mut frames = Vec::new();
    let mut nodes = Vec::new();
    let mut slots = Vec::new();

    if let Some(gameplay) = &project.document.gameplay {
        audio.extend(gameplay.sounds.iter().map(|path| logical_path(path)));
        for bank in &gameplay.frames {
            for (slot, path) in bank.png.iter().enumerate() {
                frames.push(logical_path(path));
                nodes.push(bank.node);
                slots.push(slot);
            }
        }
    }

    writeln!(
        entry,
        "import {{ registerScriptResources }} from \"aurora\";\nregisterScriptResources({}, {}, {}, {});",
        serde_json::to_string(&audio)?,
        serde_json::to_string(&frames)?,
        serde_json::to_string(&nodes)?,
        serde_json::to_string(&slots)?,
    )?;
    Ok(())
}

fn logical_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
