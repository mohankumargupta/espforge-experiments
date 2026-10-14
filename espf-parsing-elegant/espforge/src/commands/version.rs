/// Versions of tooling espforge will eventually rely on.
/// These are fake placeholders for now; swap them for real dependency
/// versions (e.g. from Cargo metadata) once esp-generate and esphal are wired in.
const ESP_GENERATE_VERSION: &str = "0.1.0-fake";
const ESPHAL_VERSION: &str = "0.1.0-fake";

/// Print version/build information for espforge and its toolchain dependencies.
pub fn run() {
    println!("espforge       {}", env!("CARGO_PKG_VERSION"));
    println!("esp-generate   {}", ESP_GENERATE_VERSION);
    println!("esphal         {}", ESPHAL_VERSION);
}
