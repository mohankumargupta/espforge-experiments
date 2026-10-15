//! Embedded ESPF examples for the `espforge` binary.
//!
//! The `examples` directory of this crate is compiled into the binary with
//! `include_dir`, and the helpers here let callers walk it the same way it is
//! laid out on disk: top-level categories (`01.Basics`, ...), each containing
//! example directories (`helloworld`, ...) that hold an `example.espf.txt`.

use std::ffi::OsStr;

use include_dir::{include_dir, Dir};

/// The example tree, embedded at compile time from `src/examples`.
pub static EXAMPLES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/examples");

/// Name of the ESPF source file inside each example directory.
const EXAMPLE_FILE: &str = "example.espf.txt";

/// A top-level example category, e.g. `01.Basics`.
pub struct Category {
    /// Category directory name, e.g. `01.Basics`.
    pub name: String,
    /// Examples directly under this category, sorted by name.
    pub examples: Vec<Example>,
}

/// A single example (a directory containing `example.espf.txt`).
pub struct Example {
    /// Example directory name, e.g. `helloworld`.
    pub name: String,
    /// Human-facing title taken from the banner at the top of the example
    /// file, when one is present (e.g. `HELLO WORLD EXAMPLE`).
    pub title: Option<String>,
    /// Full contents of the example's `example.espf.txt`.
    pub content: String,
}

/// All categories that contain at least one example, sorted by name.
pub fn categories() -> Vec<Category> {
    let mut categories: Vec<Category> = EXAMPLES_DIR
        .dirs()
        .filter_map(|cat_dir| {
            let examples: Vec<Example> = cat_dir
                .dirs()
                .filter_map(example_from_dir)
                .collect();
            (!examples.is_empty()).then(|| Category {
                name: last_path_segment(cat_dir),
                examples,
            })
        })
        .collect();
    categories.sort_by(|a, b| a.name.cmp(&b.name));
    for category in &mut categories {
        category.examples.sort_by(|a, b| a.name.cmp(&b.name));
    }
    categories
}

fn example_from_dir(ex_dir: &Dir) -> Option<Example> {
    let file = ex_dir
        .files()
        .find(|f| f.path().file_name() == Some(OsStr::new(EXAMPLE_FILE)))?;
    let content = file.contents_utf8()?.to_string();
    let title = extract_title(&content);
    Some(Example {
        name: last_path_segment(ex_dir),
        title,
        content,
    })
}

fn last_path_segment<'a>(dir: &'a Dir<'a>) -> String {
    dir.path()
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_string()
}

/// Pull the human title out of the example ADF: the text between the opening
/// `====` banner and the closing one, trimmed. Returns nothing when the
/// top-of-file layout is different.
fn extract_title(content: &str) -> Option<String> {
    let mut lines = content.lines().skip_while(|l| l.trim().is_empty());
    match lines.next()? {
        // File opens with a `====` banner: the next non-banner line is the title.
        first if is_banner(first) => lines
            .find(|l| !is_banner(l) && !l.trim().is_empty())
            .map(|l| l.trim().to_string()),
        first => Some(first.trim().to_string()),
    }
}

fn is_banner(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| c == '=')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_embed_and_parse() {
        let categories = categories();
        assert!(!categories.is_empty());
        let basics = &categories[0];
        assert_eq!(basics.name, "01.Basics");
        assert_eq!(basics.examples[0].name, "helloworld");
        assert_eq!(basics.examples[0].title.as_deref(), Some("HELLO WORLD EXAMPLE"));
        assert!(basics.examples[0].content.contains("# espforge"));
    }

    #[test]
    fn empty_categories_are_dropped() {
        // A category with no `example.espf.txt` is dropped, so categories()
        // must never contain an empty category. `02.Digital` used to be empty
        // and dropped; it now carries the converted button examples.
        let categories = categories();
        assert!(categories.iter().all(|c| !c.examples.is_empty()));
        let digital = categories
            .iter()
            .find(|c| c.name == "02.Digital")
            .expect("02.Digital examples must be embedded");
        assert_eq!(digital.examples.len(), 2);
    }
}
