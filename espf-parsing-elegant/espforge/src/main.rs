mod commands;

use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

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
    /// Generate a firmware project from an ESPF file (and compile it).
    ///
    /// Flags override the file; a `note:` says when one changes something.
    #[command(
        after_help = "Examples:\n  espforge build hello.espf --out build\n  espforge build --example 01.Basics/helloworld --stage generate\n  espforge build hello.espf --chip esp32s3 --option alloc=false --write-spec effective.espf"
    )]
    Build(commands::build::BuildArgs),
    /// Scaffold a new ESPF project (asks for what you do not give as flags).
    #[command(
        alias = "init",
        after_help = "Examples:\n  espforge new\n  espforge new my_proj\n  espforge new --yes --chip esp32c3 --runtime blocking my_proj"
    )]
    New(commands::new::NewArgs),
    /// Turn an embedded ESPF example into an editable project folder.
    ///
    /// Bare `examples` opens the interactive picker (category → example →
    /// destination folder) and materializes the chosen example. Subcommands
    /// are the non-interactive plumbing surface:
    ///
    ///   espforge examples list [category]
    ///   espforge examples get <category>/<name> --out <dir>
    #[command(
        after_help = "Examples:\n  espforge examples\n  espforge examples list 01.Basics\n  espforge examples get 01.Basics/button --out button"
    )]
    Examples {
        #[command(subcommand)]
        command: Option<ExamplesSub>,
    },
    /// Check that prerequisites (toolchain, binaries, env) are met.
    #[command(after_help = "Example:\n  espforge doctor")]
    Doctor,
    /// Print version/build information.
    #[command(after_help = "Example:\n  espforge version")]
    Version,
}

/// Non-interactive (plumbing) subcommands of `examples`: no prompts, plain
/// output, scriptable exit codes.
#[derive(Subcommand)]
enum ExamplesSub {
    /// List embedded examples, one per line: `category/name<TAB>title`.
    List {
        /// Only list examples of this category (e.g. `01.Basics`)
        category: Option<String>,
    },
    /// Copy an embedded example to a new folder (destination never overwritten).
    Get {
        /// The example to copy, as `category/name` (e.g. `01.Basics/button`)
        example: String,
        /// Destination folder for the example (required, never overwritten)
        #[arg(short, long)]
        out: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Check { file }) => commands::check::run(&file),
        Some(Commands::Build(args)) => return commands::build::run(args),
        Some(Commands::New(args)) => return commands::new::run(args),
        Some(Commands::Examples { command }) => match command {
            Some(ExamplesSub::List { category }) => {
                if let Err(e) = commands::examples::run_list(category.as_deref()) {
                    eprintln!("❌ {e:#}");
                    return ExitCode::from(1);
                }
            }
            Some(ExamplesSub::Get { example, out }) => {
                if let Err(e) = commands::examples::run_get(&example, &out) {
                    eprintln!("❌ {e:#}");
                    return ExitCode::from(1);
                }
            }
            // No plumbing subcommand: the interactive browser.
            None => commands::examples::run(),
        },
        Some(Commands::Doctor) => commands::doctor::run(),
        Some(Commands::Version) => commands::version::run(),
        // No subcommand given: show help instead of erroring.
        None => {
            Cli::command()
                .print_help()
                .expect("rendering help should not fail");
        }
    }
    ExitCode::SUCCESS
}
