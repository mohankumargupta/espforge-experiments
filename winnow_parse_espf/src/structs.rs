//! Data shape: flat structs, no hidden state.
//!
//! espf file sections (see examples/example.espf.xml):
//!
//!   # espforge     <project /> <chip /> <runtime />
//!   # peripherals  ## gpio | i2c | spi | uart
//!   # components   gpio_component | i2c_component | spi_component | uart_component
//!   # devices      free-form device tags (tmp102, ili9341, ...)
//!
//! References like "i2c0" or "status_led_io" are stored as plain String
//! values, so printing stays trivial: println!("{}", value);

use std::fmt;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct EspfFile {
    pub espforge: Option<Espforge>,
    pub peripherals: Peripherals,
    pub components: Vec<Component>,
    pub devices: Vec<Device>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Espforge {
    /// <project name="..." description="..." />
    pub name: String,
    pub description: String,
    /// <chip type="esp32c3" />
    pub chip: String,
    /// <runtime type="blocking" />  (blocking, embassy, ...)
    pub runtime: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Peripherals {
    pub gpio: Vec<Gpio>,
    pub i2c: Vec<I2c>,
    pub spi: Vec<Spi>,
    pub uart: Vec<Uart>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Gpio {
    pub id: String,
    pub pin: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct I2c {
    pub id: String,
    pub bus: u32,
    pub sda: u32,
    pub scl: u32,
    pub frequency: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Spi {
    pub id: String,
    pub bus: u32,
    pub sclk: u32,
    pub mosi: Option<u32>,
    pub miso: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Uart {
    pub id: String,
    pub tx: u32,
    pub rx: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Component {
    Gpio(GpioComponent),
    I2c(I2cComponent),
    Spi(SpiComponent),
    Uart(UartComponent),
    /// Any tag whose type we don't model yet. id is inside attrs.
    Other {
        name: String,
        attrs: Vec<(String, String)>,
    },
}

// Component ids may be omitted in the file (e.g. <i2c_component i2c="i2c0" />),
// so every component id is an Option<String>.

#[derive(Debug, PartialEq, Eq)]
pub struct GpioComponent {
    pub id: Option<String>,
    /// reference to a peripherals gpio id, e.g. "gpio4"
    pub gpio: String,
    pub direction: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct I2cComponent {
    pub id: Option<String>,
    /// reference to a peripherals i2c id, e.g. "i2c0"
    pub i2c: String,
    /// e.g. "0x48"
    pub address: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SpiComponent {
    pub id: Option<String>,
    /// reference to a peripherals spi id, e.g. "spi2"
    pub spi: String,
    pub mode: u8,
    /// reference to a peripherals gpio id, e.g. "gpio5"
    pub cs: String,
    /// e.g. "40Mhz"
    pub frequency: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct UartComponent {
    pub id: Option<String>,
    /// reference to a peripherals uart id, e.g. "uart1"
    pub uart: String,
    pub baud_rate: u32,
    /// e.g. "none"
    pub parity: String,
    pub stop_bits: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Device {
    /// <tmp102 id="..." i2c_component="..." /> — id may be empty.
    pub id: String,
    /// the tag name itself e.g. "tmp102", "ili9341", "status_led"
    pub kind: String,
    /// all attributes kept raw (references stay as plain strings)
    pub attrs: Vec<(String, String)>,
}

// ------------------------------------------------------------------
// Reference handling: stored as raw String, printed as-is.
// ------------------------------------------------------------------

/// Plain string wrapper for cross-references (peripheral ids, component ids).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefStr(pub String);

impl fmt::Display for RefStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_printable() {
        let r = RefStr("gpio4".into());
        assert_eq!(format!("{}", r), "gpio4");
    }
}
