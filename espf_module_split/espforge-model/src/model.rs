use espforge_model::{load, Gpio, I2c};

const EXAMPLE: &str = r#"--- ==============================================================
--- ESPF file format
--- mandatory sections: espforge, peripherals, components, devices
--- ==============================================================

# espforge
  <project name="example project" description="example showing espf file format" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals
  <gpio id="gpio4" pin="4" />
  <gpio id="gpio5" pin="5" />
  <i2c  id="i2c0" sda="8" scl="9" frequency="400kHz" />
  <spi  id="spi2" sclk="6" mosi="7" /> --- displays generally omit miso
  <uart id="uart1" tx="20" rx="21" />

# components
  <gpio_component id="blue_led"    gpio="$gpio4" direction="output" />
  <i2c_component  id="temp_bus"    i2c="$i2c0" address="0x48" />
  <spi_component  id="display_bus" spi="$spi2" mode="0" cs="gpio5" frequency="40MHz" />
  <uart_component id="serial_bus"  uart="$uart1" baud_rate="9600" parity="none" stop_bits="1" />

# devices
  <status_led     id="status"       gpio_component="$blue_led" />
  <tmp102         id="outdoor_temp" i2c_component="$temp_bus" />
  <ili9341        id="tft_display"  
                  spi_component="$display_bus" />
"#;

const ESPFORGE: &str =
    "<project name=\"t\" />\n<chip type=\"esp32c3\" />\n<runtime type=\"blocking\" />";

fn config(espforge: &str, peripherals: &str) -> String {
    format!("# espforge\n{espforge}\n# peripherals\n{peripherals}\n")
}

/// Rendered diagnostics for a config that must fail.
fn errors(source: &str) -> String {
    load(source).unwrap_err().to_string()
}

#[test]
fn loads_the_example_file() {
    let model = load(EXAMPLE).unwrap();

    let espforge = &model.espforge;
    assert_eq!(espforge.project_name, "example project");
    assert_eq!(
        espforge.project_description.as_deref(),
        Some("example showing espf file format")
    );
    assert_eq!(espforge.chip.name, "esp32c3");
    assert_eq!(espforge.runtime, "blocking");

    let peripherals = &model.peripherals;
    assert_eq!(peripherals.gpios.len(), 2);
    assert_eq!(
        peripherals.gpios[0],
        Gpio { id: Some("gpio4".into()), pin: 4 }
    );
    assert_eq!(
        peripherals.i2cs[0],
        I2c {
            id: Some("i2c0".into()),
            bus: 0,
            sda: 8,
            scl: 9,
            frequency: Some("400kHz".into()),
        }
    );
    assert_eq!(peripherals.spis[0].sclk, 6);
    assert_eq!(peripherals.spis[0].mosi, 7);
    assert_eq!(peripherals.spis[0].miso, None);
    assert_eq!(peripherals.uarts[0].tx, 20);
    assert_eq!(peripherals.uarts[0].rx, 21);
}

#[test]
fn declares_ids_with_hierarchical_paths() {
    let model = load(EXAMPLE).unwrap();
    let symbols = &model.symbols;

    assert_eq!(symbols.len(), 5);
    assert_eq!(symbols.get("gpio4").unwrap().path, "peripherals.gpio.pin4");
    assert_eq!(symbols.get("i2c0").unwrap().path, "peripherals.i2c.bus0");
    assert_eq!(symbols.get("i2c0").unwrap().tag, "i2c");
    assert_eq!(symbols.get("spi2").unwrap().path, "peripherals.spi.bus0");
    assert_eq!(symbols.get("uart1").unwrap().path, "peripherals.uart.port0");
}

#[test]
fn reports_unknown_chip() {
    let source = config(
        "<project name=\"t\" />\n<chip type=\"esp32c9\" />\n<runtime type=\"blocking\" />",
        "<gpio pin=\"4\" />",
    );
    let rendered = errors(&source);

    assert!(rendered.contains("Unknown chip `esp32c9`"));
    assert!(rendered.contains("`esp32c3`"));
}

#[test]
fn reports_pin_the_chip_does_not_have() {
    let rendered = errors(&config(ESPFORGE, "<gpio id=\"g\" pin=\"30\" />"));

    assert!(rendered.contains("GPIO30 does not exist on esp32c3"));
}

#[test]
fn reports_pin_that_is_not_a_number() {
    let rendered = errors(&config(ESPFORGE, "<gpio id=\"g\" pin=\"four\" />"));

    assert!(rendered.contains("`four` is not a pin number"));
}

#[test]
fn reports_missing_mandatory_section() {
    let rendered = errors(&format!("# espforge\n{ESPFORGE}\n"));

    assert!(rendered.contains("Missing mandatory section `peripherals`"));
}

#[test]
fn reports_duplicate_ids_and_names_the_first_owner() {
    let rendered = errors(&config(
        ESPFORGE,
        "<gpio id=\"gpio4\" pin=\"4\" />\n<gpio id=\"gpio4\" pin=\"5\" />",
    ));

    assert!(rendered.contains("Duplicate id `gpio4`"));
    assert!(rendered.contains("peripherals.gpio.pin4"));
}

#[test]
fn rejects_more_buses_than_the_chip_has() {
    let rendered = errors(&config(
        ESPFORGE,
        "<i2c id=\"a\" sda=\"8\" scl=\"9\" />\n<i2c id=\"b\" sda=\"10\" scl=\"11\" />",
    ));

    assert!(rendered.contains("esp32c3 supports at most 1 <i2c>"));
}

#[test]
fn reports_unknown_attribute() {
    let rendered = errors(&config(
        ESPFORGE,
        "<gpio id=\"g\" pin=\"4\" speed=\"fast\" />",
    ));

    assert!(rendered.contains("Unknown attribute `speed`"));
}

#[test]
fn reports_missing_required_attribute() {
    let rendered = errors(&config(ESPFORGE, "<i2c id=\"i\" sda=\"8\" />"));

    assert!(rendered.contains("missing required attribute `scl`"));
}

#[test]
fn reports_unknown_peripheral() {
    let rendered = errors(&config(ESPFORGE, "<pwm id=\"p\" />"));

    assert!(rendered.contains("Unknown peripheral <pwm>"));
}

#[test]
fn reports_duplicate_chip_element() {
    let source = config(
        "<project name=\"t\" />\n<chip type=\"esp32c3\" />\n<chip type=\"esp32c3\" />\n<runtime type=\"blocking\" />",
        "<gpio pin=\"4\" />",
    );

    assert!(errors(&source).contains("Duplicate <chip>"));
}

#[test]
fn reports_every_error_not_just_the_first() {
    let source = config(
        ESPFORGE,
        "<gpio pin=\"40\" />\n<gpio pin=\"41\" />\n<pwm />",
    );

    assert_eq!(load(&source).unwrap_err().0.len(), 3);
}

#[test]
fn syntax_errors_come_back_as_diagnostics_too() {
    let error = load("# espforge\n<chip type \"esp32c3\" />\n").unwrap_err();

    assert_eq!(error.0.len(), 1);
    assert!(error.to_string().contains("Invalid attribute"));
}
