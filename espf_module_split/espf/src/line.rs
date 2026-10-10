use winnow::{ascii::space0, prelude::*};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line<'a> {
    Section(&'a str),
    Tag(&'a str),
    /// A `--- ...` comment line: skipped mid-tag, so a multi-line element can
    /// carry interleaved comments.
    Comment,
    Ignore,
}

pub(crate) fn parse_line(line: &str) -> Result<Line<'_>, String> {
    let mut input = line;
    let _: Result<(&str, &str), ()> = (space0, space0).parse_next(&mut input); // always succeeds

    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return Ok(Line::Ignore);
    };

    match first {
        '#' => parse_section_tail(chars.as_str()),
        '<' => Ok(Line::Tag(chars.as_str())),
        '-' => Ok(Line::Comment),
        _ => Ok(Line::Ignore),
    }
}

fn parse_section_tail(input: &str) -> Result<Line<'_>, String> {
    let mut input = input;
    let _: Result<(&str, &str), ()> = (space0, space0).parse_next(&mut input); // always succeeds

    let name_end = input
        .find(|ch: char| ch.is_whitespace())
        .unwrap_or(input.len());

    if name_end == 0 {
        return Err("Section heading requires a name".to_owned());
    }

    Ok(Line::Section(&input[..name_end]))
}
