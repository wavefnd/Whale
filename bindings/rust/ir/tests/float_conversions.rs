use ir::*;

fn scalar(ty: &str, input: &str) -> ConstValue {
    if let Some(width) = ty.strip_prefix('f') {
        let width = width.parse().unwrap();
        ConstValue::F(
            FloatBits::parse(
                width,
                &format!(
                    "0x{:0digits$x}",
                    u64::from_str_radix(input, 16).unwrap(),
                    digits = usize::from(width) / 4
                ),
            )
            .unwrap(),
        )
    } else if ty.starts_with('i') {
        ConstValue::I(input.parse().unwrap())
    } else {
        ConstValue::U(input.parse().unwrap())
    }
}

#[test]
fn public_float_conversion_rejects_invalid_types_and_payloads() {
    let f16 = ConstValue::F(FloatBits::F16(0x3c00));
    let f32 = ConstValue::F(FloatBits::F32(0x3f800000));
    let f64 = ConstValue::F(FloatBits::F64(0x3ff0000000000000));
    for (op, src, dst, value) in [
        (CastOp::FTrunc, Type::F64, Type::F32, f16.clone()),
        (CastOp::FTrunc, Type::F64, Type::F32, ConstValue::I(1)),
        (CastOp::FExt, Type::F64, Type::F32, f64.clone()),
        (CastOp::FTrunc, Type::F16, Type::F32, f16.clone()),
        (CastOp::FExt, Type::F32, Type::F32, f32.clone()),
        (CastOp::FExt, Type::F16, Type::U32, f16.clone()),
        (CastOp::IToF_S, Type::U32, Type::F32, ConstValue::U(1)),
        (CastOp::IToF_U, Type::I32, Type::F32, ConstValue::I(1)),
        (CastOp::IToF_S, Type::I8, Type::F32, ConstValue::I(128)),
        (CastOp::IToF_U, Type::U8, Type::F32, ConstValue::F(FloatBits::F32(0))),
        (CastOp::FToI_S, Type::F32, Type::U32, f32.clone()),
        (CastOp::FToI_U, Type::F32, Type::I32, f32.clone()),
        (CastOp::FToI_S, Type::F32, Type::I32, ConstValue::U(1)),
        (CastOp::Bitcast, Type::F32, Type::F32, f32.clone()),
    ] {
        assert_eq!(float::arithmetic::convert(&op, &src, &dst, &value), None, "{op:?}");
    }
    assert_eq!(float::arithmetic::convert(&CastOp::FExt, &Type::F16, &Type::F32, &f16), Some(f32));
}

#[test]
fn independent_conversion_vectors_cover_every_width_pair_and_trap_site() {
    let mut modules = std::collections::HashMap::new();
    let mut traps = 0;
    for row in
        include_str!("fixtures/float-conversions.txt").lines().filter(|l| !l.starts_with('#'))
    {
        let p: Vec<_> = row.split_whitespace().collect();
        let (src, dst, op) = (p[0], p[1], p[2]);
        let m=modules.entry((src,dst,op)).or_insert_with(||parse_module(&format!("module {{ format_version 8 semantics_version 1 target \"x86_64-whale-linux\" datalayout {{ptr=64,endian=little}} declare @f0 \"convert\": whale ({src}) -> {dst}, linkage internal fn @f0 \"convert\"(%v0 \"x\": {src}) -> {dst}, entry %b0 {{ %b0 \"entry\": %v1: ptr<u8> = alloca u8, align 1 %v2: u8 = const u8 42 store u8 %v2, ptr<u8> %v1, align 1 %v3: {dst} = {op} {src} %v0 to {dst} ret {dst} %v3 }} }}")).unwrap());
        let result = interpret(m, FunctionId(0), &[scalar(src, p[3])]);
        if p[4] == "trap" {
            traps += 1;
            assert!(
                matches!(result,Err(InterpreterError::Trap(t)) if t.reason==TrapReason::InvalidConversion && t.site.instruction==3 && t.site.value==Some(ValueId(3)) && t.steps==4),
                "{row}"
            );
        } else {
            assert_eq!(result.unwrap().value, Some(scalar(dst, p[4])), "{row}");
        }
    }
    assert!(traps > 1000);
    for m in modules.values() {
        let s = print_module(m);
        assert_eq!(print_module(&parse_module(&s).unwrap()), s);
    }
}

#[test]
fn every_binary16_pattern_extends_exactly_and_numeric_nan_conversion_canonicalizes() {
    for raw in 0..=u16::MAX {
        let input = ConstValue::F(FloatBits::F16(raw));
        for dst in [Type::F32, Type::F64] {
            let out = float::arithmetic::convert(&CastOp::FExt, &Type::F16, &dst, &input).unwrap();
            let back = float::arithmetic::convert(&CastOp::FTrunc, &dst, &Type::F16, &out).unwrap();
            let expected = if raw & 0x7c00 == 0x7c00 && raw & 0x3ff != 0 { 0x7e00 } else { raw };
            assert_eq!(back, ConstValue::F(FloatBits::F16(expected)), "{raw:x} via {dst}");
            if let ConstValue::F(value) = out {
                if expected != 0x7e00 {
                    assert_eq!(value.to_f64(), FloatBits::F16(raw).to_f64());
                }
            }
        }
    }
}

#[test]
fn midpoint_rounding_is_direct_and_prior_traps_precede_invalid_conversions() {
    // A double just above a binary16 midpoint would double-round through f32.
    let a = ConstValue::F(FloatBits::F64(0x3ff0020000000001));
    assert_eq!(
        float::arithmetic::convert(&CastOp::FTrunc, &Type::F64, &Type::F16, &a),
        Some(ConstValue::F(FloatBits::F16(0x3c01)))
    );
    let m=parse_module("module { format_version 8 semantics_version 1 target \"x86_64-whale-linux\" datalayout {ptr=64,endian=little} declare @f0 \"test\": whale () -> u8, linkage internal fn @f0 \"test\"() -> u8, entry %b0 { %b0 \"entry\": %v0: bool = const bool true trap_if bool %v0, reason=\"earlier effect\" %v1: f64 = const f64 0x7ff0000000000000 %v2: u8 = ftoi_u f64 %v1 to u8 ret u8 %v2 } }").unwrap();
    assert!(
        matches!(interpret(&m,FunctionId(0),&[]),Err(InterpreterError::Trap(t)) if t.reason==TrapReason::Explicit("earlier effect".into()) && t.steps==2)
    );
}
