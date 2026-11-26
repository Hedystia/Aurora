//! Shared metadata parser for the editor and binding generator.

mod decorators;

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Inspector value kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PropertyType {
    /// Numeric value.
    Number,
    /// Integer value.
    Integer,
    /// Checkbox value.
    Boolean,
    /// Text value.
    String,
    /// Multiline text value.
    Text,
    /// Two numeric values.
    Vector2,
    /// Three numeric values.
    Vector3,
    /// Two-dimensional size.
    Size,
    /// Rectangle position and size as four numeric values.
    Rect,
    /// RGBA color value.
    Color,
    /// String selected from a declared set of choices.
    Enum,
    /// Scene node reference.
    Node,
    /// Prefab asset reference.
    Prefab,
    /// Audio clip asset reference.
    AudioClip,
    /// Sprite frame image asset reference.
    SpriteFrame,
    /// Numeric array.
    NumberArray,
    /// Boolean array.
    BooleanArray,
    /// Text array.
    StringArray,
    /// Scene node reference array.
    NodeArray,
    /// Prefab asset reference array.
    PrefabArray,
    /// Audio clip asset reference array.
    AudioClipArray,
    /// Sprite frame image asset reference array.
    SpriteFrameArray,
    /// Reference to another script component in the same document.
    Component,
    /// Unsigned 32-bit layer or flag mask.
    Bitmask,
}

/// Restricts a scene-node reference to nodes with the declared built-in kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeFilter {
    /// Node has an authored Text visual.
    Text,
    /// Node has an authored sprite or image-based visual.
    Image,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Component class name for [`PropertyType::Component`] fields.
    pub component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Allowed string values for [`PropertyType::Enum`].
    pub choices: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "is_false")]
    /// Whether the Inspector should display but not edit this value.
    pub readonly: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional built-in node type filter for scene-node references.
    pub node_filter: Option<NodeFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Lower numeric bound for a number or integer property.
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Upper numeric bound for a number or integer property.
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Numeric edit increment.
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional Inspector section heading.
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    /// Whether a bounded numeric value uses a slider.
    pub slider: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// Declared fields for one component.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    /// TypeScript export name for a decorated component. Legacy registrations imply a named export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<String>,
    /// Startup and tick priority. Smaller values are dispatched first.
    #[serde(default, skip_serializing_if = "is_default_order")]
    pub order: i32,
    /// Properties keyed by their declared name.
    pub props: BTreeMap<String, Property>,
}

fn is_default_order(order: &i32) -> bool {
    *order == 0
}

/// Extracts static component metadata from TypeScript source.
pub fn extract(source: &str) -> Result<BTreeMap<String, Metadata>> {
    // The canonical registration starts a line and uses a JSON literal argument.
    // This explicit grammar keeps extraction independent of the host TS runtime.
    let mut components = decorators::scan(source)?.components;
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
        "No exported @ccclass component found"
    );
    Ok(components)
}

/// Erases Aurora authoring annotations while preserving source positions for diagnostics.
pub fn lower(source: &str) -> Result<String> {
    let mut bytes = source.as_bytes().to_vec();
    for range in decorators::scan(source)?.annotations {
        for byte in &mut bytes[range] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    Ok(String::from_utf8(bytes)?)
}

/// Lowers authoring annotations and routes global timer calls through Aurora's fixed-tick scheduler.
pub fn lower_runtime(source: &str) -> Result<String> {
    let lowered = lower(source)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;
    let tree = parser
        .parse(&lowered, None)
        .context("Could not parse TypeScript timer calls")?;
    let mut replacements = Vec::new();
    let mut imports = Vec::new();
    collect_timer_calls(tree.root_node(), &lowered, &mut replacements, &mut imports);
    if imports.is_empty() {
        return Ok(lowered);
    }

    let mut output = lowered;
    for (range, name) in replacements.into_iter().rev() {
        output.replace_range(range, &name);
    }
    let import = format!("import {{ {} }} from \"aurora\";\n", imports.join(", "));
    output.insert_str(0, &import);
    Ok(output)
}

fn collect_timer_calls(
    node: tree_sitter::Node<'_>,
    source: &str,
    replacements: &mut Vec<(std::ops::Range<usize>, String)>,
    imports: &mut Vec<String>,
) {
    const TIMERS: [(&str, &str); 6] = [
        ("setTimeout", "auroraSetTimeout"),
        ("setInterval", "auroraSetInterval"),
        ("clearTimeout", "auroraClearTimer"),
        ("clearInterval", "auroraClearTimer"),
        ("requestAnimationFrame", "auroraRequestAnimationFrame"),
        ("cancelAnimationFrame", "auroraClearTimer"),
    ];

    if node.kind() == "call_expression"
        && let Some(function) = node.child_by_field_name("function")
        && function.kind() == "identifier"
    {
        let name = &source[function.byte_range()];
        if let Some((_, replacement)) = TIMERS
            .iter()
            .find(|(timer, _)| *timer == name)
            .filter(|_| !timer_is_shadowed(node, name, source))
        {
            let mut alias = format!("__aurora_{replacement}");
            while source.contains(&alias) {
                alias.push('_');
            }
            replacements.push((function.byte_range(), alias.clone()));
            let binding = format!("{replacement} as {alias}");
            if !imports.contains(&binding) {
                imports.push(binding);
            }
        }
    }

    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            collect_timer_calls(child, source, replacements, imports);
        }
    }
}

fn timer_is_shadowed(call: tree_sitter::Node<'_>, name: &str, source: &str) -> bool {
    let mut ancestor = Some(call);
    while let Some(node) = ancestor {
        if is_scope(node.kind()) && scope_binds_name(node, node, name, source) {
            return true;
        }
        ancestor = node.parent();
    }
    false
}

fn is_scope(kind: &str) -> bool {
    matches!(
        kind,
        "program"
            | "statement_block"
            | "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "arrow_function"
            | "method_definition"
            | "class_body"
            | "catch_clause"
            | "for_statement"
            | "for_in_statement"
    )
}

fn scope_binds_name(
    node: tree_sitter::Node<'_>,
    scope: tree_sitter::Node<'_>,
    name: &str,
    source: &str,
) -> bool {
    if node.id() != scope.id() && is_scope(node.kind()) {
        if matches!(node.kind(), "function_declaration" | "class_declaration")
            && node
                .child_by_field_name("name")
                .is_some_and(|binding| &source[binding.byte_range()] == name)
        {
            return true;
        }
        return false;
    }

    match node.kind() {
        "variable_declarator" => {
            if node
                .child_by_field_name("name")
                .is_some_and(|binding| pattern_binds_name(binding, name, source))
            {
                return true;
            }
        }
        "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "class_declaration" => {
            if node
                .child_by_field_name("name")
                .is_some_and(|binding| &source[binding.byte_range()] == name)
            {
                return true;
            }
        }
        "required_parameter" | "optional_parameter" | "rest_pattern" => {
            if pattern_binds_name(node, name, source) {
                return true;
            }
        }
        "import_specifier" => {
            let binding = node
                .child_by_field_name("alias")
                .or_else(|| node.child_by_field_name("name"));
            if binding.is_some_and(|binding| &source[binding.byte_range()] == name) {
                return true;
            }
        }
        "catch_clause"
            if node
                .child_by_field_name("parameter")
                .is_some_and(|binding| pattern_binds_name(binding, name, source)) =>
        {
            return true;
        }
        _ => {}
    }

    (0..node.child_count()).any(|index| {
        node.child(index)
            .is_some_and(|child| scope_binds_name(child, scope, name, source))
    })
}

fn pattern_binds_name(pattern: tree_sitter::Node<'_>, name: &str, source: &str) -> bool {
    if pattern.kind() == "identifier" && &source[pattern.byte_range()] == name {
        return true;
    }
    (0..pattern.child_count()).any(|index| {
        pattern
            .child(index)
            .is_some_and(|child| pattern_binds_name(child, name, source))
    })
}

/// TypeScript declarations for the public game scripting SDK.
pub const SDK_DECLARATIONS: &str = include_str!("../sdk.d.ts");
/// SDK declaration modules referenced by [`SDK_DECLARATIONS`].
pub const SDK_MODULES: [(&str, &str); 4] = [
    ("index.d.ts", include_str!("../sdk/index.d.ts")),
    ("events.d.ts", include_str!("../sdk/events.d.ts")),
    ("resources.d.ts", include_str!("../sdk/resources.d.ts")),
    ("scheduler.d.ts", include_str!("../sdk/scheduler.d.ts")),
];

/// Emits the TypeScript wire constants from the Rust protocol definition.
pub fn protocol_typescript() -> Result<String> {
    let mut protocol = format!(
        "// Generated by aurora-codegen.\nexport const protocol = {};\nexport const frameBytes = {};\n",
        script_api::VERSION,
        script_api::ENVELOPE_BYTES
    );
    use std::fmt::Write as _;
    writeln!(
        protocol,
        "export const bindingEnabled = {};",
        script_api::BINDING_ENABLED
    )?;
    for kind in [
        script_api::Kind::Start,
        script_api::Kind::Update,
        script_api::Kind::FixedUpdate,
        script_api::Kind::Ack,
        script_api::Kind::SetPosition,
        script_api::Kind::CollisionEnter,
        script_api::Kind::CollisionExit,
        script_api::Kind::KeyDown,
        script_api::Kind::TouchStart,
        script_api::Kind::SetLinearVelocity,
        script_api::Kind::PlayOneShot,
        script_api::Kind::AnimationEvent,
        script_api::Kind::Pause,
        script_api::Kind::Resume,
        script_api::Kind::ChangeScene,
        script_api::Kind::FlushCommands,
        script_api::Kind::MoveNode,
        script_api::Kind::ShowNode,
        script_api::Kind::SelectFrame,
        script_api::Kind::QueueSound,
        script_api::Kind::RecordScore,
        script_api::Kind::RestoreScore,
        script_api::Kind::Load,
        script_api::Kind::LateUpdate,
        script_api::Kind::QueueCreateNode,
        script_api::Kind::QueueInstantiatePrefab,
        script_api::Kind::QueueReparentNode,
        script_api::Kind::QueueDestroyNode,
        script_api::Kind::Enable,
        script_api::Kind::Disable,
        script_api::Kind::Destroy,
        script_api::Kind::Schedule,
        script_api::Kind::PollEnabled,
        script_api::Kind::RegisterPrefabNode,
        script_api::Kind::RegisterPrefabComponent,
        script_api::Kind::ReleasePrefabNode,
        script_api::Kind::ReleasePrefabComponent,
        script_api::Kind::UpdatePrefabParent,
        script_api::Kind::ProcessResources,
        script_api::Kind::QueueAddComponent,
        script_api::Kind::QueueRemoveComponent,
        script_api::Kind::RegisterComponent,
        script_api::Kind::PointerTouch,
        script_api::Kind::KeyUp,
        script_api::Kind::PointerMove,
        script_api::Kind::PointerEnd,
        script_api::Kind::PointerCancel,
        script_api::Kind::PointerNodeMove,
        script_api::Kind::PointerNodeEnd,
        script_api::Kind::PointerNodeCancel,
        script_api::Kind::SceneStart,
        script_api::Kind::MouseWheel,
        script_api::Kind::PointerNodeWheel,
        script_api::Kind::NodeTransform,
        script_api::Kind::NodeScaleRotation,
        script_api::Kind::Tween,
        script_api::Kind::ScaleNode,
        script_api::Kind::RotateNode,
        script_api::Kind::SetText,
        script_api::Kind::SetLoading,
        script_api::Kind::ConfigSet,
        script_api::Kind::ConfigValue,
        script_api::Kind::SecretValue,
        script_api::Kind::NodeTransform3D,
        script_api::Kind::NodeRotation3D,
        script_api::Kind::NodeScale3D,
        script_api::Kind::SetLinearVelocity3D,
        script_api::Kind::CollisionEnter3D,
        script_api::Kind::CollisionExit3D,
        script_api::Kind::LoadSprite,
        script_api::Kind::AssetLoaded,
        script_api::Kind::ReleaseAsset,
        script_api::Kind::LoadAudio,
        script_api::Kind::LoadPrefab,
        script_api::Kind::SetAngularVelocity3D,
        script_api::Kind::Raycast3D,
        script_api::Kind::RaycastResult,
        script_api::Kind::NodeWorldTransform3D,
        script_api::Kind::NodeWorldRotation3D,
        script_api::Kind::OverlapQuery,
        script_api::Kind::OverlapHit,
        script_api::Kind::SetCamera3D,
    ] {
        writeln!(protocol, "export const kind{kind:?} = {};", kind as u32)?;
    }
    Ok(protocol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorated_components_extract_literal_options_and_export_identity() -> Result<()> {
        let source = r#"
import { ccclass, property, Component, NodeRef, TextRef, ImageRef, Ref } from "aurora";
// @ccclass("Fake")
@ccclass('Player Mover')
export default class Actual extends Component {
  @property({ tooltip: 'It\'s a "speed".', type: Number })
  speed = 30;
  @property enabled = true;
  @property target: NodeRef | null = null;
  @property(Health) health: Health | null = null;
  @property({ type: Health, tooltip: 'Hits' }) hits: Health | null = null;
  @property clip: AudioClip | null = null;
  @property(SpriteFrame) frame: SpriteFrame | null = null;
  @property([Number]) speeds = [1, 2];
  @property({ type: [NodeRef], tooltip: 'Targets' }) targets: NodeRef[] = [];
  @property([Prefab]) prefabs: Prefab[] = [];
  @property([AudioClip]) clips: AudioClip[] = [];
  @property([SpriteFrame]) frames = [];
  @property tags: string[] = [];
  @property(Integer) lives = 3;
  @property(Text) helpText = "Ready";
  @property(Vector2) offset: [number, number] = [1, 2];
  @property(Color) tint: [number, number, number, number] = [255, 255, 255, 255];
  @property(Image) image: SpriteFrame | null = null;
  @property({ enum: ["easy", "hard"], readonly: true }) mode: string = "easy";
  @property({ type: Ref.Text }) label: TextRef | null = null;
  @property(Ref.Image) imageNode: ImageRef | null = null;
  @property(Ref.Node) anyNode: NodeRef | null = null;
  @property(Ref.Prefab) template: Prefab | null = null;
  @property(Ref.AudioClip) sound: AudioClip | null = null;
  @property(Ref.SpriteFrame) imageFrame: SpriteFrame | null = null;
  @property(Vector3) position3: [number, number, number] = [0, 0, 0];
  @property(Size) dimensions: [number, number] = [100, 50];
  @property(Rect) bounds: [number, number, number, number] = [0, 0, 10, 10];
  @property(Bitmask) layers = 3;
  @property({ type: Number, min: 0, max: 100, step: 0.5, slider: true, group: "Movement" })
  opacity = 100;
}
"#;
        let metadata = extract(source)?;
        let mover = &metadata["Player Mover"];
        assert_eq!(metadata.len(), 1);
        assert_eq!(mover.export.as_deref(), Some("default"));
        assert_eq!(
            mover.props["speed"].tooltip.as_deref(),
            Some("It's a \"speed\".")
        );
        assert_eq!(mover.props["enabled"].kind, PropertyType::Boolean);
        assert_eq!(mover.props["target"].kind, PropertyType::Node);
        assert_eq!(mover.props["health"].kind, PropertyType::Component);
        assert_eq!(mover.props["health"].component.as_deref(), Some("Health"));
        assert_eq!(mover.props["hits"].component.as_deref(), Some("Health"));
        assert_eq!(mover.props["hits"].tooltip.as_deref(), Some("Hits"));
        assert_eq!(mover.props["clip"].kind, PropertyType::AudioClip);
        assert_eq!(mover.props["frame"].kind, PropertyType::SpriteFrame);
        assert_eq!(mover.props["speeds"].kind, PropertyType::NumberArray);
        assert_eq!(mover.props["targets"].kind, PropertyType::NodeArray);
        assert_eq!(mover.props["prefabs"].kind, PropertyType::PrefabArray);
        assert_eq!(mover.props["clips"].kind, PropertyType::AudioClipArray);
        assert_eq!(mover.props["frames"].kind, PropertyType::SpriteFrameArray);
        assert_eq!(mover.props["tags"].kind, PropertyType::StringArray);
        assert_eq!(mover.props["lives"].kind, PropertyType::Integer);
        assert_eq!(mover.props["helpText"].kind, PropertyType::Text);
        assert_eq!(mover.props["offset"].kind, PropertyType::Vector2);
        assert_eq!(mover.props["tint"].kind, PropertyType::Color);
        assert_eq!(mover.props["image"].kind, PropertyType::SpriteFrame);
        assert_eq!(mover.props["mode"].kind, PropertyType::Enum);
        assert_eq!(
            mover.props["mode"].choices.as_deref(),
            Some(["easy".to_owned(), "hard".to_owned()].as_slice())
        );
        assert!(mover.props["mode"].readonly);
        assert_eq!(mover.props["label"].kind, PropertyType::Node);
        assert_eq!(mover.props["label"].node_filter, Some(NodeFilter::Text));
        assert_eq!(
            mover.props["imageNode"].node_filter,
            Some(NodeFilter::Image)
        );
        assert_eq!(mover.props["anyNode"].node_filter, None);
        assert_eq!(mover.props["template"].kind, PropertyType::Prefab);
        assert_eq!(mover.props["sound"].kind, PropertyType::AudioClip);
        assert_eq!(mover.props["imageFrame"].kind, PropertyType::SpriteFrame);
        assert_eq!(mover.props["position3"].kind, PropertyType::Vector3);
        assert_eq!(mover.props["dimensions"].kind, PropertyType::Size);
        assert_eq!(mover.props["bounds"].kind, PropertyType::Rect);
        assert_eq!(mover.props["layers"].kind, PropertyType::Bitmask);
        assert_eq!(mover.props["opacity"].min, Some(0.0));
        assert_eq!(mover.props["opacity"].max, Some(100.0));
        assert_eq!(mover.props["opacity"].step, Some(0.5));
        assert_eq!(mover.props["opacity"].group.as_deref(), Some("Movement"));
        assert!(mover.props["opacity"].slider);
        let lowered = lower(source)?;
        assert_eq!(lowered.len(), source.len());
        assert_eq!(lowered.lines().count(), source.lines().count());
        assert!(lowered.contains("export default class Actual"));
        assert!(lowered.contains("target: NodeRef | null = null;"));
        assert!(!lowered.contains("@property"));
        assert!(extract(&source.replace("type: Number", "type: 3")).is_err());
        let invalid_slider = r#"
@ccclass("Invalid")
class Invalid {
  @property({ type: Number, slider: true }) value = 0;
}
"#;
        assert!(extract(invalid_slider).is_err());
        let invalid_bounds = r#"
@ccclass("Invalid")
class Invalid {
  @property({ type: Number, min: 10, max: 0 }) value = 0;
}
"#;
        assert!(extract(invalid_bounds).is_err());
        let invalid_reference_type = r#"
@ccclass("Invalid")
export class Invalid {
  @property({ type: Ref.Text }) label: typeof Ref.Text | null = null;
}
"#;
        let Err(error) = extract(invalid_reference_type) else {
            anyhow::bail!("Invalid TextRef field annotation was accepted");
        };
        assert!(format!("{error:#}").contains("TextRef"), "{error:#}");
        Ok(())
    }

    #[test]
    fn extracts_typed_metadata_and_rejects_unknown_fields() -> Result<()> {
        let source = "\nregisterComponent([\"Hello\", {\"props\":{\"target\":{\"type\":\"node\",\"tooltip\":\"Target\"}}}]);";
        let result = serde_json::to_value(extract(source)?)?;
        assert_eq!(result["Hello"]["props"]["target"]["type"], "node");
        assert!(extract(&source.replace("\"node\"", "\"missing\"")).is_err());
        assert!(extract(&format!("{source}{source}")).is_err());
        Ok(())
    }

    #[test]
    fn runtime_lowering_routes_global_timer_calls_through_aurora() -> Result<()> {
        let source = r#"
setTimeout(() => setInterval(() => clearInterval(1), 16), 5000);
clearTimeout(2);
requestAnimationFrame((time) => console.log(time));
cancelAnimationFrame(3);
object.setTimeout(() => {});
function local(setTimeout: () => void): void {
  setTimeout();
}
"#;
        let lowered = lower_runtime(source)?;
        assert!(lowered.starts_with("import { "));
        assert!(lowered.contains("auroraSetTimeout as __aurora_auroraSetTimeout"));
        assert!(lowered.contains("auroraSetInterval as __aurora_auroraSetInterval"));
        assert!(lowered.contains("auroraClearTimer as __aurora_auroraClearTimer"));
        assert!(
            lowered.contains("auroraRequestAnimationFrame as __aurora_auroraRequestAnimationFrame")
        );
        assert!(lowered.contains("__aurora_auroraSetTimeout(()"));
        assert!(lowered.contains("object.setTimeout(()"));
        assert!(lowered.contains("function local(setTimeout: () => void): void"));
        assert!(lowered.contains("  setTimeout();"));
        let imported = lower_runtime(
            r#"import { setTimeout } from "node:timers";
setTimeout(() => {}, 1);"#,
        )?;
        assert!(imported.contains(r#"import { setTimeout } from "node:timers";"#));
        assert!(!imported.contains("auroraSetTimeout"));
        Ok(())
    }
}
