// `espforge doctor`: verifies that prerequisites for scaffolding/building an
// ESPF project are met:
//
//   1. `cargo` is installed and runnable (needed to build the scaffolded
//      project and to install/upgrade `esp-generate` itself).
//   2. The `esp-generate` CLI is installed and its version matches the one
//      espforge was compiled against (resolved by build.rs and embedded via
//      env!("ESP_GENERATE_VERSION")). A mismatch would make `espforge new`
//      behave differently from what this build of espforge expects.
//
// All checks that can be evaluated are run; the command exits:
//   0 — every check passed
//   1 — at least one check failed
use std::process::Command;

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
        (ESP_GENERATE_CLI, check_esp_generate),
    ];

    let mut failed = 0;
    for (name, check) in checks {
        match check() {
            Ok(detail) => println!("✔ {name} — {detail}"),
            Err(problem) => {
                failed += 1;
                println!("✖ {name} — {problem}");
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

/// Runs `cargo --version` and reports it. Any failure to locate or execute the
/// binary is reported with the standard install hint.
fn check_cargo() -> Result<String, String> {
    version_output("cargo").map_err(|_|
        "`cargo` was not found on PATH. Install Rust/cargo: https://rustup.rs".to_owned(),
    )
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
        Ok(format!("{installed_version} (matches the version espforge was compiled with)"))
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
