//! `espforge build`: ESPF source -> firmware project (-> `cargo build`).
//!
//! Pipeline: resolve the source (FILE or `--example`) -> parse -> syntax and
//! structure errors -> `# espforge` core -> `<options>` -> flag overrides ->
//! resolve options -> generate -> write -> optional compile.
//!
//! Exit codes: 0 success, 1 user errors (diagnostics already printed),
//! 2 internal / IO failure.

use anyhow::{Context, Result, anyhow};
use clap::{Args, ValueEnum};
use espforge_emit::{OverlayFile, RuntimeSource};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use super::answers::AnswerArgs;
use super::{frontend, spec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Stage {
    /// Write the project, stop before cargo.
    Generate,
    /// Only run `cargo build` in an already generated output directory.
    Compile,
    /// Generate, then `cargo build`.
    #[default]
    All,
}

#[derive(Args, Debug, Clone, Default)]
pub struct BuildArgs {
    /// Path to the ESPF project file (app logic: src/app.rs or app/rust/app.rs next to it)
    pub file: Option<PathBuf>,
    /// Build an embedded example instead of a file, e.g. 01.Basics/helloworld
    #[arg(long, value_name = "CATEGORY/NAME", conflicts_with = "file")]
    pub example: Option<String>,
    /// Output directory for the generated project
    #[arg(short, long, default_value = "build")]
    pub out: PathBuf,
    /// Which stages to run
    #[arg(long, value_enum, default_value_t = Stage::All)]
    pub stage: Stage,
    /// Use a local espforge-runtime crate instead of the published one
    /// (or set ESPFORGE_RUNTIME_PATH)
    #[arg(long)]
    pub runtime_path: Option<PathBuf>,
    /// Also write the effective spec (file + flag overrides) to this path
    #[arg(long, value_name = "PATH")]
    pub write_spec: Option<PathBuf>,
    #[command(flatten)]
    pub answers: AnswerArgs,
}

/// Why a build stopped.
#[derive(Debug)]
pub enum Failure {
    /// The user's input is at fault; the message / diagnostics are already printed.
    User,
    /// Something broke inside espforge or the OS.
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(e: anyhow::Error) -> Self {
        Failure::Internal(e)
    }
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Failure::Internal(e.into())
    }
}

/// Print `❌ msg` and return the user-error marker.
fn user(msg: impl std::fmt::Display) -> Failure {
    eprintln!("❌ {msg}");
    Failure::User
}

pub fn run(args: BuildArgs) -> ExitCode {
    match execute(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::User) => ExitCode::from(1),
        Err(Failure::Internal(e)) => {
            eprintln!("❌ {e:#}");
            ExitCode::from(2)
        }
    }
}

// -------------------------------------------------------------------- source

struct Source {
    /// Name used in diagnostics: the path, or `embedded:<category>/<name>`.
    name: String,
    text: String,
    /// Directory of the `.espf` (None for embedded examples).
    dir: Option<PathBuf>,
    example: Option<&'static espforge_examples::Example>,
}

fn resolve_source(args: &BuildArgs) -> Result<Source, Failure> {
    match (&args.file, &args.example) {
        (Some(_), Some(_)) => Err(user("give either FILE or --example, not both")),
        (None, None) => Err(user("give a FILE or --example <category/name>")),
        (Some(file), None) => {
            let text = std::fs::read_to_string(file)
                .map_err(|e| user(format!("cannot read '{}': {e}", file.display())))?;
            let dir = file
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            Ok(Source {
                name: file.to_string_lossy().into_owned(),
                text,
                dir: Some(dir),
                example: None,
            })
        }
        (None, Some(path)) => {
            let ex = espforge_examples::find(path).ok_or_else(|| {
                user(format!(
                    "no example '{path}'; run `espforge examples list` to see what is available"
                ))
            })?;
            Ok(Source {
                name: format!("embedded:{path}"),
                text: ex.content.clone(),
                dir: None,
                example: Some(ex),
            })
        }
    }
}

/// Files next to a `.espf` that customise the generated Wokwi setup.
const OVERLAY_FILES: [&str; 4] = ["diagram.json", "wokwi.toml", "chip.json", "chip.wasm"];

fn collect_dir(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let rel = format!("{prefix}/{}", e.file_name().to_string_lossy());
        let path = e.path();
        if path.is_dir() {
            collect_dir(&path, &rel, out)?;
        } else {
            out.push((rel, std::fs::read(&path)?));
        }
    }
    Ok(())
}

/// The example files the template render may layer on top: from the embedded
/// example, or from the folder the `.espf` lives in (a materialized example).
fn overlay_files(src: &Source) -> Result<Vec<(String, Vec<u8>)>> {
    if let Some(ex) = src.example {
        return Ok(ex
            .files
            .iter()
            .map(|f| (f.path.clone(), f.contents.to_vec()))
            .collect());
    }
    let dir = src.dir.as_ref().context("a file source has a directory")?;
    let mut out = Vec::new();
    for name in OVERLAY_FILES {
        let p = dir.join(name);
        if p.is_file() {
            out.push((
                name.to_string(),
                std::fs::read(&p).with_context(|| format!("cannot read {}", p.display()))?,
            ));
        }
    }
    collect_dir(&dir.join("chip"), "chip", &mut out)?;
    Ok(out)
}

/// App logic lives in `src/app.rs` first, `app/rust/app.rs` second.
pub fn find_app(dir: &Path) -> Option<PathBuf> {
    let preferred = dir.join("src").join("app.rs");
    if preferred.is_file() {
        return Some(preferred);
    }
    let example_layout = dir.join("app").join("rust").join("app.rs");
    example_layout.is_file().then_some(example_layout)
}

fn app_logic(src: &Source) -> Result<Vec<u8>, Failure> {
    if let Some(ex) = src.example {
        return ex
            .files
            .iter()
            .find(|f| f.path == "app/rust/app.rs")
            .map(|f| f.contents.to_vec())
            .ok_or_else(|| user("this example has no app/rust/app.rs"));
    }
    let dir = src.dir.as_ref().expect("a file source has a directory");
    let found = find_app(dir).ok_or_else(|| {
        user(format!(
            "missing {} (or {}): your app logic (setup/forever)",
            dir.join("src").join("app.rs").display(),
            dir.join("app").join("rust").join("app.rs").display()
        ))
    })?;
    Ok(std::fs::read(&found)?)
}

fn runtime_source(args: &BuildArgs) -> Result<RuntimeSource, Failure> {
    let path = args
        .runtime_path
        .clone()
        .or_else(|| std::env::var_os("ESPFORGE_RUNTIME_PATH").map(PathBuf::from));
    match path {
        Some(p) => {
            let abs = std::path::absolute(&p)?;
            if !abs.is_dir() {
                return Err(user(format!(
                    "runtime path '{}' is not a directory",
                    p.display()
                )));
            }
            Ok(RuntimeSource::Path(abs.to_string_lossy().into_owned()))
        }
        None => Ok(RuntimeSource::Published),
    }
}

fn cargo_build(out: &Path) -> Result<(), Failure> {
    if !out.join("Cargo.toml").is_file() {
        return Err(user(format!(
            "nothing to compile: {} has no Cargo.toml (generate first)",
            out.display()
        )));
    }
    let status = Command::new("cargo")
        .arg("build")
        .current_dir(out)
        .status()
        .map_err(|e| Failure::Internal(anyhow!("failed to run cargo: {e}")))?;
    if status.success() {
        println!("build: compiled {}", out.display());
        Ok(())
    } else {
        Err(user("cargo build failed"))
    }
}

// ------------------------------------------------------------------ pipeline

pub fn execute(args: &BuildArgs) -> Result<(), Failure> {
    if args.stage == Stage::Compile {
        return cargo_build(&args.out);
    }

    let source = resolve_source(args)?;
    let fe = match frontend::load(&source.text) {
        Ok(fe) => fe,
        Err(errors) => {
            frontend::report(&source.name, &source.text, &errors);
            return Err(Failure::User);
        }
    };

    // Be explicit rather than silently generating firmware that lacks devices.
    for name in ["peripherals", "components", "devices", "services"] {
        if fe.parsed.sections.get(name).is_some_and(|n| !n.is_empty()) {
            return Err(user(format!(
                "`# {name}` is not supported by `build` yet (hello-world style projects only)"
            )));
        }
    }

    // Flags win over the file; say so when they actually change something.
    let mut meta = fe.meta.clone();
    let mut answers = fe.answers.clone();
    let mut notes = Vec::new();
    args.answers
        .apply_core(&mut meta, &mut notes)
        .map_err(user)?;
    args.answers
        .apply_options(
            espforge_options::embedded_schema(),
            &mut answers,
            &mut notes,
        )
        .map_err(user)?;
    for note in &notes {
        eprintln!("{note}");
    }

    let resolved = match espforge_options::resolve(&meta, &answers, &fe.derived) {
        Ok(r) => r,
        Err(errors) => {
            frontend::report(&source.name, &source.text, &errors);
            return Err(Failure::User);
        }
    };

    if let Some(path) = &args.write_spec {
        let doc =
            spec::effective_document(&fe.parsed.sections, &meta, &answers).map_err(|errors| {
                frontend::report(&source.name, &source.text, &errors);
                Failure::User
            })?;
        let text = spec::render(&doc).map_err(|e| Failure::Internal(anyhow!("{e}")))?;
        std::fs::write(path, text)
            .map_err(|e| Failure::Internal(anyhow!("cannot write '{}': {e}", path.display())))?;
        println!("build: wrote spec to {}", path.display());
    }

    let runtime = runtime_source(args)?;
    let overlay_owned = overlay_files(&source)?;
    let overlay: Vec<OverlayFile> = overlay_owned
        .iter()
        .map(|(path, contents)| OverlayFile { path, contents })
        .collect();
    let app = app_logic(&source)?;

    let generated = espforge_emit::generate(&meta, &resolved, &runtime, &overlay)?;
    if let Err(e) = espforge_emit::write_all(&args.out, &generated) {
        let msg = format!("{e:#}");
        // An edited machine-owned file is the user's doing, not a bug.
        return Err(if msg.contains("drift detected") {
            user(msg)
        } else {
            Failure::Internal(e)
        });
    }
    // The user's app logic is copied verbatim on every build.
    std::fs::create_dir_all(args.out.join("src"))?;
    std::fs::write(args.out.join("src").join("app.rs"), app)?;

    println!(
        "build: wrote '{}' to {}",
        meta.crate_name(),
        args.out.display()
    );
    if args.stage == Stage::Generate {
        println!("  next: cd {} && cargo build", args.out.display());
        return Ok(());
    }
    cargo_build(&args.out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::answers::{ConsoleArg, RuntimeArg};

    fn example_args(example: &str, out: &Path) -> BuildArgs {
        BuildArgs {
            example: Some(example.into()),
            out: out.to_path_buf(),
            stage: Stage::Generate,
            ..Default::default()
        }
    }

    fn is_user<T: std::fmt::Debug>(r: Result<T, Failure>) -> bool {
        matches!(r, Err(Failure::User))
    }

    #[test]
    fn builds_the_helloworld_example() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        execute(&example_args("01.Basics/helloworld", &out)).unwrap();
        for f in [
            "Cargo.toml",
            "build.rs",
            "src/lib.rs",
            "src/bin/main.rs",
            "src/app.rs",
            "README.txt",
            ".espforge/manifest.json",
        ] {
            assert!(out.join(f).is_file(), "missing {f}");
        }
        assert!(
            std::fs::read_to_string(out.join("src/app.rs"))
                .unwrap()
                .contains("Hello World!")
        );
        assert!(
            std::fs::read_to_string(out.join("Cargo.toml"))
                .unwrap()
                .contains("name = \"hello_world\"")
        );
    }

    #[test]
    fn rebuilding_is_fine_but_edited_owned_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let args = example_args("01.Basics/helloworld", &out);
        execute(&args).unwrap();
        execute(&args).unwrap();
        std::fs::write(out.join("src/lib.rs"), "// mine").unwrap();
        assert!(is_user(execute(&args)));
    }

    #[test]
    fn flags_override_the_file_in_the_output() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let mut args = example_args("01.Basics/helloworld", &out);
        args.answers.chip = Some("esp32s3".into());
        args.answers.runtime = Some(RuntimeArg::Embassy);
        args.answers.console = Some(ConsoleArg::JtagSerial);
        execute(&args).unwrap();
        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains("\"esp32s3\"")
                && cargo.contains("\"jtag-serial\"")
                && cargo.contains("embassy")
        );
    }

    #[test]
    fn option_errors_from_flags_are_user_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut args = example_args("01.Basics/helloworld", &dir.path().join("out"));
        args.answers.options = vec!["wifi=true".into()]; // derived: not user-settable
        assert!(is_user(execute(&args)));
        args.answers.options = vec!["nope=1".into()];
        assert!(is_user(execute(&args)));
    }

    #[test]
    fn unsupported_sections_are_reported_not_silently_dropped() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_user(execute(&example_args(
            "02.Digital/button",
            &dir.path().join("out")
        ))));
    }

    #[test]
    fn source_must_be_exactly_one_of_file_or_example() {
        let none = BuildArgs {
            stage: Stage::Generate,
            ..Default::default()
        };
        assert!(is_user(execute(&none)));
        let both = BuildArgs {
            file: Some("x.espf".into()),
            example: Some("01.Basics/helloworld".into()),
            stage: Stage::Generate,
            ..Default::default()
        };
        assert!(is_user(execute(&both)));
        let missing = BuildArgs {
            example: Some("no/such".into()),
            stage: Stage::Generate,
            ..Default::default()
        };
        assert!(is_user(execute(&missing)));
    }

    #[test]
    fn syntax_errors_in_a_file_are_user_errors() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("bad.espf");
        std::fs::write(&f, "# espforge\n  <gpio id=\"a\" pin=\"4 />\n").unwrap();
        let args = BuildArgs {
            file: Some(f),
            out: dir.path().join("out"),
            stage: Stage::Generate,
            ..Default::default()
        };
        assert!(is_user(execute(&args)));
    }

    #[test]
    fn builds_a_file_and_finds_app_logic_in_either_layout() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path().join("proj");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let hello = espforge_examples::find("01.Basics/helloworld").unwrap();
        std::fs::write(spec_dir.join("hello.espf"), &hello.content).unwrap();
        let out = dir.path().join("out");
        let args = BuildArgs {
            file: Some(spec_dir.join("hello.espf")),
            out: out.clone(),
            stage: Stage::Generate,
            ..Default::default()
        };

        // no app logic at all
        assert!(is_user(execute(&args)));

        // example layout
        std::fs::create_dir_all(spec_dir.join("app/rust")).unwrap();
        std::fs::write(spec_dir.join("app/rust/app.rs"), "// example layout").unwrap();
        execute(&args).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("src/app.rs")).unwrap(),
            "// example layout"
        );

        // src/app.rs wins when both exist
        std::fs::create_dir_all(spec_dir.join("src")).unwrap();
        std::fs::write(spec_dir.join("src/app.rs"), "// preferred").unwrap();
        execute(&args).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("src/app.rs")).unwrap(),
            "// preferred"
        );
    }

    #[test]
    fn find_app_prefers_src_app_rs() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_app(dir.path()).is_none());
        std::fs::create_dir_all(dir.path().join("app/rust")).unwrap();
        std::fs::write(dir.path().join("app/rust/app.rs"), "").unwrap();
        assert!(find_app(dir.path()).unwrap().ends_with("app/rust/app.rs"));
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/app.rs"), "").unwrap();
        assert!(find_app(dir.path()).unwrap().ends_with("src/app.rs"));
    }

    #[test]
    fn materialized_example_folders_overlay_their_wokwi_files() {
        let dir = tempfile::tempdir().unwrap();
        // i2c_scanner has peripherals, so build only its Wokwi overlay logic
        // through overlay_files on a materialized copy.
        let ex = espforge_examples::find("04.Communication/i2c_scanner").unwrap();
        let folder = dir.path().join("ex");
        espforge_examples::materialize(ex, &folder).unwrap();
        let src = Source {
            name: "x".into(),
            text: String::new(),
            dir: Some(folder),
            example: None,
        };
        let files = overlay_files(&src).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert!(
            names.contains(&"diagram.json")
                && names.contains(&"chip.json")
                && names.contains(&"chip.wasm"),
            "{names:?}"
        );
        assert!(names.contains(&"chip/chip.zig"), "{names:?}");
        assert!(!names.contains(&"example.espf.txt"));
    }

    #[test]
    fn write_spec_produces_a_loadable_effective_spec() {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("effective.espf");
        let mut args = example_args("01.Basics/helloworld", &dir.path().join("out"));
        args.write_spec = Some(spec_path.clone());
        args.answers.chip = Some("esp32c6".into());
        execute(&args).unwrap();
        let text = std::fs::read_to_string(&spec_path).unwrap();
        assert!(text.contains("<chip type=\"esp32c6\" />"), "{text}");
        assert!(frontend::load(&text).is_ok(), "{text}");
    }

    #[test]
    fn runtime_path_is_validated_and_used() {
        let dir = tempfile::tempdir().unwrap();
        let rt = dir.path().join("rt");
        std::fs::create_dir_all(&rt).unwrap();
        let out = dir.path().join("out");
        let mut args = example_args("01.Basics/helloworld", &out);
        args.runtime_path = Some(rt.clone());
        execute(&args).unwrap();
        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains(&rt.to_string_lossy().replace('\\', "/")),
            "{cargo}"
        );

        args.runtime_path = Some(dir.path().join("not-a-dir"));
        assert!(is_user(execute(&args)));
    }

    #[test]
    fn compile_stage_needs_a_generated_project() {
        let dir = tempfile::tempdir().unwrap();
        let args = BuildArgs {
            out: dir.path().to_path_buf(),
            stage: Stage::Compile,
            ..Default::default()
        };
        assert!(is_user(execute(&args)));
    }
}
