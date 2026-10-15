mod commands;
mod sections;

use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "espforge", version, about = "ESPF runtime pipeline tool")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate and execute an ESPF runtime file end-to-end.
    ///
    /// Example:
    ///   espforge check runtime.esp
    ///   espforge validate devices.runtime
    #[command(
        alias = "validate",
        after_help = "Examples:\n  espforge check runtime.esp\n  espforge validate devices.runtime"
    )]
    Check {
        /// Path to the ESPF runtime file to check
        file: PathBuf,
    },
    /// Scaffold a new ESPF project.
    #[command(after_help = "Example:\n  espforge new")]
    New,
    /// Browse the ESPF examples embedded in this binary.
    #[command(after_help = "Example:\n  espforge examples")]
    Examples,
    /// Check that prerequisites (toolchain, binaries, env) are met.
    #[command(after_help = "Example:\n  espforge doctor")]
    Doctor,
    /// Print version/build information.
    #[command(after_help = "Example:\n  espforge version")]
    Version,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Check { file }) => commands::check::run(&file),
        Some(Commands::New) => commands::new::run(),
        Some(Commands::Examples) => commands::examples::run(),
        Some(Commands::Doctor) => commands::doctor::run(),
        Some(Commands::Version) => commands::version::run(),
        // No subcommand given: show help instead of erroring.
        None => {
            Cli::command()
                .print_help()
                .expect("rendering help should not fail");
        }
    }
}
