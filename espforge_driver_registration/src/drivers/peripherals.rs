// src/drivers/peripherals.rs
use crate::registry::{DISTRIBUTED_DRIVERS, Driver, DriverRegistration};
use std::collections::HashMap;

pub struct Peripherals;

impl Driver for Peripherals {
    fn get_attributes(&self) -> HashMap<String, String> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), "network".to_string());
        attrs.insert("protocol".to_string(), "802.11b/g/n".to_string());
        attrs
    }
}

#[linkme::distributed_slice(DISTRIBUTED_DRIVERS)]
static REG_WIFI: DriverRegistration = DriverRegistration {
    fqdn: "esphome.wifi",
    create_fn: || Box::new(WifiDriver),
};
