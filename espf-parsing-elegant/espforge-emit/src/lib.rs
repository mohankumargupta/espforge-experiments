//! Code emitters. Each one is a pure function from the model to in-memory
//! artifacts; nothing in here touches the filesystem or the environment.
//! (Host-only: never depend on `espforge-runtime` from this crate.)

pub mod rust;

use anyhow::Result;
use espforge_model::{Artifact, Project};

pub use rust::{EmitOptions, RuntimeSource};

/// Run all emitters and return every artifact espforge owns.
pub fn generate(project: &Project, opts: &EmitOptions) -> Result<Vec<Artifact>> {
    rust::emit(project, opts)
}
