use std::ops::Range;

/// The result of parsing an ESPF file.
///
/// Everything is owned: multi-line tags are joined into a new string before
/// their attributes are read, so values cannot always borrow from the source.
/// All spans are byte ranges into `source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub source: String,
    pub sections: Vec<Section>,
}

/// A `# name` heading and the elements under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    /// The heading line.
    pub span: Range<usize>,
    pub elements: Vec<Element>,
}

/// One `<tag attr="..." />`, possibly spread over several lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub tag: String,
    /// The explicit `id="..."` attribute, if present. Not repeated in `attrs`.
    pub id: Option<Attr>,
    pub attrs: Vec<Attr>,
    /// First line to last line of the element.
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub key: String,
    pub value: String,
    pub key_span: Range<usize>,
    /// The text between the quotes, so errors underline just the value.
    pub value_span: Range<usize>,
}

impl Document {
    /// The first section with this name.
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.name == name)
    }

    /// The source text covered by `span`.
    pub fn text(&self, span: Range<usize>) -> &str {
        &self.source[span]
    }
}

impl Section {
    /// The first element whose `node_id()` matches.
    pub fn get(&self, node_id: &str) -> Option<&Element> {
        self.elements
            .iter()
            .find(|element| element.node_id() == node_id)
    }
}

impl Element {
    /// The explicit id if there is one, otherwise the tag name
    /// (`<chip type="..." />` is `chip`, `<gpio id="gpio4" ... />` is `gpio4`).
    pub fn node_id(&self) -> &str {
        self.id
            .as_ref()
            .map(|id| id.value.as_str())
            .unwrap_or(self.tag.as_str())
    }

    pub fn attr(&self, key: &str) -> Option<&Attr> {
        self.attrs.iter().find(|attr| attr.key == key)
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.attr(key).map(|attr| attr.value.as_str())
    }
}
