//! TypeScript syntax-tree extraction and scriptc lowering for authoring decorators.

use std::{collections::BTreeMap, ops::Range};

use anyhow::{Context, Result, bail, ensure};
use tree_sitter::{Node, Parser};

use super::{Metadata, Property, PropertyType};

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
}

impl Declared {
    fn value(kind: PropertyType) -> Self {
        Self {
            kind,
            component: None,
        }
    }
}

fn kind(value: &str) -> Option<PropertyType> {
    match value {
        "Number" | "number" => Some(PropertyType::Number),
        "Boolean" | "boolean" => Some(PropertyType::Boolean),
        "String" | "string" => Some(PropertyType::String),
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
        });
    }
    None
}

fn field_property(annotation: Node<'_>, field: Node<'_>, source: &str) -> Result<Property> {
    let arguments = arguments(annotation);
    ensure!(arguments.len() <= 1, "Expected at most one argument");
    let mut tooltip = None;
    let mut explicit = None;
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
    let declared = explicit.or_else(inferred).context(
        "Specify Number, Boolean, String, NodeRef, Prefab, AudioClip, SpriteFrame, an array of them or a component class",
    )?;
    Ok(Property {
        kind: declared.kind,
        tooltip,
        component: declared.component,
    })
}
