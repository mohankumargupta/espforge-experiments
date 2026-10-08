//! espf parser for the new format (see examples/example.espf.xml).
//!
//! The crate is split by responsibility:
//!
//!   - [`structs`]: the data model — flat structs, no hidden state.
//!   - [`parse`]:   the line-oriented parser producing that model.
//!   - [`print`]:   a `Display` impl for the top-level file struct.
//!
//! References like "i2c0" or "status_led_io" are stored as plain String
//! values, so printing stays trivial: println!("{}", value);

mod parse;
mod print;
mod structs;

// Public API is unchanged from the single-file layout:
// testing_winnow::{parse, ParseError, EspfFile, ...}
pub use parse::{parse, ParseError};
pub use structs::{
    Component, Device, EspfFile, Espforge, Gpio, GpioComponent, I2c, I2cComponent, Peripherals,
    RefStr, Spi, SpiComponent, Uart, UartComponent,
};
