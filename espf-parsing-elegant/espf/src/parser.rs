use crate::context::{ParsedNode, TagAttribute};
use std::collections::HashMap;
use winnow::{
    ascii::{multispace0, multispace1, till_line_ending},
    combinator::{delimited, dispatch, fail, not, opt, peek, preceded, repeat, separated_pair},
    token::{any, take_until, take_while},
    ModalResult, Parser,
};

/// Identifier-like token: letters, digits, `_`, `-`.
/// Used for tag names, section names, and attribute keys.
fn identifier<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_while(1.., |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        .parse_next(input)
}

/// Skip blank lines, `---` separator lines, and comments.
fn skip_trivia(input: &mut &str) -> ModalResult<()> {
    repeat(
        0..,
        dispatch! {peek(any);
            '-' => preceded("---", till_line_ending).void(),
            '#' => preceded(
                ('#', multispace0, peek(not(identifier))),
                till_line_ending,
            ).void(),
            ' ' | '\t' | '\r' | '\n' => multispace1.void(),
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

/// `key="value"` — key may contain letters, digits, `_`, `-`.
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

/// True if remaining input (after whitespace) starts a section header.
fn at_section_header(input: &mut &str) -> ModalResult<bool> {
    let found = opt(preceded(
        multispace0,
        peek(('#', multispace0, identifier)),
    ))
    .parse_next(input)?
    .is_some();
    Ok(found)
}

/// True if remaining input (after whitespace) starts a tag.
fn at_tag(input: &mut &str) -> ModalResult<bool> {
    let found = opt(preceded(multispace0, peek('<')))
        .parse_next(input)?
        .is_some();
    Ok(found)
}

pub fn parse_espf_runtime<'a>(
    mut input: &'a str,
) -> ModalResult<HashMap<String, Vec<ParsedNode<'a>>>> {
    let mut sections: HashMap<String, Vec<ParsedNode<'a>>> = HashMap::new();

    skip_trivia.parse_next(&mut input)?;

    while !input.is_empty() {
        multispace0.parse_next(&mut input)?;

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

        let mut nodes = Vec::new();

        loop {
            if input.is_empty() {
                break;
            }
            if at_section_header(&mut input)? {
                break;
            }

            if at_tag(&mut input)? {
                match parse_node.parse_next(&mut input) {
                    Ok(node) => nodes.push(node),
                    Err(e) => {
                        eprintln!(
                            "[parser] parse_node failed at: {:?}",
                            &input[..input.len().min(200)]
                        );
                        return Err(e);
                    }
                }
            } else {
                let _ = till_line_ending.parse_next(&mut input)?;
            }

            skip_trivia.parse_next(&mut input)?;
        }

        sections.insert(section_name.to_string(), nodes);
    }

    Ok(sections)
}