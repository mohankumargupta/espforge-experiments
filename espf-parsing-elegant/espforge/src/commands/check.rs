use espf::context::RuntimeContext;
use espf::diag::{EspfSnippetError, JunkSpan, render_espf_report};
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

    println!("--- Phase 1: Launching Winnow Zero-Copy String Tokenizer ---");
    let file_str = file.to_string_lossy();
    let (mut parsed_sections, junk_spans) =
        espf::parser::parse_espf_runtime_with_diagnostics(&espf_runtime_file);

    if render_syntax_diagnostics(&file_str, &espf_runtime_file, &junk_spans) {
        std::process::exit(1);
    }

    println!("--- Phase 2: Reading the `# espforge` section ---");
    let Some(espforge_nodes) = parsed_sections.remove("espforge") else {
        eprintln!("❌ '{}' has no `# espforge` section", file_str);
        std::process::exit(1);
    };
    let meta = match parse_meta(&espforge_nodes) {
        Ok(meta) => meta,
        Err(errors) => {
            for e in &errors {
                eprintln!("❌ {e}");
            }
            eprintln!(
                "❌ Found {} problem(s) in `# espforge` of '{}'",
                errors.len(),
                file_str
            );
            std::process::exit(1);
        }
    };
    println!(
        "project: {} | chip: {} | runtime: {:?}",
        meta.name, meta.chip, meta.runtime
    );

    // Harvest empty fallbacks if specific sections are missing from target load file
    let empty = vec![];
    let peripherals = parsed_sections
        .remove("peripherals")
        .unwrap_or(empty.clone());
    let components = parsed_sections
        .remove("components")
        .unwrap_or(empty.clone());
    let devices = parsed_sections.remove("devices").unwrap_or(empty);

    println!("--- Phase 3: Resolving Dynamic Topological Dependency Chains ---");
    let mut runtime_ctx = RuntimeContext::new();
    if let Err(e) = runtime_ctx.resolve_and_bind(&peripherals, &components, &devices) {
        eprintln!("\n{e}");
        eprintln!(
            "❌ Dependency resolution failed in '{}'; check that every '$' reference points to an id defined in an earlier section.",
            file_str
        );
        std::process::exit(1);
    }

    println!("--- Phase 4: Executing Safe Array Drivers Dispatch Sweeps ---\n");
    for resolved_device in &runtime_ctx.resolved_devices {
        // Route driver matches string names using code generated instantly by build.rs
        route_driver(resolved_device.tag_name, resolved_device, &runtime_ctx).unwrap();
    }

    println!("\n🎉 System pipeline successfully initialised and executing.");
}

fn render_syntax_diagnostics(file_path: &str, espf_source: &str, junk: &[JunkSpan]) -> bool {
    if junk.is_empty() {
        return false;
    }

    let primary = EspfSnippetError::UnterminatedNode {
        span: junk[0].offset..junk[0].offset,
    };
    let report = render_espf_report(file_path, espf_source, junk, &primary);
    eprintln!("{report}");
    eprintln!(
        "❌ Found {} malformed line(s) in '{}'; run with a corrected file.",
        junk.len(),
        file_str_or(file_path)
    );
    true
}

fn file_str_or(path: &str) -> &str {
    path
}
