use espf::context::ParsedNode;
use espforge_model::{Console, Meta, Runtime};

/// Fixed-schema builtin for the `# espforge` section.
/// Collects every problem instead of stopping at the first.
pub fn parse_meta(nodes: &[ParsedNode]) -> Result<Meta, Vec<String>> {
    let mut errors = Vec::new();
    let mut project: Option<&ParsedNode> = None;
    let mut chip: Option<&str> = None;
    let mut runtime: Option<&str> = None;
    let mut console: Option<&str> = None;

    for n in nodes {
        match n.tag_name {
            "project" if project.is_none() => project = Some(n),
            "chip" if chip.is_none() => chip = n.get_attr("type"),
            "runtime" if runtime.is_none() => runtime = n.get_attr("type"),
            "console" if console.is_none() => console = n.get_attr("type"),
            "project" | "chip" | "runtime" | "console" => {
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

    // Optional; defaults to UART.
    let console = match console {
        None => Console::default(),
        Some("uart") => Console::Uart,
        Some("jtag-serial") => Console::JtagSerial,
        Some(other) => {
            errors.push(format!(
                "unknown console \"{other}\" (expected uart or jtag-serial)"
            ));
            Console::default()
        }
    };
    // Only the case we are sure of; other UART-only chips can be added here later.
    if chip == Some("esp32") && console == Console::JtagSerial {
        errors.push("esp32 has no USB-Serial-JTAG; use <console type=\"uart\" />".into());
    }

    match (name, chip, runtime) {
        (Some(name), Some(chip), Some(runtime)) if errors.is_empty() => Ok(Meta {
            name: name.to_string(),
            description: project
                .and_then(|p| p.get_attr("description"))
                .map(str::to_string),
            chip: chip.to_string(),
            runtime,
            console,
        }),
        _ => Err(errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use espf::parser::parse_espf_runtime;

    const HEAD: &str = "# espforge\n  <project name=\"hello\"/>\n  <runtime type=\"blocking\" />\n";

    fn meta(chip_and_console: &str) -> Result<Meta, Vec<String>> {
        let src = format!("{HEAD}{chip_and_console}");
        let sections = parse_espf_runtime(&src);
        parse_meta(&sections["espforge"])
    }

    #[test]
    fn parses_helloworld_header() {
        let m = meta("  <chip type=\"esp32c3\" />\n").unwrap();
        assert_eq!(m.chip, "esp32c3");
        assert_eq!(m.runtime, Runtime::Blocking);
    }

    #[test]
    fn console_defaults_to_uart() {
        assert_eq!(
            meta("  <chip type=\"esp32c3\" />\n").unwrap().console,
            Console::Uart
        );
    }

    #[test]
    fn console_can_be_jtag_serial() {
        let m = meta("  <chip type=\"esp32c3\" />\n  <console type=\"jtag-serial\" />\n").unwrap();
        assert_eq!(m.console, Console::JtagSerial);
    }

    #[test]
    fn unknown_console_is_reported() {
        assert!(meta("  <chip type=\"esp32c3\" />\n  <console type=\"usb\" />\n").is_err());
    }

    #[test]
    fn esp32_rejects_jtag_serial() {
        assert!(meta("  <chip type=\"esp32\" />\n  <console type=\"jtag-serial\" />\n").is_err());
    }

    #[test]
    fn missing_chip_is_reported() {
        let sections = parse_espf_runtime(HEAD);
        assert!(parse_meta(&sections["espforge"]).is_err());
    }
}
