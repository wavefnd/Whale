use ir::*;

fn literal(value: i128) -> ConstExpr {
    ConstExpr::literal(Type::I32, ConstValue::I(value))
}

fn reference(ty: Type, id: ConstRef) -> ConstExpr {
    ConstExpr { ty, kind: ConstExprKind::Reference(id) }
}

fn add(left: ConstExpr, right: ConstExpr) -> ConstExpr {
    ConstExpr {
        ty: left.ty.clone(),
        kind: ConstExprKind::Binary {
            op: ConstBinaryOp::Add,
            left: Box::new(left),
            right: Box::new(right),
        },
    }
}

fn module(expression: ConstExpr, result: ConstValue) -> Module {
    let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    mb.add_global_const("value", expression, result, 4);
    mb.finish()
}

#[test]
fn global_expressions_preserve_trees_and_resolve_by_identity_not_storage_order() {
    let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let a = mb.add_global_const("a", add(literal(1), literal(2)), ConstValue::I(3), 4);
    mb.add_global_const(
        "b",
        add(reference(Type::I32, ConstRef::Global(a)), literal(4)),
        ConstValue::I(7),
        4,
    );
    let mut m = mb.finish();
    for _ in 0..2 {
        let before = format!("{m:?}");
        verify_module(&m).unwrap();
        assert_eq!(before, format!("{m:?}"));
        assert!(print_module(&m).contains("add(i32 1, i32 2)"));
        m.globals.reverse();
    }
    m.globals[0].init = ConstValue::I(9);
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::InvalidConstExpression { reason: ConstEvalError::ResultMismatch, .. })
    ));
}

#[test]
fn cyclic_unknown_and_duplicate_constant_identities_fail() {
    let mut m = module(literal(1), ConstValue::I(1));
    let id = m.globals[0].id;
    m.globals[0].init_expr = reference(Type::I32, ConstRef::Global(id));
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::InvalidConstExpression { reason: ConstEvalError::CyclicReference, .. })
    ));
    m.globals[0].init_expr = reference(Type::I32, ConstRef::Global(GlobalId(999)));
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::InvalidConstExpression {
            reason: ConstEvalError::UnknownReference(_),
            ..
        })
    ));
    m.globals[0].init_expr = reference(Type::I32, ConstRef::Local(ValueId(0)));
    assert!(verify_module(&m).is_err(), "global initializers cannot reference function locals");
    m.globals[0].init_expr = literal(1);
    let mut other = m.globals[0].clone();
    other.name = "other".into();
    m.globals.push(other);
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::InvalidConstExpression {
            reason: ConstEvalError::DuplicateDefinition(_),
            ..
        })
    ));
}

#[test]
fn expression_types_literals_operators_and_cached_results_are_validated() {
    let mut wrong_root = add(literal(1), literal(2));
    wrong_root.ty = Type::I64;
    for (expression, result) in [
        (add(literal(1), literal(2)), ConstValue::I(99)),
        (wrong_root, ConstValue::I(3)),
        (add(literal(1), ConstExpr::literal(Type::U32, ConstValue::U(2))), ConstValue::I(3)),
        (
            add(
                ConstExpr::literal(Type::Bool, ConstValue::Bool(true)),
                ConstExpr::literal(Type::Bool, ConstValue::Bool(false)),
            ),
            ConstValue::Bool(true),
        ),
        (
            ConstExpr {
                ty: Type::Bool,
                kind: ConstExprKind::Compare {
                    op: ConstCompareOp::Lt,
                    left: Box::new(ConstExpr::literal(Type::Bool, ConstValue::Bool(false))),
                    right: Box::new(ConstExpr::literal(Type::Bool, ConstValue::Bool(true))),
                },
            },
            ConstValue::Bool(true),
        ),
        (add(literal(i64::MAX as i128), literal(1)), ConstValue::I(0)),
    ] {
        assert!(verify_module(&module(expression, result)).is_err());
    }
    let mut wrong_decl = module(literal(1), ConstValue::I(1));
    wrong_decl.globals[0].ty = Type::I64;
    assert!(verify_module(&wrong_decl).is_err());
    for ty in [Type::I1, Type::I8, Type::I128] {
        let expression = add(
            ConstExpr::literal(ty.clone(), ConstValue::I(-1)),
            ConstExpr::literal(ty, ConstValue::I(-1)),
        );
        let expected = if expression.ty == Type::I1 { 0 } else { -2 };
        verify_module(&module(expression, ConstValue::I(expected))).unwrap();
    }
}

#[test]
fn float_literals_compare_payloads_without_nan_or_signed_zero_false_matches() {
    for value in
        [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::from_bits(0x7ff8_0000_0000_0042)]
    {
        let mut m = module(
            ConstExpr::literal(Type::F64, ConstValue::F(ir::FloatBits::F64(value.to_bits()))),
            ConstValue::F(ir::FloatBits::F64(value.to_bits())),
        );
        verify_module(&m).unwrap();
        m.globals[0].init = ConstValue::F(ir::FloatBits::F64(value.to_bits() ^ 1));
        assert!(verify_module(&m).is_err());
    }
}

#[test]
fn local_constant_results_types_and_dependency_cycles_are_checked() {
    let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = mb.begin_function("f", vec![], Type::Void);
    let a = f.const_decl("a", add(literal(1), literal(2)), ConstValue::I(3));
    let b = f.const_decl(
        "b",
        add(reference(Type::I32, ConstRef::Local(a)), literal(4)),
        ConstValue::I(7),
    );
    f.ret(None);
    f.finish();
    let m = mb.finish();
    verify_module(&m).unwrap();
    let mut bad = m.clone();
    if let Instruction::ConstDecl { value, .. } = &mut bad.functions[0].blocks[0].instructions[1] {
        *value = ConstValue::I(8);
    }
    assert!(verify_module(&bad).is_err());
    let mut bad = m.clone();
    if let Instruction::ConstDecl { expression, .. } =
        &mut bad.functions[0].blocks[0].instructions[0]
    {
        *expression = reference(Type::I32, ConstRef::Local(b));
    }
    assert!(matches!(
        verify_module(&bad),
        Err(VerifyError::InvalidConstExpression { reason: ConstEvalError::CyclicReference, .. })
    ));
    let mut bad = m.clone();
    bad.functions[0].blocks[0].instructions[0] = Instruction::Undef { dst: a, ty: Type::I32 };
    assert!(verify_module(&bad).is_err(), "runtime values are not compile-time constants");
}

#[test]
fn named_compile_time_integer_constants_can_select_gep_fields() {
    let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = mb.begin_function("f", vec![], Type::Void);
    let base = f.alloca(Type::Struct(vec![Type::I32, Type::Bool]), 4);
    let zero = f.const_i32(0);
    let index = f.const_decl("field", add(literal(0), literal(1)), ConstValue::I(1));
    f.ret(None);
    f.finish();
    let mut m = mb.finish();
    m.functions[0].value_types.push((ValueId(99), Type::ptr_to(Type::Bool)));
    m.functions[0].blocks[0].instructions.push(Instruction::Gep {
        dst: ValueId(99),
        dst_ty: Type::ptr_to(Type::Bool),
        base_ptr: base,
        indices: vec![zero, index],
    });
    verify_module(&m).unwrap();
}
