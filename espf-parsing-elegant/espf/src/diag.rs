//! `annotate-snippets`-backed diagnostics for malformed ESPF runtime files.
//!
//! The parser reports *byte offsets* into the original input that it could not
//! make sense of. This module turns each one into a rustc-style snippet:
//!
//! ```text
//! error: unexpected end of file inside node (missing closing `/>`?)
//!   --> bad.espf.txt:2:3
//!    |
//!  2 |   <gpio id="gpio4" pin="4 />
//!    |   ^ expected a self-closing tag like <tag attr="v" /> here
//! ```

use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
use std::ops::Range;

/// A byte offset into the ESPF source that the parser could not interpret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JunkSpan {
    pub offset: usize,
    pub line: usize,
}

/// The underlying cause of an ESPF diagnostic.
#[derive(Debug)]
pub enum EspfSnippetError {
    /// A malformed self-closing tag line that the parser had to skip.
    UnterminatedNode { span: Range<usize> },
    /// A syntax error inside a `# section` header.
    InvalidSection { span: Range<usize> },
    /// Anything else (e.g. i/o failure while reading the file).
    Other { message: String },
}

impl EspfSnippetError {
    fn title(&self) -> String {
        match self {
            Self::UnterminatedNode { .. } => {
                "unexpected end of file inside node (missing closing `/>`?)".into()
            }
            Self::InvalidSection { .. } => {
                "invalid section header (expected `# name`)".into()
            }
            Self::Other { message } => format!("espf error: {message}"),
        }
    }
}

/// Line-start byte offsets of every line in `src` (0-indexed line numbers).
fn line_start_offsets(src: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(src.char_indices().filter_map(|(i, c)| (c == '\n').then_some(i + 1)))
        .collect()
}

/// Given byte offsets of every line start in `src`, find the byte offset of the
/// line that contains `offset`, plus that line's 1-based number.
fn locate(line_starts: &[usize], offset: usize) -> (usize, usize) {
    let idx = line_starts.partition_point(|&start| start <= offset) - 1;
    (line_starts[idx], idx + 1)
}

/// Line number (1-based) of a byte offset in `src`.
//
// `line_start_offsets` prepends byte offset 0, so simply counting entries that
// start at or before `offset` gives the 1-based line number for free.
pub fn line_number_for_offset(src: &str, offset: usize) -> usize {
    let ls = line_start_offsets(src);
    ls.partition_point(|&start| start <= offset)
}

/// Render the report for a set of offending byte offsets.
pub fn render_espf_report(
    path: &str,
    src: &str,
    offsets: &[JunkSpan],
    primary: &EspfSnippetError,
) -> String {
    let title = Level::ERROR.primary_title(primary.title());
    let mut group = Group::with_title(title);

    for (i, junk) in offsets.iter().enumerate() {
        let starts = line_start_offsets(src);
        let (line_start_idx, line_number) = locate(&starts, junk.offset);
        let line_end = src[line_start_idx..]
            .find('\n')
            .map_or(src.len(), |i| line_start_idx + i);

        // Show the offending line, folding everything else away.
        let view = &src[line_start_idx..line_end];
        let col_in_view = junk.offset.saturating_sub(line_start_idx).min(line_end - line_start_idx);

        let kind = if i == 0 {
            AnnotationKind::Primary
        } else {
            AnnotationKind::Context
        };

        group = group.element(
            Snippet::source(view)
                .line_start(line_number)
                .path(path)
                .fold(true)
                .annotation(
                    kind.span(col_in_view..(col_in_view + 1))
                        .label("expected a self-closing tag like <tag attr=\"v\" /> here")
                        .highlight_source(true),
                ),
        );
    }

    Renderer::styled().term_width(100).render(&[group])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unterminated_node_renders_snippet() {
        let src = "# peripherals\n  <gpio id=\"gpio4\" pin=\"4 />\n";
        let offsets = vec![JunkSpan { offset: 16, line: 2 }];
        let err = EspfSnippetError::UnterminatedNode { span: 16..16 };
        let out = render_espf_report("bad.espf.txt", src, &offsets, &err);
        eprintln!("{out}");
        assert!(out.contains("unexpected end of file inside node"));
        assert!(out.contains("bad.espf.txt:2"));
    }
}
