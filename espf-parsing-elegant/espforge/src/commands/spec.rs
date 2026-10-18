//! `--write-spec`: the effective `.espf` after flag overrides, in canonical
//! form (via `espf::writer`), so it round-trips.

use espf::context::ParsedNode;
use espf::diag::EspfError;
use espf::value::{Object, Spanned, Value};
use espf::writer::{Document, Element, WriteError, write_document};
use espforge_model::{Console, Meta, Runtime};
use espforge_options::{Answers, OptValue};
use std::collections::HashMap;

fn s(v: &str) -> Value {
    Value::Str(Spanned {
        value: v.to_string(),
        span: 0..0,
    })
}

fn element(tag: &str, attrs: Vec<(&str, Value)>) -> Element {
    let mut o = Object::new(0..0);
    for (k, v) in attrs {
        o.fields.insert(k.to_string(), v);
    }
    Element {
        tag: tag.to_string(),
        attrs: o,
    }
}

fn opt_value(v: &OptValue) -> Value {
    match v {
        OptValue::Bool(b) => s(if *b { "true" } else { "false" }),
        OptValue::Str(x) => s(x),
        OptValue::List(items) => Value::Array(
            items
                .iter()
                .map(|i| Spanned {
                    value: i.clone(),
                    span: 0..0,
                })
                .collect(),
        ),
    }
}

/// The document with `# espforge` rebuilt from the final core settings and
/// explicit option answers; the other sections are carried over unchanged.
pub fn effective_document(
    sections: &HashMap<String, Vec<ParsedNode>>,
    meta: &Meta,
    answers: &Answers,
) -> Result<Document, Vec<EspfError>> {
    let mut doc = Document::from_sections(sections)?;

    let mut project = vec![("name", s(&meta.name))];
    if let Some(d) = &meta.description {
        project.push(("description", s(d)));
    }
    let mut options = vec![(
        "defaults",
        s(if answers.defaults { "true" } else { "false" }),
    )];
    for (name, a) in &answers.values {
        options.push((name.as_str(), opt_value(&a.value)));
    }

    doc.sections.insert(
        "espforge".to_string(),
        vec![
            element("project", project),
            element("chip", vec![("type", s(&meta.chip))]),
            element(
                "runtime",
                vec![(
                    "type",
                    s(match meta.runtime {
                        Runtime::Blocking => "blocking",
                        Runtime::Embassy => "embassy",
                    }),
                )],
            ),
            element(
                "console",
                vec![(
                    "type",
                    s(match meta.console {
                        Console::Uart => "uart",
                        Console::JtagSerial => "jtag-serial",
                    }),
                )],
            ),
            element("options", options),
        ],
    );
    Ok(doc)
}

pub fn render(doc: &Document) -> Result<String, WriteError> {
    write_document(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use espf::parser::parse_espf;

    const SRC: &str = "# espforge\n  <project name=\"hello\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n# components\n# devices\n# services\n  <ntp id=\"n\" servers+=\"a\" servers+=\"b\" />\n";

    fn effective(meta: &Meta, answers: &Answers) -> (String, Document) {
        let p = parse_espf(SRC);
        let doc = effective_document(&p.sections, meta, answers).unwrap();
        (render(&doc).unwrap(), doc)
    }

    fn meta() -> Meta {
        Meta {
            name: "hello".into(),
            description: None,
            chip: "esp32s3".into(),
            runtime: Runtime::Embassy,
            console: Console::Uart,
        }
    }

    #[test]
    fn overrides_are_reflected_and_other_sections_kept() {
        let mut answers = Answers::empty(0..0);
        answers
            .apply_flag(espforge_options::embedded_schema(), "alloc", "false")
            .unwrap();
        let (text, _) = effective(&meta(), &answers);
        assert!(text.contains("<chip type=\"esp32s3\" />"), "{text}");
        assert!(text.contains("<runtime type=\"embassy\" />"));
        assert!(
            text.contains("<options alloc=\"false\" defaults=\"true\" />"),
            "{text}"
        );
        assert!(text.contains("<ntp id=\"n\" servers+=\"a\" servers+=\"b\" />"));
    }

    #[test]
    fn output_round_trips() {
        let (text, doc) = effective(&meta(), &Answers::empty(0..0));
        let again = parse_espf(&text);
        assert!(
            again.junk.is_empty() && again.structure_errors.is_empty(),
            "{text}"
        );
        assert_eq!(Document::from_sections(&again.sections).unwrap(), doc);
        // and the spec is itself loadable by the front end
        assert!(crate::commands::frontend::load(&text).is_ok(), "{text}");
    }
}
