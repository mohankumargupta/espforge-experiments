use std::{collections::HashMap, fmt, ops::Range};

use espf::{Attr, Document, Element, ParseDiagnostic, Section};

// -----------------------------------------------------------------------------
// Diagnostics
// -----------------------------------------------------------------------------

/// Every error found in one run, in the order they were reported.
#[derive(Debug)]
pub struct Diagnostics(pub Vec<ParseDiagnostic>);

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{diagnostic}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostics {}

// -----------------------------------------------------------------------------
// Symbols
// -----------------------------------------------------------------------------

/// An element that was given an explicit `id`, so other elements can refer to
/// it as `$id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The element's tag: `gpio`, `i2c`, ...
    pub tag: String,
    /// The implicit hierarchical id, e.g. `peripherals.gpio.pin4`.
    pub path: String,
    pub id_span: Range<usize>,
}

#[derive(Debug, Default)]
pub struct Symbols {
    by_id: HashMap<String, Symbol>,
}

impl Symbols {
    pub fn get(&self, id: &str) -> Option<&Symbol> {
        self.by_id.get(id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Ids of every symbol with this tag, sorted.
    pub fn ids_with_tag(&self, tag: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .by_id
            .iter()
            .filter(|(_, symbol)| symbol.tag == tag)
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }
}

// -----------------------------------------------------------------------------
// Context
// -----------------------------------------------------------------------------

/// State shared by every stage: the symbol table and the error list.
///
/// Stages report an error and carry on, so one run shows every problem.
pub struct Cx<'d> {
    doc: &'d Document,
    diagnostics: Vec<ParseDiagnostic>,
    symbols: Symbols,
}

impl<'d> Cx<'d> {
    pub fn new(doc: &'d Document) -> Self {
        Self {
            doc,
            diagnostics: Vec::new(),
            symbols: Symbols::default(),
        }
    }

    pub fn symbols(&self) -> &Symbols {
        &self.symbols
    }

    /// Report an error at `span` (a byte range of the source).
    pub fn error(&mut self, span: Range<usize>, message: impl Into<String>) {
        self.diagnostics
            .push(ParseDiagnostic::new(&self.doc.source, span, message));
    }

    /// Record the element's explicit `id` under its hierarchical `path`.
    /// Elements without an explicit id cannot be referenced, so they are skipped.
    pub fn declare(&mut self, element: &Element, path: String) {
        let Some(id) = &element.id else {
            return;
        };

        let duplicate = self
            .symbols
            .by_id
            .get(&id.value)
            .map(|existing| format!("Duplicate id `{}` (already used by {})", id.value, existing.path));

        if let Some(message) = duplicate {
            self.error(id.value_span.clone(), message);
            return;
        }

        self.symbols.by_id.insert(
            id.value.clone(),
            Symbol {
                tag: element.tag.clone(),
                path,
                id_span: id.value_span.clone(),
            },
        );
    }

    /// Report every attribute that is not in `allowed`. `id` is always allowed.
    pub fn check_attrs(&mut self, element: &Element, allowed: &[&str]) {
        for attr in &element.attrs {
            if !allowed.contains(&attr.key.as_str()) {
                self.error(
                    attr.key_span.clone(),
                    format!(
                        "Unknown attribute `{}` on <{}>; expected {}",
                        attr.key,
                        element.tag,
                        quoted_list(allowed),
                    ),
                );
            }
        }
    }

    /// The attribute `key`, or an error pointing at the element if it is missing.
    pub fn require<'e>(&mut self, element: &'e Element, key: &str) -> Option<&'e Attr> {
        let attr = element.attr(key);

        if attr.is_none() {
            self.error(
                element.span.clone(),
                format!("<{}> is missing required attribute `{key}`", element.tag),
            );
        }

        attr
    }

    /// The one `<tag>` element of a section. Missing is an error, and so is
    /// every element after the first.
    pub fn single<'s>(&mut self, section: &'s Section, tag: &str) -> Option<&'s Element> {
        let mut found = section.elements.iter().filter(|element| element.tag == tag);
        let first = found.next();

        for extra in found {
            self.error(
                extra.span.clone(),
                format!("Duplicate <{tag}>; `{}` allows only one", section.name),
            );
        }

        if first.is_none() {
            self.error(
                section.span.clone(),
                format!("Section `{}` needs a <{tag}> element", section.name),
            );
        }

        first
    }

    /// Hand back the symbol table, or every error reported along the way.
    pub fn finish(self) -> Result<Symbols, Diagnostics> {
        if self.diagnostics.is_empty() {
            Ok(self.symbols)
        } else {
            Err(Diagnostics(self.diagnostics))
        }
    }
}

/// `` `a`, `b`, `c` `` for error messages.
pub(crate) fn quoted_list(items: &[&str]) -> String {
    items
        .iter()
        .map(|item| format!("`{item}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
