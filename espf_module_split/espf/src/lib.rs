//! ESPF syntax layer: text in, `Document` out.
//! Knows nothing about chips, peripherals or drivers.

mod ast;
mod diagnostic;
mod line;
mod parse;
mod tag;

pub use ast::{Attr, Document, Element, Section};
pub use diagnostic::ParseDiagnostic;
pub use parse::parse;
