use ir::*;

fn probe(params: Vec<Type>, result: Type, ins: impl FnOnce(ValueId) -> Instruction) -> Module {
    let mut b = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = b.begin_function(
        "probe",
        params
            .into_iter()
            .enumerate()
            .map(|(i, t)| (format!("p{i}"), t))
            .collect(),
        Type::Void,
    );
    f.ret(None);
    f.finish();
    let mut m = b.finish();
    m.functions[0].value_types.push((ValueId(99), result));
    m.functions[0].blocks[0].instructions.push(ins(ValueId(99)));
    m
}
fn cast(op: CastOp, source: Type, destination: Type) -> Module {
    probe(vec![source.clone()], destination.clone(), |dst| {
        Instruction::Cast {
            dst,
            op,
            src_ty: source,
            src: ValueId(0),
            dst_ty: destination,
        }
    })
}

#[test]
fn legal_casts_cover_every_opcode_and_scalar_width() {
    for (op, source, destination) in [
        (CastOp::ZExt, Type::I8, Type::I16),
        (CastOp::ZExt, Type::U1, Type::I128),
        (CastOp::SExt, Type::I1, Type::I32),
        (CastOp::SExt, Type::U8, Type::U16),
        (CastOp::Trunc, Type::I128, Type::U1),
        (CastOp::FExt, Type::F16, Type::F32),
        (CastOp::FExt, Type::F32, Type::F64),
        (CastOp::FTrunc, Type::F64, Type::F16),
        (CastOp::FTrunc, Type::F32, Type::F16),
        (CastOp::IToF_S, Type::I128, Type::F32),
        (CastOp::IToF_U, Type::U128, Type::F64),
        (CastOp::FToI_S, Type::F32, Type::I8),
        (CastOp::FToI_U, Type::F64, Type::U128),
        (CastOp::Bitcast, Type::I32, Type::U32),
        (CastOp::Bitcast, Type::F16, Type::U16),
        (CastOp::Bitcast, Type::I32, Type::F32),
        (CastOp::Bitcast, Type::F64, Type::U64),
        (
            CastOp::Bitcast,
            Type::ptr_to(Type::I32),
            Type::ptr_to(Type::Void),
        ),
        (CastOp::PtrToInt, Type::ptr_to(Type::I32), Type::U64),
        (CastOp::IntToPtr, Type::U64, Type::ptr_to(Type::I32)),
    ] {
        let m = cast(op, source, destination);
        assert!(verify_module(&m).is_ok(), "{m:?}");
    }
    // Bool's 0/1 conversion includes u1, but never signed i1.
    for ty in [
        Type::U1,
        Type::U8,
        Type::I8,
        Type::U16,
        Type::I16,
        Type::U32,
        Type::I32,
        Type::U64,
        Type::I64,
        Type::U128,
        Type::I128,
    ] {
        verify_module(&cast(CastOp::ZExt, Type::Bool, ty)).unwrap();
    }
}

#[test]
fn casts_reject_wrong_width_direction_category_and_signedness() {
    for (op, source, destination) in [
        (CastOp::ZExt, Type::I32, Type::I32),
        (CastOp::ZExt, Type::I64, Type::I32),
        (CastOp::SExt, Type::I32, Type::I16),
        (CastOp::Trunc, Type::U8, Type::U16),
        (CastOp::Trunc, Type::I8, Type::U8),
        (CastOp::FExt, Type::F64, Type::F32),
        (CastOp::FExt, Type::F32, Type::F32),
        (CastOp::FTrunc, Type::F16, Type::F32),
        (CastOp::IToF_S, Type::U32, Type::F32),
        (CastOp::IToF_U, Type::I32, Type::F32),
        (CastOp::FToI_S, Type::F64, Type::U32),
        (CastOp::FToI_U, Type::F64, Type::I32),
        (CastOp::FToI_S, Type::I64, Type::I32),
        (CastOp::ZExt, Type::Bool, Type::I1),
        (CastOp::SExt, Type::Bool, Type::I32),
        (CastOp::Bitcast, Type::Bool, Type::U1),
        (CastOp::Bitcast, Type::U1, Type::Bool),
        (CastOp::Bitcast, Type::F64, Type::I32),
        (CastOp::Bitcast, Type::ptr_to(Type::I32), Type::U64),
        (CastOp::Bitcast, Type::Tuple(vec![Type::I32]), Type::I32),
        (CastOp::PtrToInt, Type::I64, Type::U64),
        (CastOp::PtrToInt, Type::ptr_to(Type::I32), Type::Bool),
        (CastOp::IntToPtr, Type::F64, Type::ptr_to(Type::I32)),
        (CastOp::IntToPtr, Type::I64, Type::I64),
        (CastOp::Trunc, Type::I32, Type::Void),
    ] {
        let m = cast(op, source, destination);
        assert!(
            matches!(verify_module(&m), Err(VerifyError::InvalidCast { .. })),
            "{m:?}"
        );
    }
}

#[test]
fn cast_annotation_cannot_override_the_actual_operand_type() {
    let m = probe(vec![Type::Bool], Type::I32, |dst| Instruction::Cast {
        dst,
        op: CastOp::FToI_S,
        src_ty: Type::F64,
        src: ValueId(0),
        dst_ty: Type::I32,
    });
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::OperandTypeMismatch {
            value: ValueId(0),
            expected: Type::F64,
            got: Type::Bool,
            ..
        })
    ));
    let mut m = cast(CastOp::ZExt, Type::I8, Type::I32);
    m.functions[0].value_types.last_mut().unwrap().1 = Type::I64;
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::ValueTypeMismatch { .. })
    ));
}

#[test]
fn checked_arithmetic_checks_operands_signedness_and_result_tuple() {
    for (op, ty) in [
        (CheckedOp::SAdd, Type::I1),
        (CheckedOp::SSub, Type::I32),
        (CheckedOp::SMul, Type::I128),
        (CheckedOp::UAdd, Type::U1),
        (CheckedOp::USub, Type::U32),
        (CheckedOp::UMul, Type::U128),
    ] {
        let make = |params, result, ty| {
            probe(params, result, |dst| Instruction::Checked {
                dst,
                op: op.clone(),
                ty,
                lhs: ValueId(0),
                rhs: ValueId(1),
            })
        };
        let result = Type::Tuple(vec![ty.clone(), Type::Bool]);
        verify_module(&make(
            vec![ty.clone(), ty.clone()],
            result.clone(),
            ty.clone(),
        ))
        .unwrap();
        for params in [vec![Type::Bool, ty.clone()], vec![ty.clone(), Type::Bool]] {
            assert!(matches!(
                verify_module(&make(params, result.clone(), ty.clone())),
                Err(VerifyError::OperandTypeMismatch { .. })
            ));
        }
        for bad in [
            ty.clone(),
            Type::Tuple(vec![ty.clone(), Type::I1]),
            Type::Tuple(vec![Type::Bool, ty.clone()]),
            Type::Tuple(vec![ty.clone()]),
        ] {
            assert!(matches!(
                verify_module(&make(vec![ty.clone(), ty.clone()], bad, ty.clone())),
                Err(VerifyError::ValueTypeMismatch { .. })
            ));
        }
        for invalid in [
            Type::Bool,
            Type::F32,
            Type::ptr_to(Type::I32),
            if matches!(ty, Type::I1 | Type::I32 | Type::I128) {
                Type::U32
            } else {
                Type::I32
            },
        ] {
            assert!(matches!(
                verify_module(&make(
                    vec![invalid.clone(), invalid.clone()],
                    Type::Tuple(vec![invalid.clone(), Type::Bool]),
                    invalid
                )),
                Err(VerifyError::InvalidInstructionType { .. })
            ));
        }
    }
}

#[test]
fn tuple_extraction_checks_tuple_index_and_exact_field_type() {
    for (source, index, destination, valid) in [
        (Type::Tuple(vec![Type::I32, Type::Bool]), 0, Type::I32, true),
        (
            Type::Tuple(vec![Type::I32, Type::Bool]),
            1,
            Type::Bool,
            true,
        ),
        (Type::Tuple(vec![Type::I32, Type::Bool]), 1, Type::I1, false),
        (
            Type::Tuple(vec![Type::I32, Type::Bool]),
            0,
            Type::U32,
            false,
        ),
        (
            Type::Tuple(vec![Type::I32, Type::Bool]),
            2,
            Type::I32,
            false,
        ),
        (Type::Tuple(vec![]), 0, Type::I32, false),
        (Type::I32, 0, Type::I32, false),
    ] {
        let m = probe(vec![source], destination.clone(), |dst| {
            Instruction::Extract {
                dst,
                dst_ty: destination,
                tuple: ValueId(0),
                index,
            }
        });
        assert_eq!(verify_module(&m).is_ok(), valid, "{m:?}");
    }
}
