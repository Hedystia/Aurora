//! TypeScript syntax-tree extraction and scriptc lowering for authoring decorators.

use std::{collections::BTreeMap, ops::Range};

use anyhow::{Context, Result, bail, ensure};
use tree_sitter::{Node, Parser};

use super::{Metadata, NodeFilter, Property, PropertyType};

pub(super) struct Decorated {
    pub components: BTreeMap<String, Metadata>,
    pub annotations: Vec<Range<usize>>,
}

pub(super) fn scan(source: &str) -> Result<Decorated> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;
    let tree = parser
        .parse(source, None)
        .context("Could not parse TypeScript")?;
    let root = tree.root_node();
    let mut result = Decorated {
        components: BTreeMap::new(),
        annotations: Vec::new(),
    };
    for export in children(root).filter(|node| node.kind() == "export_statement") {
        let Some(class) = export
            .child_by_field_name("declaration")
            .or_else(|| export.child_by_field_name("value"))
            .filter(|node| matches!(node.kind(), "class_declaration" | "class"))
        else {
            continue;
        };
        let Some(annotation) = annotations(export)
            .chain(annotations(class))
            .find(|node| decorator_name(*node, source) == "ccclass")
        else {
            continue;
        };
        ensure!(
            !class.has_error(),
            "Invalid decorated component near line {}",
            class.start_position().row + 1
        );
        let class_name = class
            .child_by_field_name("name")
            .map(|name| text(name, source));
        let class_arguments = arguments(annotation);
        ensure!(
            class_arguments.len() <= 1,
            "@ccclass accepts a single literal name"
        );
        let name = if let Some(&value) = class_arguments.first() {
            string(value, source)?
        } else {
            class_name
                .context("@ccclass needs a name for an anonymous class")?
                .to_owned()
        };
        ensure!(!name.trim().is_empty(), "Component name is empty");
        let is_default = (0..export.child_count())
            .filter_map(|index| export.child(index))
            .any(|node| node.kind() == "default");
        let symbol = if is_default {
            "default"
        } else {
            class_name.context("Component has no named export")?
        };
        let mut metadata = Metadata {
            props: BTreeMap::new(),
            export: Some(symbol.into()),
            order: 0,
        };
        result.annotations.push(annotation.byte_range());
        let mut priorities = annotations(export)
            .chain(annotations(class))
            .filter(|node| decorator_name(*node, source) == "executionOrder");
        if let Some(priority) = priorities.next() {
            ensure!(
                priorities.next().is_none(),
                "Duplicate @executionOrder on {name}"
            );
            let args = arguments(priority);
            ensure!(args.len() == 1, "@executionOrder expects one integer");
            metadata.order = text(args[0], source).parse().with_context(|| {
                format!("@executionOrder on {name} expects a signed 32-bit integer")
            })?;
            result.annotations.push(priority.byte_range());
        }
        let body = class
            .child_by_field_name("body")
            .context("Missing component body")?;
        for field in children(body).filter(|node| node.kind() == "public_field_definition") {
            for annotation in
                annotations(field).filter(|node| decorator_name(*node, source) == "property")
            {
                let key = field
                    .child_by_field_name("name")
                    .filter(|name| name.kind() == "property_identifier")
                    .context("@property needs a named field")?;
                let field_name = text(key, source);
                ensure!(
                    !children(field).any(|node| node.kind() == "accessibility_modifier"
                        && text(node, source) != "public")
                        && !(0..field.child_count())
                            .filter_map(|index| field.child(index))
                            .any(|node| node.kind() == "static"),
                    "@property field {field_name} must be a public instance field"
                );
                let property = field_property(annotation, field, source)
                    .with_context(|| format!("@property {field_name} on {name}"))?;
                ensure!(
                    metadata.props.insert(field_name.into(), property).is_none(),
                    "Duplicate @property {field_name}"
                );
                result.annotations.push(annotation.byte_range());
            }
        }
        ensure!(
            result.components.insert(name, metadata).is_none(),
            "Duplicate component name"
        );
    }
    Ok(result)
}

fn children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    (0..node.named_child_count()).filter_map(move |index| node.named_child(index as u32))
}

fn annotations(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    children(node).filter(|child| child.kind() == "decorator")
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn decorator_name<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    let Some(expression) = node.named_child(0) else {
        return "";
    };
    let function = expression
        .child_by_field_name("function")
        .unwrap_or(expression);
    text(
        function.child_by_field_name("property").unwrap_or(function),
        source,
    )
}

fn arguments(node: Node<'_>) -> Vec<Node<'_>> {
    node.named_child(0)
        .and_then(|call| call.child_by_field_name("arguments"))
        .into_iter()
        .flat_map(children)
        .filter(|child| child.kind() != "comment")
        .collect()
}

fn string(node: Node<'_>, source: &str) -> Result<String> {
    ensure!(node.kind() == "string", "Expected a string literal");
    let value = text(node, source);
    if value.starts_with('"') {
        return Ok(serde_json::from_str(value)?);
    }
    // Decode the common single-quoted form with the same JSON escape rules.
    let inner = &value[1..value.len() - 1];
    let mut quoted = String::from("\"");
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => quoted.push_str("\\\""),
            '\\' => match chars.next().context("Incomplete string escape")? {
                '\'' => quoted.push('\''),
                escaped => {
                    quoted.push('\\');
                    quoted.push(escaped);
                }
            },
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    Ok(serde_json::from_str(&quoted)?)
}

/// One resolved property declaration before it is stored in metadata.
struct Declared {
    kind: PropertyType,
    component: Option<String>,
    node_filter: Option<NodeFilter>,
}

impl Declared {
    fn value(kind: PropertyType) -> Self {
        Self {
            kind,
            component: None,
            node_filter: None,
        }
    }

    fn text_node() -> Self {
        Self {
            kind: PropertyType::Node,
            component: None,
            node_filter: Some(NodeFilter::Text),
        }
    }

    fn image_node() -> Self {
        Self {
            kind: PropertyType::Node,
            component: None,
            node_filter: Some(NodeFilter::Image),
        }
    }
}

fn kind(value: &str) -> Option<PropertyType> {
    match value {
        "Number" | "number" => Some(PropertyType::Number),
        "Integer" | "integer" => Some(PropertyType::Integer),
        "Boolean" | "boolean" => Some(PropertyType::Boolean),
        "String" | "string" => Some(PropertyType::String),
        "Text" | "text" => Some(PropertyType::Text),
        "Vector2" | "vector2" => Some(PropertyType::Vector2),
        "Vector3" | "vector3" => Some(PropertyType::Vector3),
        "Size" | "size" => Some(PropertyType::Size),
        "Rect" | "rect" => Some(PropertyType::Rect),
        "Color" | "color" => Some(PropertyType::Color),
        "Bitmask" | "bitmask" => Some(PropertyType::Bitmask),
        "Image" | "image" => Some(PropertyType::SpriteFrame),
        "NodeRef" => Some(PropertyType::Node),
        "Prefab" => Some(PropertyType::Prefab),
        "AudioClip" => Some(PropertyType::AudioClip),
        "SpriteFrame" => Some(PropertyType::SpriteFrame),
        _ => None,
    }
}

fn array_kind(value: &str) -> Option<PropertyType> {
    match kind(value)? {
        PropertyType::Number => Some(PropertyType::NumberArray),
        PropertyType::Boolean => Some(PropertyType::BooleanArray),
        PropertyType::String => Some(PropertyType::StringArray),
        PropertyType::Node => Some(PropertyType::NodeArray),
        PropertyType::Prefab => Some(PropertyType::PrefabArray),
        PropertyType::AudioClip => Some(PropertyType::AudioClipArray),
        PropertyType::SpriteFrame => Some(PropertyType::SpriteFrameArray),
        _ => None,
    }
}

fn declared_kind(value: &str) -> Option<Declared> {
    let value = value.trim();
    match value {
        "Ref.Node" => return Some(Declared::value(PropertyType::Node)),
        "Ref.Text" | "TextRef" => return Some(Declared::text_node()),
        "Ref.Image" | "ImageRef" => return Some(Declared::image_node()),
        "Ref.Prefab" => return Some(Declared::value(PropertyType::Prefab)),
        "Ref.AudioClip" => return Some(Declared::value(PropertyType::AudioClip)),
        "Ref.SpriteFrame" => return Some(Declared::value(PropertyType::SpriteFrame)),
        _ => {}
    }
    if let Some(inner) = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        return array_kind(inner).map(Declared::value);
    }
    if let Some(inner) = value.strip_suffix("[]") {
        return array_kind(inner).map(Declared::value);
    }
    if let Some(kind) = kind(value) {
        return Some(Declared::value(kind));
    }
    // Any other bare identifier names a component class in the same project.
    let mut chars = value.chars();
    let first = chars.next()?;
    if (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Some(Declared {
            kind: PropertyType::Component,
            component: Some(value.to_owned()),
            node_filter: None,
        });
    }
    None
}

fn field_property(annotation: Node<'_>, field: Node<'_>, source: &str) -> Result<Property> {
    let arguments = arguments(annotation);
    ensure!(arguments.len() <= 1, "Expected at most one argument");
    let mut tooltip = None;
    let mut explicit = None;
    let mut choices = None;
    let mut readonly = false;
    let mut min = None;
    let mut max = None;
    let mut step = None;
    let mut group = None;
    let mut slider = false;
    if let Some(&argument) = arguments.first() {
        if argument.kind() == "object" {
            for pair in children(argument).filter(|node| node.kind() != "comment") {
                let key = pair
                    .child_by_field_name("key")
                    .context("Expected a literal option")?;
                let value = pair
                    .child_by_field_name("value")
                    .context("Missing option value")?;
                match text(key, source) {
                    "type" => {
                        explicit = Some(
                            declared_kind(text(value, source))
                                .context("Unsupported property type")?,
                        )
                    }
                    "enum" => {
                        let values = children(value)
                            .filter(|node| node.kind() != "comment")
                            .map(|node| string(node, source))
                            .collect::<Result<Vec<_>>>()?;
                        ensure!(
                            !values.is_empty()
                                && values.len() <= 256
                                && values.iter().all(|value| !value.trim().is_empty())
                                && values
                                    .iter()
                                    .collect::<std::collections::BTreeSet<_>>()
                                    .len()
                                    == values.len(),
                            "Enum choices must be unique nonempty strings"
                        );
                        choices = Some(values);
                    }
                    "readonly" => {
                        readonly = match text(value, source) {
                            "true" => true,
                            "false" => false,
                            _ => bail!("readonly must be a boolean literal"),
                        }
                    }
                    "min" => min = Some(text(value, source).parse::<f64>()?),
                    "max" => max = Some(text(value, source).parse::<f64>()?),
                    "step" => step = Some(text(value, source).parse::<f64>()?),
                    "group" => group = Some(string(value, source)?),
                    "slider" => {
                        slider = match text(value, source) {
                            "true" => true,
                            "false" => false,
                            _ => bail!("slider must be a boolean literal"),
                        }
                    }
                    "tooltip" => tooltip = Some(string(value, source)?),
                    option => bail!("Unsupported property option {option}"),
                }
            }
        } else {
            explicit =
                Some(declared_kind(text(argument, source)).context("Unsupported property type")?);
        }
    }
    let inferred = || {
        if let Some(annotation) = field.child_by_field_name("type") {
            let mut variants = text(annotation, source)
                .trim_start_matches(':')
                .split('|')
                .map(str::trim)
                .filter(|variant| !matches!(*variant, "null" | "undefined"));
            let inferred = declared_kind(variants.next()?);
            return if variants.next().is_none() {
                inferred
            } else {
                None
            };
        }
        match field.child_by_field_name("value")?.kind() {
            "number" => Some(Declared::value(PropertyType::Number)),
            "true" | "false" => Some(Declared::value(PropertyType::Boolean)),
            "string" => Some(Declared::value(PropertyType::String)),
            _ => None,
        }
    };
    let declared = explicit
        .or_else(inferred)
        .or_else(|| choices.as_ref().map(|_| Declared::value(PropertyType::Enum)))
        .context(
            "Specify Number, Integer, Boolean, String, Text, Vector2, Vector3, Size, Rect, Color, Bitmask, NodeRef, Prefab, AudioClip, SpriteFrame, an array of them, enum choices, or a component class",
        )?;
    if declared.kind == PropertyType::Node
        && let Some(annotation) = field.child_by_field_name("type")
    {
        let expected = match declared.node_filter {
            Some(NodeFilter::Text) => "TextRef",
            Some(NodeFilter::Image) => "ImageRef",
            None => "NodeRef",
        };
        let mut variants = text(annotation, source)
            .trim_start_matches(':')
            .split('|')
            .map(str::trim)
            .filter(|variant| !matches!(*variant, "null" | "undefined"));
        ensure!(
            variants.next() == Some(expected) && variants.next().is_none(),
            "Scene-node references must use {expected} as their value type, not the Ref marker type"
        );
    }
    let kind = if choices.is_some() {
        ensure!(
            declared.kind == PropertyType::String || declared.kind == PropertyType::Enum,
            "Enum choices require a string property"
        );
        PropertyType::Enum
    } else {
        declared.kind
    };
    if min.is_some() || max.is_some() || step.is_some() || slider {
        ensure!(
            matches!(kind, PropertyType::Number | PropertyType::Integer),
            "Numeric constraints are only supported on number and integer properties"
        );
        ensure!(
            min.is_none_or(f64::is_finite) && max.is_none_or(f64::is_finite),
            "Numeric bounds must be finite"
        );
        ensure!(
            min.zip(max)
                .is_none_or(|(minimum, maximum)| minimum <= maximum),
            "Minimum must not exceed maximum"
        );
        ensure!(
            step.is_none_or(|value| value.is_finite() && value > 0.0),
            "Step must be a finite positive number"
        );
        if kind == PropertyType::Integer {
            ensure!(
                min.is_none_or(|value| value.fract() == 0.0)
                    && max.is_none_or(|value| value.fract() == 0.0)
                    && step.is_none_or(|value| value.fract() == 0.0),
                "Integer bounds and steps must be whole numbers"
            );
        }
    }
    if slider {
        ensure!(
            min.is_some() && max.is_some(),
            "Slider properties need min and max"
        );
    }
    ensure!(
        group.as_ref().is_none_or(|value| !value.trim().is_empty()),
        "Property group must not be empty"
    );
    Ok(Property {
        kind,
        tooltip,
        component: declared.component,
        choices,
        readonly,
        node_filter: declared.node_filter,
        min,
        max,
        step,
        group,
        slider,
    })
}
