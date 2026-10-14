// `espforge doctor`: verifies that prerequisites for scaffolding/building an
// ESPF project are met:
//
//   1. `cargo` is installed and runnable (needed to build the scaffolded
//      project and to install/upgrade `esp-generate` itself).
//   2. Every RISC-V Rust target needed by any chip `esp-generate` supports is
//      installed. The chip list and their target triples are read from the
//      `esp-metadata-generated` crate — the same generated metadata
//      esp-generate uses to build `esp-generate list-options` — so the check
//      follows the esp-generate version compiled into this binary instead of
//      duplicating a hardcoded list here.
//   3. The `esp-generate` CLI is installed and its version matches the one
//      espforge was compiled against (resolved by build.rs and embedded via
//      env!("ESP_GENERATE_VERSION")). A mismatch would make `espforge new`
//      behave differently from what this build of espforge expects.
//
// All checks that can be evaluated are run; the command exits:
//   0 — every check passed
//   1 — at least one check failed
use std::process::Command;
use std::str::FromStr;

use semver::Version;

const ESP_GENERATE_CLI: &str = "esp-generate";

/// The exact esp-generate version this espforge build was compiled against,
/// resolved from the workspace dependency graph by build.rs.
const COMPILED_ESP_GENERATE_VERSION: &str = env!("ESP_GENERATE_VERSION");

pub fn run() {
    println!("espforge doctor");
    println!("===============\n");

    let checks = [
        ("cargo", check_cargo as fn() -> Result<String, String>),
        ("rustc targets", check_rustc_targets),
        (ESP_GENERATE_CLI, check_esp_generate),
    ];

    let mut failed = 0;
    for (name, check) in checks {
        match check() {
            Ok(detail) => print_check_details('✔', name, &detail),
            Err(problem) => {
                failed += 1;
                print_check_details('✖', name, &problem);
            }
        }
    }

    println!();
    if failed == 0 {
        println!("All prerequisites met.");
    } else {
        println!("✖ {failed} check(s) failed; see the fix hints above.");
        std::process::exit(1);
    }
}

/// Prints `✔ name — <summary>` (first line of `details`) and indents any
/// further lines below the first, so a check's result can be a full report
/// instead of a single sentence.
fn print_check_details(mark: char, name: &str, details: &str) {
    let mut lines = details.lines();
    let first = lines.next().unwrap_or("");

    if first.is_empty() {
        println!("{mark} {name}");
    } else {
        println!("{mark} {name} — {first}");
    }
    for line in lines {
        if line.is_empty() {
            println!();
        } else {
            println!("      {line}");
        }
    }
}

/// Runs `cargo --version` and reports it. Any failure to locate or execute the
/// binary is reported with the standard install hint.
fn check_cargo() -> Result<String, String> {
    version_output("cargo").map_err(|_| {
        "`cargo` was not found on PATH. Install Rust/cargo: https://rustup.rs".to_owned()
    })
}

/// One RISC-V chip that this build's esp-generate version can scaffold for,
/// paired with the Rust target triple a project for that chip is built for.
struct ChipTarget {
    name: &'static str,
    target: &'static str,
}

/// Enumerates the RISC-V chips this build's esp-generate version supports.
///
/// Candidates come from `esp_metadata_generated::Chip::iter()` and are then
/// filtered two ways:
///   * Xtensa-architecture chips are dropped — they are built with the
///     espup-installed `esp` toolchain rather than a plain rustup target, so
///     they are not part of this check.
///   * Chips the compiled-in esp-generate version does not offer for
///     scaffolding are dropped by matching the name against its `Chip` enum.
///     This keeps doctor honest if the resolved esp-metadata-generated is
///     ever newer than esp-generate (e.g. esp32p4 is present in the metadata
///     but absent from `esp-generate list-options` today).
///
/// The list is derived, not hardcoded, so per-chip data stays validated
/// against `esp-generate list-options` of the esp-generate version this
/// binary was compiled against.
fn riscv_chip_targets() -> Vec<ChipTarget> {
    let mut chips = Vec::new();
    for chip in esp_metadata_generated::Chip::iter() {
        if chip.is_xtensa() {
            continue;
        }
        if esp_generate::Chip::from_str(chip.name()).is_err() {
            continue;
        }
        chips.push(ChipTarget {
            name: chip.name(),
            target: chip.target(),
        });
    }
    chips
}

/// Verifies that every RISC-V target the supported chips compile for is
/// installed, producing a per-target report instead of a bare pass/fail.
///
/// Prefers asking `rustup target list --installed`; if rustup is not on PATH
/// but `rustc` is (e.g. a distro toolchain), falls back to compiling an empty
/// `#![no_std]` probe per target and treating a `core` lookup failure as
/// "target not installed".
fn check_rustc_targets() -> Result<String, String> {
    let chips = riscv_chip_targets();
    if chips.is_empty() {
        return Err(format!(
            "expected esp-generate {COMPILED_ESP_GENERATE_VERSION} to support at least one RISC-V chip"
        ));
    }

    // Collapse chips that share a target triple into groups, in chip order:
    // e.g. (esp32c2, esp32c3) both need riscv32imc-unknown-none-elf.
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for chip in &chips {
        match groups.iter_mut().find(|(target, _)| *target == chip.target) {
            Some((_, names)) => names.push(chip.name),
            None => groups.push((chip.target, vec![chip.name])),
        }
    }
    let targets: Vec<&str> = groups.iter().map(|(target, _)| *target).collect();

    let installed = installed_targets(&targets)?;

    let missing: Vec<&str> = targets
        .iter()
        .copied()
        .filter(|target| !installed.contains(target))
        .collect();

    // Human-readable report, one line per group:
    //   esp32c2, esp32c3       riscv32imc  ✔ installed
    let mut report = format!(
        "RISC-V Rust targets needed by the chips `esp-generate {COMPILED_ESP_GENERATE_VERSION}` scaffolds:"
    );
    for (target, names) in &groups {
        let status = if installed.contains(target) {
            "✔ installed"
        } else {
            "✖ NOT INSTALLED"
        };
        // Pad the chip-list column so the target columns line up.
        report.push_str(&format!(
            "\n  {:<40} {}  {status}",
            names.join(", "),
            short_target(target)
        ));
    }

    if missing.is_empty() {
        report.push_str("\nall RISC-V targets installed.");
        Ok(report)
    } else {
        let add_list = missing.join(" ");
        let short_missing = missing
            .iter()
            .map(|t| short_target(t))
            .collect::<Vec<_>>()
            .join(" ");
        Err(format!(
            "{report}\n\nmissing: {short_missing}. Fix with:\n  rustup target add {add_list}"
        ))
    }
}

/// Returns the subset of `targets` that `rustc` can currently build for.
///
/// With `rustup` on PATH, asks `rustup target list --installed`; if rustup is
/// present but misbehaves (non-zero exit, unreadable output), that is a hard
/// error we cannot paper over. Without rustup (e.g. a distro toolchain), a
/// trivial `#![no_std]` probe per target decides usability instead.
fn installed_targets<'a>(targets: &[&'a str]) -> Result<Vec<&'a str>, String> {
    if which("rustup") {
        let output = Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output();
        let installed = output
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
            .ok_or_else(|| {
                "failed to read `rustup target list --installed`. Fix rustup: https://rustup.rs"
                    .to_owned()
            })?;

        return Ok(targets
            .iter()
            .copied()
            .filter(|target| installed.lines().any(|line| line.trim() == *target))
            .collect());
    }

    Ok(targets
        .iter()
        .copied()
        .filter(|target| target_compiles_no_std(target))
        .collect())
}

/// Drops the `-unknown-none-elf` suffix for terse display.
fn short_target(triple: &str) -> &str {
    triple.strip_suffix("-unknown-none-elf").unwrap_or(triple)
}

/// Returns true if a trivial `#![no_std]` lib compiles for `target`, i.e. at
/// least `core` is available. Returns false on any rustc invocation failure;
/// `installed_targets` wording still points the user at rustup.
fn target_compiles_no_std(target: &str) -> bool {
    let out_dir = std::env::temp_dir().join("espforge-doctor-probe");
    if !out_dir.exists() && std::fs::create_dir_all(&out_dir).is_err() {
        return false;
    }
    let source_path = out_dir.join("probe.rs");
    let output_path = out_dir.join("probe.rmeta");
    if std::fs::write(&source_path, "#![no_std]\n").is_err() {
        return false;
    }
    let output = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "--crate-type=lib",
            "--crate-name=espforge_probe",
            "--emit=metadata",
            "-o",
            output_path.to_string_lossy().as_ref(),
            "--target",
            target,
            source_path.to_string_lossy().as_ref(),
        ])
        .output();
    let _ = std::fs::remove_file(&source_path);
    let _ = std::fs::remove_file(&output_path);
    matches!(output, Ok(output) if output.status.success())
}

/// Runs `esp-generate --version` and compares the installed version against
/// the one espforge was compiled with (`COMPILED_ESP_GENERATE_VERSION`).
fn check_esp_generate() -> Result<String, String> {
    if version_output(ESP_GENERATE_CLI).is_err() {
        return Err(format!(
            "{ESP_GENERATE_CLI} was not found on PATH. Install it with:\n  \
             cargo install {ESP_GENERATE_CLI} --version {COMPILED_ESP_GENERATE_VERSION} --locked"
        ));
    }

    let installed = version_output(ESP_GENERATE_CLI).map_err(|_| "unreachable".to_owned())?;
    parse_version(&installed).ok_or_else(|| {
        format!(
            "could not parse a version out of `{ESP_GENERATE_CLI} --version` output: \"{installed}\""
        )
    })?;

    let installed_version = parse_version(&installed).map(|v| v.to_string()).unwrap();
    let compiled = parse_version(COMPILED_ESP_GENERATE_VERSION)
        .expect("COMPILED_ESP_GENERATE_VERSION is produced by build.rs and must be a valid semver")
        .to_string();

    if installed_version == compiled {
        Ok(format!(
            "{installed_version} (matches the version espforge was compiled with)"
        ))
    } else {
        Err(format!(
            "installed {installed_version}, but espforge was compiled with {compiled}. Fix with:\n  \
             cargo install {ESP_GENERATE_CLI} --version {compiled} --locked"
        ))
    }
}

/// Extracts and parses a semver `Version` from a version-reporting string such
/// as `cargo 1.99.0 (5f94df478 2026-08-27)` or `esp-generate v1.4.0`.
fn parse_version(version_string: &str) -> Option<Version> {
    // split_whitespace ignores surrounding whitespace and would ignore any
    // before the version token anyway, so no explicit trim is needed.
    version_string
        .split_whitespace()
        .find_map(|token| Version::parse(token.trim_start_matches('v')).ok())
}

/// Returns true if a binary with this name can be located.
fn which(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(binary).is_file()))
}

/// Runs `binary --version` and returns its trimmed output line. Uses stderr as
/// a fallback in case the tool prints its version there.
fn version_output(binary: &str) -> Result<String, String> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|error| format!("failed to run `{binary} --version`: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    match (!stdout.is_empty(), !stderr.is_empty()) {
        (true, _) => Ok(stdout),
        (false, true) => Ok(stderr),
        (false, false) => Err(format!("`{binary} --version` produced no output")),
    }
}
