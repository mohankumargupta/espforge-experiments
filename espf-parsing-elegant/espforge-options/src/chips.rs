//! Chip facts used by validation.

/// Every chip `esp-metadata-generated` knows, by name.
pub fn supported_chips() -> Vec<&'static str> {
    esp_metadata_generated::Chip::iter()
        .map(|c| c.name())
        .collect()
}

pub fn is_supported_chip(name: &str) -> bool {
    esp_metadata_generated::Chip::iter().any(|c| c.name() == name)
}

/// Whether the chip has the built-in USB-Serial-JTAG peripheral.
///
/// NOTE: this is a hand-kept list (it replaces the old `chip == "esp32"`
/// special case). It should eventually be read from the esp-metadata
/// peripheral data instead; this function is the only place to change.
pub fn has_usb_serial_jtag(chip: &str) -> bool {
    matches!(
        chip,
        "esp32c3" | "esp32c5" | "esp32c6" | "esp32h2" | "esp32s3" | "esp32p4"
    )
}
