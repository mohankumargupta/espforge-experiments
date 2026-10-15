//! Rust source emitters: `Cargo.toml` (merged), `src/generated.rs`,
//! `src/lib.rs` and `src/bin/main.rs`.

use anyhow::{Context, Result, bail};
use espforge_model::{Artifact, Console, Project, Runtime};
use std::collections::BTreeSet;
use toml_edit::{Array, DocumentMut, InlineTable, Item, value};

/// Version of `espforge-runtime` written into generated manifests.
pub const RUNTIME_VERSION: &str = "0.1.0";

#[derive(Debug, Clone)]
pub enum RuntimeSource {
    /// Use the published crate from crates.io.
    Published,
    /// Path to the `espforge-runtime` crate directory, as it should appear
    /// in the generated Cargo.toml (relative to the output dir).
    Path(String),
}

#[derive(Debug, Clone)]
pub struct EmitOptions {
    pub runtime_source: RuntimeSource,
    /// The `Cargo.toml` produced by the scaffold step (esp-generate).
    /// Our dependencies are merged into it.
    pub base_manifest: String,
}

pub fn emit(project: &Project, opts: &EmitOptions) -> Result<Vec<Artifact>> {
    if project.meta.runtime == Runtime::Embassy {
        bail!("the embassy runtime is not supported yet");
    }
    // Runtime module features required by instances (none for hello world).
    let features = BTreeSet::<String>::new();

    Ok(vec![
        Artifact::owned(
            "Cargo.toml",
            emit_cargo_toml(opts, &features, project.meta.console)?,
        ),
        Artifact::owned("src/generated.rs", emit_generated()),
        Artifact::owned("src/lib.rs", emit_lib()),
        Artifact::owned("src/bin/main.rs", emit_main_blocking(project)),
    ])
}

// ---------------------------------------------------------------- Cargo.toml

/// `static_cell` backs the `CTX` global in the generated `lib.rs`.
const STATIC_CELL_DEP: &str = "static_cell = \"2.1.1\"";
/// esp-generate's `alloc` option normally ships this already.
const ESP_ALLOC_DEP: &str = "esp-alloc = \"0.11\"";

/// Every esp-println output backend we manage. Exactly one is kept.
const CONSOLE_FEATURES: [&str; 2] = ["uart", "jtag-serial"];

fn console_feature(console: Console) -> &'static str {
    match console {
        Console::Uart => "uart",
        Console::JtagSerial => "jtag-serial",
    }
}

fn runtime_dep(source: &RuntimeSource, features: &BTreeSet<String>) -> String {
    let feats = if features.is_empty() {
        String::new()
    } else {
        let list = features
            .iter()
            .map(|f| format!("\"{f}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!(", features = [{list}]")
    };
    match source {
        RuntimeSource::Published => {
            format!("espforge-runtime = {{ version = \"{RUNTIME_VERSION}\"{feats} }}")
        }
        RuntimeSource::Path(p) => {
            let p = p.replace('\\', "/");
            format!(
                "espforge-runtime = {{ path = \"{p}\", version = \"{RUNTIME_VERSION}\"{feats} }}"
            )
        }
    }
}

fn emit_cargo_toml(
    opts: &EmitOptions,
    features: &BTreeSet<String>,
    console: Console,
) -> Result<String> {
    let base = &opts.base_manifest;
    // Our own dependency is always replaced so rebuilds pick up changes;
    // other crates are left alone if esp-generate already declared them.
    let merged = merge_dep(base, &runtime_dep(&opts.runtime_source, features), true);
    let merged = merge_dep(&merged, STATIC_CELL_DEP, false);
    let merged = merge_dep(&merged, ESP_ALLOC_DEP, false);

    // esp-generate leaves esp-println on its default backend, which gives no
    // serial output on some chips. Select the requested one explicitly.
    let wanted = console_feature(console);
    let stale: Vec<&str> = CONSOLE_FEATURES
        .into_iter()
        .filter(|f| *f != wanted)
        .collect();
    let mut merged = set_dep_features(&merged, "esp-println", &[wanted], &stale, false)?;

    if !merged.ends_with('\n') {
        merged.push('\n');
    }
    Ok(merged)
}

/// Does this manifest line declare the dependency `name`?
fn declares(line: &str, name: &str) -> bool {
    line.trim_start()
        .strip_prefix(name)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// Targeted string edit (not a re-serialise) so esp-generate's pins and
/// multi-line feature arrays survive untouched.
fn merge_dep(manifest: &str, dep_line: &str, replace: bool) -> String {
    let name = dep_line.split_whitespace().next().unwrap_or("");
    let declared = manifest.lines().any(|l| declares(l, name));
    if declared && !replace {
        return manifest.to_string();
    }
    let without = manifest
        .lines()
        .filter(|l| !declares(l, name))
        .collect::<Vec<_>>()
        .join("\n");
    match without.find("[dependencies]") {
        Some(idx) => {
            let at = idx + "[dependencies]".len();
            format!("{}\n{}{}", &without[..at], dep_line, &without[at..])
        }
        None => format!("{without}\n\n[dependencies]\n{dep_line}\n"),
    }
}

/// Edit an existing dependency in place with `toml_edit` (comments and
/// formatting survive): enable `add`, drop `remove`, and optionally turn off
/// default features. Idempotent, so it's safe on rebuilds.
pub fn set_dep_features(
    manifest: &str,
    dep: &str,
    add: &[&str],
    remove: &[&str],
    default_features: bool,
) -> Result<String> {
    let mut doc: DocumentMut = manifest.parse().context("invalid Cargo.toml")?;
    let deps = doc["dependencies"]
        .as_table_mut()
        .context("Cargo.toml has no [dependencies]")?;
    let item = deps.get_mut(dep).with_context(|| {
        format!("`{dep}` not found in [dependencies] (did esp-generate run with the `log` option?)")
    })?;

    // `dep = "1"` -> `dep = { version = "1" }` so we can attach keys.
    if let Some(ver) = item.as_str().map(str::to_owned) {
        let mut t = InlineTable::new();
        t.insert("version", ver.into());
        *item = Item::Value(t.into());
    }
    let table = item
        .as_table_like_mut()
        .context("unsupported dependency shape")?;

    if !default_features {
        table.insert("default-features", value(false));
    }
    let arr = table
        .entry("features")
        .or_insert(value(Array::new()))
        .as_array_mut()
        .context("`features` is not an array")?;
    arr.retain(|v| !v.as_str().is_some_and(|s| remove.contains(&s)));
    for f in add {
        if !arr.iter().any(|v| v.as_str() == Some(*f)) {
            arr.push(*f);
        }
    }
    Ok(doc.to_string())
}

// ------------------------------------------------------------- generated.rs

fn emit_generated() -> String {
    r#"// GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project .espf file.
#![allow(dead_code)]

/// Raw ESP32 peripherals, moved out of `esp_hal::peripherals::Peripherals` once at init.
pub struct PeripheralRegistry {
    pub peripherals: esp_hal::peripherals::Peripherals,
}

impl PeripheralRegistry {
    pub fn new(peripherals: esp_hal::peripherals::Peripherals) -> Self {
        Self { peripherals }
    }
}

/// Constructed components (none yet).
pub struct Components {}

/// Constructed devices (none yet).
pub struct Devices {}
"#
    .to_string()
}

// -------------------------------------------------------------------- lib.rs

fn emit_lib() -> String {
    r#"// GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project .espf file.
#![no_std]

pub mod generated;

// Re-exports for `app.rs` (it only uses `crate::` paths).
pub use espforge_runtime::Delay;

/// Access a component from the `Context`. Usage: `component!(ctx, my_led)`
#[macro_export]
macro_rules! component {
    ($ctx:expr, $name:ident) => {
        &$ctx.components.$name
    };
}

/// Access a device from the `Context`. Usage: `device!(ctx, my_screen)`
#[macro_export]
macro_rules! device {
    ($ctx:expr, $name:ident) => {
        &$ctx.devices.$name
    };
}

pub mod app;

pub use generated::{Components, Devices, PeripheralRegistry};

/// The whole context the app receives.
pub struct Context {
    pub logger: espforge_runtime::Logger,
    pub delay: espforge_runtime::Delay,
    pub components: Components,
    pub devices: Devices,
}

/// Set by the entry point once wiring is complete.
pub static CTX: static_cell::StaticCell<Context> = static_cell::StaticCell::new();
"#
    .to_string()
}

// ------------------------------------------------------------------- main.rs

fn emit_main_blocking(project: &Project) -> String {
    let name = project.meta.crate_name();
    format!(
        r#"// GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project .espf file.
#![no_std]
#![no_main]

use esp_backtrace as _;
use {name}::{{app, Components, Context, Devices, PeripheralRegistry, CTX}};

#[esp_hal::main]
fn main() -> ! {{
    esp_println::logger::init_logger_from_env();
    esp_println::print!("\x1b[20h");
    esp_bootloader_esp_idf::esp_app_desc!();
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let _registry = PeripheralRegistry::new(peripherals);

    let logger = espforge_runtime::Logger::new();
    let delay = espforge_runtime::Delay::new();

    let components = Components {{}};
    let devices = Devices {{}};

    let ctx = Context {{ logger, delay, components, devices }};
    let ctx = CTX.init(ctx);
    app::setup(ctx);
    loop {{
        app::forever(ctx);
    }}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_model::Meta;

    const BASE: &str = "[package]\nname = \"example_project\"\nversion = \"0.1.0\"\n\n[dependencies]\nesp-hal = \"1\"\nesp-println = { version = \"0.18.0\", features = [\"esp32c3\", \"log-04\"] }\n";

    fn hello(runtime: Runtime, console: Console) -> Project {
        Project::new(Meta {
            name: "example project".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime,
            console,
        })
    }

    fn opts(source: RuntimeSource, base: &str) -> EmitOptions {
        EmitOptions {
            runtime_source: source,
            base_manifest: base.into(),
        }
    }

    fn find<'a>(arts: &'a [Artifact], path: &str) -> &'a str {
        &arts.iter().find(|a| a.path == path).unwrap().content
    }

    fn blocking() -> Project {
        hello(Runtime::Blocking, Console::Uart)
    }

    #[test]
    fn emits_the_four_files() {
        let arts = emit(&blocking(), &opts(RuntimeSource::Published, BASE)).unwrap();
        let paths: Vec<_> = arts.iter().map(|a| a.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "Cargo.toml",
                "src/generated.rs",
                "src/lib.rs",
                "src/bin/main.rs"
            ]
        );
    }

    #[test]
    fn main_uses_slugged_crate_name() {
        let arts = emit(&blocking(), &opts(RuntimeSource::Published, BASE)).unwrap();
        assert!(find(&arts, "src/bin/main.rs").contains("use example_project::{app,"));
    }

    #[test]
    fn manifest_gets_runtime_path_dep() {
        let arts = emit(
            &blocking(),
            &opts(
                RuntimeSource::Path("../espforge/espforge-runtime".into()),
                BASE,
            ),
        )
        .unwrap();
        let toml = find(&arts, "Cargo.toml");
        assert!(toml.contains("espforge-runtime = { path = \"../espforge/espforge-runtime\""));
        assert!(toml.contains("static_cell"));
        assert!(toml.contains("esp-hal = \"1\""));
    }

    #[test]
    fn rebuild_replaces_stale_runtime_dep() {
        let p = blocking();
        let first = emit(&p, &opts(RuntimeSource::Path("../a".into()), BASE)).unwrap();
        let first_toml = find(&first, "Cargo.toml").to_string();
        // Second build starts from the previous output, like a real rebuild.
        let second = emit(&p, &opts(RuntimeSource::Path("../b".into()), &first_toml)).unwrap();
        let toml = find(&second, "Cargo.toml");
        assert!(toml.contains("../b") && !toml.contains("../a"));
        assert_eq!(toml.matches("espforge-runtime").count(), 1);
        assert_eq!(toml.matches("static_cell").count(), 1);
        assert_eq!(toml.matches("\"uart\"").count(), 1);
    }

    #[test]
    fn embassy_is_rejected_for_now() {
        assert!(
            emit(
                &hello(Runtime::Embassy, Console::Uart),
                &opts(RuntimeSource::Published, BASE)
            )
            .is_err()
        );
    }

    #[test]
    fn uart_console_disables_defaults_and_adds_feature() {
        let arts = emit(&blocking(), &opts(RuntimeSource::Published, BASE)).unwrap();
        let toml = find(&arts, "Cargo.toml");
        let line = toml.lines().find(|l| l.starts_with("esp-println")).unwrap();
        assert!(line.contains("default-features = false"));
        assert!(line.contains("\"uart\""));
        // esp-generate's own features survive.
        assert!(line.contains("\"esp32c3\"") && line.contains("\"log-04\""));
    }

    #[test]
    fn switching_console_replaces_the_old_feature() {
        let first = emit(&blocking(), &opts(RuntimeSource::Published, BASE)).unwrap();
        let first_toml = find(&first, "Cargo.toml").to_string();
        let jtag = hello(Runtime::Blocking, Console::JtagSerial);
        let second = emit(&jtag, &opts(RuntimeSource::Published, &first_toml)).unwrap();
        let toml = find(&second, "Cargo.toml");
        assert!(toml.contains("\"jtag-serial\""));
        assert!(!toml.contains("\"uart\""));
    }

    #[test]
    fn missing_esp_println_is_an_error() {
        let base = "[package]\nname = \"x\"\n\n[dependencies]\nesp-hal = \"1\"\n";
        assert!(emit(&blocking(), &opts(RuntimeSource::Published, base)).is_err());
    }

    #[test]
    fn plain_string_dependency_is_upgraded() {
        let out = set_dep_features(
            "[dependencies]\nesp-println = \"0.18.0\"\n",
            "esp-println",
            &["uart"],
            &[],
            false,
        )
        .unwrap();
        assert!(out.contains("version = \"0.18.0\"") && out.contains("\"uart\""));
    }
}
