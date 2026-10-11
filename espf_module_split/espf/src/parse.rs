use winnow::{
    ascii::{line_ending, till_line_ending},
    combinator::opt,
    prelude::*,
    stream::{LocatingSlice, Location},
};

use crate::{
    ast::{Document, Section},
    diagnostic::{diagnostic_at_current, ParseDiagnostic},
    line::{parse_line, Line},
    tag::{finish_tag_body, parse_element},
};

pub fn parse(source: &str) -> Result<Document, ParseDiagnostic> {
    let mut input = LocatingSlice::new(source);
    let mut sections: Vec<Section> = Vec::new();

    while !input.is_empty() {
        // LocatingSlice tracks this line's range in the original source.
        let (line, line_span) = till_line_ending
            .with_span()
            .parse_next(&mut input)
            .map_err(|error: winnow::error::ContextError| {
                diagnostic_at_current(
                    source,
                    input.current_token_start(),
                    format!("Could not read line: {error:?}"),
                )
            })?;

        // Consume the line ending, if present.
        let _ = opt::<_, _, winnow::error::ContextError, _>(line_ending).parse_next(&mut input);

        let parsed_line = parse_line(line)
            .map_err(|message| ParseDiagnostic::new(source, line_span.clone(), message))?;

        match parsed_line {
            Line::Section(name) => sections.push(Section {
                name: name.to_owned(),
                span: line_span,
                elements: Vec::new(),
            }),

            Line::Tag(tag_tail) => {
                let body = finish_tag_body(&mut &mut input, source, line_span.clone(), tag_tail)?;

                let Some(section) = sections.last_mut() else {
                    return Err(ParseDiagnostic::new(
                        source,
                        body.span.clone(),
                        "Tag encountered before a section heading",
                    ));
                };

                let element = parse_element(&body)
                    .map_err(|message| ParseDiagnostic::new(source, body.span.clone(), message))?;

                section.elements.push(element);
            }

            Line::Ignore | Line::Comment => {}
        }
    }

    Ok(Document {
        source: source.to_owned(),
        sections,
    })
}
