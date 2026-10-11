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