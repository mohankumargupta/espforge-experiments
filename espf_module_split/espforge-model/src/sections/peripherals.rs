use espf::{Element, Section};

use crate::{
    chips::ChipSpec,
    cx::{quoted_list, Cx},
};

/// The `# peripherals` section. Fixed schema, checked against the chip.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Peripherals {
    pub gpios: Vec<Gpio>,
    pub i2cs: Vec<I2c>,
    pub spis: Vec<Spi>,
    pub uarts: Vec<Uart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gpio {
    pub id: Option<String>,
    pub pin: u8,
}

/// `bus` is the declaration order among `<i2c>` elements, not a hardware id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2c {
    pub id: Option<String>,
    pub bus: u8,
    pub sda: u8,
    pub scl: u8,
    pub frequency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spi {
    pub id: Option<String>,
    pub bus: u8,
    pub sclk: u8,
    pub mosi: u8,
    pub miso: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uart {
    pub id: Option<String>,
    pub port: u8,
    pub tx: u8,
    pub rx: u8,
}

const TAGS: &[&str] = &["gpio", "i2c", "spi", "uart"];

/// Check and collect the peripherals. Errors go to `cx`; the returned value is
/// only meaningful when `cx` ends up with none.
///
/// An element whose pins or bus are invalid is not declared in the symbol
/// table, so a later reference to its id will also be reported.
pub fn extract(cx: &mut Cx, section: &Section, chip: &ChipSpec) -> Peripherals {
    let mut out = Peripherals::default();

    for element in &section.elements {
        let id = element.id.as_ref().map(|attr| attr.value.clone());

        match element.tag.as_str() {
            "gpio" => {
                cx.check_attrs(element, &["pin"]);

                let Some(pin) = read_pin(cx, chip, element, "pin", true) else {
                    continue;
                };

                cx.declare(element, format!("peripherals.gpio.pin{pin}"));
                out.gpios.push(Gpio { id, pin });
            }

            "i2c" => {
                cx.check_attrs(element, &["sda", "scl", "frequency"]);

                let sda = read_pin(cx, chip, element, "sda", true);
                let scl = read_pin(cx, chip, element, "scl", true);
                let bus = claim_unit(cx, chip, element, out.i2cs.len(), chip.i2c_buses);

                let (Some(sda), Some(scl), Some(bus)) = (sda, scl, bus) else {
                    continue;
                };

                cx.declare(element, format!("peripherals.i2c.bus{bus}"));
                out.i2cs.push(I2c {
                    id,
                    bus,
                    sda,
                    scl,
                    frequency: element.value("frequency").map(str::to_owned),
                });
            }

            "spi" => {
                cx.check_attrs(element, &["sclk", "mosi", "miso"]);

                let sclk = read_pin(cx, chip, element, "sclk", true);
                let mosi = read_pin(cx, chip, element, "mosi", true);
                let miso = read_pin(cx, chip, element, "miso", false);
                let bus = claim_unit(cx, chip, element, out.spis.len(), chip.spi_buses);

                let (Some(sclk), Some(mosi), Some(bus)) = (sclk, mosi, bus) else {
                    continue;
                };

                cx.declare(element, format!("peripherals.spi.bus{bus}"));
                out.spis.push(Spi { id, bus, sclk, mosi, miso });
            }

            "uart" => {
                cx.check_attrs(element, &["tx", "rx"]);

                let tx = read_pin(cx, chip, element, "tx", true);
                let rx = read_pin(cx, chip, element, "rx", true);
                let port = claim_unit(cx, chip, element, out.uarts.len(), chip.uart_ports);

                let (Some(tx), Some(rx), Some(port)) = (tx, rx, port) else {
                    continue;
                };

                cx.declare(element, format!("peripherals.uart.port{port}"));
                out.uarts.push(Uart { id, port, tx, rx });
            }

            other => cx.error(
                element.span.clone(),
                format!("Unknown peripheral <{other}>; expected {}", quoted_list(TAGS)),
            ),
        }
    }

    out
}

/// Read a pin attribute and check that the chip has that GPIO.
///
/// A missing optional attribute gives `None` silently. A present but invalid
/// value reports an error and also gives `None`.
fn read_pin(
    cx: &mut Cx,
    chip: &ChipSpec,
    element: &Element,
    key: &str,
    required: bool,
) -> Option<u8> {
    let attr = if required {
        cx.require(element, key)?
    } else {
        element.attr(key)?
    };

    let Ok(pin) = attr.value.parse::<u8>() else {
        cx.error(
            attr.value_span.clone(),
            format!("`{}` is not a pin number", attr.value),
        );
        return None;
    };

    if !chip.has_gpio(pin) {
        cx.error(
            attr.value_span.clone(),
            format!("GPIO{pin} does not exist on {}", chip.name),
        );
        return None;
    }

    Some(pin)
}

/// Hand out the next bus or port number, or report that the chip has no more.
fn claim_unit(
    cx: &mut Cx,
    chip: &ChipSpec,
    element: &Element,
    taken: usize,
    available: u8,
) -> Option<u8> {
    if taken < usize::from(available) {
        return u8::try_from(taken).ok();
    }

    cx.error(
        element.span.clone(),
        format!(
            "{} supports at most {available} <{}> element(s)",
            chip.name, element.tag
        ),
    );
    None
}
