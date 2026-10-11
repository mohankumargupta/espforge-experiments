use std::{fmt, ops::Range};

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};

/// A diagnostic refers to a byte range in the original source.
#[derive(Debug)]
pub struct ParseDiagnostic {
    source: String,
    span: Range<usize>,
    message: String,
}

impl ParseDiagnostic {
    pub fn new(source: &str, span: Range<usize>, message: impl Into<String>) -> Self {
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

pub(crate) fn diagnostic_at_current(
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
