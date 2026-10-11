use crate::context::{ParsedNode, TagAttribute};
use std::collections::HashMap;
use winnow::{
    ascii::{alpha1, alphanumeric1, multispace0, multispace1, till_line_ending},
    combinator::{alt, delimited, not, opt, peek, preceded, repeat, separated_pair},
    token::take_until,
    ModalResult, Parser,
};

/// Skip blank lines, `---` separator lines, and comments.
///
/// A comment is `#` NOT followed by an alphabetic character (that would be
/// a section header). Every branch is guaranteed to consume at least one
/// byte when it succeeds, so `repeat` cannot loop forever.
fn skip_trivia(input: &mut &str) -> ModalResult<()> {
    repeat(
        0..,
        alt((
            // `---` separator line: consume through end of line
            preceded("---", till_line_ending).void(),
            // comment line: '#' not followed by alpha, then rest of line
            preceded(('#', peek(not(alpha1))), till_line_ending).void(),
            // require at least one whitespace char to guarantee progress
            multispace1.void(),
        )),
    )
    .parse_next(input)
}

/// Parse a section header: `# name` where name is alphabetic.
fn section_header<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    preceded(('#', multispace0), alpha1).parse_next(input)
}

/// Everything up to the closing `"`.
fn unquoted_value<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_until(1.., "\"").parse_next(input)
}

/// `key="value"`
fn attribute<'a>(input: &mut &'a str) -> ModalResult<TagAttribute<'a>> {
    separated_pair(alphanumeric1, '=', delimited('"', unquoted_value, '"'))
        .map(|(key, val)| TagAttribute { key, val })
        .parse_next(input)
}

/// Self-closing tag: `<tag attr="v" ... />`
fn parse_node<'a>(input: &mut &'a str) -> ModalResult<ParsedNode<'a>> {
    preceded(
        (multispace0, '<'),
        (
            alpha1,
            repeat(0.., preceded(multispace1, attribute)),
            preceded(multispace0, "/>"),
        ),
    )
    .map(
        |(tag_name, attributes, _): (&str, Vec<TagAttribute>, &str)| {
            let id = attributes
                .iter()
                .find(|a| a.key == "id")
                .map(|a| a.val)
                .unwrap_or("");

            ParsedNode {
                tag_name,
                id,
                attributes,
            }
        },
    )
    .parse_next(input)
}

/// Return `true` if the remaining input (after optional whitespace) starts a
/// new section header — i.e. a `#` not followed by an alphabetic char.
fn at_section_header(input: &mut &str) -> ModalResult<bool> {
    let found = opt(preceded(
        multispace0,
        peek(preceded('#', not(alpha1))),
    ))
    .parse_next(input)?
    .is_some();
    Ok(found)
}

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
        let section_name = section_header.parse_next(&mut input)?;
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
                let node = parse_node.parse_next(&mut input)?;
                nodes.push(node);
            } else {
                // Unknown line inside a section — skip it.
                let _ = till_line_ending.parse_next(&mut input)?;
            }

            skip_trivia.parse_next(&mut input)?;
        }

        sections.insert(section_name.to_string(), nodes);
    }

    Ok(sections)
}