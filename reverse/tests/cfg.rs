use reverse::*;

#[test]
fn absent_cfg_core_is_explicit() {
    assert!(!backend_available());
    for (bytes, base, seeds) in [(&[0xc3][..], 0, &[0][..]), (&[][..], u64::MAX, &[usize::MAX][..])]
    {
        assert_eq!(
            analyze(bytes, base, seeds, &[], CfgLimits::default()),
            Err(CfgError::CoreUnavailable)
        );
    }
}
