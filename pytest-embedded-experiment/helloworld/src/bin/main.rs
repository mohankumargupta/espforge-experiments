#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::main;
use esp_hal::time::{Duration, Instant};
use log::info;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    esp_println::logger::init_logger_from_env();

    // generator version: 1.4.0
    // generator parameters: -o esp32c3 -o esp32c3-mini-1 -o unstable-hal -o log -o esp-backtrace -o agents -o neovim-classic -o stable-aarch64-unknown-linux-gnu

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // The following pins are used to bootstrap the chip. They are available
    // for use, but check the datasheet of the module for more information on them.
    // - GPIO2
    // - GPIO8
    // - GPIO9
    // These GPIO pins are in use by some feature of the module and should not be used.
    let _gpio11 = peripherals.GPIO11;
    let _gpio12 = peripherals.GPIO12;
    let _gpio13 = peripherals.GPIO13;
    let _gpio14 = peripherals.GPIO14;
    let _gpio15 = peripherals.GPIO15;
    let _gpio16 = peripherals.GPIO16;
    let _gpio17 = peripherals.GPIO17;

    loop {
        info!("Hello world!");
        let delay_start = Instant::now();
        while delay_start.elapsed() < Duration::from_millis(500) {}
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.2.2/examples
}
