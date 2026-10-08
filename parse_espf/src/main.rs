//! ESPF parser, built on the same winnow structure as the original prototype.
//!
//! Two stages:
//!   1. Syntax   (winnow)  : text -> RawFile { sections -> elements -> attributes }
//!   2. Semantics (plain Rust): RawFile -> typed `Espf` model, with every error
//!      reported together, each pointing at a line and column in the source.
//!
//! Cargo.toml:  winnow = "1.0.4"

use std::collections::{HashMap, HashSet};

use winnow::ascii::{multispace0, multispace1, space0, till_line_ending};
use winnow::combinator::{alt, cut_err, delimited, eof, not, preceded, repeat, terminated};
use winnow::error::{ContextError, StrContext, StrContextValue};
use winnow::prelude::*;
use winnow::token::take_while;

// ==========================================
// 1. The core trait (unchanged from the prototype)
// ==========================================
pub trait FromCustomFormat: Sized {
    /// The linear parser engine
    fn parser(input: &mut &str) -> ModalResult<Self, ContextError>;

    /// The top-level user-facing API
    fn decode(input: &str) -> Result<Self, String> {
        Self::parser
            .parse(input)
            .map_err(|err| format!("Syntax error:\n{}", err))
    }
}

// ==========================================
// 2. Raw syntax tree
// ==========================================
// `tail` is the number of bytes left in the input when the node started.
// offset = source.len() - tail, which lets the semantic stage report
// line/column without threading a span type through every parser.

#[derive(Debug)]
pub struct RawElement {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    tail: usize,
}

impl RawElement {
    fn get(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug)]
pub struct RawSection {
    pub name: String,
    pub elements: Vec<RawElement>,
    tail: usize,
}

#[derive(Debug)]
pub struct RawFile {
    pub sections: Vec<RawSection>,
}

// ==========================================
// 3. Base reusable parsers
// ==========================================

/// Whitespace, newlines and `---` comments (a comment runs to end of line).
fn trivia(input: &mut &str) -> ModalResult<()> {
    repeat(
        0..,
        alt((
            take_while(1.., |c: char| c.is_whitespace()).void(),
            preceded("---", till_line_ending).void(),
        )),
    )
    .fold(|| (), |(), ()| ())
    .parse_next(input)
}

fn ident<'i>(input: &mut &'i str) -> ModalResult<&'i str> {
    take_while(1.., |c: char| c.is_ascii_alphanumeric() || c == '_')
        .context(StrContext::Label("identifier"))
        .parse_next(input)
}

/// `"..."` on a single line, no escapes.
fn quoted<'i>(input: &mut &'i str) -> ModalResult<&'i str> {
    delimited(
        '"',
        take_while(0.., |c: char| c != '"' && c != '\n'),
        cut_err('"').context(StrContext::Expected(StrContextValue::CharLiteral('"'))),
    )
    .context(StrContext::Label("quoted string"))
    .parse_next(input)
}

/// `key = "value"`
fn attr(input: &mut &str) -> ModalResult<(String, String)> {
    let key = ident.parse_next(input)?;
    cut_err((space0, '=', space0))
        .context(StrContext::Expected(StrContextValue::CharLiteral('=')))
        .parse_next(input)?;
    let val = cut_err(quoted).parse_next(input)?;
    Ok((key.to_string(), val.to_string()))
}

/// `<tag key="value" ... />`
fn element(input: &mut &str) -> ModalResult<RawElement> {
    let tail = input.len();
    '<'.parse_next(input)?;
    let tag = cut_err(ident)
        .context(StrContext::Label("element name"))
        .parse_next(input)?;
    let attrs: Vec<(String, String)> =
        repeat(0.., preceded(multispace1, attr)).parse_next(input)?;
    multispace0.parse_next(input)?;
    cut_err("/>")
        .context(StrContext::Expected(StrContextValue::StringLiteral("/>")))
        .parse_next(input)?;
    Ok(RawElement {
        tag: tag.to_string(),
        attrs,
        tail,
    })
}

/// `# name` followed by elements. A second-level `##` is rejected on purpose.
fn section(input: &mut &str) -> ModalResult<RawSection> {
    let tail = input.len();
    '#'.parse_next(input)?;
    cut_err(not('#'))
        .context(StrContext::Expected(StrContextValue::Description(
            "a section name after a single `#` (`##` headings are not part of ESPF)",
        )))
        .parse_next(input)?;
    space0.parse_next(input)?;
    let name = cut_err(ident)
        .context(StrContext::Label("section name"))
        .parse_next(input)?;
    trivia.parse_next(input)?;
    let elements: Vec<RawElement> = repeat(0.., terminated(element, trivia)).parse_next(input)?;
    Ok(RawSection {
        name: name.to_string(),
        elements,
        tail,
    })
}

impl FromCustomFormat for RawFile {
    fn parser(input: &mut &str) -> ModalResult<Self, ContextError> {
        trivia.parse_next(input)?;
        let sections: Vec<RawSection> = cut_err(repeat(1.., section))
            .context(StrContext::Expected(StrContextValue::Description(
                "a `# section` heading",
            )))
            .parse_next(input)?;
        cut_err(eof)
            .context(StrContext::Expected(StrContextValue::Description(
                "an element like `<tag attr=\"value\" />`, a `# section` heading, or end of file",
            )))
            .parse_next(input)?;
        Ok(RawFile { sections })
    }
}

// ==========================================
// 4. Typed model
// ==========================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Esp32c3,
    Esp32c6,
}

impl Chip {
    fn name(self) -> &'static str {
        match self {
            Chip::Esp32c3 => "esp32c3",
            Chip::Esp32c6 => "esp32c6",
        }
    }
    /// Highest GPIO number that exists on the chip.
    fn max_gpio(self) -> u8 {
        match self {
            Chip::Esp32c3 => 21,
            Chip::Esp32c6 => 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Blocking,
    Embassy,
}

#[derive(Debug)]
pub struct Project {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug)]
pub enum Peripheral {
    Gpio {
        id: String,
        pin: u8,
    },
    I2c {
        id: String,
        sda: u8,
        scl: u8,
        frequency_hz: u32,
    },
    Spi {
        id: String,
        sclk: u8,
        mosi: u8,
        miso: Option<u8>,
    },
    Uart {
        id: String,
        tx: u8,
        rx: u8,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy)]
pub enum Parity {
    None,
    Even,
    Odd,
}

#[derive(Debug)]
pub enum Component {
    Gpio {
        id: String,
        gpio: String,
        direction: Direction,
    },
    I2c {
        id: String,
        i2c: String,
        address: u8,
    },
    Spi {
        id: String,
        spi: String,
        mode: u8,
        cs: String,
        frequency_hz: u32,
    },
    Uart {
        id: String,
        uart: String,
        baud_rate: u32,
        parity: Parity,
        stop_bits: u8,
    },
}

#[derive(Debug)]
pub struct Device {
    pub id: String,
    /// The element name, e.g. `tmp102`.
    pub kind: String,
    /// (attribute, component id), e.g. ("i2c_component", "temp_bus").
    pub refs: Vec<(String, String)>,
}

#[derive(Debug)]
pub struct Espf {
    pub project: Project,
    pub chip: Chip,
    pub runtime: Runtime,
    pub peripherals: Vec<Peripheral>,
    pub components: Vec<Component>,
    pub devices: Vec<Device>,
}

// ==========================================
// 5. Semantic stage: diagnostics and helpers
// ==========================================
struct Diagnostic {
    offset: Option<usize>,
    message: String,
}

/// What an `id` names. All ids share one namespace across the whole file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Gpio,
    I2c,
    Spi,
    Uart,
    GpioComponent,
    I2cComponent,
    SpiComponent,
    UartComponent,
    Device,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Gpio => "gpio",
            Kind::I2c => "i2c",
            Kind::Spi => "spi",
            Kind::Uart => "uart",
            Kind::GpioComponent => "gpio_component",
            Kind::I2cComponent => "i2c_component",
            Kind::SpiComponent => "spi_component",
            Kind::UartComponent => "uart_component",
            Kind::Device => "device",
        }
    }

    fn from_tag(section: &str, tag: &str) -> Option<Kind> {
        match (section, tag) {
            ("peripherals", "gpio") => Some(Kind::Gpio),
            ("peripherals", "i2c") => Some(Kind::I2c),
            ("peripherals", "spi") => Some(Kind::Spi),
            ("peripherals", "uart") => Some(Kind::Uart),
            ("components", "gpio_component") => Some(Kind::GpioComponent),
            ("components", "i2c_component") => Some(Kind::I2cComponent),
            ("components", "spi_component") => Some(Kind::SpiComponent),
            ("components", "uart_component") => Some(Kind::UartComponent),
            ("devices", _) => Some(Kind::Device),
            _ => None,
        }
    }
}

struct Ctx<'a> {
    src: &'a str,
    diags: Vec<Diagnostic>,
    ids: HashMap<String, (Kind, usize)>,
    pins: HashMap<u8, String>,
    chip: Option<Chip>,
}

impl Ctx<'_> {
    fn error_at(&mut self, tail: usize, message: impl Into<String>) {
        self.diags.push(Diagnostic {
            offset: Some(self.src.len() - tail),
            message: message.into(),
        });
    }

    fn error(&mut self, message: impl Into<String>) {
        self.diags.push(Diagnostic {
            offset: None,
            message: message.into(),
        });
    }

    fn line_of(&self, tail: usize) -> usize {
        self.src[..self.src.len() - tail].matches('\n').count() + 1
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for d in &self.diags {
            out.push_str(&format!("error: {}\n", d.message));
            if let Some(off) = d.offset {
                let before = &self.src[..off];
                let line_no = before.matches('\n').count() + 1;
                let line_start = before.rfind('\n').map_or(0, |i| i + 1);
                let col = off - line_start + 1;
                let line_text = self.src[line_start..].lines().next().unwrap_or("");
                let gutter = line_no.to_string();
                let pad = " ".repeat(gutter.len());
                out.push_str(&format!(
                    "{pad} --> line {line_no}, column {col}\n{pad} |\n{gutter} | {line_text}\n{pad} | {}^\n",
                    " ".repeat(col - 1)
                ));
            }
            out.push('\n');
        }
        out.push_str(&format!("{} error(s)\n", self.diags.len()));
        out
    }
}

fn valid_id(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn pick<T: Copy>(v: &str, table: &[(&str, T)]) -> Result<T, String> {
    table
        .iter()
        .find(|(name, _)| *name == v)
        .map(|(_, t)| *t)
        .ok_or_else(|| {
            let names: Vec<&str> = table.iter().map(|(n, _)| *n).collect();
            format!("expected one of: {}", names.join(", "))
        })
}

/// Strict unit grammar: whole number immediately followed by Hz, kHz or MHz.
fn parse_hz(v: &str) -> Result<u32, String> {
    let split = v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let n: u32 = num
        .parse()
        .map_err(|_| "expected a number followed by Hz, kHz or MHz (e.g. 400kHz)".to_string())?;
    let mult: u32 = match unit {
        "Hz" => 1,
        "kHz" => 1_000,
        "MHz" => 1_000_000,
        _ => return Err("unit must be exactly Hz, kHz or MHz (case-sensitive)".to_string()),
    };
    n.checked_mul(mult)
        .ok_or_else(|| "frequency is too large".to_string())
}

/// 7-bit I2C address, decimal or `0x` hex. 0x00-0x07 and 0x78-0x7F are reserved.
fn parse_addr(v: &str) -> Result<u8, String> {
    let parsed = match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(hex) => u8::from_str_radix(hex, 16),
        None => v.parse::<u8>(),
    };
    let addr = parsed.map_err(|_| "expected an address like 0x48".to_string())?;
    if (0x08..=0x77).contains(&addr) {
        Ok(addr)
    } else {
        Err("7-bit I2C addresses must be in 0x08..=0x77".to_string())
    }
}

fn known_attrs(cx: &mut Ctx, el: &RawElement, allowed: &[&str]) {
    for (i, (k, _)) in el.attrs.iter().enumerate() {
        if !allowed.contains(&k.as_str()) {
            cx.error_at(
                el.tail,
                format!(
                    "unknown attribute `{k}` on `<{}>`; allowed: {}",
                    el.tag,
                    allowed.join(", ")
                ),
            );
        } else if el.attrs[..i].iter().any(|(kk, _)| kk == k) {
            cx.error_at(
                el.tail,
                format!("duplicate attribute `{k}` on `<{}>`", el.tag),
            );
        }
    }
}

fn req<'a>(cx: &mut Ctx, el: &'a RawElement, key: &str) -> Option<&'a str> {
    let v = el.get(key);
    if v.is_none() {
        cx.error_at(
            el.tail,
            format!("`<{}>` is missing required attribute `{key}`", el.tag),
        );
    }
    v
}

fn req_with<T>(
    cx: &mut Ctx,
    el: &RawElement,
    key: &str,
    f: impl FnOnce(&str) -> Result<T, String>,
) -> Option<T> {
    let raw = req(cx, el, key)?;
    match f(raw) {
        Ok(v) => Some(v),
        Err(msg) => {
            cx.error_at(el.tail, format!("`{key}=\"{raw}\"`: {msg}"));
            None
        }
    }
}

fn req_num<T: std::str::FromStr>(cx: &mut Ctx, el: &RawElement, key: &str) -> Option<T> {
    req_with(cx, el, key, |v| {
        v.parse::<T>()
            .map_err(|_| "expected a whole number".to_string())
    })
}

fn req_id<'a>(cx: &mut Ctx, el: &'a RawElement) -> Option<&'a str> {
    let id = req(cx, el, "id")?;
    if !valid_id(id) {
        cx.error_at(
            el.tail,
            format!("invalid id `{id}`: ids are snake_case (a letter first, then a-z, 0-9 or _)"),
        );
        return None;
    }
    Some(id)
}

fn req_pin(cx: &mut Ctx, el: &RawElement, key: &str) -> Option<u8> {
    let chip = cx.chip;
    req_with(cx, el, key, |v| {
        let pin: u8 = v.parse().map_err(|_| "expected a pin number".to_string())?;
        match chip {
            Some(c) if pin > c.max_gpio() => Err(format!(
                "pin {pin} does not exist on {} (valid pins: 0..={})",
                c.name(),
                c.max_gpio()
            )),
            _ => Ok(pin),
        }
    })
}

/// A reference to another element's id, written `$id`.
/// The `$` is required, so a reference can never be mistaken for a plain value.
/// Checked for existence and kind. Returns the id without the `$`.
fn req_ref<'a>(cx: &mut Ctx, el: &'a RawElement, key: &str, want: Kind) -> Option<&'a str> {
    let raw = req(cx, el, key)?;
    let Some(target) = raw.strip_prefix('$') else {
        cx.error_at(
            el.tail,
            format!("`{key}=\"{raw}\"` must be a reference: write `{key}=\"${raw}\"`"),
        );
        return None;
    };
    match cx.ids.get(target).copied() {
        None => {
            cx.error_at(
                el.tail,
                format!("`{key}=\"{raw}\"` refers to an id that does not exist"),
            );
            None
        }
        Some((found, _)) if found != want => {
            cx.error_at(
                el.tail,
                format!(
                    "`{key}=\"{raw}\"` is a {}, but `{key}` needs a {}",
                    found.label(),
                    want.label()
                ),
            );
            None
        }
        Some(_) => Some(target),
    }
}

fn claim_pin(cx: &mut Ctx, el: &RawElement, pin: u8, owner: &str) {
    if let Some(prev) = cx.pins.get(&pin).cloned() {
        cx.error_at(el.tail, format!("pin {pin} is already used by `{prev}`"));
    } else {
        cx.pins.insert(pin, owner.to_string());
    }
}

// ==========================================
// 6. Semantic stage: one function per section
// ==========================================
fn register_ids(cx: &mut Ctx, sec: &RawSection) {
    for el in &sec.elements {
        let Some(kind) = Kind::from_tag(&sec.name, &el.tag) else {
            continue;
        };
        let Some(id) = el.get("id") else { continue };
        if !valid_id(id) {
            continue; // reported when the element is lowered
        }
        match cx.ids.get(id).copied() {
            Some((prev_kind, prev_tail)) => {
                let line = cx.line_of(prev_tail);
                cx.error_at(
                    el.tail,
                    format!(
                        "id `{id}` is already used by a {} on line {line}",
                        prev_kind.label()
                    ),
                );
            }
            None => {
                cx.ids.insert(id.to_string(), (kind, el.tail));
            }
        }
    }
}

fn lower_espforge(
    cx: &mut Ctx,
    sec: &RawSection,
) -> (Option<Project>, Option<Chip>, Option<Runtime>) {
    let (mut project, mut chip, mut runtime) = (None, None, None);
    let mut seen: HashSet<&str> = HashSet::new();
    for el in &sec.elements {
        if !seen.insert(el.tag.as_str()) {
            cx.error_at(
                el.tail,
                format!("`<{}>` may only appear once in `# espforge`", el.tag),
            );
            continue;
        }
        match el.tag.as_str() {
            "project" => {
                known_attrs(cx, el, &["name", "description"]);
                project = req(cx, el, "name").map(|n| Project {
                    name: n.to_string(),
                    description: el.get("description").map(str::to_string),
                });
            }
            "chip" => {
                known_attrs(cx, el, &["type"]);
                chip = req_with(cx, el, "type", |v| {
                    pick(v, &[("esp32c3", Chip::Esp32c3), ("esp32c6", Chip::Esp32c6)])
                });
            }
            "runtime" => {
                known_attrs(cx, el, &["type"]);
                runtime = req_with(cx, el, "type", |v| {
                    pick(
                        v,
                        &[
                            ("blocking", Runtime::Blocking),
                            ("embassy", Runtime::Embassy),
                        ],
                    )
                });
            }
            other => cx.error_at(
                el.tail,
                format!(
                    "unknown element `<{other}>` in `# espforge`; expected: project, chip, runtime"
                ),
            ),
        }
    }
    for need in ["project", "chip", "runtime"] {
        if !seen.contains(need) {
            cx.error_at(sec.tail, format!("`# espforge` is missing `<{need}>`"));
        }
    }
    (project, chip, runtime)
}

fn lower_peripherals(cx: &mut Ctx, sec: &RawSection) -> Vec<Peripheral> {
    let mut out = Vec::new();
    for el in &sec.elements {
        match el.tag.as_str() {
            "gpio" => {
                known_attrs(cx, el, &["id", "pin"]);
                let id = req_id(cx, el);
                let pin = req_pin(cx, el, "pin");
                if let (Some(id), Some(pin)) = (id, pin) {
                    claim_pin(cx, el, pin, id);
                    out.push(Peripheral::Gpio { id: id.into(), pin });
                }
            }
            "i2c" => {
                known_attrs(cx, el, &["id", "sda", "scl", "frequency"]);
                let id = req_id(cx, el);
                let sda = req_pin(cx, el, "sda");
                let scl = req_pin(cx, el, "scl");
                let frequency_hz = req_with(cx, el, "frequency", parse_hz);
                if let (Some(id), Some(sda), Some(scl), Some(frequency_hz)) =
                    (id, sda, scl, frequency_hz)
                {
                    claim_pin(cx, el, sda, id);
                    claim_pin(cx, el, scl, id);
                    out.push(Peripheral::I2c {
                        id: id.into(),
                        sda,
                        scl,
                        frequency_hz,
                    });
                }
            }
            "spi" => {
                known_attrs(cx, el, &["id", "sclk", "mosi", "miso"]);
                let id = req_id(cx, el);
                let sclk = req_pin(cx, el, "sclk");
                let mosi = req_pin(cx, el, "mosi");
                // miso is optional (displays usually omit it); a bad value is still an error.
                let miso = if el.get("miso").is_some() {
                    req_pin(cx, el, "miso").map(Some)
                } else {
                    Some(None)
                };
                if let (Some(id), Some(sclk), Some(mosi), Some(miso)) = (id, sclk, mosi, miso) {
                    for pin in [Some(sclk), Some(mosi), miso].into_iter().flatten() {
                        claim_pin(cx, el, pin, id);
                    }
                    out.push(Peripheral::Spi {
                        id: id.into(),
                        sclk,
                        mosi,
                        miso,
                    });
                }
            }
            "uart" => {
                known_attrs(cx, el, &["id", "tx", "rx"]);
                let id = req_id(cx, el);
                let tx = req_pin(cx, el, "tx");
                let rx = req_pin(cx, el, "rx");
                if let (Some(id), Some(tx), Some(rx)) = (id, tx, rx) {
                    claim_pin(cx, el, tx, id);
                    claim_pin(cx, el, rx, id);
                    out.push(Peripheral::Uart {
                        id: id.into(),
                        tx,
                        rx,
                    });
                }
            }
            other => cx.error_at(
                el.tail,
                format!(
                    "unknown element `<{other}>` in `# peripherals`; expected: gpio, i2c, spi, uart"
                ),
            ),
        }
    }
    out
}

fn lower_components(cx: &mut Ctx, sec: &RawSection) -> Vec<Component> {
    let mut out = Vec::new();
    for el in &sec.elements {
        match el.tag.as_str() {
            "gpio_component" => {
                known_attrs(cx, el, &["id", "gpio", "direction"]);
                let id = req_id(cx, el);
                let gpio = req_ref(cx, el, "gpio", Kind::Gpio);
                let direction = req_with(cx, el, "direction", |v| {
                    pick(v, &[("input", Direction::Input), ("output", Direction::Output)])
                });
                if let (Some(id), Some(gpio), Some(direction)) = (id, gpio, direction) {
                    out.push(Component::Gpio { id: id.into(), gpio: gpio.into(), direction });
                }
            }
            "i2c_component" => {
                known_attrs(cx, el, &["id", "i2c", "address"]);
                let id = req_id(cx, el);
                let i2c = req_ref(cx, el, "i2c", Kind::I2c);
                let address = req_with(cx, el, "address", parse_addr);
                if let (Some(id), Some(i2c), Some(address)) = (id, i2c, address) {
                    out.push(Component::I2c { id: id.into(), i2c: i2c.into(), address });
                }
            }
            "spi_component" => {
                known_attrs(cx, el, &["id", "spi", "mode", "cs", "frequency"]);
                let id = req_id(cx, el);
                let spi = req_ref(cx, el, "spi", Kind::Spi);
                let mode = req_with(cx, el, "mode", |v| {
                    pick(v, &[("0", 0u8), ("1", 1), ("2", 2), ("3", 3)])
                });
                let cs = req_ref(cx, el, "cs", Kind::Gpio);
                let frequency_hz = req_with(cx, el, "frequency", parse_hz);
                if let (Some(id), Some(spi), Some(mode), Some(cs), Some(frequency_hz)) =
                    (id, spi, mode, cs, frequency_hz)
                {
                    out.push(Component::Spi {
                        id: id.into(),
                        spi: spi.into(),
                        mode,
                        cs: cs.into(),
                        frequency_hz,
                    });
                }
            }
            "uart_component" => {
                known_attrs(cx, el, &["id", "uart", "baud_rate", "parity", "stop_bits"]);
                let id = req_id(cx, el);
                let uart = req_ref(cx, el, "uart", Kind::Uart);
                let baud_rate = req_num::<u32>(cx, el, "baud_rate");
                let parity = req_with(cx, el, "parity", |v| {
                    pick(v, &[("none", Parity::None), ("even", Parity::Even), ("odd", Parity::Odd)])
                });
                let stop_bits = req_with(cx, el, "stop_bits", |v| pick(v, &[("1", 1u8), ("2", 2)]));
                if let (Some(id), Some(uart), Some(baud_rate), Some(parity), Some(stop_bits)) =
                    (id, uart, baud_rate, parity, stop_bits)
                {
                    out.push(Component::Uart {
                        id: id.into(),
                        uart: uart.into(),
                        baud_rate,
                        parity,
                        stop_bits,
                    });
                }
            }
            other => cx.error_at(
                el.tail,
                format!(
                    "unknown element `<{other}>` in `# components`; expected: gpio_component, i2c_component, spi_component, uart_component"
                ),
            ),
        }
    }
    out
}

/// Device element names are open-ended (tmp102, ili9341, ...). Their only
/// attributes are `id` and `*_component` references, so dangling references
/// and wrong-kind references are caught without knowing the device type.
fn lower_devices(cx: &mut Ctx, sec: &RawSection) -> Vec<Device> {
    let mut out = Vec::new();
    for el in &sec.elements {
        let id = req_id(cx, el);
        let mut ok = id.is_some();
        let mut refs = Vec::new();
        for (k, _) in &el.attrs {
            if k == "id" {
                continue;
            }
            let want = match k.as_str() {
                "gpio_component" => Kind::GpioComponent,
                "i2c_component" => Kind::I2cComponent,
                "spi_component" => Kind::SpiComponent,
                "uart_component" => Kind::UartComponent,
                _ => {
                    cx.error_at(
                        el.tail,
                        format!(
                            "unknown attribute `{k}` on `<{}>`; devices take `id` and `*_component` references",
                            el.tag
                        ),
                    );
                    ok = false;
                    continue;
                }
            };
            match req_ref(cx, el, k, want) {
                Some(target) => refs.push((k.clone(), target.to_string())),
                None => ok = false,
            }
        }
        if let (true, Some(id)) = (ok, id) {
            out.push(Device {
                id: id.into(),
                kind: el.tag.clone(),
                refs,
            });
        }
    }
    out
}

// ==========================================
// 7. Public entry point
// ==========================================
const SECTIONS: [&str; 4] = ["espforge", "peripherals", "components", "devices"];

pub fn parse_espf(src: &str) -> Result<Espf, String> {
    let raw = RawFile::decode(src)?;

    let mut cx = Ctx {
        src,
        diags: Vec::new(),
        ids: HashMap::new(),
        pins: HashMap::new(),
        chip: None,
    };

    // Each mandatory section exactly once; order is free.
    let mut by_name: HashMap<&str, &RawSection> = HashMap::new();
    for s in &raw.sections {
        if !SECTIONS.contains(&s.name.as_str()) {
            cx.error_at(
                s.tail,
                format!(
                    "unknown section `{}`; expected: {}",
                    s.name,
                    SECTIONS.join(", ")
                ),
            );
        } else if by_name.insert(s.name.as_str(), s).is_some() {
            cx.error_at(
                s.tail,
                format!("section `{}` appears more than once", s.name),
            );
        }
    }
    for name in SECTIONS {
        if !by_name.contains_key(name) {
            cx.error(format!("missing mandatory section `# {name}`"));
        }
    }

    let (project, chip, runtime) = by_name
        .get("espforge")
        .map(|s| lower_espforge(&mut cx, s))
        .unwrap_or((None, None, None));
    cx.chip = chip;

    // Pass 1: collect ids so references resolve regardless of section order.
    for name in ["peripherals", "components", "devices"] {
        if let Some(s) = by_name.get(name) {
            register_ids(&mut cx, s);
        }
    }

    // Pass 2: typed lowering with validation.
    let peripherals = by_name
        .get("peripherals")
        .map(|s| lower_peripherals(&mut cx, s))
        .unwrap_or_default();
    let components = by_name
        .get("components")
        .map(|s| lower_components(&mut cx, s))
        .unwrap_or_default();
    let devices = by_name
        .get("devices")
        .map(|s| lower_devices(&mut cx, s))
        .unwrap_or_default();

    if !cx.diags.is_empty() {
        return Err(format!("Invalid ESPF:\n{}", cx.render()));
    }
    match (project, chip, runtime) {
        (Some(project), Some(chip), Some(runtime)) => Ok(Espf {
            project,
            chip,
            runtime,
            peripherals,
            components,
            devices,
        }),
        _ => Err("internal error: espforge section incomplete without a diagnostic".into()),
    }
}

// ==========================================
// 8. Demo (same A-D scenario layout as the prototype)
// ==========================================
const VALID: &str = r##"--- ====================
--- ESPF file format
--- mandatory sections: espforge, peripherals, components, devices
--- ====================

# espforge
  <project name="example project" description="example showing espf file format" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals
  <gpio id="gpio4" pin="4" />
  <gpio id="gpio5" pin="5" />
  <i2c  id="i2c0" sda="8" scl="9" frequency="400kHz" />
  --- displays generally omit miso
  <spi  id="spi2" sclk="6" mosi="7" />
  <uart id="uart1" tx="20" rx="21" />

# components
  <gpio_component id="blue_led"    gpio="$gpio4" direction="output" />
  <i2c_component  id="temp_bus"    i2c="$i2c0" address="0x48" />
  <spi_component  id="display_bus" spi="$spi2" mode="0" cs="$gpio5" frequency="40MHz" />
  <uart_component id="serial_bus"  uart="$uart1" baud_rate="9600" parity="none" stop_bits="1" />

# devices
  <status_led id="status"       gpio_component="$blue_led" />
  <tmp102     id="outdoor_temp" i2c_component="$temp_bus" />
  <ili9341    id="tft_display"  spi_component="$display_bus" />
"##;

fn print_summary(cfg: &Espf) {
    println!("Successfully parsed!");
    println!("  Project:     {}", cfg.project.name);
    println!("  Chip:        {}", cfg.chip.name());
    println!("  Runtime:     {:?}", cfg.runtime);
    println!("  Peripherals: {}", cfg.peripherals.len());
    println!("  Components:  {}", cfg.components.len());
    for d in &cfg.devices {
        println!("  Device:      {} `{}` -> {:?}", d.kind, d.id, d.refs);
    }
}

fn run(title: &str, src: &str) {
    println!("\n--- {title} ---");
    match parse_espf(src) {
        Ok(cfg) => print_summary(&cfg),
        Err(msg) => println!("{msg}"),
    }
}

const USAGE: &str = "usage: espf_parser <file.espf>\n       espf_parser --demo";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--demo" => demo(),
        [path] => {
            let src = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read `{path}`: {e}");
                    std::process::exit(2);
                }
            };
            match parse_espf(&src) {
                Ok(cfg) => print_summary(&cfg),
                Err(msg) => {
                    eprintln!("{path}: {msg}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

fn demo() {
    // Scenario A: happy path
    run("A: valid file", VALID);

    // Scenario B: syntax error (missing closing quote)
    run(
        "B: missing closing quote",
        &VALID.replace("<chip type=\"esp32c3\" />", "<chip type=\"esp32c3 />"),
    );

    // Scenario C: semantic errors, all reported together
    //   - scl on a pin that does not exist on the C3
    //   - empty device id
    //   - reference to a component that was never defined
    let bad = VALID
        .replace("scl=\"9\"", "scl=\"22\"")
        .replace("<tmp102     id=\"outdoor_temp\"", "<tmp102     id=\"\"")
        .replace(
            "spi_component=\"$display_bus\"",
            "spi_component=\"$display\"",
        );
    run("C: semantic errors", &bad);

    // Scenario D: a second-level heading (no longer part of ESPF)
    run(
        "D: second-level heading",
        &VALID.replace("# peripherals\n", "# peripherals\n  ## gpio\n"),
    );
}
