use std::ops::Range;

use winnow::{
    ascii::{line_ending, space0, till_line_ending},
    combinator::{delimited, opt},
    prelude::*,
    stream::{LocatingSlice, Location},
    token::{take_till, take_while},
};

use crate::{
    diagnostic::{diagnostic_at_current, ParseDiagnostic},
    line::{parse_line, Line},
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TagInfo<'a> {
    pub(crate) name: &'a str,
    pub(crate) id: Option<&'a str>,
}

// -----------------------------------------------------------------------------
// Tag body accumulation
// -----------------------------------------------------------------------------

/// Why we stopped scanning: the tag is complete, or something is wrong.
enum TagBodyEnd {
    Complete,
    /// A tag was never opened (first char isn't `<`) or already closed.
    NoOpenTag,
    /// Reached EOF without a tag name and closing `>`.
    Unclosed,
}

/// Returns true if the byte at byte index `index` lies outside any quoted
/// attribute value that precedes it in `body`.
fn is_outside_quotes(body: &str, index: usize) -> bool {
    let quote_count = body[..index].as_bytes().iter().filter(|&&b| b == b'"').count();

    // An even number of preceding quotes means we are outside a value.
    quote_count.is_multiple_of(2)
}

/// Classify a finished tag body: has it seen a name and a closing `>` outside
/// of any quoted attribute value, preceded by only whitespace/comment text?
fn classify_tag_body(body: &str) -> TagBodyEnd {
    let Some(open) = body.find('<') else {
        return TagBodyEnd::NoOpenTag;
    };

    // The tag must have a non-empty name right after `<`.
    let after_open = &body[open + '<'.len_utf8()..];
    let has_name = after_open
        .chars()
        .next()
        .map(|ch| !ch.is_whitespace() && ch != '>')
        .unwrap_or(false);

    if !has_name {
        return TagBodyEnd::NoOpenTag;
    }

    for (index, ch) in body.char_indices() {
        if ch != '>' {
            continue;
        }

        if !is_outside_quotes(body, index) {
            continue;
        }

        let rest = body[index + ch.len_utf8()..].trim_start();

        if rest.is_empty() || rest.starts_with("---") {
            return TagBodyEnd::Complete;
        }
    }

    TagBodyEnd::Unclosed
}

/// Consume indentation-led continuation lines while the tag is still open.
pub(crate) fn finish_tag_body(
    input: &mut &mut LocatingSlice<&str>,
    source: &str,
    first_line_span: Range<usize>,
    first_tail: &str,
) -> Result<(String, Range<usize>), ParseDiagnostic> {
    // parse_line hands us the text after the opening `<`.
    let mut body = format!("<{first_tail}");
    let mut span = first_line_span;

    loop {
        match classify_tag_body(&body) {
            TagBodyEnd::Complete => return Ok((body, span)),
            TagBodyEnd::NoOpenTag => {
                return Err(ParseDiagnostic::new(
                    source,
                    span,
                    "Tag text encountered outside a tag",
                ))
            }
            TagBodyEnd::Unclosed => {
                if input.is_empty() {
                    return Err(ParseDiagnostic::new(
                        source,
                        span,
                        "Unterminated tag: reached end of file before `>`",
                    ));
                }

                let (line, line_span) = till_line_ending
                    .with_span()
                    .parse_next(&mut **input)
                    .map_err(|error: winnow::error::ContextError| {
                        let offset = input.current_token_start();
                        diagnostic_at_current(source, offset, format!("Could not read line: {error:?}"))
                    })?;
                let _ = opt::<_, _, winnow::error::ContextError, _>(line_ending)
                    .parse_next(&mut **input);

                let parsed = parse_line(line).map_err(|message| {
                    ParseDiagnostic::new(source, line_span.clone(), message)
                })?;

                match parsed {
                    // Mid-tag comment lines are dropped from the joined body.
                    Line::Comment => {
                        span.end = line_span.end;
                        continue;
                    }
                    Line::Ignore => {}
                    Line::Section(_) | Line::Tag(_) => {
                        return Err(ParseDiagnostic::new(
                            source,
                            span,
                            "Unterminated tag: reached a new construct before `>`",
                        ));
                    }
                }

                span.end = line_span.end;
                let trimmed = line.trim();
                body.push(' ');
                body.push_str(trimmed);
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Tag and attribute parsing
// -----------------------------------------------------------------------------

pub(crate) fn parse_tag<'a>(input: &mut &'a str) -> Result<TagInfo<'a>, String> {
    let name = parse_tag_name(input).map_err(|error| format!("Expected a tag name: {error:?}"))?;

    let mut id = None;

    loop {
        skip_tag_whitespace(input);

        if opt::<_, _, winnow::error::ContextError, _>("/>")
            .parse_next(input)
            .map_err(|error| format!("Invalid tag ending: {error:?}"))?
            .is_some()
        {
            return Ok(TagInfo { name, id });
        }

        let (key, value) =
            parse_attribute(input).map_err(|error| format!("Invalid attribute: {error:?}"))?;

        if key == "id" {
            if id.is_some() {
                return Err("Duplicate `id` attribute".to_owned());
            }

            id = Some(value);
        }
    }
}

fn is_tag_name_char(ch: char) -> bool {
    !ch.is_whitespace() && !matches!(ch, '/' | '>' | '=' | '"' | '<')
}

fn is_attribute_key_char(ch: char) -> bool {
    !ch.is_whitespace() && !matches!(ch, '=' | '/' | '>' | '"' | '<')
}

fn parse_tag_name<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_while(1.., is_tag_name_char).parse_next(input)
}

pub(crate) fn parse_attribute<'a>(input: &mut &'a str) -> ModalResult<(&'a str, &'a str)> {
    let key = take_while(1.., is_attribute_key_char).parse_next(input)?;

    space0.parse_next(input)?;
    '='.parse_next(input)?;
    space0.parse_next(input)?;

    let value = parse_quoted_value(input)?;

    Ok((key, value))
}

fn parse_quoted_value<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    delimited('"', take_till(0.., '"'), '"').parse_next(input)
}

pub(crate) fn skip_tag_whitespace(input: &mut &str) {
    let _: Result<(&str, &str), ()> = (space0, space0).parse_next(input); // always succeeds
}
