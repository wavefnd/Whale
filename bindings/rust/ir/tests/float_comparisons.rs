use ir::*;

// Independent IEEE truth table: columns less/equal/greater/unordered. The
// literal ranks below are human-assigned, rather than using production compare.
const PREDICATES: [(&str, FCmpPred, [bool; 4]); 14] = [
    ("oeq", FCmpPred::Oeq, [false, true, false, false]),
    ("one", FCmpPred::One, [true, false, true, false]),
    ("olt", FCmpPred::Olt, [true, false, false, false]),
    ("ole", FCmpPred::Ole, [true, true, false, false]),
    ("ogt", FCmpPred::Ogt, [false, false, true, false]),
    ("oge", FCmpPred::Oge, [false, true, true, false]),
    ("ord", FCmpPred::Ord, [true, true, true, false]),
    ("uno", FCmpPred::Uno, [false, false, false, true]),
    ("ueq", FCmpPred::Ueq, [false, true, false, true]),
    ("une", FCmpPred::Une, [true, false, true, true]),
    ("ult", FCmpPred::Ult, [true, false, false, true]),
    ("ule", FCmpPred::Ule, [true, true, false, true]),
    ("ugt", FCmpPred::Ugt, [false, false, true, true]),
    ("uge", FCmpPred::Uge, [false, true, true, true]),
];

const GENERIC: [(&str, CmpOp, ConstCompareOp, usize); 6] = [
    ("feq", CmpOp::FEq, ConstCompareOp::Eq, 0),
    ("fne", CmpOp::FNe, ConstCompareOp::Ne, 9),
    ("flt", CmpOp::FLt, ConstCompareOp::Lt, 2),
    ("fle", CmpOp::FLe, ConstCompareOp::Le, 3),
    ("fgt", CmpOp::FGt, ConstCompareOp::Gt, 4),
    ("fge", CmpOp::FGe, ConstCompareOp::Ge, 5),
];

fn samples(ty: &Type) -> Vec<(FloatBits, Option<i8>)> {
    let bits: [u64; 14] = match ty {
        Type::F16 => [
            0xfc00, 0xfbff, 0xbc00, 0x8001, 0x8000, 0, 1, 0x3c00, 0x7bff, 0x7c00, 0x7c01, 0x7e55,
            0xfe01, 0xfc01,
        ],
        Type::F32 => [
            0xff800000, 0xff7fffff, 0xbf800000, 0x80000001, 0x80000000, 0, 1, 0x3f800000,
            0x7f7fffff, 0x7f800000, 0x7f800001, 0x7fc01234, 0xffc00001, 0xff800001,
        ],
        Type::F64 => [
            0xfff0000000000000,
            0xffefffffffffffff,
            0xbff0000000000000,
            0x8000000000000001,
            0x8000000000000000,
            0,
            1,
            0x3ff0000000000000,
            0x7fefffffffffffff,
            0x7ff0000000000000,
            0x7ff0000000000001,
            0x7ff8123456789abc,
            0xfff8000000000001,
            0xfff0000000000001,
        ],
        _ => unreachable!(),
    };
    let ranks = [
        Some(-4),
        Some(-3),
        Some(-2),
        Some(-1),
        Some(0),
        Some(0),
        Some(1),
        Some(2),
        Some(3),
        Some(4),
        None,
        None,
        None,
        None,
    ];
    bits.into_iter()
        .zip(ranks)
        .map(|(bits, rank)| {
            (
                match ty {
                    Type::F16 => FloatBits::F16(bits as u16),
                    Type::F32 => FloatBits::F32(bits as u32),
                    _ => FloatBits::F64(bits),
                },
                rank,
            )
        })
        .collect()
}

fn relation(a: Option<i8>, b: Option<i8>) -> usize {
    match a.zip(b).map(|(a, b)| a.cmp(&b)) {
        Some(std::cmp::Ordering::Less) => 0,
        Some(std::cmp::Ordering::Equal) => 1,
        Some(std::cmp::Ordering::Greater) => 2,
        None => 3,
    }
}

fn module(ty: &Type, family: &str, predicate: &str) -> Module {
    parse_module(&format!("module {{ format_version 7 semantics_version 1 target \"x86_64-whale-linux\" datalayout {{ ptr=64, endian=little }} declare @f0 \"cmp\": whale ({ty}, {ty}) -> bool, linkage internal fn @f0 \"cmp\"(%v0 \"a\": {ty}, %v1 \"b\": {ty}) -> bool, entry %b0 {{ %b0 \"entry\": %v2: bool = {family} {predicate} {ty} %v0, %v1 ret bool %v2 }} }}")).unwrap()
}

#[test]
fn all_explicit_predicates_execute_the_independent_shared_ieee_vectors() {
    for ty in [Type::F16, Type::F32, Type::F64] {
        for (name, predicate, expected) in &PREDICATES {
            let module = module(&ty, "fcmp", name);
            let text = print_module(&module);
            assert_eq!(print_module(&parse_module(&text).unwrap()), text);
            for (lhs, arank) in samples(&ty) {
                for (rhs, brank) in samples(&ty) {
                    let expected = expected[relation(arank, brank)];
                    assert_eq!(
                        lhs.compare(rhs, predicate),
                        Some(expected),
                        "{ty} {name} {lhs} {rhs}"
                    );
                    let result = interpret(
                        &module,
                        FunctionId(0),
                        &[ConstValue::F(lhs), ConstValue::F(rhs)],
                    )
                    .unwrap();
                    assert_eq!(
                        result.value,
                        Some(ConstValue::Bool(expected)),
                        "{ty} {name} {lhs} {rhs}"
                    );
                    assert_eq!(result.steps, 2);
                }
            }
        }
    }
}

#[test]
fn generic_operators_and_constant_evaluation_share_the_explicit_mapping_and_vectors() {
    for ty in [Type::F16, Type::F32, Type::F64] {
        for (name, op, constant, index) in GENERIC {
            assert_eq!(op.float_predicate(), Some(PREDICATES[index].1.clone()));
            let module = module(&ty, "cmp", name);
            for (lhs, arank) in samples(&ty) {
                for (rhs, brank) in samples(&ty) {
                    let expected = PREDICATES[index].2[relation(arank, brank)];
                    let expr = ConstExpr {
                        ty: Type::Bool,
                        kind: ConstExprKind::Compare {
                            op: constant,
                            left: Box::new(ConstExpr::literal(ty.clone(), ConstValue::F(lhs))),
                            right: Box::new(ConstExpr::literal(ty.clone(), ConstValue::F(rhs))),
                        },
                    };
                    assert_eq!(
                        expr.evaluate(&|_| None),
                        Ok(ConstValue::Bool(expected)),
                        "constant {ty} {name} {lhs} {rhs}"
                    );
                    assert_eq!(
                        interpret(
                            &module,
                            FunctionId(0),
                            &[ConstValue::F(lhs), ConstValue::F(rhs)]
                        )
                        .unwrap()
                        .value,
                        Some(ConstValue::Bool(expected)),
                        "runtime {ty} {name} {lhs} {rhs}"
                    );
                    let equal = lhs.compare(rhs, &FCmpPred::Oeq).unwrap();
                    let unequal = lhs.compare(rhs, &FCmpPred::Une).unwrap();
                    assert_eq!(unequal, !equal);
                }
            }
        }
    }
    assert_eq!(CmpOp::Eq.float_predicate(), None);
    assert_eq!(CmpOp::SLt.float_predicate(), None);
    assert_eq!(FloatBits::F16(0).compare(FloatBits::F32(0), &FCmpPred::Oeq), None);
}

#[test]
fn every_binary16_encoding_agrees_with_an_independent_numeric_oracle() {
    // f16 -> f64 is exact for finite values. This test-only host oracle is
    // independent of production's integer comparison implementation.
    let rhs = [
        FloatBits::F16(0),
        FloatBits::F16(0x3c00),
        FloatBits::F16(0xbc00),
        FloatBits::F16(0x7c00),
        FloatBits::F16(0xfc00),
    ];
    for bits in 0..=u16::MAX {
        let lhs = FloatBits::F16(bits);
        for rhs in rhs {
            let (a, b) = (lhs.to_f64(), rhs.to_f64());
            for (pred, expected) in [
                (FCmpPred::Oeq, a == b),
                (FCmpPred::Une, a != b),
                (FCmpPred::Olt, a < b),
                (FCmpPred::Ole, a <= b),
                (FCmpPred::Ogt, a > b),
                (FCmpPred::Oge, a >= b),
            ] {
                assert_eq!(lhs.compare(rhs, &pred), Some(expected));
            }
        }
    }
}

#[test]
fn comparisons_keep_unused_o0_instructions_and_prior_trap_order() {
    let source="module { format_version 7 semantics_version 1 target \"x86_64-whale-linux\" datalayout { ptr=64, endian=little } declare @f0 \"order\": whale () -> void, linkage internal fn @f0 \"order\"() -> void, entry %b0 { %b0 \"entry\": %v0: f32 = const f32 0x7f800001 %v1: bool = fcmp uno f32 %v0, %v0 %v2: bool = cmp fne f32 %v0, %v0 trap_if bool %v2, reason=\"after comparisons\" trap reason=\"later\" } }";
    let module = parse_module(source).unwrap();
    let printed = print_module(&module);
    assert!(printed.contains("fcmp uno"));
    assert!(printed.contains("cmp fne"));
    assert!(
        matches!(interpret(&module,FunctionId(0),&[]),Err(InterpreterError::Trap(InterpreterTrap{site:ExecutionSite{instruction:3,..},reason:TrapReason::Explicit(reason),steps:4})) if reason=="after comparisons")
    );
    let prior =
        source.replace("%v1: bool = fcmp", "trap reason=\"first\" %b1 \"dead\": %v1: bool = fcmp");
    // The dead block needs its own dominating constants to remain valid O0 IR.
    let prior = prior
        .replace("%b1 \"dead\":", "%b1 \"dead\": %v3: f32 = const f32 0x7f800001")
        .replace("fcmp uno f32 %v0, %v0", "fcmp uno f32 %v3, %v3")
        .replace("cmp fne f32 %v0, %v0", "cmp fne f32 %v3, %v3");
    assert!(
        matches!(interpret(&parse_module(&prior).unwrap(),FunctionId(0),&[]),Err(InterpreterError::Trap(InterpreterTrap{site:ExecutionSite{instruction:1,..},reason:TrapReason::Explicit(reason),steps:2})) if reason=="first")
    );
}

#[test]
fn comparing_signaling_nan_values_never_quiets_or_replaces_their_payloads() {
    for ty in [Type::F16, Type::F32, Type::F64] {
        for (bits, rank) in samples(&ty) {
            if rank.is_some() {
                continue;
            }
            let mut module = module(&ty, "fcmp", "uno");
            module.declarations[0].signature.ret = ty.clone();
            module.functions[0].ret_ty = ty.clone();
            module.functions[0].blocks[0].terminator =
                Some(Terminator::Ret { ty: ty.clone(), value: Some(ValueId(0)) });
            verify_module(&module).unwrap();
            assert_eq!(
                interpret(&module, FunctionId(0), &[ConstValue::F(bits), ConstValue::F(bits)])
                    .unwrap()
                    .value,
                Some(ConstValue::F(bits))
            );
        }
    }
}
