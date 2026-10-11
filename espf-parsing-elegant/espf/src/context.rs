use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(pub u16);

#[derive(Default)]
pub struct IdInterner<'a> {
    map: HashMap<&'a str, Handle>,
    strings: Vec<&'a str>,
}

impl<'a> IdInterner<'a> {
    pub fn get_or_insert(&mut self, id: &'a str) -> Handle {
        if let Some(&handle) = self.map.get(id) {
            return handle;
        }
        let handle = Handle(self.strings.len() as u16);
        self.map.insert(id, handle);
        self.strings.push(id);
        handle
    }

    pub fn get(&self, id: &str) -> Option<Handle> {
        self.map.get(id).copied()
    }
}

#[derive(Debug, Clone)]
pub struct TagAttribute<'a> {
    pub key: &'a str,
    pub val: &'a str,
}

#[derive(Debug, Clone)]
pub struct ParsedNode<'a> {
    pub tag_name: &'a str,
    pub id: &'a str,
    pub attributes: Vec<TagAttribute<'a>>,
}

impl<'a> ParsedNode<'a> {
    pub fn get_attr(&self, key: &str) -> Option<&'a str> {
        self.attributes.iter().find(|a| a.key == key).map(|a| a.val)
    }
}

pub struct ResolvedDevice<'a> {
    pub tag_name: &'a str,
    pub id_handle: Handle,
    pub component_handle: Handle,
    pub raw_node: ParsedNode<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum ActivePeripheral {
    I2c { sda: u8, scl: u8 },
    Spi,
    Gpio,
    Uart,
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveComponent {
    pub parent_peripheral: Handle,
    pub address: u8,
}

pub struct RuntimeContext<'a> {
    pub interner: IdInterner<'a>,
    pub active_peripherals: Vec<Option<ActivePeripheral>>,
    pub active_components: Vec<Option<ActiveComponent>>,
    pub resolved_devices: Vec<ResolvedDevice<'a>>,
}

impl<'a> RuntimeContext<'a> {
    pub fn new() -> Self {
        Self {
            interner: IdInterner::default(),
            active_peripherals: vec![None; 128],
            active_components: vec![None; 128],
            resolved_devices: Vec::new(),
        }
    }

    pub fn resolve_and_bind(
        &mut self,
        raw_peripherals: &[ParsedNode<'a>],
        raw_components: &[ParsedNode<'a>],
        raw_devices: &[ParsedNode<'a>],
    ) -> Result<(), String> {
        // 1. Map Peripherals
        for peri in raw_peripherals {
            let peri_handle = self.interner.get_or_insert(peri.id);
            let active = match peri.tag_name {
                "i2c" => {
                    let sda = peri.get_attr("sda").unwrap_or("0").parse::<u8>().unwrap();
                    let scl = peri.get_attr("scl").unwrap_or("0").parse::<u8>().unwrap();
                    ActivePeripheral::I2c { sda, scl }
                }
                "spi" => ActivePeripheral::Spi,
                "uart" => ActivePeripheral::Uart,
                _ => ActivePeripheral::Gpio,
            };
            self.active_peripherals[peri_handle.0 as usize] = Some(active);
        }

        // 2. Resolve Components
        for comp in raw_components {
            let comp_handle = self.interner.get_or_insert(comp.id);
            
            // Search dynamically for any attribute referencing a parent connection
            let bus_attr = comp.attributes.iter()
                .find(|a| a.key == "i2c" || a.key == "spi" || a.key == "gpio" || a.key == "uart")
                .ok_or_else(|| format!("Component '{}' missing physical controller link", comp.id))?;

            let parent_str = bus_attr.val.trim_start_matches('$');
            let peri_handle = self.interner.get(parent_str)
                .ok_or_else(|| format!("❌ Config Error: Component '{}' references undefined hardware port '{}'", comp.id, parent_str))?;

            let addr = comp.get_attr("address")
                .map(|a| u8::from_str_radix(a.trim_start_matches("0x"), 16).unwrap_or(0))
                .unwrap_or(0);

            self.active_components[comp_handle.0 as usize] = Some(ActiveComponent {
                parent_peripheral: peri_handle,
                address: addr,
            });
        }

        // 3. Resolve Leaf Devices
        for dev in raw_devices {
            let dev_handle = self.interner.get_or_insert(dev.id);
            
            let comp_attr = dev.attributes.iter()
                .find(|a| a.key.contains("component"))
                .ok_or_else(|| format!("Device '{}' missing logical component layer mapping", dev.id))?;

            let comp_str = comp_attr.val.trim_start_matches('$');
            let comp_handle = self.interner.get(comp_str)
                .ok_or_else(|| format!("❌ Dependency Missing: Device <{} id='{}'> binds to non-existent component architecture '{}'", dev.tag_name, dev.id, comp_str))?;

            self.resolved_devices.push(ResolvedDevice {
                tag_name: dev.tag_name,
                id_handle: dev_handle,
                component_handle: comp_handle,
                raw_node: dev.clone(),
            });
        }

        Ok(())
    }
}
