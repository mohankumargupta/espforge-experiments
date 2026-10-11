//! The shared checker for the extensible sections (`components`, `devices`).
//!
//! Each element is looked up by tag in a registry of `ElementSchema`s, then
//! its attributes are read and its `$references` are resolved against the
//! symbols declared so far. Stages run in dependency order, so a reference
//! can only point at something an earlier stage declared.

use espf::{Attr, Element, Section};

use crate::{
    cx::{quoted_list, Cx},
    registry::{AttrField, ElementSchema, Kind, RefField},
};

/// One checked component or device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub tag: String,
    pub id: String,
    /// Hierarchical id, e.g. `devices.tmp102.outdoor_temp`.
    pub path: String,
    pub refs: Vec<Resolved>,
    pub values: Vec<(String, Value)>,
}

/// A `$reference` that points at a declared element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The attribute the reference was written in.
    pub key: String,
    pub target_id: String,
    pub target_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Int(u32),
}

impl Instance {
    pub fn reference(&self, key: &str) -> Option<&Resolved> {
        self.refs.iter().find(|resolved| resolved.key == key)
    }

    pub fn value(&self, key: &str) -> Option<&Value> {
        self.values
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    pub fn int(&self, key: &str) -> Option<u32> {
        match self.value(key) {
            Some(Value::Int(number)) => Some(*number),
            _ => None,
        }
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.value(key) {
            Some(Value::Text(text)) => Some(text),
            _ => None,
        }
    }
}

/// Check every element of `section` against `schemas`. Errors go to `cx`.
///
/// An element with any error is not declared in the symbol table, so later
/// references to it are reported too.
pub fn extract(cx: &mut Cx, section: &Section, schemas: &[ElementSchema]) -> Vec<Instance> {
    let mut out = Vec::new();

    for element in &section.elements {
        let Some(schema) = schemas.iter().find(|schema| schema.tag == element.tag) else {
            let tags: Vec<&str> = schemas.iter().map(|schema| schema.tag).collect();
            let known = if tags.is_empty() {
                "nothing is registered".to_owned()
            } else {
                quoted_list(&tags)
            };

            cx.error(
                element.span.clone(),
                format!("Unknown <{}> in `{}`; known: {known}", element.tag, section.name),
            );
            continue;
        };

        if let Some(instance) = instantiate(cx, section, element, schema) {
            out.push(instance);
        }
    }

    out
}

fn instantiate(
    cx: &mut Cx,
    section: &Section,
    element: &Element,
    schema: &ElementSchema,
) -> Option<Instance> {
    let mut ok = true;

    let id = element.id.as_ref().map(|attr| attr.value.clone());
    if id.is_none() {
        cx.error(element.span.clone(), format!("<{}> needs an id", element.tag));
        ok = false;
    }

    let allowed: Vec<&str> = schema
        .refs
        .iter()
        .map(|field| field.key)
        .chain(schema.attrs.iter().map(|field| field.key))
        .collect();
    cx.check_attrs(element, &allowed);

    let mut refs = Vec::new();
    for field in schema.refs {
        match resolve(cx, element, field) {
            Ok(Some(resolved)) => refs.push(resolved),
            Ok(None) => {}
            Err(()) => ok = false,
        }
    }

    let mut values = Vec::new();
    for field in schema.attrs {
        match read_value(cx, element, field) {
            Ok(Some(value)) => values.push((field.key.to_owned(), value)),
            Ok(None) => {}
            Err(()) => ok = false,
        }
    }

    if !ok {
        return None;
    }

    let id = id?;
    let path = format!("{}.{}.{id}", section.name, element.tag);

    cx.declare(element, path.clone());

    Some(Instance {
        tag: element.tag.clone(),
        id,
        path,
        refs,
        values,
    })
}

/// `Err(())` means an error was reported; `Ok(None)` is an absent optional.
fn find<'e>(
    cx: &mut Cx,
    element: &'e Element,
    key: &str,
    required: bool,
) -> Result<Option<&'e Attr>, ()> {
    if required {
        cx.require(element, key).map(Some).ok_or(())
    } else {
        Ok(element.attr(key))
    }
}

fn resolve(cx: &mut Cx, element: &Element, field: &RefField) -> Result<Option<Resolved>, ()> {
    let Some(attr) = find(cx, element, field.key, field.required)? else {
        return Ok(None);
    };

    let Some(name) = attr.value.strip_prefix('$') else {
        cx.error(
            attr.value_span.clone(),
            format!(
                "`{0}` must be a reference such as `${0}`",
                attr.value
            ),
        );
        return Err(());
    };

    let found = cx.symbols().get(name).cloned();

    match found {
        Some(symbol) if symbol.tag == field.target => Ok(Some(Resolved {
            key: field.key.to_owned(),
            target_id: name.to_owned(),
            target_path: symbol.path,
        })),

        Some(symbol) => {
            cx.error(
                attr.value_span.clone(),
                format!(
                    "`${name}` is a <{}>, but `{}` needs a <{}>",
                    symbol.tag, field.key, field.target
                ),
            );
            Err(())
        }

        None => {
            let names: Vec<String> = cx
                .symbols()
                .ids_with_tag(field.target)
                .iter()
                .map(|id| format!("${id}"))
                .collect();
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let known = if names.is_empty() {
                "none declared".to_owned()
            } else {
                quoted_list(&names)
            };

            cx.error(
                attr.value_span.clone(),
                format!(
                    "Unknown reference `${name}`; known <{}> ids: {known}",
                    field.target
                ),
            );
            Err(())
        }
    }
}

fn read_value(cx: &mut Cx, element: &Element, field: &AttrField) -> Result<Option<Value>, ()> {
    let Some(attr) = find(cx, element, field.key, field.required)? else {
        return Ok(None);
    };

    match &field.kind {
        Kind::Text => Ok(Some(Value::Text(attr.value.clone()))),

        Kind::Int => match parse_int(&attr.value) {
            Some(number) => Ok(Some(Value::Int(number))),
            None => {
                cx.error(
                    attr.value_span.clone(),
                    format!("`{}` is not a whole number", attr.value),
                );
                Err(())
            }
        },

        Kind::OneOf(options) => {
            if options.contains(&attr.value.as_str()) {
                Ok(Some(Value::Text(attr.value.clone())))
            } else {
                cx.error(
                    attr.value_span.clone(),
                    format!(
                        "`{}` is not valid for `{}`; expected {}",
                        attr.value,
                        field.key,
                        quoted_list(options)
                    ),
                );
                Err(())
            }
        }
    }
}

fn parse_int(text: &str) -> Option<u32> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}
