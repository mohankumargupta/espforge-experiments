use cliclack::{input, intro, outro, select};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Start the interactive CLI session
    intro("ESP32 RISC-V Project Generator")?;

    // 1. Ask for the project name
    let project_name: String = input("What is your project name?")
        .placeholder("my-esp32-project")
        .validate(|input: &String| {
            if input.trim().is_empty() {
                Err("Project name cannot be empty.")
            } else {
                Ok(())
            }
        })
        .interact()?;

    // 2. Ask for the ESP32 RISC-V target chip
    let chip_target: &str = select("Select your ESP32 RISC-V target chip:")
        .initial_value("esp32c3")
        .item("esp32c2", "ESP32-C2", "Ultra-low power, single-core RISC-V")
        .item(
            "esp32c3",
            "ESP32-C3",
            "Popular single-core RISC-V (Wi-Fi/BLE)",
        )
        .item(
            "esp32c6",
            "ESP32-C6",
            "RISC-V with Wi-Fi 6, BLE, and Zigbee",
        )
        .item("esp32h2", "ESP32-H2", "RISC-V with Thread, Zigbee, and BLE")
        .item(
            "esp32p4",
            "ESP32-P4",
            "High-performance dual-core RISC-V (No Wi-Fi)",
        )
        .interact()?;

    // End the CLI session and print the confirmation
    outro(format!(
        "Ready! Project '{}' configured for {}.",
        project_name, chip_target
    ))?;

    Ok(())
}
