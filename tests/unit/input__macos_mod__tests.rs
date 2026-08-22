#[test]
fn smoke_test_modifier_detection() {
    // Just proves the FFI link + call don't crash.
    let s = super::snapshot();
    let _ = (s.command, s.option);
}
