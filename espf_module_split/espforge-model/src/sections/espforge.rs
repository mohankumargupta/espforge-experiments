use espf::Section;

use crate::{
    chips::{self, ChipSpec},
    cx::{quoted_list, Cx},
};

/// The `# espforge` section. Fixed schema, nothing in it is referenced by id.
#[derive(Debug, Clone)]
pub struct EspForge {
    pub project_name: String,
    pub project_description: Option<String>,
    pub chip: &'static ChipSpec,
    pub runtime: String,
}

const TAGS: &[&str] = &["project", "chip", "runtime"];

/// Returns `None` if a required piece is missing or the chip is unknown; the
/// reason has already been reported on `cx`.
pub fn extract(cx: &mut Cx, section: &Section) -> Option<EspForge> {
    for element in &section.elements {
        if !TAGS.contains(&element.tag.as_str()) {
            cx.error(
                element.span.clone(),
                format!(
                    "Unknown <{}> in `espforge`; expected {}",
                    element.tag,
                    quoted_list(TAGS),
                ),
            );
        }
    }

    let project = cx.single(section, "project");
    let chip = cx.single(section, "chip");
    let runtime = cx.single(section, "runtime");

    let mut project_name = None;
    let mut project_description = None;
    if let Some(element) = project {
        cx.check_attrs(element, &["name", "description"]);
        project_name = cx.require(element, "name").map(|attr| attr.value.clone());
        project_description = element.value("description").map(str::to_owned);
    }

    let mut chip_spec = None;
    if let Some(element) = chip {
        cx.check_attrs(element, &["type"]);

        if let Some(attr) = cx.require(element, "type") {
            chip_spec = chips::lookup(&attr.value);

            if chip_spec.is_none() {
                cx.error(
                    attr.value_span.clone(),
                    format!(
                        "Unknown chip `{}`; supported: {}",
                        attr.value,
                        quoted_list(&chips::names()),
                    ),
                );
            }
        }
    }

    let mut runtime_type = None;
    if let Some(element) = runtime {
        cx.check_attrs(element, &["type"]);
        runtime_type = cx.require(element, "type").map(|attr| attr.value.clone());
    }

    Some(EspForge {
        project_name: project_name?,
        project_description,
        chip: chip_spec?,
        runtime: runtime_type?,
    })
}
