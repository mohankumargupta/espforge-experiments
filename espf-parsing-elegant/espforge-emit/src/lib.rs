//! Code emitters and the thin I/O layer around them.
//! `rust::emit` and friends are pure (model in, artifacts out); `scaffold`
//! and `write` touch the filesystem.
//! Host-only: never depend on `espforge-runtime` from this crate.

pub mod context;
pub mod manifest;
pub mod render;
pub mod rust;
pub mod scaffold;

use anyhow::{Context, Result, bail};
use espforge_model::{Artifact, Asset, Ownership, Project};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub use rust::{EmitOptions, RuntimeSource};

/// Run all emitters and return every artifact espforge owns (plus README and
/// manifest). Does not touch the filesystem.
pub fn generate(project: &Project, opts: &EmitOptions) -> Result<Vec<Artifact>> {
    let mut out = rust::emit(project, opts)?;
    let owned = manifest::owned_paths(&out);
    out.push(Artifact::owned(
        "README.txt",
        manifest::emit_readme(project, &owned),
    ));
    out.push(manifest::emit_manifest(&out)?);
    Ok(out)
}

/// Write artifacts into `out_dir`, respecting ownership:
/// - `Owned` files are overwritten, unless the previous manifest shows the
///   file was edited since we wrote it (drift): then we refuse.
/// - `SeedOnce` files are written only if absent.
/// - `NotOwned` files are never written.
pub fn write(out_dir: &Path, artifacts: &[Artifact]) -> Result<()> {
    let prev = read_prev_manifest(out_dir)?;
    for a in artifacts {
        if a.ownership == Ownership::NotOwned {
            continue;
        }
        let path = out_dir.join(&a.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        match a.ownership {
            Ownership::SeedOnce => {
                if !path.exists() {
                    fs::write(&path, &a.content)?;
                }
            }
            Ownership::Owned => {
                if let Some(prev_sum) = prev.get(&a.path) {
                    if let Ok(existing) = fs::read_to_string(&path) {
                        if manifest::checksum(&existing) != *prev_sum && existing != a.content {
                            bail!(
                                "refusing to overwrite edited file `{}` (drift detected). \
                                 Restore it or delete it to regenerate.",
                                a.path
                            );
                        }
                    }
                }
                fs::write(&path, &a.content)?;
            }
            Ownership::NotOwned => {}
        }
    }
    Ok(())
}

/// Write binary assets into `out_dir`: `SeedOnce` only if absent, `Owned`
/// always overwritten (assets are not drift-tracked), `NotOwned` never.
pub fn write_assets(out_dir: &Path, assets: &[Asset]) -> Result<()> {
    for a in assets {
        let path = out_dir.join(&a.path);
        match a.ownership {
            Ownership::NotOwned => continue,
            Ownership::SeedOnce if path.exists() => continue,
            _ => {}
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &a.bytes)?;
    }
    Ok(())
}

fn read_prev_manifest(out_dir: &Path) -> Result<BTreeMap<String, String>> {
    let p = out_dir.join(".espforge/manifest.json");
    match fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).context("corrupt .espforge/manifest.json"),
        Err(_) => Ok(BTreeMap::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_model::{Console, Meta, Runtime};

    fn artifacts() -> Vec<Artifact> {
        let project = Project::new(Meta {
            name: "hello".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::default(),
        });
        let opts = EmitOptions {
            runtime_source: RuntimeSource::Published,
            base_manifest: "[package]\nname = \"hello\"\n\n[dependencies]\n".into(),
        };
        generate(&project, &opts).unwrap()
    }

    #[test]
    fn write_creates_files_readme_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &artifacts()).unwrap();
        assert!(dir.path().join("src/lib.rs").is_file());
        assert!(dir.path().join("README.txt").is_file());
        assert!(dir.path().join(".espforge/manifest.json").is_file());
    }

    #[test]
    fn rewriting_unchanged_output_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &artifacts()).unwrap();
        write(dir.path(), &artifacts()).unwrap();
    }

    #[test]
    fn assets_follow_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let seed = Asset::seed_once("chip.wasm", vec![0, 97, 115, 109]);
        write_assets(dir.path(), std::slice::from_ref(&seed)).unwrap();
        fs::write(dir.path().join("chip.wasm"), b"mine").unwrap();
        write_assets(dir.path(), std::slice::from_ref(&seed)).unwrap();
        assert_eq!(fs::read(dir.path().join("chip.wasm")).unwrap(), b"mine");
    }

    #[test]
    fn edited_owned_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &artifacts()).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "// my edit").unwrap();
        let err = write(dir.path(), &artifacts()).unwrap_err();
        assert!(err.to_string().contains("drift"));
    }
}
