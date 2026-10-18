use espf::context::{ParsedNode, RuntimeContext};
use espforge_bindings::drivers::generated_registry::route_driver;
use std::path::Path;

use super::frontend;

/// Validate and execute an ESPF runtime file end-to-end.
pub fn run(file: &Path) {
    let src = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("❌ Cannot read '{}': {}", file.display(), e);
            std::process::exit(1);
        }
    };
    let name = file.to_string_lossy();

    // 1-2. Syntax, structure, `# espforge` core and `<options>`.
    let fe = match frontend::load(&src) {
        Ok(fe) => fe,
        Err(errors) => frontend::fail(&name, &src, &errors),
    };

    // 3. Option resolution (defaults, derived flags, requires, compatibility).
    if let Err(errors) = espforge_options::resolve(&fe.meta, &fe.answers, &fe.derived) {
        frontend::fail(&name, &src, &errors);
    }

    // 4. `$` dependency graph across sections.
    let empty: Vec<ParsedNode> = Vec::new();
    let section = |n: &str| fe.parsed.sections.get(n).unwrap_or(&empty);
    let mut ctx = RuntimeContext::new();
    if let Err(errors) = ctx.resolve_and_bind(
        section("peripherals"),
        section("components"),
        section("devices"),
    ) {
        frontend::fail(&name, &src, &errors);
    }

    for device in &ctx.resolved_devices {
        if let Err(msg) = route_driver(device.tag_name, device, &ctx) {
            eprintln!("❌ {msg}");
            std::process::exit(1);
        }
    }

    println!("✓ {name} is valid");
}
