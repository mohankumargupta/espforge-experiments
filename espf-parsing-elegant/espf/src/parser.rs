use crate::context::{ParsedNode, TagAttribute};
use std::collections::HashMap;
use winnow::{
    ascii::{multispace0, multispace1, till_line_ending},
    combinator::{alt, delimited, dispatch, fail, opt, peek, preceded, repeat, separated_pair},
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
/// Every branch consumes at least one byte on success, so this cannot loop
/// forever inside `repeat(0.., ...)`.
fn skip_trivia(input: &mut &str) -> ModalResult<()> {
    repeat(
        0..,
        dispatch! {peek(any);
            // `--` comment line: consume through end of line
            '-' => preceded(('-', '-'), till_line_ending).void(),
            // one-or-more whitespace chars
            ' ' | '\t' | '\r' | '\n' => multispace1.void(),
            // anything else: stop skipping, do not consume
            _ => fail,
        },
    )
    .parse_next(input)
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
            repeat(0.., preceded(multispace1, attribute)),
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

/// Parse the contents of a single section, up to (but not consuming) the next
/// section header or EOF.
///
/// Uses an explicit loop rather than `repeat(0.., ...)` so we can guarantee
/// that every successful iteration consumes at least one byte — otherwise
/// Winnow's `repeat` asserts "parsers must always consume".
fn section_content<'a>(input: &mut &'a str) -> ModalResult<Vec<ParsedNode<'a>>> {
    let mut nodes = Vec::new();

    loop {
        // 1. Skip whitespace / comments so we're at a meaningful token.
        skip_trivia.parse_next(input)?;

        // 2. Stop at EOF.
        if input.is_empty() {
            break;
        }

        // 3. Stop if the next thing is a section header (peek without consuming).
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

        // 5. Otherwise consume exactly one line as junk (including newline).
        //    Guarantees progress even if the line is blank.
        let _ = till_line_ending.parse_next(input)?;
        let _ = opt(alt(("\r\n", "\n", "\r"))).parse_next(input)?;
    }

    Ok(nodes)
}

/// Main parser entry point.
pub fn parse_espf_runtime<'a>(
    mut input: &'a str,
) -> ModalResult<HashMap<String, Vec<ParsedNode<'a>>>> {
    // 1. Skip any leading trivia.
    skip_trivia.parse_next(&mut input)?;

    // 2. Parse a sequence of (header, contents) pairs.
    let mut sections: HashMap<String, Vec<ParsedNode<'a>>> = HashMap::new();

    while !input.is_empty() {
        // Skip whitespace between sections.
        skip_trivia.parse_next(&mut input)?;
        if input.is_empty() {
            break;
        }

        // Section header.
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

        // Skip trivia between header and its contents.
        skip_trivia.parse_next(&mut input)?;

        // Parse contents until the next header or EOF.
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