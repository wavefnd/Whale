use reverse::*;

#[test]
fn absent_core_has_an_explicit_capability_error() {
    assert!(!backend_available());
    assert_eq!(backend_name(), "unavailable");
    for (bytes, offset) in [(&[0x90][..], 0), (&[][..], 0), (&[0xc3][..], usize::MAX)] {
        assert_eq!(decode(bytes, offset), Err(DecodeError::CoreUnavailable));
    }
}
