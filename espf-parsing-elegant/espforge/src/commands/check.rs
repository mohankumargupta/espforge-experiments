use espf::context::RuntimeContext;
use espf::diag::{EspfError, render_errors};
use espforge_bindings::drivers::generated_registry::route_driver;
use std::path::Path;

use crate::sections::espforge::parse_meta;

/// Validate and execute an ESPF runtime file end-to-end.
pub fn run(file: &Path) {
    let espf_runtime_file = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("❌ Cannot read '{}': {}", file.display(), e);
            std::process::exit(1);
        }
    };

    let file_str = file.to_string_lossy();
    let (mut parsed_sections, junk_spans, section_offsets) =
        espf::parser::parse_espf_runtime_with_diagnostics(&espf_runtime_file);

    // 1. Syntax: junk lines the parser could not interpret.
    let errors = espf::diag::junk_errors(&junk_spans);
    if !errors.is_empty() {
        fail_with_diagnostics(&file_str, &espf_runtime_file, &errors);
    }

    let Some(espforge_nodes) = parsed_sections.remove("espforge") else {
        eprintln!("❌ '{}' has no `# espforge` section", file_str);
        std::process::exit(1);
    };

    // 2. Meta schema: everything under `# espforge`.
    if let Err(errors) = parse_meta(
        section_offsets.get("espforge").copied().unwrap_or(0),
        &espforge_nodes,
    ) {
        fail_with_diagnostics(&file_str, &espf_runtime_file, &errors);
    }

    let empty = vec![];
    let peripherals = parsed_sections
        .remove("peripherals")
        .unwrap_or(empty.clone());
    let components = parsed_sections
        .remove("components")
        .unwrap_or(empty.clone());
    let devices = parsed_sections.remove("devices").unwrap_or(empty);

    // 3. `$` dependency graph across sections.
    let mut runtime_ctx = RuntimeContext::new();
    if let Err(errors) = runtime_ctx.resolve_and_bind(&peripherals, &components, &devices) {
        fail_with_diagnostics(&file_str, &espf_runtime_file, &errors);
    }

    for resolved_device in &runtime_ctx.resolved_devices {
        route_driver(resolved_device.tag_name, resolved_device, &runtime_ctx).unwrap();
    }

    println!("✓ {} is valid", file_str);
}

/// Print each errors as a snippet block, then a one-line summary, then exit.
pub(crate) fn fail_with_diagnostics(file_path: &str, src: &str, errors: &[EspfError]) -> ! {
    eprintln!("{}", render_errors(file_path, src, errors));
    eprintln!(
        "❌ Found {} problem(s) in '{}'; fix them and re-run.",
        errors.len(),
        file_path
    );
    std::process::exit(1);
}
