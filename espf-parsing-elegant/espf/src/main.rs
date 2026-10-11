pub mod context;
pub mod parser;
pub mod generated_registry; // Automatically updated by build.rs file watchers!

use context::RuntimeContext;

fn main() {
    // Simulated layout file loaded dynamically at runtime (e.g. from flash filesystem)
    let espf_runtime_file = r#"
--- ==============================================================
--- ESPF runtime file format instance
--- ==============================================================

# peripherals
  <gpio id="gpio4" pin="4" />
  <i2c  id="i2c0" sda="8" scl="9" frequency="400kHz" />
  <spi  id="spi2" sclk="6" mosi="7" />

# components
  <i2c_component  id="temp_bus"    i2c="$i2c0" address="0x48" />
  <spi_component  id="display_bus" spi="$spi2" mode="0" cs="gpio5" frequency="40MHz" />

# devices
  <tmp102         id="outdoor_temp" i2c_component="$temp_bus" />
  <ili9341        id="tft_display"  spi_component="$display_bus" />
    "#;

    println!("--- Phase 1: Launching Winnow Zero-Copy String Tokenizer ---");
    let mut parsed_sections = parser::parse_espf_runtime(espf_runtime_file)
        .expect("Fatal Error: Syntactical parsing error inside configuration file structure.");

    // Harvest empty fallbacks if specific sections are missing from target load file
    let empty = vec![];
    let peripherals = parsed_sections.remove("peripherals").unwrap_or(empty.clone());
    let components = parsed_sections.remove("components").unwrap_or(empty.clone());
    let devices = parsed_sections.remove("devices").unwrap_or(empty);

    println!("--- Phase 2: Resolving Dynamic Topological Dependency Chains ---");
    let mut runtime_ctx = RuntimeContext::new();
    runtime_ctx.resolve_and_bind(&peripherals, &components, &devices)
        .expect("Fatal Error: Topological dependency verification validation broken.");

    println!("--- Phase 3: Executing Safe Array Drivers Dispatch Sweeps ---\n");
    for resolved_device in &runtime_ctx.resolved_devices {
        // Route driver matches string names using code generated instantly by build.rs
        generated_registry::route_driver(resolved_device.tag_name, resolved_device, &runtime_ctx)
            .unwrap();
    }

    println!("\n🎉 System pipeline successfully initialised and executing.");
}
