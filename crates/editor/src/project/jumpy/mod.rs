//! Install the original editable game into a newly claimed project.
use std::{fs,io::Write,path::Path};

use anyhow::Result;

macro_rules! sample_files {
    ($($path:literal),+ $(,)?) => {
        &[$(($path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/game-template/jumpy/", $path)).as_slice())),+]
    };
}

const FILES: &[(&str, &[u8])] = sample_files!(
    "assets/0.png",
    "assets/1.png",
    "assets/2.png",
    "assets/3.png",
    "assets/4.png",
    "assets/5.png",
    "assets/6.png",
    "assets/7.png",
    "assets/8.png",
    "assets/9.png",
    "assets/Jumpy.ts",
    "assets/best.png",
    "assets/die.wav",
    "assets/ground.png",
    "assets/hit.wav",
    "assets/jumpy.png",
    "assets/over.png",
    "assets/pillar.png",
    "assets/point.wav",
    "assets/ready.png",
    "assets/score.png",
    "assets/sky.png",
    "assets/swoosh.wav",
    "assets/wing.png",
    "pillars.prefab",
    "main.scene",
);

pub(super) fn create(root: &Path) -> Result<()> {
    let mut installed = Vec::new();
    for &(name, bytes) in FILES {
        let path = root.join(name);
        let result = (|| -> Result<()> {
            let mut file = tempfile::NamedTempFile::new_in(root)?;
            file.write_all(bytes)?;
            file.persist_noclobber(&path)?;
            Ok(())
        })();
        if let Err(error) = result {
            for path in installed { let _ = fs::remove_file(path); }
            return Err(error);
        }
        installed.push(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{Project, ProjectKind};
    use anyhow::Result;

    #[test]
    fn new_jumpy_project_opens_with_all_game_resources() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let root = folder.path().join("My Jumpy Bird");
        let project = Project::create(&root, ProjectKind::Jumpy)?;
        for &(name, bytes) in super::FILES {
            assert_eq!(std::fs::read(root.join(name))?, bytes);
        }
        assert_eq!(project.document.instances.len(), 4);
        assert_eq!(project.document.nodes[5].scripts[0].component, "Jumpy");
        assert!(Project::create(&root, ProjectKind::Jumpy).is_err());
        Ok(())
    }
}
