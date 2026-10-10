use std::{collections::HashMap, fmt, ops::Range};

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use winnow::{
    ascii::{line_ending, space0, till_line_ending},
    combinator::{delimited, opt},
    prelude::*,
    stream::{Location, LocatingSlice},
    token::{take_till, take_while},
};

pub type LeafFields = HashMap<String, String>;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LeafTable {
    pub fields: LeafFields,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct EspForgeSection {
    pub project_name: String,
    pub project_description: String,
    pub chip_type: String,
    pub runtime_type: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Line<'a> {
    Section(&'a str),
    Tag(&'a str),
    /// A `--- ...` comment line: skipped mid-tag, so a multi-line element can
    /// carry interleaved comments.
    Comment,
    Ignore,
}

#[derive(Debug, PartialEq, Eq)]
struct TagInfo<'a> {
    name: &'a str,
    id: Option<&'a str>,
}

/// A diagnostic refers to a byte range in the original source.
#[derive(Debug)]
pub struct ParseDiagnostic {
    source: String,
    span: Range<usize>,
    message: String,
}

impl ParseDiagnostic {
    fn new(source: &str, span: Range<usize>, message: impl Into<String>) -> Self {
        let start = span.start.min(source.len());
        let end = span.end.min(source.len()).max(start);

        Self {
            source: source.to_owned(),
            span: start..end,
            message: message.into(),
        }
    }

    /// Render a source-annotated diagnostic.
    pub fn render(&self) -> String {
        let span = non_empty_span(&self.source, self.span.clone());
        let message = self.message.as_str();

        let report = [Level::ERROR.primary_title(message).element(
            Snippet::source(self.source.as_str())
                .line_start(1)
                .annotation(AnnotationKind::Primary.span(span).label(message)),
        )];

        Renderer::plain().render(&report)
    }
}

impl fmt::Display for ParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

impl std::error::Error for ParseDiagnostic {}

/// annotate_snippets annotations are easier to see with a non-empty span.
/// For an empty line, highlight the newline if one exists, otherwise EOF.
fn non_empty_span(source: &str, span: Range<usize>) -> Range<usize> {
    if span.start < span.end {
        return span;
    }

    if span.start < source.len() {
        let next = source[span.start..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);

        return span.start..(span.start + next).min(source.len());
    }

    if source.is_empty() {
        return 0..0;
    }

    source[..span.start]
        .char_indices()
        .next_back()
        .map(|(index, ch)| index..index + ch.len_utf8())
        .unwrap_or(0..source.len())
}

// -----------------------------------------------------------------------------
// Public API
// -----------------------------------------------------------------------------

pub fn parse_to_leaf_table(source: &str) -> Result<LeafTable, ParseDiagnostic> {
    let mut input = LocatingSlice::new(source);
    let mut table = LeafTable::default();
    let mut section: Option<&str> = None;

    while !input.is_empty() {
        // LocatingSlice tracks this line's range in the original source.
        let (line, line_span) = till_line_ending
            .with_span()
            .parse_next(&mut input)
            .map_err(|error:    winnow::error::ContextError| {
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

impl EspForgeSection {

    pub fn extract_from(table: &LeafTable) -> Self {
        Self {
            project_name: get_field(table, "espforge.project.name"),
            project_description: get_field(table, "espforge.project.description"),
            chip_type: get_field(table, "espforge.chip.type"),
            runtime_type: get_field(table, "espforge.runtime.type"),
        }
    }
}

fn diagnostic_at_current(
    source: &str,
    offset: usize,
    message: impl Into<String>,
) -> ParseDiagnostic {
    ParseDiagnostic::new(
        source,
        offset..offset.saturating_add(1).min(source.len()),
        message,
    )
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
fn finish_tag_body(
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
// Line parsing
// -----------------------------------------------------------------------------

fn parse_line(line: &str) -> Result<Line<'_>, String> {
    let mut input = line;
    let _: Result<(&str, &str), ()> = (space0, space0).parse_next(&mut input); // always succeeds

    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return Ok(Line::Ignore);
    };

    match first {
        '#' => parse_section_tail(chars.as_str()),
        '<' => Ok(Line::Tag(chars.as_str())),
        '-' => Ok(Line::Comment),
        _ => Ok(Line::Ignore),
    }
}

fn parse_section_tail(input: &str) -> Result<Line<'_>, String> {
    let mut input = input;
    let _: Result<(&str, &str), ()> = (space0, space0).parse_next(&mut input); // always succeeds

    let name_end = input
        .find(|ch: char| ch.is_whitespace())
        .unwrap_or(input.len());

    if name_end == 0 {
        return Err("Section heading requires a name".to_owned());
    }

    Ok(Line::Section(&input[..name_end]))
}

// -----------------------------------------------------------------------------
// Tag and attribute parsing
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

fn parse_tag<'a>(input: &mut &'a str) -> Result<TagInfo<'a>, String> {
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

// -----------------------------------------------------------------------------
// Owned table insertion
// -----------------------------------------------------------------------------

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

fn get_field(table: &LeafTable, path: &str) -> String {
    table.fields.get(path).cloned().unwrap_or_default()
}

// -----------------------------------------------------------------------------
// Binary entry point
// -----------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    let Some(path) = std::env::args().nth(1) else {
        anyhow::bail!("usage: espf_winnow <file.espf>");
    };
    let source = std::fs::read_to_string(&path)?;
    let table = parse_to_leaf_table(&source)?;
    let mut paths: Vec<&String> = table.fields.keys().collect();
    paths.sort();

    for (path, value) in paths.into_iter().map(|p| (p, &table.fields[p])) {
        println!("{path} = {value}");
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"---
--- ESPF file format
--- mandatory sections: espforge, peripherals, components, devices

# espforge
  <project name="example project" description="example showing espf file format" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals
  <gpio id="gpio4" pin="4" />
  <gpio id="gpio5" pin="5" />
  <i2c id="i2c0" sda="8" scl="9" frequency="400kHz" />
  <spi id="spi2" sclk="6" mosi="7" /> --- displays generally omit miso
"#;

    const MULTI_LINE_CONFIG: &str = "# espforge\n  <project name=\"example project\"\n           description = \"example showing espf file format\" />\n  <chip type=\"esp32c3\" />\n\n# peripherals\n  <i2c id=\"i2c0\" sda=\"8\" scl=\"9\"\n      frequency=\"400kHz\" />\n";

    #[test]
    fn parses_config() {
        let table = parse_to_leaf_table(CONFIG).unwrap();


        assert_eq!(
            table.fields.get("espforge.project.name").unwrap(),
            "example project"
        );
        assert_eq!(
            table.fields.get("espforge.project.description").unwrap(),
            "example showing espf file format"
        );
        assert_eq!(table.fields.get("espforge.chip.type").unwrap(), "esp32c3");
        assert_eq!(table.fields.get("peripherals.gpio4.pin").unwrap(), "4");
        assert_eq!(
            table.fields.get("peripherals.i2c0.frequency").unwrap(),
            "400kHz"
        );
    }

    #[test]
    fn accepts_tag_with_trailing_comment() {
        let table = parse_to_leaf_table(CONFIG).unwrap();

        assert_eq!(table.fields.get("peripherals.spi2.sclk").unwrap(), "6");
        assert_eq!(table.fields.get("peripherals.spi2.mosi").unwrap(), "7");
    }

    #[test]
    fn extracts_espforge_section() {
        let table = parse_to_leaf_table(CONFIG).unwrap();
        let section = EspForgeSection::extract_from(&table);

        assert_eq!(section.project_name, "example project");
        assert_eq!(section.chip_type, "esp32c3");
        assert_eq!(section.runtime_type, "blocking");
    }

    #[test]
    fn reports_malformed_attributes() {
        let source = "# espforge\n<chip type \"esp32c3\" />\n";
        let error = parse_to_leaf_table(source).unwrap_err();
        let rendered = error.render();

        assert!(rendered.contains("Invalid attribute"));
        assert!(rendered.contains("esp32c3"));
    }

    #[test]
    fn reports_unclosed_quotes() {
        let source = "# espforge\n<chip type=\"esp32c3 />\n";
        assert!(parse_to_leaf_table(source).is_err());
    }

    #[test]
    fn reports_duplicate_paths() {
        let source = concat!(
            "# espforge\n",
            "<chip type=\"esp32c3\" />\n",
            "<chip type=\"esp32c6\" />\n",
        );

        let error = parse_to_leaf_table(source).unwrap_err();
        assert!(error.to_string().contains("Duplicate field path"));
    }

    #[test]
    fn reports_duplicate_ids() {
        let source = "# espforge\n<gpio id=\"one\" id=\"two\" pin=\"4\" />\n";

        let error = parse_to_leaf_table(source).unwrap_err();
        assert!(error.to_string().contains("Duplicate `id`"));
    }

    #[test]
    fn reports_tags_before_sections() {
        let source = "<chip type=\"esp32c3\" />\n";
        let error = parse_to_leaf_table(source).unwrap_err();

        assert!(error.to_string().contains("before a section"));
    }

    #[test]
    fn accepts_tag_spanning_multiple_lines() {
        let source = concat!(
            "# espforge\n",
            "  <project name=\"example project\"\n",
            "           description=\"example showing espf file format\" />\n",
        );
        let table = parse_to_leaf_table(source).unwrap();

        assert_eq!(
            table.fields.get("espforge.project.name").unwrap(),
            "example project"
        );
        assert_eq!(
            table.fields.get("espforge.project.description").unwrap(),
            "example showing espf file format"
        );
    }

    #[test]
    fn accepts_multi_line_tag_in_larger_file() {
        let table = parse_to_leaf_table(MULTI_LINE_CONFIG).unwrap();

        assert_eq!(
            table.fields.get("espforge.project.description").unwrap(),
            "example showing espf file format"
        );
        assert_eq!(table.fields.get("espforge.chip.type").unwrap(), "esp32c3");
        assert_eq!(
            table.fields.get("peripherals.i2c0.frequency").unwrap(),
            "400kHz"
        );
    }

    #[test]
    fn accepts_comments_between_lines_of_multi_line_tag() {
        let source = concat!(
            "# espforge\n",
            "  <chip\n",
            "    --- this is the main chip\n",
            "    type=\"esp32c3\" />\n",
        );
        let table = parse_to_leaf_table(source).unwrap();

        assert_eq!(table.fields.get("espforge.chip.type").unwrap(), "esp32c3");
    }

    #[test]
    fn reports_unclosed_multi_line_tag() {
        let source = concat!(
            "# espforge\n",
            "  <chip\n",
            "    type=\"esp32c3\"  \n",  // no closing /> — should error
        );
        let error = parse_to_leaf_table(source).unwrap_err();

        assert!(error.to_string().contains("Unterminated tag"));
    }

    #[test]
    fn reports_tag_before_section_in_multi_line_tag() {
        let source = "  <chip type=\"esp32c3\"\n           />\n";
        let error = parse_to_leaf_table(source).unwrap_err();

        assert!(error.to_string().contains("before a section"));
    }

    #[test]
    fn accepts_final_line_without_newline() {
        let source = "# espforge\n<chip type=\"esp32c3\" />";
        let table = parse_to_leaf_table(source).unwrap();

        assert_eq!(table.fields.get("espforge.chip.type").unwrap(), "esp32c3");
    }
}
