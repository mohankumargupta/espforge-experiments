use clap::{Parser, Subcommand};
use esforge_bindings::drivers::generated_registry::route_driver;
use espf::context::RuntimeContext;
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
            let mut parsed_sections = espf::parser::parse_espf_runtime(&espf_runtime_file)
                .expect("Fatal Error: Syntactical parsing error inside configuration file structure.");

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
