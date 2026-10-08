//! Parser: a single winnow grammar over the whole file.
//!
//! Reads the espf text format and builds structs::EspfFile.
//! References are preserved as-is (plain strings).

use crate::structs::{
    Component, Device, EspfFile, Espforge, Gpio, GpioComponent, I2c, I2cComponent, Spi,
    SpiComponent, Uart, UartComponent,
};
use std::fmt;

use winnow::ascii::{dec_uint, line_ending, multispace0, space0, space1, Caseless, Uint};
use winnow::combinator::{alt, cut_err, dispatch, eof, fail, opt, peek, repeat, terminated};
use winnow::error::{ContextError, StrContext, StrContextValue};
use winnow::prelude::*;
use winnow::token::{any, literal, take_till, take_until, take_while};
use winnow::BStr;

/// Parsing runs on `u8` tokens; `BStr` keeps slices readable in errors and debug output.
type Input<'i> = &'i BStr;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
enum Section {
    Espforge,
    Peripherals,
    Components,
    Devices,
    /// section header we don't route tags into yet
    #[default]
    Other,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Kind {
    Gpio,
    I2c,
    Spi,
    Uart,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Subsection<'i> {
    kind: Option<Kind>,
    title: &'i str,
}

impl<'i> Default for Subsection<'i> {
    fn default() -> Self {
        Self {
            kind: None,
            title: "#",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Line<'i> {
    Ignored,
    Section(Section),
    Subsection(Subsection<'i>),
    Tag(Tag<'i>),
}

#[derive(Debug, PartialEq, Eq)]
struct Tag<'i> {
    name: &'i str,
    attrs: Vec<(&'i str, &'i str)>,
}

#[derive(Debug, Default)]
struct Builder<'i> {
    file: EspfFile,
    section: Section,
    subsection: Subsection<'i>,
    line: usize,
    error: Option<ParseError>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// Read file content and build EspfFile. References are preserved as-is.
pub fn parse(content: &str) -> Result<EspfFile, ParseError> {
    let mut document = document();
    let input = BStr::new(content.as_bytes());
    let builder = document.parse(input).map_err(|error| ParseError {
        line: line_of(content.as_bytes(), error.offset()),
        message: error.inner().to_string().replace('\n', ": "),
    })?;
    match builder.error {
        Some(error) => Err(error),
        None => Ok(builder.file),
    }
}

/// The combinator chain stays inside the closure so its type never reaches a signature.
fn document<'i>() -> impl Parser<Input<'i>, Builder<'i>, ContextError> {
    move |input: &mut Input<'i>| {
        let builder = repeat(0.., line).fold(Builder::default, |mut builder, event| {
            builder.apply(event);
            builder
        })
        .parse_next(input)?;
        multispace0.parse_next(input)?;
        Ok(builder)
    }
}

/// One physical line per successful run, so `Builder::line` matches the file.
fn line<'i>(input: &mut Input<'i>) -> ModalResult<Line<'i>> {
    space0.parse_next(input)?;
    let event = dispatch! {peek(any);
        b'#' => alt((subsection_header, section_header, ignored)),
        b'<' => cut_err(tag_line),
        _ => ignored,
    }
    .parse_next(input)?;
    take_till(0.., b'\n').parse_next(input)?;
    opt(line_ending).parse_next(input)?;
    Ok(event)
}

fn ignored<'i>(_: &mut Input<'i>) -> ModalResult<Line<'i>> {
    Ok(Line::Ignored)
}

fn section_header<'i>(input: &mut Input<'i>) -> ModalResult<Line<'i>> {
    b'#'.parse_next(input)?;
    space1.parse_next(input)?;
    space0.parse_next(input)?;
    let title = take_till(0.., b'\n').parse_next(input)?;
    Ok(Line::Section(section_of(text(title))))
}

fn subsection_header<'i>(input: &mut Input<'i>) -> ModalResult<Line<'i>> {
    literal(b"##").parse_next(input)?;
    space1.parse_next(input)?;
    space0.parse_next(input)?;
    let title = take_till(0.., b'\n').parse_next(input)?;
    let title = text(title);
    Ok(Line::Subsection(Subsection {
        kind: kind_of(title),
        title,
    }))
}

fn tag_line<'i>(input: &mut Input<'i>) -> ModalResult<Line<'i>> {
    b'<'.parse_next(input)?;
    let tag = alt((simple_tag, full_tag)).parse_next(input)?;
    Ok(Line::Tag(tag))
}

/// Fast path for `<name key="value" ... />`; anything else falls back to [`full_tag`].
fn simple_tag<'i>(input: &mut Input<'i>) -> ModalResult<Tag<'i>> {
    let name = take_until(1.., (b' ', b'/', b'\n')).parse_next(input)?;
    let attrs = repeat(0.., simple_attribute).parse_next(input)?;
    (space0, literal(b"/>")).parse_next(input)?;
    Ok(Tag {
        name: text(name),
        attrs,
    })
}

fn simple_attribute<'i>(input: &mut Input<'i>) -> ModalResult<(&'i str, &'i str)> {
    space1.parse_next(input)?;
    let key = attribute_key.parse_next(input)?;
    space0.parse_next(input)?;
    b'='.parse_next(input)?;
    space0.parse_next(input)?;
    b'"'.parse_next(input)?;
    let value = take_while(0.., |b: u8| b != b'"' && b != b'\n').parse_next(input)?;
    b'"'.parse_next(input)?;
    Ok((key, text(value)))
}

fn full_tag<'i>(input: &mut Input<'i>) -> ModalResult<Tag<'i>> {
    let name = take_while(1.., |b: u8| b != b' ' && b != b'/' && !b.is_ascii_whitespace())
        .context(StrContext::Label("tag name"))
        .parse_next(input)?;
    let attrs = repeat(0.., attribute).parse_next(input)?;
    (space0, literal(b"/>"))
        .context(StrContext::Expected(StrContextValue::Description(
            "end of tag",
        )))
        .parse_next(input)?;
    Ok(Tag {
        name: text(name),
        attrs,
    })
}

fn attribute<'i>(input: &mut Input<'i>) -> ModalResult<(&'i str, &'i str)> {
    space0.parse_next(input)?;
    let key = attribute_key.parse_next(input)?;
    let value = opt(assignment).parse_next(input)?;
    Ok((key, value.unwrap_or("")))
}

fn assignment<'i>(input: &mut Input<'i>) -> ModalResult<&'i str> {
    space0.parse_next(input)?;
    b'='.parse_next(input)?;
    space0.parse_next(input)?;
    attribute_value.parse_next(input)
}

fn attribute_key<'i>(input: &mut Input<'i>) -> ModalResult<&'i str> {
    let key = take_while(1.., |b: u8| b != b'=' && b != b'/' && !b.is_ascii_whitespace())
        .parse_next(input)?;
    Ok(text(key))
}

/// Quoted values never cross a line, so a missing quote reports its own line.
fn attribute_value<'i>(input: &mut Input<'i>) -> ModalResult<&'i str> {
    alt((
        quoted_value,
        take_while(0.., |b: u8| !b.is_ascii_whitespace() && b != b'/').map(text),
    ))
    .parse_next(input)
}

fn quoted_value<'i>(input: &mut Input<'i>) -> ModalResult<&'i str> {
    b'"'.parse_next(input)?;
    let value = cut_err(terminated(
        take_while(0.., |b: u8| b != b'"' && b != b'\n'),
        b'"',
    ))
    .context(StrContext::Label("attribute value"))
    .context(StrContext::Expected(StrContextValue::Description(
        "closing '\"'",
    )))
    .parse_next(input)?;
    Ok(text(value))
}

fn section_of(title: &str) -> Section {
    let mut rest = title;
    let parsed: ModalResult<Section> = dispatch! {peek(any);
        'e' | 'E' => literal(Caseless("espforge")).value(Section::Espforge),
        'p' | 'P' => literal(Caseless("peripherals")).value(Section::Peripherals),
        'c' | 'C' => literal(Caseless("components")).value(Section::Components),
        'd' | 'D' => literal(Caseless("devices")).value(Section::Devices),
        _ => fail,
    }
    .parse_next(&mut rest);
    parsed.unwrap_or(Section::Other)
}

fn kind_of(name: &str) -> Option<Kind> {
    let mut rest = name;
    let parsed: ModalResult<Kind> = dispatch! {peek(any);
        'g' | 'G' => literal(Caseless("gpio")).value(Kind::Gpio),
        'i' | 'I' => literal(Caseless("i2c")).value(Kind::I2c),
        's' | 'S' => literal(Caseless("spi")).value(Kind::Spi),
        'u' | 'U' => literal(Caseless("uart")).value(Kind::Uart),
        _ => fail,
    }
    .parse_next(&mut rest);
    parsed.ok()
}

/// Number of lines before `offset`, counted with winnow rather than by hand.
fn line_of(input: &[u8], offset: usize) -> usize {
    let mut rest = &input[..offset];
    let mut lines = repeat(0.., (take_till(0.., b'\n'), b'\n')).fold(|| 1usize, |line, _| line + 1);
    let counted: ModalResult<usize> = lines.parse_next(&mut rest);
    counted.unwrap_or(1)
}

/// Input comes from `&str`, so every slice of it is valid UTF-8.
fn text(slice: &[u8]) -> &str {
    core::str::from_utf8(slice).expect("espf input is valid UTF-8")
}

fn dec<T: Uint>(raw: &str) -> ModalResult<T> {
    let mut input = raw;
    let value = dec_uint.parse_next(&mut input)?;
    eof.parse_next(&mut input)?;
    Ok(value)
}

fn number_error(tag: &Tag<'_>, key: &str, raw: &str) -> String {
    format!(
        "<{}> attribute '{}={}' is not a number",
        tag.name, key, raw
    )
}

fn required_u32(tag: &Tag<'_>, key: &str) -> Result<u32, String> {
    let raw = tag.required(key)?;
    dec::<u32>(raw).map_err(|_| number_error(tag, key, raw))
}

fn optional_u32(tag: &Tag<'_>, key: &str) -> Result<Option<u32>, String> {
    match tag.attr(key) {
        None | Some("") => Ok(None),
        Some(raw) => dec::<u32>(raw)
            .map(Some)
            .map_err(|_| number_error(tag, key, raw)),
    }
}

impl<'i> Tag<'i> {
    fn attr(&self, key: &str) -> Option<&'i str> {
        self.attrs
            .iter()
            .copied()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }

    fn required(&self, key: &str) -> Result<&'i str, String> {
        self.attr(key)
            .ok_or_else(|| format!("<{}> is missing attribute '{}'", self.name, key))
    }

    fn optional(&self, key: &str) -> Option<&'i str> {
        self.attr(key).filter(|value| !value.is_empty())
    }
}

impl<'i> Builder<'i> {
    fn apply(&mut self, event: Line<'i>) {
        if self.error.is_some() {
            return;
        }
        self.line += 1;
        match event {
            Line::Ignored => {}
            Line::Section(section) => {
                self.section = section;
                self.subsection = Subsection::default();
            }
            Line::Subsection(subsection) => self.subsection = subsection,
            Line::Tag(tag) => {
                if let Err(message) = self.build(&tag) {
                    self.error = Some(ParseError {
                        line: self.line,
                        message,
                    });
                }
            }
        }
    }

    fn build(&mut self, tag: &Tag<'i>) -> Result<(), String> {
        match self.section {
            Section::Espforge => self.espforge(tag),
            Section::Peripherals => peripheral(&mut self.file, tag, self.subsection),
            Section::Components => {
                self.file.components.push(component(tag)?);
                Ok(())
            }
            Section::Devices => {
                self.file.devices.push(device(tag));
                Ok(())
            }
            Section::Other => Ok(()),
        }
    }

    fn espforge(&mut self, tag: &Tag<'i>) -> Result<(), String> {
        match tag.name {
            "project" => {
                if self.file.espforge.is_some() {
                    return Err("<project> defined twice".to_string());
                }
                self.file.espforge = Some(Espforge {
                    name: tag.required("name")?.to_string(),
                    description: tag.optional("description").unwrap_or_default().to_string(),
                    chip: String::new(),
                    runtime: String::new(),
                });
            }
            "chip" => {
                let chip = tag.required("type")?.to_string();
                self.file
                    .espforge
                    .get_or_insert_with(Espforge::default)
                    .chip = chip;
            }
            "runtime" => {
                let runtime = tag.required("type")?.to_string();
                self.file
                    .espforge
                    .get_or_insert_with(Espforge::default)
                    .runtime = runtime;
            }
            other => return Err(format!("unexpected tag <{other}> in espforge section")),
        }
        Ok(())
    }
}

fn peripheral(
    file: &mut EspfFile,
    tag: &Tag<'_>,
    subsection: Subsection<'_>,
) -> Result<(), String> {
    // The tag name decides first; the ## subsection is the fallback.
    let Some(kind) = kind_of(tag.name).or(subsection.kind) else {
        return Err(format!(
            "unrecognized peripheral <{}> under ## {}",
            tag.name, subsection.title
        ));
    };
    let peripherals = &mut file.peripherals;
    match kind {
        Kind::Gpio => peripherals.gpio.push(Gpio {
            id: tag.required("id")?.to_string(),
            pin: required_u32(tag, "pin")?,
        }),
        Kind::I2c => peripherals.i2c.push(I2c {
            id: tag.required("id")?.to_string(),
            bus: required_u32(tag, "bus")?,
            sda: required_u32(tag, "sda")?,
            scl: required_u32(tag, "scl")?,
            frequency: tag.required("frequency")?.to_string(),
        }),
        Kind::Spi => peripherals.spi.push(Spi {
            id: tag.required("id")?.to_string(),
            bus: required_u32(tag, "bus")?,
            sclk: required_u32(tag, "sclk")?,
            mosi: optional_u32(tag, "mosi")?,
            miso: optional_u32(tag, "miso")?,
        }),
        Kind::Uart => peripherals.uart.push(Uart {
            id: tag.required("id")?.to_string(),
            tx: required_u32(tag, "tx")?,
            rx: required_u32(tag, "rx")?,
        }),
    }
    Ok(())
}

fn component(tag: &Tag<'_>) -> Result<Component, String> {
    Ok(match tag.name {
        "gpio_component" => Component::Gpio(GpioComponent {
            id: tag.optional("id").map(str::to_string),
            gpio: tag.required("gpio")?.to_string(),
            direction: tag.required("direction")?.to_string(),
        }),
        "i2c_component" => Component::I2c(I2cComponent {
            id: tag.optional("id").map(str::to_string),
            i2c: tag.required("i2c")?.to_string(),
            address: tag.required("address")?.to_string(),
        }),
        "spi_component" => Component::Spi(SpiComponent {
            id: tag.optional("id").map(str::to_string),
            spi: tag.required("spi")?.to_string(),
            mode: {
                let raw = tag.required("mode")?;
                dec::<u8>(raw).map_err(|_| format!("<spi_component> mode='{raw}' is not a number"))?
            },
            cs: tag.required("cs")?.to_string(),
            frequency: tag.required("frequency")?.to_string(),
        }),
        "uart_component" => Component::Uart(UartComponent {
            id: tag.optional("id").map(str::to_string),
            uart: tag.required("uart")?.to_string(),
            baud_rate: required_u32(tag, "baud_rate")?,
            parity: tag.required("parity")?.to_string(),
            stop_bits: required_u32(tag, "stop_bits")?,
        }),
        other => Component::Other {
            name: other.to_string(),
            attrs: tag
                .attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        },
    })
}

fn device(tag: &Tag<'_>) -> Device {
    Device {
        id: tag.optional("id").unwrap_or_default().to_string(),
        kind: tag.name.to_string(),
        attrs: tag
            .attrs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# espforge
  <project name="example project" description="example showing espf file format" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals
  ## gpio
     <gpio id="gpio4" pin="4" />
     <gpio id="gpio5" pin="5" />
  ## i2c
     <i2c id="i2c0" bus="0" sda="21" scl="22" frequency="400kHz" />
  ## spi
     --- displays generally omit miso
     <spi id="spi2" bus="2" sclk="18" mosi="23" miso="19" />
  ## uart
     <uart id="uart1" tx="17" rx="16" />

# components
  <gpio_component id="blue_led" gpio="gpio4" direction="output" />
  <i2c_component i2c="i2c0" address="0x48" />
  <spi_component spi="spi2" mode="0" cs="gpio5" frequency="40Mhz" />
  <uart_component uart="uart1" baud_rate="9600" parity="none" stop_bits="1" />

# devices
  ## light
     <status_led id="is_raining" gpio_component="status_led_io" />
  ## environment_sensor
     <tmp102 id="" i2c_component = "temp_sensor_bus" />
  ## display
     <ili9341 id="tft_display" spi_component="display_bus" />
"#;

    #[test]
    fn parse_new_format() {
        let file = parse(SAMPLE).expect("parse ok");
        let ef = file.espforge.as_ref().unwrap();
        assert_eq!(ef.name, "example project");
        assert_eq!(ef.chip, "esp32c3");
        assert_eq!(ef.runtime, "blocking");

        assert_eq!(file.peripherals.gpio.len(), 2);
        assert_eq!(file.peripherals.gpio[0], Gpio { id: "gpio4".into(), pin: 4 });
        assert_eq!(file.peripherals.i2c[0].frequency, "400kHz");
        assert_eq!(file.peripherals.i2c[0].scl, 22);
        assert_eq!(file.peripherals.spi[0],
            Spi { id: "spi2".into(), bus: 2, sclk: 18, mosi: Some(23), miso: Some(19) });
        assert_eq!(file.peripherals.uart[0],
            Uart { id: "uart1".into(), tx: 17, rx: 16 });

        assert_eq!(file.components.len(), 4);
        assert_eq!(file.components[0],
            Component::Gpio(GpioComponent {
                id: Some("blue_led".into()), gpio: "gpio4".into(), direction: "output".into(),
            }));
        match &file.components[2] {
            Component::Spi(s) => {
                assert_eq!(s.id, None);
                assert_eq!(s.mode, 0);
                assert_eq!(s.cs, "gpio5");
            }
            other => panic!("unexpected component {other:?}"),
        }
        match &file.components[3] {
            Component::Uart(u) => {
                assert_eq!(u.baud_rate, 9600);
                assert_eq!(u.stop_bits, 1);
            }
            other => panic!("unexpected component {other:?}"),
        }

        assert_eq!(file.devices.len(), 3);
        let tft = file.devices.iter().find(|d| d.kind == "ili9341").expect("device");
        assert_eq!(tft.id, "tft_display");
        let v = tft.attrs.iter().find(|(k, _)| k == "spi_component").cloned().unwrap();
        assert_eq!(v.1, "display_bus");
    }

    #[test]
    fn missing_section_tags_ignored() {
        let file = parse("  <foo id=\"x\" />").unwrap();
        assert!(file.espforge.is_none());
        assert!(file.components.is_empty());
        assert!(file.devices.is_empty());
    }

    #[test]
    fn parse_error_has_line() {
        let err = parse("# espforge\n<project name=\"x\" />\n<chip />").unwrap_err();
        assert_eq!(err.line, 3);
    }

    #[test]
    fn tag_syntax_errors_report_line_and_reason() {
        let err = parse("# devices\n<gpio id=\"x />\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("closing '\"'"), "{}", err.message);

        let err = parse("# devices\n<gpio =x />\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("end of tag"), "{}", err.message);

        let err = parse("# devices\n< />\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("tag name"), "{}", err.message);
    }

    #[test]
    fn parse_real_example_file() {
        let text = include_str!("../examples/example.espf.xml");
        let file = parse(text).expect("example file parses");
        assert_eq!(file.espforge.as_ref().unwrap().chip, "esp32c3");
        assert_eq!(file.peripherals.gpio.len(), 2);
        assert_eq!(file.devices.len(), 3);
    }
}
