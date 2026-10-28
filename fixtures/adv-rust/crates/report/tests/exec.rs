//! Runs another package's binary (`cargo run -p adv-tool`): no dependency on it.

use std::process::Command;

#[test]
fn runs_the_tool() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["run", "--quiet", "--locked", "-p", "adv-tool"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "adv-tool 1");
}
