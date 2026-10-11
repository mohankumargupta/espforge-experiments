use std::ops::Range;

/// What a chip variant provides. Peripherals are checked against this.
///
/// Check these numbers against the chip's datasheet and technical reference
/// manual before relying on them.
#[derive(Debug)]
pub struct ChipSpec {
    pub name: &'static str,
    /// Usable GPIO numbers, as ranges (some chips have gaps).
    pub gpio: &'static [Range<u8>],
    /// General-purpose I2C controllers.
    pub i2c_buses: u8,
    /// General-purpose SPI controllers (not the flash/PSRAM ones).
    pub spi_buses: u8,
    pub uart_ports: u8,
}

impl ChipSpec {
    pub fn has_gpio(&self, pin: u8) -> bool {
        self.gpio.iter().any(|range| range.contains(&pin))
    }
}

pub static CHIPS: &[ChipSpec] = &[ChipSpec {
    name: "esp32c3",
    gpio: &[0..22], // GPIO0..=GPIO21
    i2c_buses: 1,
    spi_buses: 1, // SPI2 only; SPI0/1 are for flash
    uart_ports: 2, // UART0 and UART1; verify against the TRM
}];

pub fn lookup(name: &str) -> Option<&'static ChipSpec> {
    CHIPS.iter().find(|chip| chip.name == name)
}

pub fn names() -> Vec<&'static str> {
    CHIPS.iter().map(|chip| chip.name).collect()
}
