#[test]
fn values_come_from_the_schema() {
    assert_eq!(adv_gen::RETRIES, 3);
    assert_eq!(adv_gen::TIMEOUT_SECS, 30);
}
