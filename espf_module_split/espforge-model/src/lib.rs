//! Semantic layer: turns an `espf::Document` into a checked `Model`.
//!
//! Sections are processed in dependency order. Each stage takes what the
//! previous ones produced, so the order is enforced by the data they need:
//!
//! ```text
//! espforge -> chip spec -> peripherals -> components -> devices
//! ```

pub mod chips;
mod cx;
pub mod registry;
mod sections;

pub use chips::ChipSpec;
pub use cx::{Cx, Diagnostics, Symbol, Symbols};
pub use registry::{AttrField, ElementSchema, Kind, RefField, COMPONENTS, DEVICES};
pub use sections::elements::{Instance, Resolved, Value};
pub use sections::espforge::EspForge;
pub use sections::peripherals::{Gpio, I2c, Peripherals, Spi, Uart};

use espf::Document;

/// Everything checked and resolved from one ESPF file.
#[derive(Debug)]
pub struct Model {
    pub espforge: EspForge,
    pub peripherals: Peripherals,
    pub components: Vec<Instance>,
    pub devices: Vec<Instance>,
    pub symbols: Symbols,
}

/// Parse and check ESPF source text. Syntax errors and semantic errors come
/// back as the same `Diagnostics` type.
pub fn load(source: &str) -> Result<Model, Diagnostics> {
    let doc = espf::parse(source).map_err(|error| Diagnostics(vec![error]))?;
    build(&doc)
}

/// Check an already parsed document.
pub fn build(doc: &Document) -> Result<Model, Diagnostics> {
    let mut cx = Cx::new(doc);

    let espforge = match doc.section("espforge") {
        Some(section) => sections::espforge::extract(&mut cx, section),
        None => {
            cx.error(0..0, "Missing mandatory section `espforge`");
            None
        }
    };

    let peripherals = match (doc.section("peripherals"), &espforge) {
        (Some(section), Some(espforge)) => Some(sections::peripherals::extract(
            &mut cx,
            section,
            espforge.chip,
        )),
        // Without a known chip there is nothing to check pins against.
        (Some(_), None) => None,
        (None, _) => {
            cx.error(0..0, "Missing mandatory section `peripherals`");
            None
        }
    };

    // Components and devices refer to peripherals. If those could not be
    // built, every reference would fail too, so skip them rather than bury
    // the real error.
    let (components, devices) = if peripherals.is_some() {
        let components = element_stage(&mut cx, doc, "components", &COMPONENTS);
        let devices = element_stage(&mut cx, doc, "devices", &DEVICES);
        (components, devices)
    } else {
        (Vec::new(), Vec::new())
    };

    let symbols = cx.finish()?;

    // `finish` only succeeds when no error was reported, and every stage that
    // returns `None` reports an error first.
    Ok(Model {
        espforge: espforge.expect("no diagnostics means the espforge stage produced output"),
        peripherals: peripherals.expect("no diagnostics means the peripherals stage produced output"),
        components,
        devices,
        symbols,
    })
}

fn element_stage(
    cx: &mut Cx,
    doc: &Document,
    name: &str,
    schemas: &[ElementSchema],
) -> Vec<Instance> {
    match doc.section(name) {
        Some(section) => sections::elements::extract(cx, section, schemas),
        None => {
            cx.error(0..0, format!("Missing mandatory section `{name}`"));
            Vec::new()
        }
    }
}
