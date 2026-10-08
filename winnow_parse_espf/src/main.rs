use winnow_parse_espf::{parse, EspfFile};

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: testing_winnow <espf-file>");
            std::process::exit(2);
        }
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            std::process::exit(1);
        }
    };

    match parse(&text) {
        Ok(file) => {
            println!("{file}");
            summarize(&file);
        }
        Err(e) => {
            eprintln!("error: {path}: {e}");
            std::process::exit(1);
        }
    }
}

fn summarize(file: &EspfFile) {
    println!("--- summary ---");
    if let Some(ef) = &file.espforge {
        println!("project '{}' on {} ({})", ef.name, ef.chip, ef.runtime);
    }
    println!(
        "gpio: {}, i2c: {}, spi: {}, uart: {}, components: {}, devices: {}",
        file.peripherals.gpio.len(),
        file.peripherals.i2c.len(),
        file.peripherals.spi.len(),
        file.peripherals.uart.len(),
        file.components.len(),
        file.devices.len(),
    );
}
