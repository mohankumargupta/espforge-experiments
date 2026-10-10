use std::collections::HashMap;

use winnow::{
    ascii::{line_ending, till_line_ending},
    combinator::opt,
    prelude::*,
    stream::{LocatingSlice, Location},
};

use crate::{
    diagnostic::{diagnostic_at_current, ParseDiagnostic},
    line::{parse_line, Line},
    tag::{finish_tag_body, parse_attribute, parse_tag, skip_tag_whitespace, TagInfo},
};

pub type LeafFields = HashMap<String, String>;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LeafTable {
    pub fields: LeafFields,
}

pub fn parse_to_leaf_table(source: &str) -> Result<LeafTable, ParseDiagnostic> {
    let mut input = LocatingSlice::new(source);
    let mut table = LeafTable::default();
    let mut section: Option<&str> = None;

    while !input.is_empty() {
        // LocatingSlice tracks this line's range in the original source.
        let (line, line_span) = till_line_ending
            .with_span()
            .parse_next(&mut input)
            .map_err(|error: winnow::error::ContextError| {
                diagnostic_at_current(
                    source,
                    input.current_token_start(),
                    format!("Could not read line: {error:?}"),
                )
            })?;

        // Consume the line ending, if present.
        let _ = opt::<_, _, winnow::error::ContextError, _>(line_ending).parse_next(&mut input);

        let parsed_line = parse_line(line)
            .map_err(|message| ParseDiagnostic::new(source, line_span.clone(), message))?;

        match parsed_line {
            Line::Section(name) => section = Some(name),

            Line::Tag(tag_tail) => {
                let (tag_body, tag_span) =
                    finish_tag_body(&mut &mut input, source, line_span.clone(), tag_tail)?;

                let current_section = section.ok_or_else(|| {
                    ParseDiagnostic::new(
                        source,
                        tag_span.clone(),
                        "Tag encountered before a section heading",
                    )
                })?;

                insert_tag(current_section, tag_body.strip_prefix('<').unwrap_or(&tag_body), &mut table.fields)
                    .map_err(|message| ParseDiagnostic::new(source, tag_span, message))?;
            }

            Line::Ignore | Line::Comment => {}
        }
    }

    Ok(table)
}

// -----------------------------------------------------------------------------
// Tag insertion (flattening into the LeafTable; replaced by the AST in step 2)
// -----------------------------------------------------------------------------

fn insert_tag(section: &str, tag_tail: &str, fields: &mut LeafFields) -> Result<(), String> {
    // Pass 1: validate the tag and discover its ID.
    let mut first_pass = tag_tail;
    let info = parse_tag(&mut first_pass).map_err(|error| format!("Malformed tag: {error}"))?;

    if !first_pass.is_empty() {
        skip_tag_whitespace(&mut first_pass);

        // Allow trailing comments like `--- why this pin choice`.
        // Fall through to pass 2 so the fields still get inserted.
        if first_pass.starts_with("---") {
            return insert_fields(section, info, tag_tail, fields);
        }

        return Err(format!("Unexpected text after tag: {first_pass:?}"));
    }

    insert_fields(section, info, tag_tail, fields)
}

// Pass 2 only: insert borrowed attribute values into the owned table.
fn insert_fields(
    section: &str,
    info: TagInfo<'_>,
    tag_tail: &str,
    fields: &mut LeafFields,
) -> Result<(), String> {
    let node_id = info.id.unwrap_or(info.name);

    // We already know the tag name from pass 1, so skip it without re-parsing.
    let mut second_pass = &tag_tail[info.name.len()..];

    loop {
        skip_tag_whitespace(&mut second_pass);

        if opt::<_, _, winnow::error::ContextError, _>("/>")
            .parse_next(&mut second_pass)
            .map_err(|error| format!("Invalid tag ending: {error:?}"))?
            .is_some()
        {
            break;
        }

        let (key, value) = parse_attribute(&mut second_pass).map_err(|error| {
            format!("Invalid attribute in <{}>: {error:?}", info.name)
        })?;

        if key == "id" {
            continue;
        }

        insert_field(fields, section, node_id, key, value)?;
    }

    skip_tag_whitespace(&mut second_pass);

    // Allow trailing comments like `--- why this pin choice`.
    if second_pass.starts_with("---") {
        return Ok(());
    }

    if !second_pass.is_empty() {
        return Err(format!(
            "Unexpected text after <{}>: {second_pass:?}",
            info.name
        ));
    }

    Ok(())
}

fn insert_field(
    fields: &mut LeafFields,
    section: &str,
    node_id: &str,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let path = format!("{section}.{node_id}.{key}");

    if fields.contains_key(&path) {
        return Err(format!("Duplicate field path `{path}`"));
    }

    fields.insert(path, value.to_owned());
    Ok(())
}
