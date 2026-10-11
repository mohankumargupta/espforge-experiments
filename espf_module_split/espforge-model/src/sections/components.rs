//! The built-in components: each wraps one peripheral.
//!
//! Attribute lists and allowed values are inferred from the example file;
//! edit these tables to match the real schema.

use linkme::distributed_slice;

use crate::registry::{AttrField, COMPONENTS, ElementSchema, Kind, RefField};

#[distributed_slice(COMPONENTS)]
static GPIO_COMPONENT: ElementSchema = ElementSchema {
    tag: "gpio_component",
    refs: &[RefField { key: "gpio", target: "gpio", required: true }],
    attrs: &[AttrField {
        key: "direction",
        kind: Kind::OneOf(&["input", "output"]),
        required: true,
    }],
};

#[distributed_slice(COMPONENTS)]
static I2C_COMPONENT: ElementSchema = ElementSchema {
    tag: "i2c_component",
    refs: &[RefField { key: "i2c", target: "i2c", required: true }],
    attrs: &[AttrField { key: "address", kind: Kind::Int, required: true }],
};

#[distributed_slice(COMPONENTS)]
static SPI_COMPONENT: ElementSchema = ElementSchema {
    tag: "spi_component",
    refs: &[
        RefField { key: "spi", target: "spi", required: true },
        RefField { key: "cs", target: "gpio", required: false },
    ],
    attrs: &[
        AttrField {
            key: "mode",
            kind: Kind::OneOf(&["0", "1", "2", "3"]),
            required: false,
        },
        AttrField { key: "frequency", kind: Kind::Text, required: false },
    ],
};

#[distributed_slice(COMPONENTS)]
static UART_COMPONENT: ElementSchema = ElementSchema {
    tag: "uart_component",
    refs: &[RefField { key: "uart", target: "uart", required: true }],
    attrs: &[
        AttrField { key: "baud_rate", kind: Kind::Int, required: true },
        AttrField {
            key: "parity",
            kind: Kind::OneOf(&["none", "even", "odd"]),
            required: false,
        },
        AttrField {
            key: "stop_bits",
            kind: Kind::OneOf(&["1", "2"]),
            required: false,
        },
    ],
};
