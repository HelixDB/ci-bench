//! Generates one `u32` constant per `name = value` line of the workspace's `schema/values.txt`
//! (outside every package's target directories).

use std::path::Path;

fn main() {
    let input = Path::new("../../schema/values.txt");
    println!("cargo::rerun-if-changed={}", input.display());
    let text = std::fs::read_to_string(input).expect("schema/values.txt");
    let consts: String = text
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(name, value)| {
            format!(
                "/// From `schema/values.txt`.\npub const {}: u32 = {};\n",
                name.trim().to_uppercase(),
                value.trim()
            )
        })
        .collect();
    let out = std::env::var_os("OUT_DIR").expect("OUT_DIR");
    std::fs::write(Path::new(&out).join("values.rs"), consts).expect("values.rs");
}
