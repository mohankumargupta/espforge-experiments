mod commands;

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
    Check {
        /// Path to the ESPF runtime file to check
        file: PathBuf,
    },
    /// Scaffold a new ESPF project.
    New,
    /// Print version/build information.
    Version,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Check { file }) => commands::check::run(&file),
        Some(Commands::New) => commands::new::run(),
        Some(Commands::Version) => commands::version::run(),
        // No subcommand given: show help instead of erroring.
        None => {
            Cli::command().print_help().expect("rendering help should not fail");
        }
    }
}
