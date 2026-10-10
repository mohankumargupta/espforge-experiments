//! ESPF syntax layer: text in, structured result out.
//! Knows nothing about chips, peripherals or drivers.

mod diagnostic;
mod line;
mod parse;
mod tag;

pub use diagnostic::ParseDiagnostic;
pub use parse::{parse_to_leaf_table, LeafFields, LeafTable};
