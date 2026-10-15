use anyhow::{Context, Result, anyhow, bail};
use espforge_emit::{EmitOptions, RuntimeSource};
use espforge_model::Project;
use std::path::{Path, PathBuf};

use crate::sections::espforge::parse_meta;

/// Generate a firmware project from an ESPF file.
pub fn run(file: &Path, out: &Path, runtime_path: Option<PathBuf>) -> Result<()> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("cannot read '{}'", file.display()))?;
    let file_str = file.to_string_lossy();

    let (mut sections, junk, section_offsets) = espf::parser::parse_espf_runtime_with_diagnostics(&src);
    let syntax_errors = espf::diag::junk_errors(&junk);
    if !syntax_errors.is_empty() {
        super::check::fail_with_diagnostics(&file_str, &src, &syntax_errors);
    }

    let nodes = sections
        .remove("espforge")
        .ok_or_else(|| anyhow!("'{file_str}' has no `# espforge` section"))?;
    let meta = parse_meta(
        section_offsets.get("espforge").copied().unwrap_or(0),
        &nodes,
    )
    .unwrap_or_else(|errors| {
        super::check::fail_with_diagnostics(&file_str, &src, &errors)
    });

    // Be explicit rather than silently generating firmware that lacks devices.
    for name in ["peripherals", "components", "devices"] {
        if sections.get(name).is_some_and(|n| !n.is_empty()) {
            bail!("`# {name}` is not supported by `build` yet (hello-world style projects only)");
        }
    }

    // The user's app logic lives next to the .espf file. Preferred layout:
    // <dir>/src/app.rs; the layout used by embedded examples (<dir>/app/rust/app.rs)
    // is accepted too (see cli.md, stage 2).
    let spec_dir = file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let app_src = spec_dir.join("src").join("app.rs");
    let app_src = if app_src.is_file() {
        app_src
    } else {
        let example_layout = spec_dir.join("app").join("rust").join("app.rs");
        if !example_layout.is_file() {
            bail!(
                "missing {} (or {}) — your app logic: setup/forever",
                app_src.display(),
                example_layout.display()
            );
        }
        example_layout
    };

    let project = Project::new(meta);

    // Layer 1: esp-generate scaffold (never clobbers existing files).
    espforge_emit::scaffold::scaffold(&project, out)?;

    // Layer 2: our files, merged into the scaffold's Cargo.toml.
    let base_manifest = std::fs::read_to_string(out.join("Cargo.toml"))
        .context("scaffold did not produce a Cargo.toml")?;
    let runtime_source = match runtime_path
        .or_else(|| std::env::var_os("ESPFORGE_RUNTIME_PATH").map(PathBuf::from))
    {
        Some(p) => {
            let abs = std::path::absolute(&p)?;
            if !abs.is_dir() {
                bail!("runtime path '{}' is not a directory", p.display());
            }
            RuntimeSource::Path(abs.to_string_lossy().into_owned())
        }
        None => RuntimeSource::Published,
    };
    let artifacts = espforge_emit::generate(
        &project,
        &EmitOptions {
            runtime_source,
            base_manifest,
        },
    )?;
    espforge_emit::write(out, &artifacts)?;

    // Layer 3: user-owned app.rs, copied verbatim on every build.
    std::fs::create_dir_all(out.join("src"))?;
    std::fs::copy(&app_src, out.join("src").join("app.rs")).context("failed to copy src/app.rs")?;

    println!(
        "build: wrote '{}' to {}",
        project.meta.crate_name(),
        out.display()
    );
    println!("  next: cd {} && cargo build", out.display());
    Ok(())
}
