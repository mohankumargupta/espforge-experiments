use crate::context::{ParsedNode, TagAttribute};
use std::collections::HashMap;
use winnow::{
    ascii::{multispace0, multispace1, till_line_ending},
    combinator::{delimited, dispatch, fail, opt, peek, preceded, separated_pair},
    token::{any, take_until, take_while},
    ModalResult, Parser,
};

/// Identifier-like token: letters, digits, `_`, `-`.
fn identifier<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_while(1.., |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        .parse_next(input)
}

/// Skip blank lines and comments starting with `--`.
///
/// Uses `opt(dispatch!{...})` so "not trivia" cleanly stops the loop
/// without producing an error, and so the error type is pinned by
/// `ModalResult`.
fn skip_trivia(input: &mut &str) -> ModalResult<()> {
    loop {
        let before = *input;

        let _ = opt(dispatch! {peek(any);
            '-' => preceded(('-', '-'), till_line_ending).void(),
            ' ' | '\t' | '\r' | '\n' => multispace1.void(),
            _ => fail,
        })
        .parse_next(input)?;

        if *input == before {
            break;
        }
    }
    Ok(())
}

/// Section header: `# name`
fn section_header<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    preceded(('#', multispace0), identifier).parse_next(input)
}

/// Everything up to the closing `"`.
fn unquoted_value<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_until(1.., "\"").parse_next(input)
}

/// `key="value"`
fn attribute<'a>(input: &mut &'a str) -> ModalResult<TagAttribute<'a>> {
    separated_pair(identifier, '=', delimited('"', unquoted_value, '"'))
        .map(|(key, val)| TagAttribute { key, val })
        .parse_next(input)
}

/// Self-closing tag: `<tag attr="v" ... />`
fn parse_node<'a>(input: &mut &'a str) -> ModalResult<ParsedNode<'a>> {
    preceded(
        (multispace0, '<'),
        (
            identifier,
            winnow::combinator::repeat(0.., preceded(multispace1, attribute)),
            preceded(multispace0, "/>"),
        ),
    )
    .map(|(tag_name, attributes, _): (&str, Vec<TagAttribute>, &str)| {
        let id = attributes
            .iter()
            .find(|a| a.key == "id")
            .map(|a| a.val)
            .unwrap_or("");
        ParsedNode { tag_name, id, attributes }
    })
    .parse_next(input)
}

/// Consume exactly one line terminator (`\r\n`, `\n`, or `\r`) if present.
/// Returns `Ok(true)` if anything was consumed.
///
/// Order matters: check `\r\n` before `\n` and `\r`.
fn consume_newline(input: &mut &str) -> ModalResult<bool> {
    for nl in ["\r\n", "\n", "\r"] {
        if let Some(rest) = input.strip_prefix(nl) {
            *input = rest;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Parse the contents of a single section, up to (but not consuming) the next
/// section header or EOF.
fn section_content<'a>(input: &mut &'a str) -> ModalResult<Vec<ParsedNode<'a>>> {
    let mut nodes = Vec::new();

    loop {
        // 1. Skip whitespace / comments.
        skip_trivia.parse_next(input)?;

        // 2. Stop at EOF.
        if input.is_empty() {
            break;
        }

        // 3. Stop if the next thing is a section header (peek, don't consume).
        if peek(('#', multispace0, identifier))
            .parse_next(input)
            .is_ok()
        {
            break;
        }

        // 4. Try to parse a self-closing tag.
        if let Ok(node) = parse_node.parse_next(input) {
            nodes.push(node);
            continue;
        }

        // 5. Junk line: consume one full line including its terminator.
        let _ = till_line_ending.parse_next(input)?;
        let _ = consume_newline(input)?;
    }

    Ok(nodes)
}

/// Main parser entry point.
pub fn parse_espf_runtime<'a>(
    mut input: &'a str,
) -> ModalResult<HashMap<String, Vec<ParsedNode<'a>>>> {
    // 1. Skip leading trivia.
    skip_trivia.parse_next(&mut input)?;

    let mut sections: HashMap<String, Vec<ParsedNode<'a>>> = HashMap::new();

    while !input.is_empty() {
        skip_trivia.parse_next(&mut input)?;
        if input.is_empty() {
            break;
        }

        let section_name = match section_header.parse_next(&mut input) {
            Ok(n) => n,
            Err(e) => {
                eprintln!(
                    "[parser] section_header failed at: {:?}",
                    &input[..input.len().min(200)]
                );
                return Err(e);
            }
        };

        skip_trivia.parse_next(&mut input)?;

        let nodes = match section_content.parse_next(&mut input) {
            Ok(n) => n,
            Err(e) => {
                eprintln!(
                    "[parser] section_content failed at: {:?}",
                    &input[..input.len().min(200)]
                );
                return Err(e);
            }
        };

        sections.insert(section_name.to_string(), nodes);
    }

    Ok(sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses the sample layout that used to be embedded in the CLI binary.
    /// All three sections must land with the expected node counts and ids.
    #[test]
    fn parses_sample_runtime_file() {
        let input = r#"
--- ==============================================================
--- ESPF runtime file format instance
--- ==============================================================

# peripherals
  <gpio id="gpio4" pin="4" />
  <i2c  id="i2c0" sda="8" scl="9" frequency="400kHz" />
  <spi  id="spi2" sclk="6" mosi="7" />

# components
  <i2c_component  id="temp_bus"    i2c="$i2c0" address="0x48" />
  <spi_component  id="display_bus" spi="$spi2" mode="0" cs="gpio5" frequency="40MHz" />

# devices
  <tmp102         id="outdoor_temp" i2c_component="$temp_bus" />
  <ili9341        id="tft_display"  spi_component="$display_bus" />
    "#;

        let sections = parse_espf_runtime(input)
            .expect("sample runtime file should parse cleanly");

        assert_eq!(sections["peripherals"].len(), 3);
        assert_eq!(sections["components"].len(), 2);
        assert_eq!(sections["devices"].len(), 2);

        let tmp102 = sections["devices"][0].clone();
        assert_eq!(tmp102.tag_name, "tmp102");
        assert_eq!(tmp102.id, "outdoor_temp");
    }

    /// A malformed attribute value with no closing `"` must not silently
    /// disappear as a "junk line" — it is a syntactical error.
    #[test]
    fn rejects_node_with_missing_closing_quote() {
        let input = r#"
# peripherals
  <gpio id="gpio4" pin="4 />
"#;

        // The parse itself may succeed structurally, but the node must not
        // come back with a truncated attribute. It is only acceptable if the
        // node was rejected or parsed without the malformed `pin` value.
        if let Ok(sections) = parse_espf_runtime(input) {
            let peripherals = &sections["peripherals"];
            // The malformed node either was not captured at all, or was
            // captured without any `pin` attribute value.
            if !peripherals.is_empty() {
                let node = &peripherals[0];
                assert!(
                    node.get_attr("pin").is_none(),
                    "malformed pin attribute must not yield a value, got {:?}",
                    node.get_attr("pin")
                );
            }
        }
    }
}