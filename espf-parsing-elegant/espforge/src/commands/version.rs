/// Print version/build information: the espforge crate version itself, and
/// the exact version of the esp-generate scaffolding backend espforge was
/// compiled against (resolved by build.rs).
pub fn run() {
    println!("espforge       {}", env!("CARGO_PKG_VERSION"));
    println!("esp-generate   {}", env!("ESP_GENERATE_VERSION"));
}
