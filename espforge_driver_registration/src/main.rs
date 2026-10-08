// src/main.rs
mod registry;
mod drivers {
    pub mod gpio; // Just declare the modules so they are included in the compilation unit
    pub mod wifi;
}

use registry::DriverRegistry;

fn main() {
    // 1. Initialise our lightweight global singleton
    let registry = DriverRegistry::global();

    // 2. Simulate looking up a leaf node from a parsed YAML file (e.g., "esphome.gpio")
    let target_fqdn = "esphome.gpio";

    println!("Looking up driver for: '{}'", target_fqdn);

    if let Some(driver) = registry.get_driver(target_fqdn) {
        // 3. Dynamically dispatch the method call onto the trait object
        let attributes = driver.get_attributes();

        println!("Driver Found! Attributes successfully fetched:");
        for (key, val) in attributes {
            println!("  - {}: {}", key, val);
        }
    } else {
        println!("Error: No driver registered under FQDN '{}'", target_fqdn);
    }
}
