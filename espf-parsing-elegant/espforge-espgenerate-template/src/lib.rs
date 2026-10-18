use include_dir::{Dir, include_dir};

/// The project template tree, embedded at compile time from `src/template`.
/// Rendered by `espforge-emit` (see its `render` module).
pub static TEMPLATE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/template");

/// The option schema (KDL v2), embedded at compile time.
/// Loaded by `espforge-options`.
pub const OPTIONS_KDL: &str = include_str!("options.kdl");
