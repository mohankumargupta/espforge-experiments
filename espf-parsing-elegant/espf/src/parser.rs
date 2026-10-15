use crate::context::{ParsedNode, TagAttribute};
use crate::diag::{JunkKind, JunkSpan};
use std::collections::HashMap;
use winnow::{
    ascii::{multispace0, multispace1, till_line_ending},
    combinator::{dispatch, fail, opt, peek, preceded},
    stream::Stream,
    token::{any, take_while},
    ModalResult, Parser,
};

/// Identifier-like token: letters, digits, `_`, `-`.
fn identifier<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_while(1.., |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        .parse_next(input)
}

/// One comment block: an opening line starting with one or more `=`, one or
/// more comment lines, and a closing line made only of `=`. The first line is
/// already positioned at the opening `=` (skipped dispatch matched it), so
/// consuming until the line ending swallows `===...  title text` too.
fn block_comment(input: &mut &str) -> ModalResult<()> {
    // Opening line: `=` sigils plus any title text, then its newline.
    till_line_ending.void().parse_next(input)?;
    consume_newline(input)?;

    // Body lines. A line whose trimmed content is only `=` closes the block;
    // anything else is a comment line, including blank lines.
    while !input.is_empty() {
        let line_end = input.find(['\r', '\n']).map_or(input.len(), |i| i);
        let is_closing = !input[..line_end].trim_end().is_empty()
            && input[..line_end].trim_end().chars().all(|c| c == '=');

        *input = &input[line_end..];
        consume_newline(input)?;

        if is_closing {
            break;
        }
    }
    // An unterminated block (EOF before the closing `=` line) is tolerated.

    Ok(())
}

/// Skip blank lines and `=`-delimited comment blocks.
///
/// Uses `opt(dispatch!{...})` so "not trivia" cleanly stops the loop
/// without producing an error, and so the error type is pinned by
/// `ModalResult`.
fn skip_trivia(input: &mut &str) -> ModalResult<()> {
    loop {
        let before = *input;

        let _ = opt(dispatch! {peek(any);
            '=' => block_comment,
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

/// The amount of input `parser` consumed, relative to the start of the file.
/// Runs the parser on a throwaway reborrow of `input` purely for measurement.
fn consumed_since(input: &str, original_start: &str) -> usize {
    original_start.len() - input.len()
}

/// Section header: `# name`
fn section_header<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    preceded(('#', multispace0), identifier).parse_next(input)
}

/// Everything up to the closing `"`. Values may not span lines: a newline is
/// not an acceptable terminator, so an unterminated quote fails the attribute
/// (and thus the line) instead of reaching the next line's `"`.
fn unquoted_value<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_while(0.., |c: char| c != '"' && c != '\n' && c != '\r').parse_next(input)
}

/// `key="value"`
///
/// Runs in two phases so the value's byte offset into the original file can
/// be recorded for span-aware diagnostics. Parsing semantics are unchanged:
/// a missing closing `"` fails the attribute (and thus the line).
fn attribute<'a>(input: &mut &'a str, file: &str) -> ModalResult<TagAttribute<'a>> {
    let key = identifier.parse_next(input)?;
    let _ = '='.void().parse_next(input)?;
    let _ = '"'.void().parse_next(input)?;
    let val_offset = consumed_since(input, file);
    let val = unquoted_value.parse_next(input)?;
    let _ = '"'.void().parse_next(input)?;
    Ok(TagAttribute { key, val, val_offset })
}

/// Self-closing tag: `<tag attr="v" ... />`
fn parse_node<'a>(input: &mut &'a str, file: &str) -> ModalResult<ParsedNode<'a>> {
    preceded(
        (multispace0, '<'),
        (
            // Record the byte offset of the tag start for diagnostics:
            // exactly one byte (`<`) has been consumed here.
            move |input: &mut &'a str| {
                let tag_offset = consumed_since(input, file).saturating_sub(1);
                identifier.parse_next(input).map(|t| (t, tag_offset))
            },
            winnow::combinator::repeat(0.., preceded(multispace1, move |input: &mut &'a str| attribute(input, file))),
            preceded(multispace0, "/>"),
        ),
    )
    .map(|(tag, attributes, _): ((&str, usize), Vec<TagAttribute>, &str)| {
        let (tag_name, tag_offset) = tag;
        let id = attributes
            .iter()
            .find(|a| a.key == "id")
            .map(|a| a.val)
            .unwrap_or("");
        ParsedNode { offset: tag_offset, tag_name, id, attributes }
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
///
/// `junk_spans` collects byte offsets and line numbers of lines that did not
/// look like `<tag attr="v" />` and were skipped as junk. Callers that care
/// about nice error messages can render them with `annotate-snippets`.
fn section_content<'a>(
    input: &mut &'a str,
    file: &'a str,
    junk_spans: &mut Vec<JunkSpan>,
) -> ModalResult<Vec<ParsedNode<'a>>> {
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
        //
        // On failure winnow may have partially consumed `input`, so reset to
        // the checkpoint first: the junk offset must point at the *start* of
        // the offending line.
        let checkpoint = input.checkpoint();
        if let Ok(node) = parse_node(input, file) {
            // A leaf node must stand on its own line: after `/>` only blanks
            // may remain before the newline/EOF, except for a trailing
            // inline `//` comment, which is allowed directly after the
            // self-closing element (and nowhere else).
            let first_non_blank = input
                .find(|c: char| c != ' ' && c != '\t')
                .unwrap_or(input.len());
            let tail = &input[first_non_blank..];
            let is_whole_line = tail.is_empty()
                || tail.starts_with('\r')
                || tail.starts_with('\n');
            if is_whole_line {
                nodes.push(node);
                continue;
            }
            if tail.starts_with("//") {
                let _ = till_line_ending.parse_next(input)?;
                let _ = consume_newline(input)?;
                nodes.push(node);
                continue;
            }
        }
        input.reset(&checkpoint);

        // 5. Junk line: record where it starts, then consume one full line.
        let junk_offset = consumed_since(input, file);
        junk_spans.push(JunkSpan {
            offset: junk_offset,
            line: crate::diag::line_number_for_offset(file, junk_offset),
            kind: JunkKind::NodeLine,
        });

        let _ = till_line_ending.parse_next(input)?;
        let _ = consume_newline(input)?;
    }

    Ok(nodes)
}

/// Parse a whole ESPF file, recording junk (malformed) lines as offsets.
fn parse_impl<'a>(
    mut input: &'a str,
    file: &'a str,
    junk_spans: &mut Vec<JunkSpan>,
    header_offsets: &mut HashMap<String, usize>,
) -> HashMap<String, Vec<ParsedNode<'a>>> {
    skip_trivia.parse_next(&mut input).ok();

    let mut sections: HashMap<String, Vec<ParsedNode<'a>>> = HashMap::new();

    while !input.is_empty() {
        skip_trivia.parse_next(&mut input).ok();
        if input.is_empty() {
            break;
        }

        // Byte offset of the start of this section header in `file`.
        let header_offset = consumed_since(input, file);

        let section_name = match section_header.parse_next(&mut input) {
            Ok(n) => n,
            Err(_) => {
                // Malformed section header: record and skip the whole line so
                // we can keep going and report *all* problems, not just the
                // first one.
                junk_spans.push(JunkSpan {
                    offset: header_offset,
                    line: crate::diag::line_number_for_offset(file, header_offset),
                    kind: JunkKind::SectionHeader,
                });
                let _: ModalResult<&str> = till_line_ending.parse_next(&mut input);
                let _ = consume_newline(&mut input);
                continue;
            }
        };

        skip_trivia.parse_next(&mut input).ok();

        match section_content(&mut input, file, junk_spans) {
            Ok(nodes) => {
                header_offsets.insert(section_name.to_string(), header_offset);
                sections.insert(section_name.to_string(), nodes);
            }
            Err(_) => {
                // section_content basically never errors (junk lines are
                // always consumed), but if it does, skip to the next line.
                let _: ModalResult<&str> = till_line_ending.parse_next(&mut input);
                let _ = consume_newline(&mut input);
            }
        }
    }

    sections
}

/// Main parser entry point.
///
/// Malformed lines are silently skipped (recorded in `junk_spans` if provided)
/// so the caller decides whether to render an error or proceed.
pub fn parse_espf_runtime_with_diagnostics<'a>(
    input: &'a str,
) -> (
    HashMap<String, Vec<ParsedNode<'a>>>,
    Vec<JunkSpan>,
    HashMap<String, usize>,
) {
    let mut junk_spans = Vec::new();
    let mut header_offsets = HashMap::new();
    let sections = parse_impl(input, input, &mut junk_spans, &mut header_offsets);
    (sections, junk_spans, header_offsets)
}

/// Backwards-compatible entry point: keeps the original strict-ish behavior of
/// returning a `ModalResult` and printing nothing, skipping junk lines.
pub fn parse_espf_runtime<'a>(
    input: &'a str,
) -> HashMap<String, Vec<ParsedNode<'a>>> {
    parse_espf_runtime_with_diagnostics(input).0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses the sample layout that used to be embedded in the CLI binary.
    /// All three sections must land with the expected node counts and ids.
    #[test]
    fn parses_sample_runtime_file() {
        let input = r#"
==============
= ESPF runtime file format instance
==============

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

        let sections = parse_espf_runtime(input);

        assert_eq!(sections["peripherals"].len(), 3);
        assert_eq!(sections["components"].len(), 2);
        assert_eq!(sections["devices"].len(), 2);

        let tmp102 = sections["devices"][0].clone();
        assert_eq!(tmp102.tag_name, "tmp102");
        assert_eq!(tmp102.id, "outdoor_temp");
    }

    /// A comment block may span multiple lines before and inside sections,
    /// and its body must not leak into sections as junk or nodes.
    #[test]
    fn block_comments_are_consumed_and_not_junk() {
        let input = concat!(
            "=\n",
            "title text survives on the opening line\n",
            "this body line and the blank one below are swallowed too\n",
            "\n",
            "==\n",
            "# peripherals\n",
            "  <gpio id=\"gpio4\" pin=\"4\" />\n",
            "=== trailing note\n",
            "closing line comes late\n",
            "=\n",
        );

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert!(junk.is_empty(), "comment lines must not be junk: {junk:?}");
        assert_eq!(sections["peripherals"].len(), 1);
    }

    /// A double-quoted value may not span lines: an unterminated quote must
    /// make the whole node line junk instead of reaching the next line's `"`.
    #[test]
    fn quoted_values_cannot_be_multiline() {
        let input = "# peripherals\n  <gpio id=\"gpio4\" pin=\"4 />\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        let peripherals = &sections["peripherals"];
        assert_eq!(peripherals.len(), 1, "only the well-formed spi node parses");
        assert_eq!(peripherals[0].tag_name, "spi");

        assert_eq!(junk.len(), 1, "expected exactly one junk line");
        assert_eq!(junk[0].line, 2);
        assert_eq!(&input[junk[0].offset..junk[0].offset + 5], "<gpio");
    }

    /// Trailing text after a leaf node that is not an inline `//` comment
    /// is junk: the whole line is reported.
    #[test]
    fn non_comment_text_after_leaf_node_is_junk() {
        let input = "# peripherals\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" /> --- note\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert!(sections["peripherals"].is_empty());
        assert_eq!(junk.len(), 1);
        assert_eq!(junk[0].line, 2);
    }

    /// An inline `//` comment directly after a self-closing element is
    /// grammar: the node parses and the comment is swallowed.
    #[test]
    fn inline_comment_after_leaf_node_parses() {
        let input = "# peripherals\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" /> // displays generally omit miso\n  <gpio id=\"gpio4\" pin=\"4\" />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert!(junk.is_empty(), "expected no junk, got {junk:?}");
        let peripherals = &sections["peripherals"];
        assert_eq!(peripherals.len(), 2);
        assert_eq!(peripherals[0].tag_name, "spi");
        assert_eq!(peripherals[0].get_attr("mosi"), Some("7"));
        assert_eq!(peripherals[1].tag_name, "gpio");
    }

    /// An inline `//` comment on its own line (not at the end of a
    /// self-closing element) is not grammar: it is reported as junk.
    #[test]
    fn inline_comment_not_after_leaf_node_is_junk() {
        let input = "# peripherals\n  // displays generally omit miso\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert_eq!(junk.len(), 1, "standalone // line must be junk: {junk:?}");
        assert_eq!(junk[0].line, 2);
        // The well-formed node still parses.
        assert_eq!(sections["peripherals"].len(), 1);
        assert_eq!(sections["peripherals"][0].tag_name, "spi");
    }

    /// Inline trailing comments after a section heading are no longer
    /// grammar: the trailing text is reported as junk, but the heading and
    /// the rest of the section still parse.
    #[test]
    fn no_comment_after_section_heading() {
        let input =
            "# peripherals --- note\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert_eq!(junk.len(), 1);
        assert_eq!(junk[0].line, 1);
        // The heading itself parsed, so the section is entered and its
        // well-formed nodes load.
        assert_eq!(sections["peripherals"].len(), 1);
        assert_eq!(sections["peripherals"][0].tag_name, "spi");
    }

    /// A malformed attribute value with no closing `"` must be reported as a
    /// junk line at a precise offset, not silently swallowed.
    #[test]
    fn rejects_node_with_missing_closing_quote() {
        let input = "# peripherals\n  <gpio id=\"gpio4\" pin=\"4 />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        // The malformed node must not come back with a truncated attribute.
        let peripherals = &sections["peripherals"];
        if !peripherals.is_empty() {
            let node = &peripherals[0];
            assert!(
                node.get_attr("pin").is_none(),
                "malformed pin attribute must not yield a value, got {:?}",
                node.get_attr("pin")
            );
        }

        // ...and the junk offset must point right at the node.
        assert_eq!(junk.len(), 1, "expected exactly one junk line");
        assert_eq!(junk[0].line, 2);
        assert_eq!(&input[junk[0].offset..junk[0].offset + 5], "<gpio");
    }

    /// A junk line inside a section is reported at the correct file offset.
    #[test]
    fn junk_offset_points_at_offending_line() {
        let input = "# peripherals\n<gpio id=\"a\"/ />\n";
        let (_sections, junk, _) = parse_espf_runtime_with_diagnostics(input);
        assert_eq!(junk.len(), 1);
        assert_eq!(junk[0].offset, "# peripherals\n".len());
    }
}
