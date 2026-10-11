use std::ops::Range;

use winnow::{
    ascii::{line_ending, space0, till_line_ending},
    combinator::{delimited, opt},
    prelude::*,
    stream::{LocatingSlice, Location},
    token::{take_till, take_while},
};

use crate::{
    ast::{Attr, Element},
    diagnostic::{diagnostic_at_current, ParseDiagnostic},
    line::{parse_line, Line},
};

// -----------------------------------------------------------------------------
// Tag body accumulation
// -----------------------------------------------------------------------------

/// A run of `TagBody::text` that was copied verbatim from the source.
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// Byte offset of the run in the joined text.
    joined: usize,
    /// Byte offset of the same run in the original source.
    source: usize,
    len: usize,
}

/// A complete tag, joined onto one line, plus what is needed to map offsets in
/// the joined text back to the original source.
#[derive(Debug)]
pub(crate) struct TagBody {
    /// Starts with the opening `<`. Continuation lines are trimmed and joined
    /// with a single space; mid-tag comment lines are dropped.
    pub(crate) text: String,
    /// First line to last line of the tag in the source.
    pub(crate) span: Range<usize>,
    pieces: Vec<Piece>,
}

impl TagBody {
    /// Map a byte range of `text` back to the original source.
    pub(crate) fn source_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.source_offset(range.start);

        let end = if range.end > range.start {
            self.source_offset(range.end - 1) + 1
        } else {
            start
        };

        start..end
    }

    fn source_offset(&self, offset: usize) -> usize {
        let piece = self
            .pieces
            .iter()
            .rev()
            .find(|piece| piece.joined <= offset)
            .expect("the first piece starts at offset 0");

        // The clamp only matters for an offset on a joining space.
        piece.source + (offset - piece.joined).min(piece.len)
    }
}

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
) -> Result<TagBody, ParseDiagnostic> {
    // parse_line hands us the text after the opening `<`, which is a suffix of
    // the first line, so the `<` sits one byte before it in the source.
    let open_offset = first_line_span.end - first_tail.len() - 1;
    let mut body = format!("<{first_tail}");
    let mut pieces = vec![Piece {
        joined: 0,
        source: open_offset,
        len: body.len(),
    }];
    let mut span = first_line_span;

    loop {
        match classify_tag_body(&body) {
            TagBodyEnd::Complete => return Ok(TagBody { text: body, span, pieces }),
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
                let leading = line.len() - line.trim_start().len();

                body.push(' ');
                pieces.push(Piece {
                    joined: body.len(),
                    source: line_span.start + leading,
                    len: trimmed.len(),
                });
                body.push_str(trimmed);
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Element and attribute parsing
// -----------------------------------------------------------------------------

/// Parse a complete tag body into an `Element`, with spans in source offsets.
pub(crate) fn parse_element(body: &TagBody) -> Result<Element, String> {
    let text = body.text.as_str();

    // `finish_tag_body` always starts the text with the opening `<`.
    let mut input = text.strip_prefix('<').unwrap_or(text);

    let tag = parse_tag_name(&mut input)
        .map_err(|error| format!("Malformed tag: Expected a tag name: {error:?}"))?;

    let mut id: Option<Attr> = None;
    let mut attrs = Vec::new();

    loop {
        skip_tag_whitespace(&mut input);

        if opt::<_, _, winnow::error::ContextError, _>("/>")
            .parse_next(&mut input)
            .map_err(|error| format!("Malformed tag: Invalid tag ending: {error:?}"))?
            .is_some()
        {
            break;
        }

        let (key, value) = parse_attribute(&mut input)
            .map_err(|error| format!("Malformed tag: Invalid attribute: {error:?}"))?;

        let attr = Attr {
            key: key.to_owned(),
            value: value.to_owned(),
            key_span: body.source_range(span_of(text, key)),
            value_span: body.source_range(span_of(text, value)),
        };

        if attr.key == "id" {
            if id.is_some() {
                return Err("Malformed tag: Duplicate `id` attribute".to_owned());
            }

            id = Some(attr);
        } else {
            attrs.push(attr);
        }
    }

    skip_tag_whitespace(&mut input);

    // Allow trailing comments like `--- why this pin choice`.
    if !input.is_empty() && !input.starts_with("---") {
        return Err(format!("Unexpected text after <{tag}>: {input:?}"));
    }

    Ok(Element {
        tag: tag.to_owned(),
        id,
        attrs,
        span: body.span.clone(),
    })
}

/// Byte range of `part` inside `whole`. `part` must be a subslice of `whole`,
/// which is what the winnow parsers above return.
fn span_of(whole: &str, part: &str) -> Range<usize> {
    let start = part.as_ptr() as usize - whole.as_ptr() as usize;
    start..start + part.len()
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

fn parse_attribute<'a>(input: &mut &'a str) -> ModalResult<(&'a str, &'a str)> {
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

fn skip_tag_whitespace(input: &mut &str) {
    let _: Result<(&str, &str), ()> = (space0, space0).parse_next(input); // always succeeds
}
