//! Supported and blocked build targets and their versioned builder image references.

use anyhow::Result;

/// Registry that hosts Aurora's build-toolchain images.
pub const DEFAULT_REGISTRY: &str = "ghcr.io/hedystia";

/// Repository name for builder images below the registry host.
pub const IMAGE_REPOSITORY: &str = "aurora-builder";

/// Whether a target can be produced by the current builder images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// A local container can produce and package this target.
    Supported,
    /// The target builds natively on its own host operating system.
    HostOnly {
        /// The host requirement and why a container cannot substitute for it.
        note: &'static str,
    },
    /// The target is defined, but a dependency or platform requirement blocks it.
    Blocked {
        /// Why the target cannot be produced end to end.
        reason: &'static str,
        /// The concrete next step that would unblock it.
        next: &'static str,
    },
}

/// One build target and the builder image contract for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    /// Short identifier used on the command line.
    pub id: &'static str,
    /// Rust target triple produced by the builder.
    pub triple: &'static str,
    /// Operating system and architecture recorded in the export manifest.
    pub manifest_target: &'static str,
    /// Current support status.
    pub status: Status,
    /// Cargo features enabled for the runtime build.
    pub features: &'static [&'static str],
}

impl Target {
    /// Returns the supported-target table in display order.
    pub const ALL: [Self; 4] = [
        Self {
            id: "linux",
            triple: "x86_64-unknown-linux-gnu",
            manifest_target: "linux-x86_64",
            status: Status::Supported,
            features: &[],
        },
        Self {
            id: "windows",
            triple: "x86_64-pc-windows-gnu",
            manifest_target: "windows-x86_64",
            status: Status::Blocked {
                reason: "Scripted exports need Aurora's Linux-only script IPC, and the editor's \
                         export manifest stamps the compiling host target. A container can \
                         cross-build the player but cannot yet produce a verified Windows game \
                         package.",
                next: "Port the script transport and native packaging off Linux, then add a \
                       Windows export target whose manifest records the cross target.",
            },
            features: &[],
        },
        Self {
            id: "android",
            triple: "aarch64-linux-android",
            manifest_target: "android-aarch64",
            status: Status::Blocked {
                reason: "Aurora has no Android application runtime. There is no Activity, \
                         SurfaceView, or Gradle packaging path that can host the wgpu player, and \
                         script IPC is Linux-only.",
                next: "Add a minimal Android app shell that owns the native window and surface, \
                       then package the built player with the command-line SDK and NDK.",
            },
            features: &[],
        },
        Self {
            id: "macos",
            triple: "aarch64-apple-darwin",
            manifest_target: "macos-aarch64",
            status: Status::HostOnly {
                note: "Builds natively for the host architecture (macos-aarch64 or macos-x86_64) \
                       on an Apple-branded macOS host. Apple's SLA permits macOS virtualization \
                       only on Apple hardware, so Linux container images such as dockurr/macos \
                       are not supported. Scripted exports use the shared-memory Unix transport.",
            },
            features: &[],
        },
    ];

    /// Finds a target by its command-line identifier.
    pub fn parse(value: &str) -> Result<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|target| target.id == normalized)
            .ok_or_else(|| {
                let names = Self::ALL
                    .iter()
                    .map(|target| target.id)
                    .collect::<Vec<_>>()
                    .join(", ");
                anyhow::anyhow!("Unknown build target {value:?}. Available targets: {names}")
            })
    }

    /// Returns the full image reference for an engine version.
    pub fn image(&self, version: &str) -> String {
        format!(
            "{DEFAULT_REGISTRY}/{IMAGE_REPOSITORY}:{version}-{}",
            self.id
        )
    }

    /// Fails with the precise blocker when the target cannot be built locally.
    pub fn ensure_supported(&self) -> Result<()> {
        match self.status {
            Status::Supported | Status::HostOnly { .. } => Ok(()),
            Status::Blocked { reason, next } => Err(anyhow::anyhow!(
                "Target {} is not buildable with local containers: {reason}\nNext step: {next}",
                self.id
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_table_has_unique_ids_and_triples() {
        let mut ids = std::collections::BTreeSet::new();
        let mut triples = std::collections::BTreeSet::new();
        for target in Target::ALL {
            assert!(ids.insert(target.id), "duplicate target id {}", target.id);
            assert!(
                triples.insert(target.triple),
                "duplicate triple {}",
                target.triple
            );
        }
    }

    #[test]
    fn parses_ids_case_insensitively() -> Result<()> {
        assert_eq!(Target::parse("Linux")?.id, "linux");
        assert_eq!(Target::parse("WINDOWS")?.id, "windows");
        assert!(Target::parse("web").is_err());
        Ok(())
    }

    #[test]
    fn linux_is_container_supported_and_macos_is_host_only() -> Result<()> {
        assert_eq!(Target::parse("linux")?.status, Status::Supported);
        assert!(matches!(
            Target::parse("macos")?.status,
            Status::HostOnly { .. }
        ));
        assert!(Target::parse("macos")?.ensure_supported().is_ok());
        for id in ["windows", "android"] {
            assert!(Target::parse(id)?.ensure_supported().is_err());
        }
        Ok(())
    }

    #[test]
    fn image_reference_is_versioned_per_target() -> Result<()> {
        let target = Target::parse("linux")?;
        assert_eq!(
            target.image("0.1.0"),
            "ghcr.io/hedystia/aurora-builder:0.1.0-linux"
        );
        Ok(())
    }
}
