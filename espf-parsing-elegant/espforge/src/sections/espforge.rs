use espf::context::ParsedNode;
use espforge_model::{Meta, Runtime};

/// Fixed-schema builtin for the `# espforge` section.
/// Collects every problem instead of stopping at the first.
pub fn parse_meta(nodes: &[ParsedNode]) -> Result<Meta, Vec<String>> {
    let mut errors = Vec::new();
    let mut project: Option<&ParsedNode> = None;
    let mut chip: Option<&str> = None;
    let mut runtime: Option<&str> = None;

    for n in nodes {
        match n.tag_name {
            "project" if project.is_none() => project = Some(n),
            "chip" if chip.is_none() => chip = n.get_attr("type"),
            "runtime" if runtime.is_none() => runtime = n.get_attr("type"),
            "project" | "chip" | "runtime" => {
                errors.push(format!("duplicate <{}> in # espforge", n.tag_name))
            }
            other => errors.push(format!("unknown element <{other}> in # espforge")),
        }
    }

    let name = project.and_then(|p| p.get_attr("name"));
    if name.is_none() {
        errors.push("# espforge needs <project name=\"...\" />".into());
    }
    if chip.is_none() {
        errors.push("# espforge needs <chip type=\"...\" />".into());
    }
    let runtime = match runtime {
        Some("blocking") => Some(Runtime::Blocking),
        Some("embassy") => Some(Runtime::Embassy),
        Some(other) => {
            errors.push(format!(
                "unknown runtime \"{other}\" (expected blocking or embassy)"
            ));
            None
        }
        None => {
            errors.push("# espforge needs <runtime type=\"...\" />".into());
            None
        }
    };

    match (name, chip, runtime) {
        (Some(name), Some(chip), Some(runtime)) if errors.is_empty() => Ok(Meta {
            name: name.to_string(),
            description: project
                .and_then(|p| p.get_attr("description"))
                .map(str::to_string),
            chip: chip.to_string(),
            runtime,
        }),
        _ => Err(errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use espf::parser::parse_espf_runtime;

    #[test]
    fn parses_helloworld_header() {
        let src = "# espforge\n  <project name=\"hello\"/>\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n";
        let sections = parse_espf_runtime(src);
        let meta = parse_meta(&sections["espforge"]).unwrap();
        assert_eq!(meta.chip, "esp32c3");
        assert_eq!(meta.runtime, Runtime::Blocking);
    }

    #[test]
    fn missing_chip_is_reported() {
        let src = "# espforge\n  <project name=\"hello\"/>\n  <runtime type=\"blocking\" />\n";
        let sections = parse_espf_runtime(src);
        assert!(parse_meta(&sections["espforge"]).is_err());
    }
}
