use ir::{ConstValue, DataLayout, Instruction, Module, ModuleBuilder, Type, VerifyError};

fn constant(ty: Type, value: ConstValue) -> Module {
    let mut m = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = m.begin_function("constant", vec![], ty.clone());
    let v = f.undef(ty.clone());
    f.ret(Some(v));
    f.finish();
    let mut m = m.finish();
    m.functions[0].blocks[0].instructions[0] = Instruction::Const { dst: v, ty, value };
    m
}

#[test]
fn signed_and_unsigned_boundaries_include_real_one_bit_integers() {
    for (ty, bits) in [
        (Type::I1, 1),
        (Type::I8, 8),
        (Type::I16, 16),
        (Type::I32, 32),
        (Type::I64, 64),
        (Type::I128, 128),
    ] {
        let min = if bits == 128 { i128::MIN } else { -(1i128 << (bits - 1)) };
        let max = if bits == 128 { i128::MAX } else { (1i128 << (bits - 1)) - 1 };
        for v in [min, 0, max] {
            assert!(ir::verify_module(&constant(ty.clone(), ConstValue::I(v))).is_ok(), "{ty} {v}");
        }
        for v in [min.checked_sub(1), max.checked_add(1)].into_iter().flatten() {
            assert!(
                matches!(ir::verify_module(&constant(ty.clone(), ConstValue::I(v))), Err(VerifyError::InvalidConstant { func, ty: got, .. }) if func == "constant" && got == ty)
            );
        }
    }
    for (ty, bits) in [
        (Type::U1, 1),
        (Type::U8, 8),
        (Type::U16, 16),
        (Type::U32, 32),
        (Type::U64, 64),
        (Type::U128, 128),
    ] {
        let max = if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 };
        for v in [0, max] {
            assert!(ir::verify_module(&constant(ty.clone(), ConstValue::U(v))).is_ok());
        }
        if let Some(v) = max.checked_add(1) {
            assert!(matches!(
                ir::verify_module(&constant(ty, ConstValue::U(v))),
                Err(VerifyError::InvalidConstant { .. })
            ));
        }
    }
}

#[test]
fn constant_categories_do_not_implicitly_convert() {
    for (ty, value) in [
        (Type::I32, ConstValue::F(ir::FloatBits::F64(1.5f64.to_bits()))),
        (Type::F32, ConstValue::I(1)),
        (Type::Bool, ConstValue::I(1)),
        (Type::Bool, ConstValue::U(0)),
        (Type::I1, ConstValue::Bool(true)),
        (Type::U1, ConstValue::Bool(false)),
        (Type::I8, ConstValue::U(1)),
        (Type::U8, ConstValue::I(1)),
        (Type::Void, ConstValue::I(0)),
        (Type::ptr_to(Type::I8), ConstValue::U(0)),
        (Type::Array(Box::new(Type::I8), 1), ConstValue::I(0)),
        (Type::Tuple(vec![Type::I8]), ConstValue::I(0)),
        (Type::Struct(vec![Type::I8]), ConstValue::I(0)),
    ] {
        assert!(matches!(
            ir::verify_module(&constant(ty, value)),
            Err(VerifyError::InvalidConstant { .. })
        ));
    }
    for value in [false, true] {
        assert!(ir::verify_module(&constant(Type::Bool, ConstValue::Bool(value))).is_ok());
    }
    for ty in [Type::F16, Type::F32, Type::F64] {
        assert!(ir::verify_module(&constant(
            ty.clone(),
            ConstValue::F(
                ir::FloatBits::from_f64(
                    match ty {
                        Type::F16 => 16,
                        Type::F32 => 32,
                        _ => 64,
                    },
                    1.5
                )
                .expect("supported float width")
            )
        ))
        .is_ok());
    }
}

#[test]
fn globals_are_checked_even_without_functions() {
    for align in [0, 3, 6, u32::MAX] {
        let mut m = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        m.add_global("bad", Type::I32, ConstValue::I(1), align);
        assert!(
            matches!(ir::verify_module(&m.finish()), Err(VerifyError::InvalidGlobalAlignment { name, align: got }) if name == "bad" && got == align)
        );
    }
    for align in [1, 2, 4, 8, 1 << 31] {
        let mut m = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        m.add_global("valid", Type::I32, ConstValue::I(1), align);
        assert!(ir::verify_module(&m.finish()).is_ok());
    }
    for (ty, value) in [
        (Type::I32, ConstValue::F(ir::FloatBits::F64(1.5f64.to_bits()))),
        (Type::U8, ConstValue::U(256)),
        (Type::Bool, ConstValue::I(1)),
    ] {
        let mut m = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        m.add_global("bad", ty, value, 1);
        assert!(
            matches!(ir::verify_module(&m.finish()), Err(VerifyError::InvalidGlobalInitializer { name, .. }) if name == "bad")
        );
    }
}

#[test]
fn function_and_global_namespaces_are_independent() {
    let mut m = constant(Type::I32, ConstValue::I(1));
    m.globals.push(ir::Global {
        id: ir::GlobalId(0),
        name: "constant".into(),
        ty: Type::I32,
        init: ConstValue::I(2),
        init_expr: ir::ConstExpr::literal(Type::I32, ConstValue::I(2)),
        align: 4,
    });
    assert!(ir::verify_module(&m).is_ok());
    let mut duplicate = m.clone();
    duplicate.functions.push(m.functions[0].clone());
    assert!(
        matches!(ir::verify_module(&duplicate), Err(VerifyError::DuplicateFunction { name }) if name == "constant")
    );
    m.globals.push(m.globals[0].clone());
    assert!(
        matches!(ir::verify_module(&m), Err(VerifyError::DuplicateGlobal { name }) if name == "constant")
    );
}

#[test]
fn integers_cannot_implicitly_become_branch_select_or_trap_conditions() {
    for (ty, payload) in [
        (Type::Bool, ConstValue::Bool(true)),
        (Type::I1, ConstValue::I(-1)),
        (Type::U1, ConstValue::U(1)),
        (Type::I32, ConstValue::I(1)),
    ] {
        for kind in 0..3 {
            let mut m = constant(ty.clone(), payload.clone());
            let f = &mut m.functions[0];
            let value = f.value_types[0].0;
            match kind {
                0 => {
                    let exit = ir::BlockId(99);
                    f.blocks.push(ir::BasicBlock {
                        id: exit,
                        name: "exit".into(),
                        instructions: vec![],
                        terminator: f.blocks[0].terminator.clone(),
                    });
                    f.blocks[0].terminator =
                        Some(ir::Terminator::CBr { cond: value, then_bb: exit, else_bb: exit })
                },
                1 => f.blocks[0]
                    .instructions
                    .push(Instruction::TrapIf { cond: value, reason: "test".into() }),
                _ => {
                    let dst = ir::ValueId(99);
                    f.value_types.push((dst, ty.clone()));
                    f.blocks[0].instructions.push(Instruction::Select {
                        dst,
                        ty: ty.clone(),
                        cond: value,
                        on_true: value,
                        on_false: value,
                    });
                },
            }
            let result = ir::verify_module(&m);
            if ty == Type::Bool {
                assert!(result.is_ok());
            } else {
                assert!(
                    matches!(result, Err(VerifyError::ConditionTypeMismatch { got, .. }) if got == ty)
                );
            }
        }
    }
}
