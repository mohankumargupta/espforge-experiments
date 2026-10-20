mod commands;

use clap::{Args, CommandFactory, Parser, Subcommand};
use commands::build::{BuildArgs, Stage};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "espforge", version, about = "ESPF runtime pipeline tool")]
struct Cli {
    /// Run as if started in DIR (like `cargo -C`)
    #[arg(short = 'C', long = "directory", global = true, value_name = "DIR")]
    directory: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new ESPF project (asks for what you do not give as flags).
    #[command(
        alias = "init",
        after_help = "Examples:\n  espforge new\n  espforge new my_proj\n  espforge new --yes --chip esp32c3 --runtime blocking my_proj"
    )]
    New(commands::new::NewArgs),
    /// Validate an ESPF file: syntax, options, `$` references, drivers.
    ///
    /// Uses the same pipeline and override flags as `build`.
    #[command(
        alias = "validate",
        after_help = "Examples:\n  espforge check\n  espforge check hello.espf --chip esp32s3\n  espforge check --example 01.Basics/helloworld"
    )]
    Check(commands::check::CheckArgs),
    /// Generate the firmware project, then `cargo build` it.
    ///
    /// Flags override the file; a `note:` says when one changes something.
    #[command(
        after_help = "Examples:\n  espforge build\n  espforge build hello.espf --out build --release\n  espforge build --example 01.Basics/helloworld"
    )]
    Build(BuildCmd),
    /// Generate the firmware project only (no cargo).
    #[command(
        after_help = "Examples:\n  espforge gen\n  espforge gen hello.espf --chip esp32s3 --option alloc=false --write-spec effective.espf"
    )]
    Gen(BuildArgs),
    /// Turn an embedded ESPF example into an editable project folder.
    ///
    /// Bare `examples` opens the interactive picker (category → example →
    /// destination folder). Subcommands are the non-interactive plumbing
    /// surface:
    ///
    ///   espforge examples categories
    ///   espforge examples list [category]
    ///   espforge examples get <category>/<name> --out <dir>
    #[command(
        after_help = "Examples:\n  espforge examples\n  espforge examples categories\n  espforge examples list\n  espforge examples list 01.Basics\n  espforge examples get 01.Basics/button --out button"
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

#[derive(Args)]
struct BuildCmd {
    #[command(flatten)]
    args: BuildArgs,
    /// Build the generated project in release mode
    #[arg(long)]
    release: bool,
}

/// Non-interactive (plumbing) subcommands of `examples`: no prompts, plain
/// output, scriptable exit codes.
#[derive(Subcommand)]
enum ExamplesSub {
    /// List the example categories, one per line: `category<TAB>N examples`.
    Categories,
    /// List embedded examples, one per line: `category/name<TAB>title`.
    ///
    /// Without a category, every example of every category is listed.
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

fn finish(result: anyhow::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("❌ {e:#}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Some(dir) = &cli.directory {
        if let Err(e) = std::env::set_current_dir(dir) {
            eprintln!("❌ cannot change to '{}': {e}", dir.display());
            return ExitCode::from(2);
        }
    }

    match cli.command {
        Some(Commands::New(args)) => commands::new::run(args),
        Some(Commands::Check(args)) => commands::check::run(args),
        Some(Commands::Build(cmd)) => {
            let mut args = cmd.args;
            args.release = cmd.release;
            commands::build::run(args)
        }
        Some(Commands::Gen(mut args)) => {
            args.stage = Stage::Generate;
            commands::build::run(args)
        }
        Some(Commands::Examples { command }) => match command {
            Some(ExamplesSub::Categories) => finish(commands::examples::run_categories()),
            Some(ExamplesSub::List { category }) => {
                finish(commands::examples::run_list(category.as_deref()))
            }
            Some(ExamplesSub::Get { example, out }) => {
                finish(commands::examples::run_get(&example, &out))
            }
            // No plumbing subcommand: the interactive browser.
            None => {
                commands::examples::run();
                ExitCode::SUCCESS
            }
        },
        Some(Commands::Doctor) => {
            commands::doctor::run();
            ExitCode::SUCCESS
        }
        Some(Commands::Version) => {
            commands::version::run();
            ExitCode::SUCCESS
        }
        // No subcommand given: show help instead of erroring.
        None => {
            Cli::command()
                .print_help()
                .expect("rendering help should not fail");
            ExitCode::SUCCESS
        }
    }
}
