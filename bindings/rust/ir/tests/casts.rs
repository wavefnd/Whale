use ir::*;

fn cast(src: &str, dst: &str, op: &str) -> Module {
    parse_module(&format!("module {{ format_version 8 semantics_version 1 target \"x86_64-whale-linux\" datalayout {{ptr=64,endian=little}} declare @f0 \"cast\": whale ({src}) -> {dst}, linkage internal fn @f0 \"cast\"(%v0 \"x\": {src}) -> {dst}, entry %b0 {{ %b0 \"entry\": %v1: {dst} = {op} {src} %v0 to {dst} ret {dst} %v1 }} }}")).unwrap()
}

#[test]
fn checked_integer_matrix_preserves_values_or_traps_at_cast() {
    for sb in [1, 8, 16, 32, 64, 128] {
        for db in [1, 8, 16, 32, 64, 128] {
            for signed_src in [false, true] {
                for signed_dst in [false, true] {
                    let src = format!("{}{sb}", if signed_src { "i" } else { "u" });
                    let dst = format!("{}{db}", if signed_dst { "i" } else { "u" });
                    let m = cast(&src, &dst, "checked_int");
                    let text = print_module(&m);
                    assert_eq!(print_module(&parse_module(&text).unwrap()), text);
                    assert!(parse_module(&text.replace("format_version 8", "format_version 7"))
                        .is_err());
                    let umax = if sb == 128 { u128::MAX } else { (1u128 << sb) - 1 };
                    let smin = if sb == 128 { i128::MIN } else { -(1i128 << (sb - 1)) };
                    let smax = if sb == 128 { i128::MAX } else { (1i128 << (sb - 1)) - 1 };
                    let vals = if signed_src {
                        vec![
                            ConstValue::I(smin),
                            ConstValue::I(-1),
                            ConstValue::I(0),
                            ConstValue::I(smax),
                        ]
                    } else {
                        vec![ConstValue::U(0), ConstValue::U(1), ConstValue::U(umax)]
                    };
                    for v in vals {
                        // Independent destination interval test on mathematical input.
                        let expected = match v {
                            ConstValue::I(x) if signed_dst => {
                                let lo = if db == 128 { i128::MIN } else { -(1i128 << (db - 1)) };
                                let hi =
                                    if db == 128 { i128::MAX } else { (1i128 << (db - 1)) - 1 };
                                (lo..=hi).contains(&x)
                            },
                            ConstValue::I(x) => {
                                x >= 0 && (db == 128 || (x as u128) < (1u128 << db))
                            },
                            ConstValue::U(x) if signed_dst => {
                                x <= if db == 128 {
                                    i128::MAX as u128
                                } else {
                                    (1u128 << (db - 1)) - 1
                                }
                            },
                            ConstValue::U(x) => db == 128 || x < (1u128 << db),
                            _ => unreachable!(),
                        };
                        let out = interpret(&m, FunctionId(0), std::slice::from_ref(&v));
                        if expected {
                            let want = match v {
                                ConstValue::I(x) if signed_dst => ConstValue::I(x),
                                ConstValue::I(x) => ConstValue::U(x as u128),
                                ConstValue::U(x) if signed_dst => ConstValue::I(x as i128),
                                ConstValue::U(x) => ConstValue::U(x),
                                _ => unreachable!(),
                            };
                            assert_eq!(out.unwrap().value, Some(want), "{src}->{dst}");
                        } else {
                            assert!(
                                matches!(out,Err(InterpreterError::Trap(t)) if t.reason==TrapReason::InvalidConversion && t.site.instruction==0 && t.site.value==Some(ValueId(1))),
                                "{src}->{dst}"
                            );
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        interpret(&cast("u16", "u8", "trunc"), FunctionId(0), &[ConstValue::U(256)]).unwrap().value,
        Some(ConstValue::U(0))
    );
}

#[cfg(feature = "socket")]
#[test]
fn frontend_cast_mapping_and_version_gate() {
    use ir::lower_ast::{frontend as a, interchange};
    for (sb, ss, db, ds, kind, input, op, expected) in [
        (8, true, 16, true, a::IntegerCast::Numeric, "-1", "sext", ConstValue::I(-1)),
        (8, false, 16, true, a::IntegerCast::Numeric, "255", "zext", ConstValue::I(255)),
        (16, false, 8, false, a::IntegerCast::Numeric, "256", "trunc", ConstValue::U(0)),
        (8, false, 8, true, a::IntegerCast::Numeric, "255", "bitcast", ConstValue::I(-1)),
        (8, true, 16, false, a::IntegerCast::Checked, "1", "checked_int", ConstValue::U(1)),
    ] {
        let ret = a::TypeRef::Int { bits: db, signed: ds };
        let p = a::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![a::Function {
                name: "cast".into(),
                convention: CallingConvention::Whale,
                linkage: Linkage::Internal,
                link_name: None,
                parameters: vec![],
                return_type: ret.clone(),
                body: vec![a::Stmt::Return(Some(a::Expr::Cast {
                    value: Box::new(a::Expr::Lit(a::Lit::Int {
                        bits: sb,
                        signed: ss,
                        value: input.into(),
                    })),
                    ty: ret,
                    kind,
                }))],
            }],
        };
        let json = interchange::encode(p).unwrap();
        assert!(interchange::decode(
            &json.replace("\"format_version\": 5", "\"format_version\": 4")
        )
        .unwrap_err()
        .contains("require AST format 5"));
        let p = interchange::decode(&json).unwrap();
        let m = lower_o0(&p, "x86_64-whale-linux", DataLayout::default_64bit_le()).unwrap();
        assert!(print_module(&m).contains(op));
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(expected));
    }
}

#[test]
fn invalid_checked_pairs_and_earlier_traps_are_preserved() {
    let good = print_module(&cast("u8", "i8", "checked_int"));
    for (src, dst) in [("bool", "i8"), ("f32", "i8"), ("u8", "ptr<u8>")] {
        let bad = good
            .replace("%v0 \"x\": u8", &format!("%v0 \"x\": {src}"))
            .replace("whale (u8)", &format!("whale ({src})"))
            .replace("checked_int u8", &format!("checked_int {src}"))
            .replace("i8", dst);
        assert!(parse_module(&bad).is_err());
    }
    let text = good.replace(
        "    %v1:",
        "    %v2: bool = const bool true\n    trap_if bool %v2, reason=\"earlier\"\n    %v1:",
    );
    let m = parse_module(&text).unwrap();
    assert!(
        matches!(interpret(&m,FunctionId(0),&[ConstValue::U(255)]),Err(InterpreterError::Trap(t)) if t.reason==TrapReason::Explicit("earlier".into()))
    );
}
