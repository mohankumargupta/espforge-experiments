//! Scaffold a standard Espressif no_std project via the external `esp-generate`
//! tool, then merge it into the output dir without clobbering existing files
//! (espforge's own emitted files and user edits survive a re-run).

use anyhow::{Context, Result, bail};
use espforge_model::Project;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The exact argument list passed to `esp-generate`.
/// The chip is a plain `-o` option (e.g. `-o esp32c3`).
pub fn esp_generate_args(project: &Project) -> Vec<String> {
    let mut options: Vec<&str> = vec![
        project.meta.chip.as_str(),
        "log",
        "unstable-hal",
        "alloc",
        "esp-backtrace",
        "wokwi",
        "vscode",
    ];
    if project.flags.is_embassy {
        options.push("embassy");
    }
    if project.flags.has_wifi {
        options.push("wifi");
    }

    // `--skip-update-check` keeps headless runs from hitting the network.
    let mut args: Vec<String> = vec!["--headless".into(), "--skip-update-check".into()];
    for o in options {
        args.push("-o".into());
        args.push(o.into());
    }
    args.push(project.meta.crate_name());
    args
}

pub fn scaffold(project: &Project, out_dir: &Path) -> Result<()> {
    let args = esp_generate_args(project);
    println!("scaffolding via: esp-generate {}", args.join(" "));

    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let temp_base = std::env::temp_dir().join(format!("espforge_{}_{millis}", std::process::id()));
    fs::create_dir_all(&temp_base).context("failed to create temp dir")?;

    let output = Command::new("esp-generate")
        .current_dir(&temp_base)
        .args(&args)
        .output()
        .context(
            "failed to run esp-generate (is it installed? try: cargo binstall esp-generate)",
        )?;
    if !output.status.success() {
        let _ = fs::remove_dir_all(&temp_base);
        bail!(
            "esp-generate failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let generated = temp_base.join(project.meta.crate_name());
    merge_directories(&generated, out_dir)?;
    let _ = fs::remove_dir_all(&temp_base);
    Ok(())
}

/// Copy `src` into `dst`, never overwriting a file that already exists.
fn merge_directories(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            merge_directories(&entry.path(), &dst_path)?;
        } else if !dst_path.exists() {
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_model::{Console, Meta, Runtime};

    fn project(runtime: Runtime) -> Project {
        Project::new(Meta {
            name: "example project".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime,
            console: Console::default(),
        })
    }

    #[test]
    fn args_for_blocking_project() {
        let args = esp_generate_args(&project(Runtime::Blocking));
        assert_eq!(&args[..2], ["--headless", "--skip-update-check"]);
        assert_eq!(&args[2..4], ["-o", "esp32c3"]);
        assert_eq!(args.last().unwrap(), "example_project");
        assert!(!args.contains(&"embassy".to_string()));
    }

    #[test]
    fn embassy_adds_option() {
        assert!(esp_generate_args(&project(Runtime::Embassy)).contains(&"embassy".to_string()));
    }
}
