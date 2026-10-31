//! Reads another package's test data at run time (no dependency on it).

use std::path::Path;

#[test]
fn reads_the_core_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/testdata/expected.txt");
    let text = std::fs::read_to_string(path).unwrap();
    assert_eq!(adv_report::lines(&text), ["alpha", "beta"]);
}
