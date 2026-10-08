//! Print helper — references are plain strings, so Display is direct.

use crate::structs::{Component, EspfFile, RefStr};
use std::fmt;

impl fmt::Display for EspfFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ef) = &self.espforge {
            writeln!(f, "espforge: {}", ef.name)?;
            if ef.description.is_empty() {
                writeln!(f, "         : {ef:?}")?;
            } else {
                writeln!(f, "desc     : {}", ef.description)?;
            }
            writeln!(f, "chip     : {}", ef.chip)?;
            writeln!(f, "runtime  : {}", ef.runtime)?;
        }
        let p = &self.peripherals;
        writeln!(f, "peripherals gpio: {} i2c: {} spi: {} uart: {}",
            p.gpio.len(), p.i2c.len(), p.spi.len(), p.uart.len())?;
        for g in &p.gpio {
            writeln!(f, "  gpio {} pin {}", RefStr(g.id.clone()), g.pin)?;
        }
        for c in &p.i2c {
            writeln!(f, "  i2c {} bus {} sda {} scl {} {}",
                RefStr(c.id.clone()), c.bus, c.sda, c.scl, RefStr(c.frequency.clone()))?;
        }
        for c in &p.spi {
            writeln!(f, "  spi {} bus {} sclk {} mosi {:?} miso {:?}",
                RefStr(c.id.clone()), c.bus, c.sclk, c.mosi, c.miso)?;
        }
        for c in &p.uart {
            writeln!(f, "  uart {} tx {} rx {}", RefStr(c.id.clone()), c.tx, c.rx)?;
        }
        writeln!(f, "components: {}", self.components.len())?;
        for c in &self.components {
            match c {
                Component::Gpio(g) => writeln!(f, "  gpio_component {:?} gpio {} dir {}",
                    g.id, RefStr(g.gpio.clone()), g.direction)?,
                Component::I2c(c2) => writeln!(f, "  i2c_component {:?} i2c {} address {}",
                    c2.id, RefStr(c2.i2c.clone()), c2.address)?,
                Component::Spi(s) => writeln!(f, "  spi_component {:?} spi {} mode {} cs {} {}",
                    s.id, RefStr(s.spi.clone()), s.mode,
                    RefStr(s.cs.clone()), RefStr(s.frequency.clone()))?,
                Component::Uart(u) => writeln!(f,
                    "  uart_component {:?} uart {} baud {} parity {} stop {}",
                    u.id, RefStr(u.uart.clone()), u.baud_rate,
                    u.parity, u.stop_bits)?,
                Component::Other { name, attrs } => {
                    writeln!(f, "  other {name} attrs {attrs:?}")?;
                }
            }
        }
        writeln!(f, "devices: {}", self.devices.len())?;
        for d in &self.devices {
            writeln!(f, "  {} id=\"{}\" attrs {:?}", d.kind, d.id, d.attrs)?;
        }
        Ok(())
    }
}
