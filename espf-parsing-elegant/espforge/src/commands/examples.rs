//! `espforge examples`: browse the ESPF examples embedded in the binary.
//!
//! Presents the example tree the same way it is laid out in the
//! `espforge-examples` crate: a top-level category picker followed by a picker
//! of the examples inside the chosen category. Categories without examples are
//! never offered. The selected example's `example.espf.txt` is printed
//! verbatim to stdout.
use std::io;

use cliclack::{Select, intro, outro_cancel};
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

    println!();
    println!("Category: {}", category.name);
    println!("Example:  {}", example.name);
    println!(
        "File:     {}/{}/{}",
        category.name, example.name, EXAMPLE_FILE
    );
    println!();
    println!("{}", example.content);

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
