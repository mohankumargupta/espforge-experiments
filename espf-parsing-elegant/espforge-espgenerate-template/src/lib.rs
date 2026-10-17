use include_dir::{Dir, include_dir};

/// The example tree, embedded at compile time from `src/examples`.
pub static EXAMPLES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/template");
