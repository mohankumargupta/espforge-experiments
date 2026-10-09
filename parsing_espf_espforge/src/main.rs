use std::collections::HashMap;
use winnow::ascii::{line_ending, space0, till_line_ending};
use winnow::combinator::{delimited, opt, preceded, terminated};
use winnow::prelude::*;
use winnow::token::take_till;

/// Global flat table mapping unique leaf paths to key-value fields.
/// E.g., "espforge.name" -> "example project"
/// E.g., "peripherals.gpio4.pin" -> "4"
#[derive(Debug, Default)]
pub struct LeafTable {
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct EspForgeSection {
    pub project_name: String,
    pub project_description: String,
    pub chip_type: String,
    pub runtime_type: String,
}

/// Extracts all valid `key="value"` attribute pairs within a leaf tag's body.
fn parse_attributes(mut input: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    while !input.is_empty() {
        // Strip leading space
        let _: &str = space0::<_, ()>.parse_next(&mut input).unwrap_or_default();
        if input.is_empty() {
            break;
        }

        // Parse key
        let key = match take_till::<_, &str, ()>(1.., |c: char| c == '=' || c == ' ' || c == '/')
            .parse_next(&mut input)
        {
            Ok(k) if !k.is_empty() => k.to_string(),
            _ => break,
        };

        // Consume '=' and '"'
        if preceded((space0::<_, ()>, '=', space0::<_, ()>, '"'), ())
            .parse_next(&mut input)
            .is_ok()
        {
            if let Ok(val) = terminated(take_till::<_, &str, ()>(0.., '"'), '"')
                .parse_next(&mut input)
            {
                attrs.insert(key, val.to_string());
            }
        }
    }
    attrs
}

/// Sweeps the text to fill the global flat leaf table.
pub fn parse_to_leaf_table(input: &mut &str) -> ModalResult<LeafTable> {
    let mut table = LeafTable::default();
    let mut current_section = String::new();

    while !input.is_empty() {
        // Skip whitespace and comments
        if input.starts_with("---") || input.trim_start().is_empty() {
            let _ = till_line_ending.parse_next(input)?;
            let _ = opt(line_ending).parse_next(input)?;
            continue;
        }

        // Catch Section Headers (e.g., "# espforge")
        if input.starts_with('#') {
            let header = preceded(('#', space0), till_line_ending).parse_next(input)?;
            current_section = header.trim().to_string();
            let _ = opt(line_ending).parse_next(input)?;
            continue;
        }

        // Parse Inline Leaf Tag Content (e.g., <chip type="esp32c3" />)
        let trimmed = input.trim_start();
        if trimmed.starts_with('<') {
            // Match up to self-closing tag terminator
            let _ = space0.parse_next(input)?;
            let mut raw_tag = delimited('<', take_till(0.., ('/', '>')), ('/', '>')).parse_next(input)?;
            let _ = till_line_ending.parse_next(input)?;
            let _ = opt(line_ending).parse_next(input)?;

            // Extract the tag name (e.g., "project")
            let tag_name = take_till(1.., |c| c == ' ' || c == '/')
                .parse_next(&mut raw_tag)?
                .trim()
                .to_string();
            let attrs = parse_attributes(raw_tag);

            // Compute implicit parent ID base path
            let node_id = attrs.get("id").cloned().unwrap_or_else(|| tag_name.clone());

            let base_path = if current_section.is_empty() {
                node_id
            } else {
                format!("{}.{}", current_section, node_id)
            };

            // Commit all attributes to the table, resolving names natively
            for (key, val) in attrs {
                if key != "id" {
                    table.fields.insert(format!("{}.{}", base_path, key), val);
                }
            }
            continue;
        }

        // Fallback for line handling
        let _ = till_line_ending.parse_next(input)?;
        let _ = opt(line_ending).parse_next(input)?;
    }

    Ok(table)
}

impl EspForgeSection {
    pub fn extract_from(table: &LeafTable) -> Self {
        Self {
            project_name: table
                .fields
                .get("espforge.project.name")
                .cloned()
                .unwrap_or_default(),
            project_description: table
                .fields
                .get("espforge.project.description")
                .cloned()
                .unwrap_or_default(),
            chip_type: table
                .fields
                .get("espforge.chip.type")
                .cloned()
                .unwrap_or_default(),
            runtime_type: table
                .fields
                .get("espforge.runtime.type")
                .cloned()
                .unwrap_or_default(),
        }
    }
}

fn main() {
    let mut input = r#"--- ==============================================================
--- ESPF file format
--- mandatory sections: espforge, peripherals, components, devices
--- ==============================================================

# espforge
  <project name="example project" description="example showing espf file format" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals
  <gpio id="gpio4" pin="4" />
  <gpio id="gpio5" pin="5" />
  <i2c  id="i2c0" sda="8" scl="9" frequency="400kHz" />"#;

    // Pass 1: Parse string completely into an implicit flat leaf table
    let table = parse_to_leaf_table(&mut input).unwrap();
    println!("--- Pass 1 Flat Leaf Table Keys ---");
    for (k, v) in &table.fields {
        println!("{} => {}", k, v);
    }

    // Pass 2: Build target runtime structure cleanly
    let espforge = EspForgeSection::extract_from(&table);
    println!("\n--- Pass 2 Extracted Target Struct ---");
    println!("{:#?}", espforge);
}
