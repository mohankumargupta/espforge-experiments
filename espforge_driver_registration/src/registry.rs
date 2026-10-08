// src/registry.rs
use linkme::distributed_slice;
use std::collections::HashMap;
use std::sync::OnceLock;

// 1. The trait defining what a driver can do
pub trait Driver: Send + Sync {
    fn get_attributes(&self) -> HashMap<String, String>;
}

// 2. The structural metadata stored in the binary's custom linker section
pub struct DriverRegistration {
    pub fqdn: &'static str,
    pub create_fn: fn() -> Box<dyn Driver>,
}

// 3. Define the static distributed slice. The linker automatically fills this.
#[distributed_slice]
pub static DISTRIBUTED_DRIVERS: [DriverRegistration];

// 4. The actual Runtime Singleton (HashMap lookup)
pub struct DriverRegistry {
    drivers: HashMap<&'static str, Box<dyn Driver>>,
}

static REGISTRY_SINGLETON: OnceLock<DriverRegistry> = OnceLock::new();

impl DriverRegistry {
    /// Initialises the lookup map exactly once using the data collected by the linker
    pub fn global() -> &'static Self {
        REGISTRY_SINGLETON.get_or_init(|| {
            let mut drivers = HashMap::new();
            for registration in DISTRIBUTED_DRIVERS {
                // Call the factory function to instantiate the driver trait object
                drivers.insert(registration.fqdn, (registration.create_fn)());
            }
            DriverRegistry { drivers }
        })
    }

    /// Fetches a driver by its FQDN
    pub fn get_driver(&self, fqdn: &str) -> Option<&dyn Driver> {
        self.drivers.get(fqdn).map(|boxed| boxed.as_ref())
    }
}
