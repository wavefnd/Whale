use ir::{ConstValue, DataLayout, FloatBits, ModuleBuilder, Type};

#[test]
fn binary16_storage_converts_without_intermediate_binary32_rounding() {
    assert!(FloatBits::from_f64(17, 1.0).is_err());
    for bits in 0..=u16::MAX {
        let value = FloatBits::F16(bits);
        assert_eq!(FloatBits::parse(16, &value.to_string()).unwrap(), value);
        if bits & 0x7c00 != 0x7c00 {
            assert_eq!(
                FloatBits::from_f64(16, value.to_f64()).expect("supported float width"),
                value
            );
        }
    }
    // Midpoint at 1, plus a binary64 increment invisible to an intermediate f32.
    assert_eq!(
        FloatBits::from_f64(16, 1.0 + 2f64.powi(-11)).expect("supported float width"),
        FloatBits::F16(0x3c00)
    );
    assert_eq!(
        FloatBits::from_f64(16, 1.0 + 2f64.powi(-11) + 2f64.powi(-40))
            .expect("supported float width"),
        FloatBits::F16(0x3c01)
    );
    assert_eq!(
        FloatBits::from_f64(16, 65520.0).expect("supported float width"),
        FloatBits::F16(0x7c00)
    );
    assert_eq!(
        FloatBits::from_f64(16, 2f64.powi(-25)).expect("supported float width"),
        FloatBits::F16(0)
    );
    assert_eq!(
        FloatBits::from_f64(16, -0.0).expect("supported float width"),
        FloatBits::F16(0x8000)
    );
}

#[test]
fn width_mismatch_is_invalid_ir_and_bit_equality_distinguishes_zero_and_nan() {
    let mut module = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    module.add_global("wrong", Type::F32, ConstValue::F(FloatBits::F64(0)), 4);
    assert!(ir::verify_module(&module.finish()).is_err());
    assert_ne!(ConstValue::F(FloatBits::F32(0)), ConstValue::F(FloatBits::F32(0x80000000)));
    assert_eq!(
        ConstValue::F(FloatBits::F32(0x7fc00001)),
        ConstValue::F(FloatBits::F32(0x7fc00001))
    );
    assert_ne!(
        ConstValue::F(FloatBits::F32(0x7fc00001)),
        ConstValue::F(FloatBits::F32(0x7fc00002))
    );
}
