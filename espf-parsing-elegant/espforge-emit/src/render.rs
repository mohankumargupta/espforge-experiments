//! Renders the embedded project template (`espforge-espgenerate-template`)
//! with minijinja, deciding per file whether it is emitted, where it goes,
//! and who owns it.
//!
//! * Every file in the template tree is registered by its path, so
//!   `{% include ".template/partials/main_blocking.rs" %}` resolves.
//! * `metadata.toml` is not a template; it controls emission (see
//!   [`parse_metadata`]). Files under `.template/` are partials: includable
//!   but never emitted themselves.
//! * A file whose name ends in `.tmpl` is emitted without that suffix
//!   (`Cargo.toml.tmpl` -> `Cargo.toml`; the suffix keeps cargo from treating
//!   the template directory as a nested package).

use anyhow::{Context as _, Result, anyhow, bail};
use espforge_espgenerate_template::TEMPLATE_DIR;
use espforge_model::{Artifact, Ownership};
use include_dir::Dir;
use minijinja::{AutoEscape, Environment, UndefinedBehavior, Value};
use toml_edit::DocumentMut;

const METADATA_FILE: &str = "metadata.toml";
const PARTIALS_PREFIX: &str = ".template/";

// ------------------------------------------------------------- template files

/// Every embedded file as (`/`-separated path, bytes), paths composed level by
/// level so they do not depend on include_dir's prefix choices.
fn collect_files(dir: &Dir<'static>, prefix: &str, out: &mut Vec<(String, &'static [u8])>) {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        }
    };
    for f in dir.files() {
        let name = f
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        out.push((join(name), f.contents()));
    }
    for d in dir.dirs() {
        let name = d
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        collect_files(d, &join(name), out);
    }
}

fn all_files() -> Vec<(String, &'static [u8])> {
    let mut out = Vec::new();
    collect_files(&TEMPLATE_DIR, "", &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Paths of the files that can be emitted (excludes `metadata.toml` and the
/// `.template/` partials), sorted.
pub fn template_paths() -> Vec<String> {
    all_files()
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p != METADATA_FILE && !p.starts_with(PARTIALS_PREFIX))
        .collect()
}

/// The minijinja environment: strict about undefined names, no stray blank
/// lines around `{% %}` blocks, no escaping (these are not HTML), and every
/// template file (including partials) registered by path.
pub fn environment() -> Result<Environment<'static>> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.set_keep_trailing_newline(true);
    // The default callback would JSON-escape values in `*.json` templates.
    env.set_auto_escape_callback(|_| AutoEscape::None);

    for (path, bytes) in all_files() {
        if path == METADATA_FILE {
            continue;
        }
        let text = std::str::from_utf8(bytes)
            .with_context(|| format!("template `{path}` is not valid UTF-8"))?
            .to_string();
        env.add_template_owned(path.clone(), text)
            .map_err(|e| anyhow!("template `{path}`: {e:#}"))?;
    }
    Ok(env)
}

pub fn render_template(env: &Environment, name: &str, ctx: &Value) -> Result<String> {
    env.get_template(name)
        .and_then(|t| t.render(ctx))
        .map_err(|e| anyhow!("rendering `{name}`: {e:#}"))
}

// ------------------------------------------------------------------ metadata

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitEntry {
    /// Output path (without `.tmpl`).
    pub path: String,
    /// minijinja expression; the file is emitted only if it is true.
    pub when: Option<String>,
    /// minijinja template for a different output path.
    pub rename: Option<String>,
    /// Ownership for this file, overriding the glob rules.
    pub own: Option<Ownership>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub emit: Vec<EmitEntry>,
    /// (glob, ownership); first match wins; default is `Owned`.
    pub ownership: Vec<(String, Ownership)>,
}

fn parse_own(s: &str) -> Result<Ownership> {
    match s {
        "owned" => Ok(Ownership::Owned),
        "seed-once" => Ok(Ownership::SeedOnce),
        "not-owned" => Ok(Ownership::NotOwned),
        other => bail!("unknown ownership \"{other}\" (expected owned, seed-once or not-owned)"),
    }
}

/// Globs: an exact path, or `dir/**` for everything below `dir`.
fn check_glob(g: &str) -> Result<()> {
    if g.contains('*') && !(g.ends_with("/**") && !g[..g.len() - 3].contains('*')) {
        bail!("unsupported glob \"{g}\" (use an exact path or `dir/**`)");
    }
    Ok(())
}

pub fn glob_match(glob: &str, path: &str) -> bool {
    match glob.strip_suffix("/**") {
        Some(dir) => path.starts_with(&format!("{dir}/")),
        None => glob == path,
    }
}

/// Parse `metadata.toml`:
///
/// ```toml
/// emit = [
///   { path = "diagram.json", when = "opt.wokwi" },
///   { path = ".vscode/tasks.json", when = "opt.vscode", own = "seed-once" },
/// ]
/// ownership = [
///   { glob = "src/**", own = "owned" },
///   { glob = ".cargo/**", own = "seed-once" },
/// ]
/// ```
///
/// Files not listed under `emit` are always emitted.
pub fn parse_metadata(src: &str) -> Result<Metadata> {
    let doc: DocumentMut = src.parse().context("invalid metadata.toml")?;
    let mut md = Metadata::default();

    if let Some(arr) = doc.get("emit").and_then(|i| i.as_array()) {
        for item in arr.iter() {
            let t = item
                .as_inline_table()
                .context("each `emit` entry must be an inline table")?;
            let get = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
            md.emit.push(EmitEntry {
                path: get("path").context("an `emit` entry is missing `path`")?,
                when: get("when"),
                rename: get("as"),
                own: get("own").map(|s| parse_own(&s)).transpose()?,
            });
        }
    }
    if let Some(arr) = doc.get("ownership").and_then(|i| i.as_array()) {
        for item in arr.iter() {
            let t = item
                .as_inline_table()
                .context("each `ownership` entry must be an inline table")?;
            let glob = t
                .get("glob")
                .and_then(|v| v.as_str())
                .context("an `ownership` entry is missing `glob`")?;
            let own = t
                .get("own")
                .and_then(|v| v.as_str())
                .context("an `ownership` entry is missing `own`")?;
            check_glob(glob)?;
            md.ownership.push((glob.to_string(), parse_own(own)?));
        }
    }
    Ok(md)
}

pub fn metadata() -> Result<Metadata> {
    let file = TEMPLATE_DIR
        .get_file(METADATA_FILE)
        .context("the template has no metadata.toml")?;
    parse_metadata(file.contents_utf8().context("metadata.toml is not UTF-8")?)
}

// ---------------------------------------------------------------------- plan

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// Template path to render.
    pub template: String,
    /// Output path (relative to the project directory).
    pub out_path: String,
    pub ownership: Ownership,
}

/// Decide which template files are emitted for this context, under which
/// names, and with what ownership. `when` conditions are evaluated only for
/// files that actually exist in the template.
pub fn plan(env: &Environment, md: &Metadata, ctx: &Value) -> Result<Vec<Planned>> {
    let mut out = Vec::new();
    for template in template_paths() {
        let default_out = template
            .strip_suffix(".tmpl")
            .unwrap_or(&template)
            .to_string();
        let entry = md.emit.iter().find(|e| e.path == default_out);
        let mut out_path = default_out;

        if let Some(e) = entry {
            if let Some(when) = &e.when {
                let on = env
                    .compile_expression(when)
                    .and_then(|x| x.eval(ctx))
                    .map_err(|err| anyhow!("condition `{when}` for `{}`: {err:#}", e.path))?;
                if !on.is_true() {
                    continue;
                }
            }
            if let Some(rename) = &e.rename {
                out_path = env
                    .render_str(rename, ctx)
                    .map_err(|err| anyhow!("`as` for `{}`: {err:#}", e.path))?;
            }
        }

        let ownership = entry
            .and_then(|e| e.own)
            .or_else(|| {
                md.ownership
                    .iter()
                    .find(|(g, _)| glob_match(g, &out_path))
                    .map(|(_, o)| *o)
            })
            .unwrap_or(Ownership::Owned);

        out.push(Planned {
            template,
            out_path,
            ownership,
        });
    }
    Ok(out)
}

/// Render every file the plan selects.
pub fn render_project(ctx: &Value) -> Result<Vec<Artifact>> {
    let env = environment()?;
    let md = metadata()?;
    plan(&env, &md, ctx)?
        .into_iter()
        .map(|p| {
            let content = render_template(&env, &p.template, ctx)?;
            Ok(Artifact {
                path: p.out_path,
                content,
                ownership: p.ownership,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::build_context;
    use crate::rust::RuntimeSource;
    use espforge_model::{Console, Meta, Runtime};
    use espforge_options::{Answers, DerivedInputs};

    fn ctx_with(runtime: Runtime, defaults: bool) -> Value {
        let meta = Meta {
            name: "hello world".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime,
            console: Console::Uart,
        };
        let mut answers = Answers::empty(0..0);
        answers.defaults = defaults;
        let opts = espforge_options::resolve(&meta, &answers, &DerivedInputs::default()).unwrap();
        build_context(&meta, &opts, &RuntimeSource::Published).unwrap()
    }

    fn hello() -> Value {
        ctx_with(Runtime::Blocking, true)
    }

    fn render(name: &str, ctx: &Value) -> String {
        render_template(&environment().unwrap(), name, ctx).unwrap()
    }

    #[test]
    fn environment_registers_the_partials_and_is_strict() {
        let env = environment().unwrap();
        assert!(
            env.get_template(".template/partials/main_blocking.rs")
                .is_ok()
        );
        assert!(env.render_str("{{ nope }}", hello()).is_err());
    }

    #[test]
    fn blocking_main_includes_its_partial() {
        let out = render("src/bin/main.rs", &hello());
        assert!(
            out.contains(
                "use hello_world::{app, Components, Context, Devices, PeripheralRegistry, CTX};"
            ),
            "{out}"
        );
        assert!(out.contains("#[esp_hal::main]"));
        assert!(!out.contains("{{") && !out.contains("{%"));
        assert!(!out.contains("\n\n\n"), "stray blank lines:\n{out}");
    }

    #[test]
    fn embassy_main_includes_the_async_partial() {
        let out = render("src/bin/main.rs", &ctx_with(Runtime::Embassy, true));
        assert!(out.contains("embassy_executor::Spawner"), "{out}");
        assert!(!out.contains("{{") && !out.contains("{%"));
        assert!(!out.contains("\n\n\n"), "stray blank lines:\n{out}");
    }

    #[test]
    fn wokwi_and_toolchain_files_render_cleanly() {
        let c = hello();
        let wokwi = render("wokwi.toml", &c);
        assert!(wokwi.contains("/debug/hello_world"), "{wokwi}");
        let tc = render("rust-toolchain.toml", &c);
        assert!(tc.contains("channel"), "{tc}");
        assert!(tc.starts_with("[toolchain]\n"));
        assert!(!wokwi.contains("\n\n\n") && !tc.contains("\n\n\n"));
    }

    #[test]
    fn diagram_json_is_valid_json_with_board_and_serial_monitor() {
        let out = render("diagram.json", &hello());
        let v: serde_json::Value =
            serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}\n{out}"));
        assert_eq!(v["parts"][0]["type"], "board-esp32-c3-devkitm-1");
        assert_eq!(v["connections"][0][1], "$serialMonitor:RX");
    }

    #[test]
    fn metadata_toml_parses() {
        let md = metadata().unwrap();
        assert!(md.emit.iter().any(|e| e.path == "diagram.json"));
        assert!(
            md.ownership
                .iter()
                .any(|(g, o)| g == ".cargo/**" && *o == Ownership::SeedOnce)
        );
    }

    #[test]
    fn plan_applies_conditions_names_and_ownership() {
        let env = environment().unwrap();
        let md = metadata().unwrap();

        let on = plan(&env, &md, &hello()).unwrap();
        let find = |p: &[Planned], path: &str| p.iter().find(|x| x.out_path == path).cloned();
        let cargo = find(&on, "Cargo.toml").expect("Cargo.toml.tmpl is emitted as Cargo.toml");
        assert_eq!(cargo.template, "Cargo.toml.tmpl");
        assert_eq!(cargo.ownership, Ownership::Owned);
        assert_eq!(find(&on, "src/lib.rs").unwrap().ownership, Ownership::Owned);
        assert_eq!(
            find(&on, ".cargo/config.toml").unwrap().ownership,
            Ownership::SeedOnce
        );
        assert_eq!(
            find(&on, "rust-toolchain.toml").unwrap().ownership,
            Ownership::SeedOnce
        );
        assert_eq!(
            find(&on, "diagram.json").unwrap().ownership,
            Ownership::Owned
        );
        assert!(find(&on, "metadata.toml").is_none());
        assert!(on.iter().all(|p| !p.out_path.starts_with(".template/")));

        // defaults="false" turns the wokwi option off, dropping its files.
        let off = plan(&env, &md, &ctx_with(Runtime::Blocking, false)).unwrap();
        assert!(find(&off, "diagram.json").is_none());
        assert!(find(&off, "wokwi.toml").is_none());
        assert!(find(&off, "src/lib.rs").is_some());
    }

    #[test]
    fn metadata_parser_handles_rename_own_and_rejects_bad_input() {
        let md = parse_metadata(
            r#"
emit = [
  { path = "a.txt", when = "opt.x", as = "{{ project_name }}.txt", own = "seed-once" },
]
ownership = [ { glob = "src/**", own = "owned" } ]
"#,
        )
        .unwrap();
        assert_eq!(md.emit[0].rename.as_deref(), Some("{{ project_name }}.txt"));
        assert_eq!(md.emit[0].own, Some(Ownership::SeedOnce));

        assert!(parse_metadata("emit = [ { when = \"x\" } ]").is_err());
        assert!(parse_metadata("emit = [ { path = \"a\", own = \"mine\" } ]").is_err());
        assert!(
            parse_metadata("ownership = [ { glob = \"src/*.rs\", own = \"owned\" } ]").is_err()
        );
    }

    #[test]
    fn globs() {
        assert!(glob_match("src/**", "src/bin/main.rs"));
        assert!(!glob_match("src/**", "src"));
        assert!(!glob_match("src/**", "srcs/x"));
        assert!(glob_match("Cargo.toml", "Cargo.toml"));
        assert!(!glob_match("Cargo.toml", "x/Cargo.toml"));
    }

    #[test]
    fn rename_is_rendered_through_the_engine() {
        let env = environment().unwrap();
        let md = Metadata {
            emit: vec![EmitEntry {
                path: "wokwi.toml".into(),
                when: None,
                rename: Some("{{ project_name }}.wokwi.toml".into()),
                own: None,
            }],
            ownership: vec![],
        };
        let p = plan(&env, &md, &hello()).unwrap();
        assert!(p.iter().any(|x| x.out_path == "hello_world.wokwi.toml"));
    }
}
