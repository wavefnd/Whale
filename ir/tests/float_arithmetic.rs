use ir::*;

fn bits(width: u16, raw: u64) -> FloatBits {
    match width {
        16 => FloatBits::F16(raw as u16),
        32 => FloatBits::F32(raw as u32),
        _ => FloatBits::F64(raw),
    }
}

fn op(name: &str) -> BinOp {
    match name {
        "fadd" => BinOp::FAdd,
        "fsub" => BinOp::FSub,
        "fmul" => BinOp::FMul,
        "fdiv" => BinOp::FDiv,
        "frem" => BinOp::FRem,
        _ => unreachable!(),
    }
}

#[test]
fn non_float_opcodes_never_use_the_nan_fast_path() {
    for op in [
        BinOp::Add,
        BinOp::Sub,
        BinOp::Mul,
        BinOp::UDiv,
        BinOp::SDiv,
        BinOp::URem,
        BinOp::SRem,
        BinOp::And,
        BinOp::Or,
        BinOp::Xor,
        BinOp::Shl,
        BinOp::LShr,
        BinOp::AShr,
    ] {
        for raw in [0x3f800000, 0x7f800000, 0x7fc00000, 0x7f800001] {
            assert_eq!(
                float::arithmetic::arithmetic(&op, FloatBits::F32(raw), FloatBits::F32(0)),
                None,
                "{op:?} {raw:x}"
            );
        }
        assert_eq!(
            float::arithmetic::arithmetic(&op, FloatBits::F16(0x7e00), FloatBits::F32(0)),
            None
        );
    }
}

#[test]
fn exact_fraction_vectors_execute_at_every_declared_precision() {
    let mut modules = std::collections::HashMap::new();
    for row in include_str!("fixtures/float-arithmetic.txt").lines().filter(|l| !l.starts_with('#'))
    {
        let p: Vec<_> = row.split_whitespace().collect();
        let width: u16 = p[0].parse().unwrap();
        let a = bits(width, u64::from_str_radix(p[2], 16).unwrap());
        let b = bits(width, u64::from_str_radix(p[3], 16).unwrap());
        let want = bits(width, u64::from_str_radix(p[4], 16).unwrap());
        assert_eq!(float::arithmetic::arithmetic(&op(p[1]), a, b), Some(want), "{row}");
        let m=modules.entry((width,p[1])).or_insert_with(||parse_module(&format!("module {{ format_version 8 semantics_version 1 target \"x86_64-whale-linux\" datalayout {{ptr=64,endian=little}} declare @f0 \"float\": whale (f{width}, f{width}) -> f{width}, linkage internal fn @f0 \"float\"(%v0 \"a\": f{width}, %v1 \"b\": f{width}) -> f{width}, entry %b0 {{ %b0 \"entry\": %v2: f{width} = {} f{width} %v0, %v1 ret f{width} %v2 }} }}",p[1])).unwrap());
        assert_eq!(
            interpret(m, FunctionId(0), &[ConstValue::F(a), ConstValue::F(b)]).unwrap().value,
            Some(ConstValue::F(want)),
            "{row}"
        );
        if let Some(cop) = match p[1] {
            "fadd" => Some(ConstBinaryOp::Add),
            "fsub" => Some(ConstBinaryOp::Sub),
            "fmul" => Some(ConstBinaryOp::Mul),
            _ => None,
        } {
            let ty = match width {
                16 => Type::F16,
                32 => Type::F32,
                _ => Type::F64,
            };
            let e = ConstExpr {
                ty: ty.clone(),
                kind: ConstExprKind::Binary {
                    op: cop,
                    left: Box::new(ConstExpr::literal(ty.clone(), ConstValue::F(a))),
                    right: Box::new(ConstExpr::literal(ty, ConstValue::F(b))),
                },
            };
            assert_eq!(e.evaluate(&|_| None).unwrap(), ConstValue::F(want));
        }
    }
    for m in modules.values() {
        let text = print_module(m);
        assert_eq!(print_module(&parse_module(&text).unwrap()), text);
    }
}

#[test]
fn native_f32_f64_random_oracle() {
    let mut seed = 42u64;
    for _ in 0..10000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let a = seed;
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let b = seed;
        for op in [BinOp::FAdd, BinOp::FSub, BinOp::FMul, BinOp::FDiv, BinOp::FRem] {
            let (x, y) = (f64::from_bits(a), f64::from_bits(b));
            let z = match op {
                BinOp::FAdd => x + y,
                BinOp::FSub => x - y,
                BinOp::FMul => x * y,
                BinOp::FDiv => x / y,
                _ => x % y,
            };
            let want = if z.is_nan() { 0x7ff8000000000000 } else { z.to_bits() };
            assert_eq!(
                float::arithmetic::arithmetic(&op, FloatBits::F64(a), FloatBits::F64(b)),
                Some(FloatBits::F64(want))
            );
            let (x, y) = (f32::from_bits(a as u32), f32::from_bits(b as u32));
            let z = match op {
                BinOp::FAdd => x + y,
                BinOp::FSub => x - y,
                BinOp::FMul => x * y,
                BinOp::FDiv => x / y,
                _ => x % y,
            };
            let want = if z.is_nan() { 0x7fc00000 } else { z.to_bits() };
            assert_eq!(
                float::arithmetic::arithmetic(
                    &op,
                    FloatBits::F32(a as u32),
                    FloatBits::F32(b as u32)
                ),
                Some(FloatBits::F32(want))
            );
        }
    }
}

#[cfg(feature = "socket")]
#[test]
fn unary_negation_runtime_and_constants_preserve_bits_and_wrap_integer_minimum() {
    use ir::lower_ast::{frontend as a, interchange};
    for width in [16u16, 32, 64] {
        let sign = 1u64 << (width - 1);
        let inf = match width {
            16 => 0x7c00,
            32 => 0x7f800000,
            _ => 0x7ff0000000000000,
        };
        for raw in [0, sign, inf, inf | sign, inf | 1, inf | ((inf >> 1) & !inf)] {
            for constant in [false, true] {
                let ty = a::TypeRef::Float { bits: width };
                let expr = a::Expr::Neg(Box::new(a::Expr::Lit(a::Lit::Float {
                    bits: width,
                    value: format!("{}", bits(width, raw)),
                })));
                let p = a::Program {
                    declarations: vec![],
                    globals: vec![],
                    functions: vec![a::Function {
                        name: "neg".into(),
                        convention: CallingConvention::Whale,
                        linkage: Linkage::Internal,
                        link_name: None,
                        parameters: vec![],
                        return_type: ty.clone(),
                        body: if constant {
                            vec![
                                a::Stmt::ConstDecl { name: "n".into(), ty: ty.clone(), init: expr },
                                a::Stmt::Return(Some(a::Expr::Var("n".into()))),
                            ]
                        } else {
                            vec![a::Stmt::Return(Some(expr))]
                        },
                    }],
                };
                let json = interchange::encode(p).unwrap();
                assert!(interchange::decode(
                    &json.replace("\"format_version\": 5", "\"format_version\": 4")
                )
                .is_err());
                let m = lower_o0(
                    &interchange::decode(&json).unwrap(),
                    "x86_64-whale-linux",
                    DataLayout::default_64bit_le(),
                )
                .unwrap();
                let text = print_module(&m);
                assert_eq!(print_module(&parse_module(&text).unwrap()), text);
                assert_eq!(
                    interpret(&m, FunctionId(0), &[]).unwrap().value,
                    Some(ConstValue::F(bits(width, raw ^ sign)))
                );
                assert_eq!(
                    float::arithmetic::negate(float::arithmetic::negate(bits(width, raw))),
                    bits(width, raw)
                );
            }
        }
    }
    for (ty, min) in [(Type::I1, -1), (Type::I8, -128), (Type::I128, i128::MIN)] {
        let mut b = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        let mut f = b.begin_function("neg", vec![], ty.clone());
        let x = f.const_int(ty.clone(), min);
        let y = f.neg(ty, x);
        f.ret(Some(y));
        f.finish();
        assert_eq!(
            interpret(&b.finish(), FunctionId(0), &[]).unwrap().value,
            Some(ConstValue::I(min))
        );
    }
}
