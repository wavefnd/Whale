use ir::*;

fn module(
    params: Vec<Type>,
    result: Option<Type>,
    instruction: impl FnOnce(ValueId) -> Instruction,
) -> Module {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = builder.begin_function(
        "probe",
        params.into_iter().enumerate().map(|(i, t)| (format!("p{i}"), t)).collect(),
        Type::Void,
    );
    f.ret(None);
    f.finish();
    let mut m = builder.finish();
    let dst = ValueId(99);
    if let Some(ty) = result {
        m.functions[0].value_types.push((dst, ty));
    }
    m.functions[0].blocks[0].instructions.push(instruction(dst));
    m
}

#[test]
fn binary_operations_check_both_operands_and_opcode_category() {
    for (op, ty, lhs, rhs, valid) in [
        (BinOp::Add, Type::I32, Type::I32, Type::I32, true),
        (BinOp::Add, Type::I32, Type::Bool, Type::I32, false),
        (BinOp::Sub, Type::I32, Type::I32, Type::I64, false),
        (BinOp::Mul, Type::F32, Type::F32, Type::F32, false),
        (BinOp::FAdd, Type::F32, Type::F32, Type::F32, true),
        (BinOp::FSub, Type::I32, Type::I32, Type::I32, false),
        (BinOp::Add, Type::Bool, Type::Bool, Type::Bool, false),
        (BinOp::Add, Type::I1, Type::I1, Type::I1, true),
        (BinOp::Add, Type::U1, Type::U1, Type::U1, true),
        (BinOp::SDiv, Type::I32, Type::I32, Type::I32, true),
        (BinOp::UDiv, Type::U32, Type::U32, Type::U32, true),
        (BinOp::UDiv, Type::I32, Type::I32, Type::I32, false),
        (BinOp::SDiv, Type::U32, Type::U32, Type::U32, false),
        (BinOp::And, Type::F32, Type::F32, Type::F32, false),
        (BinOp::Shl, Type::U32, Type::U32, Type::U32, true),
    ] {
        let m = module(vec![lhs, rhs], Some(ty.clone()), |dst| Instruction::Bin {
            dst,
            op,
            ty,
            lhs: ValueId(0),
            rhs: ValueId(1),
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
}

#[test]
fn comparisons_check_operand_types_and_categories() {
    for (op, ty, valid) in [
        (CmpOp::Eq, Type::Bool, true),
        (CmpOp::Ne, Type::ptr_to(Type::I32), true),
        (CmpOp::SLt, Type::I32, true),
        (CmpOp::ULt, Type::U32, true),
        (CmpOp::SLt, Type::U32, false),
        (CmpOp::ULt, Type::I32, false),
        (CmpOp::FEq, Type::F64, true),
        (CmpOp::FEq, Type::I32, false),
        (CmpOp::Eq, Type::F64, false),
        (CmpOp::SLt, Type::Bool, false),
        (CmpOp::ULt, Type::ptr_to(Type::I32), true),
    ] {
        let m = module(vec![ty.clone(), ty.clone()], Some(Type::Bool), |dst| Instruction::Cmp {
            dst,
            op,
            ty,
            lhs: ValueId(0),
            rhs: ValueId(1),
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
    for (ty, floating, valid) in [
        (Type::F64, true, true),
        (Type::F64, false, false),
        (Type::I32, false, true),
        (Type::I32, true, false),
    ] {
        let m = module(vec![ty.clone(), ty.clone()], Some(Type::Bool), |dst| {
            if floating {
                Instruction::FCmp { dst, pred: FCmpPred::Oeq, ty, lhs: ValueId(0), rhs: ValueId(1) }
            } else {
                Instruction::ICmp { dst, pred: ICmpPred::Eq, ty, lhs: ValueId(0), rhs: ValueId(1) }
            }
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
    let m = module(vec![Type::I32, Type::I64], Some(Type::Bool), |dst| Instruction::ICmp {
        dst,
        pred: ICmpPred::Eq,
        ty: Type::I32,
        lhs: ValueId(0),
        rhs: ValueId(1),
    });
    assert!(verify_module(&m).is_err());
}

#[test]
fn byte_memory_operations_match_the_printed_operand_contract() {
    let ptr = Type::ptr_to(Type::U8);
    for (destination, source, length, align, valid) in [
        (ptr.clone(), ptr.clone(), Type::U64, 1, true),
        (Type::U64, ptr.clone(), Type::U64, 1, false),
        (ptr.clone(), Type::ptr_to(Type::I32), Type::U64, 1, false),
        (ptr.clone(), ptr.clone(), Type::I64, 1, false),
        (ptr.clone(), ptr.clone(), Type::U64, 0, false),
    ] {
        let m = module(vec![destination, source, length], None, |_| Instruction::Memcpy {
            dst: ValueId(0),
            src: ValueId(1),
            n: ValueId(2),
            align,
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
    for (destination, value, length, align, valid) in [
        (ptr.clone(), Type::U8, Type::U64, 4, true),
        (Type::U64, Type::U8, Type::U64, 1, false),
        (ptr.clone(), Type::I8, Type::U64, 1, false),
        (ptr.clone(), Type::U8, Type::I64, 1, false),
        (ptr, Type::U8, Type::U64, 3, false),
    ] {
        let m = module(vec![destination, value, length], None, |_| Instruction::Memset {
            dst: ValueId(0),
            val: ValueId(1),
            n: ValueId(2),
            align,
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
}

#[test]
fn moves_not_and_traps_validate_their_input_types() {
    for (ty, actual, valid) in
        [(Type::I32, Type::I32, true), (Type::I32, Type::U32, false), (Type::F64, Type::F64, false)]
    {
        let m = module(vec![actual], Some(ty.clone()), |dst| Instruction::Not {
            dst,
            ty,
            src: ValueId(0),
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
    for actual in [Type::Bool, Type::I32] {
        let m = module(vec![actual.clone()], Some(Type::I32), |dst| Instruction::Mov {
            dst,
            ty: Type::I32,
            src: ValueId(0),
        });
        assert_eq!(verify_module(&m).is_ok(), actual == Type::I32);
        let m = module(vec![actual.clone()], None, |_| Instruction::TrapIf {
            cond: ValueId(0),
            reason: "test".into(),
        });
        assert_eq!(verify_module(&m).is_ok(), actual == Type::Bool);
    }
}

#[test]
fn select_checks_condition_and_both_alternatives() {
    for (cond, yes, no, valid) in [
        (Type::Bool, Type::I32, Type::I32, true),
        (Type::U1, Type::I32, Type::I32, false),
        (Type::Bool, Type::I64, Type::I32, false),
        (Type::Bool, Type::I32, Type::I64, false),
    ] {
        let m = module(vec![cond, yes, no], Some(Type::I32), |dst| Instruction::Select {
            dst,
            ty: Type::I32,
            cond: ValueId(0),
            on_true: ValueId(1),
            on_false: ValueId(2),
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
}

#[test]
fn load_store_and_alloca_check_access_types_and_explicit_alignments() {
    for (ptr, align, valid) in [
        (Type::ptr_to(Type::I32), 4, true),
        (Type::I32, 4, false),
        (Type::ptr_to(Type::I64), 4, false),
        (Type::ptr_to(Type::I32), 0, false),
        (Type::ptr_to(Type::I32), 3, false),
    ] {
        let m = module(vec![ptr.clone()], Some(Type::I32), |dst| Instruction::Load {
            dst,
            ty: Type::I32,
            ptr: ValueId(0),
            align,
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
        let m = module(vec![ptr, Type::I32], None, |_| Instruction::Store {
            ty: Type::I32,
            ptr: ValueId(0),
            value: ValueId(1),
            align,
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
    let m = module(vec![Type::ptr_to(Type::I32), Type::Bool], None, |_| Instruction::Store {
        ty: Type::I32,
        ptr: ValueId(0),
        value: ValueId(1),
        align: 4,
    });
    assert!(verify_module(&m).is_err());
    for (ty, align, valid) in
        [(Type::I32, 4, true), (Type::I32, 0, false), (Type::I32, 3, false), (Type::Void, 1, false)]
    {
        let m = module(vec![], Some(Type::ptr_to(ty.clone())), |dst| Instruction::Alloca {
            dst,
            ty,
            align,
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
}

#[test]
fn integer_division_and_remainder_reject_pointers_in_api_and_text() {
    let ptr = Type::ptr_to(Type::U8);
    for (op, spelling, ty) in [
        (BinOp::SDiv, "sdiv", Type::I32),
        (BinOp::SRem, "srem", Type::I32),
        (BinOp::UDiv, "udiv", Type::U32),
        (BinOp::URem, "urem", Type::U32),
    ] {
        for (ty, valid) in [(ptr.clone(), false), (ty, true)] {
            let m = module(vec![ty.clone(), ty.clone()], Some(ty.clone()), |dst| {
                Instruction::Bin { dst, op: op.clone(), ty, lhs: ValueId(0), rhs: ValueId(1) }
            });
            if valid {
                verify_module(&m).unwrap();
                parse_module(&print_module(&m)).unwrap();
            } else {
                assert!(
                    matches!(verify_module(&m), Err(VerifyError::InvalidInstructionType { .. })),
                    "{spelling}"
                );
                let error = parse_module(&print_module(&m)).unwrap_err();
                assert!(
                    matches!(error.kind, ParseErrorKind::Verification(ref e) if matches!(**e, VerifyError::InvalidInstructionType { .. })),
                    "{spelling}: {error}"
                );
            }
        }
    }
}
