//! `espforge examples`: turn an embedded example into an editable folder.
//!
//! Bare `examples` is porcelain (cli.md layer 2): a top-level category picker
//! followed by a picker of the examples inside the chosen category (categories
//! without examples are never offered), then an `Input` prompt for the
//! destination folder pre-filled with the example's own directory name — Enter
//! accepts it, the user may retype it. The chosen example is materialized to
//! that folder and the full pipeline is printed as next steps (edit the
//! example, then `espforge build`).
//!
//! The subcommands `list` / `get` are the plumbing surface from `cli.md`:
//! non-interactive, plain output, they only delegate to `espforge_examples`.
//! Porcelain never duplicates plumbing logic: picking, defaults and printed
//! next-steps live here; copying lives in `espforge_examples::materialize`.
use std::io;
use std::path::Path;

use anyhow::Context;

use cliclack::{Input, Select, intro, outro, outro_cancel};
use espforge_examples::{Category, Example};

/// Name of the ESPF source file inside each example directory.
const EXAMPLE_FILE: &str = "example.espf.txt";

pub fn run() {
    match run_ui() {
        Ok(()) => {}
        // Esc / Ctrl-C inside a cliclack prompt surfaces as `Interrupted`.
        Err(err) if err.kind() == io::ErrorKind::Interrupted => {
            let _ = outro_cancel("Cancelled.");
            std::process::exit(130);
        }
        Err(err) => {
            eprintln!("⚠️ Could not run the example browser: {err}");
            std::process::exit(2);
        }
    }
}

fn run_ui() -> io::Result<()> {
    intro("espforge examples")?;

    let categories = espforge_examples::categories();
    if categories.is_empty() {
        let _ = outro_cancel("No examples are embedded in this build.");
        return Ok(());
    }

    // Top-level picker: the example categories that actually have examples.
    let entries: Vec<(usize, String, String)> = categories
        .iter()
        .enumerate()
        .map(|(i, category)| (i, category.name.clone(), category_hint(category)))
        .collect();
    let category_index = Select::<usize>::new("Select a category")
        .items(&entries)
        .interact()?;
    let category = &categories[category_index];

    // Second picker: the examples inside the chosen category.
    let entries: Vec<(usize, String, String)> = category
        .examples
        .iter()
        .enumerate()
        .map(|(i, example)| (i, example.name.clone(), example_hint(example)))
        .collect();
    let example_index = Select::<usize>::new(format!(
        "Select an example in {}",
        category.name
    ))
    .items(&entries)
    .interact()?;
    let example = &category.examples[example_index];

    // Destination prompt: pre-filled with the example's own directory name;
    // Enter accepts it, the user may retype it. Empty input falls back to the
    // default, so the suggested destination cannot be lost to a stray Enter.
    let dest: String = Input::new(format!(
        "Where should '{}/{}' be copied to?",
        category.name, example.name
    ))
    .default_input(&example.name)
    .interact()?;

    // Porcelain delegates the copying to the plumbing layer. A conflict (the
    // destination already holds files) cancels instead of overwriting.
    let written = match espforge_examples::materialize(example, Path::new(&dest)) {
        Ok(written) => written,
        Err(err) => {
            let _ = outro_cancel(format!("Could not copy the example: {err}"));
            std::process::exit(2);
        }
    };

    // The full pipeline (cli.md): stage 1 done here, stages 2+3 are the
    // user's edits followed by `build` — in build's `next:` style.
    outro(format!(
        "Copied {}/{} to {} ({} files)",
        category.name,
        example.name,
        dest,
        written.len()
    ))?;
    println!(
        "  next: cd {dest} → edit the example → espforge build {dest}/{EXAMPLE_FILE}"
    );

    Ok(())
}

/// `examples list [category]`: plain `category/name<TAB>title` lines.
pub fn run_list(category: Option<&str>) -> anyhow::Result<()> {
    let lines = espforge_examples::list(category)
        .map_err(|err| anyhow::anyhow!("{err}"))
        .context("failed to list examples")?;
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

/// `examples get <category>/<name> --out <dir>`: materialize the embedded
/// example tree at the user-chosen destination.
pub fn run_get(example_path: &str, out: &Path) -> anyhow::Result<()> {
    let example = espforge_examples::find(example_path)
        .context(format!(
            "no example at '{example_path}'; run `espforge examples list` to see what is available"
        ))?;
    let written = espforge_examples::materialize(example, out)
        .with_context(|| format!("failed to copy '{example_path}' into {}", out.display()))?;
    for path in written {
        println!("{}", path.display());
    }
    Ok(())
}

/// Human title from the example file's banner, empty when there is none.
fn example_hint(example: &Example) -> String {
    match &example.title {
        Some(title) => title.clone(),
        None => String::new(),
    }
}

/// Number of examples in the category, singular when there's exactly one.
fn category_hint(category: &Category) -> String {
    let noun = if category.examples.len() == 1 {
        "example"
    } else {
        "examples"
    };
    format!("{} {noun}", category.examples.len())
}
