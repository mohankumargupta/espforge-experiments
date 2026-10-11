//! What a component or device looks like, as data.
//!
//! Adding a component or device means adding one `ElementSchema` to a
//! distributed slice, from any crate linked into the final binary. The
//! checking itself is shared (see `sections::elements`).

use linkme::distributed_slice;

/// How an attribute's text is read.
#[derive(Debug)]
pub enum Kind {
    Text,
    /// Decimal (`9600`) or hex (`0x48`).
    Int,
    OneOf(&'static [&'static str]),
}

/// A plain attribute: `address="0x48"`.
#[derive(Debug)]
pub struct AttrField {
    pub key: &'static str,
    pub kind: Kind,
    pub required: bool,
}

/// An attribute that must be a `$reference` to another element.
#[derive(Debug)]
pub struct RefField {
    pub key: &'static str,
    /// The tag the referenced element must have, e.g. `i2c`.
    pub target: &'static str,
    pub required: bool,
}

#[derive(Debug)]
pub struct ElementSchema {
    pub tag: &'static str,
    pub refs: &'static [RefField],
    pub attrs: &'static [AttrField],
}

/// Elements allowed under `# components`.
#[distributed_slice]
pub static COMPONENTS: [ElementSchema];

/// Elements allowed under `# devices`.
#[distributed_slice]
pub static DEVICES: [ElementSchema];
