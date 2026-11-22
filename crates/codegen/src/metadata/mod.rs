//! Shared metadata parser for the editor and binding generator.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Inspector value kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PropertyType {
    /// Numeric value.
    Number,
    /// Checkbox value.
    Boolean,
    /// Text value.
    String,
    /// Scene node reference.
    Node,
    /// Prefab asset reference.
    Prefab,
}

/// Inspector field declaration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Property {
    #[serde(rename = "type")]
    /// Expected value or reference type.
    pub kind: PropertyType,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional field help.
    pub tooltip: Option<String>,
}

/// Declared fields for one component.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    /// Properties keyed by their declared name.
    pub props: BTreeMap<String, Property>,
}

/// Extracts static component metadata from TypeScript source.
pub fn extract(source: &str) -> Result<BTreeMap<String, Metadata>> {
    // The canonical registration starts a line and uses a JSON literal argument.
    // This explicit grammar keeps extraction independent of the host TS runtime.
    let mut components = BTreeMap::new();
    for (offset, _) in source.match_indices("\nregisterComponent(") {
        let args = &source[offset + "\nregisterComponent(".len()..];
        let mut json = serde_json::Deserializer::from_str(args).into_iter::<(String, Metadata)>();
        let (name, metadata) = json.next().context("Missing component metadata")??;
        ensure!(
            args[json.byte_offset()..].trim_start().starts_with(");"),
            "Expected end of registerComponent call"
        );
        ensure!(!name.trim().is_empty(), "Component name is empty");
        ensure!(
            components.insert(name, metadata).is_none(),
            "Duplicate component registration"
        );
    }
    ensure!(
        !components.is_empty(),
        "No static registerComponent calls found"
    );
    Ok(components)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_typed_metadata_and_rejects_unknown_fields() -> Result<()> {
        let source = "\nregisterComponent([\"Hello\", {\"props\":{\"target\":{\"type\":\"node\",\"tooltip\":\"Target\"}}}]);";
        let result = serde_json::to_value(extract(source)?)?;
        assert_eq!(result["Hello"]["props"]["target"]["type"], "node");
        assert!(extract(&source.replace("\"node\"", "\"missing\"")).is_err());
        assert!(extract(&format!("{source}{source}")).is_err());
        Ok(())
    }
}
