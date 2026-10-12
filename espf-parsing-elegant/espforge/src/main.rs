use clap::{Parser, Subcommand};
use esforge_bindings::drivers::generated_registry::route_driver;
use espf::context::RuntimeContext;
use espf::diag::{render_espf_report, EspfSnippetError, JunkSpan};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "espforge", version, about = "ESPF runtime pipeline tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate and execute an ESPF runtime file end-to-end.
    Check {
        /// Path to the ESPF runtime file to check
        file: PathBuf,
    },
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
        file_path
    );
    true
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Check { file } => {
            let espf_runtime_file = match std::fs::read_to_string(&file) {
                Ok(content) => content,
                Err(e) => {
                    eprintln!("❌ Cannot read '{}': {}", file.display(), e);
                    std::process::exit(1);
                }
            };

            println!("--- Phase 1: Launching Winnow Zero-Copy String Tokenizer ---");
            let file_str = file.to_string_lossy();
            let (parsed_sections_opt, junk_spans) =
                espf::parser::parse_espf_runtime_with_diagnostics(&espf_runtime_file);

            if render_syntax_diagnostics(&file_str, &espf_runtime_file, &junk_spans) {
                std::process::exit(1);
            }

            let mut parsed_sections = parsed_sections_opt;

            // Harvest empty fallbacks if specific sections are missing from target load file
            let empty = vec![];
            let peripherals = parsed_sections.remove("peripherals").unwrap_or(empty.clone());
            let components = parsed_sections.remove("components").unwrap_or(empty.clone());
            let devices = parsed_sections.remove("devices").unwrap_or(empty);

            println!("--- Phase 2: Resolving Dynamic Topological Dependency Chains ---");
            let mut runtime_ctx = RuntimeContext::new();
            runtime_ctx.resolve_and_bind(&peripherals, &components, &devices)
                .expect("Fatal Error: Topological dependency verification validation broken.");

            println!("--- Phase 3: Executing Safe Array Drivers Dispatch Sweeps ---\n");
            for resolved_device in &runtime_ctx.resolved_devices {
                // Route driver matches string names using code generated instantly by build.rs
                route_driver(resolved_device.tag_name, resolved_device, &runtime_ctx)
                    .unwrap();
            }

            println!("\n🎉 System pipeline successfully initialised and executing.");
        }
    }
}
