use ir::*;

fn switch(ty: Type, actual: Type, keys: Vec<ConstValue>) -> Module {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = builder.begin_function("switch", vec![("x".into(), actual)], Type::Void);
    let bb = f.create_block("exit");
    f.br(bb);
    f.set_insert_point(bb);
    f.ret(None);
    f.finish();
    let mut m = builder.finish();
    m.functions[0].blocks[0].terminator = Some(Terminator::Switch {
        ty,
        value: ValueId(0),
        default_bb: bb,
        cases: keys.into_iter().map(|k| (k, bb)).collect(),
    });
    m
}

#[test]
fn switch_checks_scrutinee_category_range_and_duplicates() {
    for (ty, actual, keys) in [
        (Type::I32, Type::I64, vec![]),
        (Type::I32, Type::I32, vec![ConstValue::I(1), ConstValue::I(1)]),
        (Type::I32, Type::I32, vec![ConstValue::Bool(true)]),
        (Type::I32, Type::I32, vec![ConstValue::U(1)]),
        (Type::I8, Type::I8, vec![ConstValue::I(128)]),
        (Type::I32, Type::I32, vec![ConstValue::F(ir::FloatBits::F64(1.0f64.to_bits()))]),
        (Type::F32, Type::F32, vec![]),
        (Type::Bool, Type::Bool, vec![ConstValue::Bool(true), ConstValue::Bool(true)]),
    ] {
        assert!(verify_module(&switch(ty, actual, keys)).is_err());
    }
    for (ty, keys) in [
        (Type::I32, vec![]),
        (Type::I32, vec![ConstValue::I(i32::MIN as i128), ConstValue::I(i32::MAX as i128)]),
        (Type::U128, vec![ConstValue::U(u128::MAX), ConstValue::U(0)]),
        (Type::I1, vec![ConstValue::I(-1), ConstValue::I(0)]),
        (Type::Bool, vec![ConstValue::Bool(false), ConstValue::Bool(true)]),
    ] {
        assert!(verify_module(&switch(ty.clone(), ty, keys)).is_ok());
    }
}
