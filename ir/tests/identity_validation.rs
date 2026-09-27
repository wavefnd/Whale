use ir::{DataLayout, Instruction, Module, ModuleBuilder, Type, ValueId};

fn module() -> Module {
    let mut m = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = m.begin_function("f", vec![("p".into(), Type::I32)], Type::I32);
    let v = f.const_i32(1);
    f.ret(Some(v));
    f.finish();
    m.finish()
}

#[test]
fn duplicate_blocks_and_definitions_are_rejected() {
    let mut m = module();
    let block = m.functions[0].blocks[0].clone();
    m.functions[0].blocks.push(block);
    assert!(ir::verify_module(&m).is_err());
    let mut m = module();
    let ins = m.functions[0].blocks[0].instructions[0].clone();
    m.functions[0].blocks[0].instructions.push(ins);
    assert!(ir::verify_module(&m).is_err());
    let mut m = module();
    m.functions[0].blocks[0]
        .instructions
        .push(Instruction::Undef {
            dst: ValueId(0),
            ty: Type::I32,
        });
    assert!(ir::verify_module(&m).is_err());
    let mut m = module();
    let p = m.functions[0].params[0].clone();
    m.functions[0].params.push(p);
    assert!(ir::verify_module(&m).is_err());
}

#[test]
fn every_definition_needs_exactly_one_matching_type_entry() {
    for id in [ValueId(0), ValueId(1)] {
        let mut m = module();
        m.functions[0].value_types.retain(|(v, _)| *v != id);
        assert!(ir::verify_module(&m).is_err());
        let mut m = module();
        m.functions[0]
            .value_types
            .iter_mut()
            .find(|(v, _)| *v == id)
            .unwrap()
            .1 = Type::I64;
        assert!(ir::verify_module(&m).is_err());
        for ty in [Type::I32, Type::I64] {
            let mut m = module();
            m.functions[0].value_types.push((id, ty));
            assert!(ir::verify_module(&m).is_err());
        }
    }
    let mut m = module();
    m.functions[0].value_types.push((ValueId(99), Type::I32));
    assert!(ir::verify_module(&m).is_err());
}

#[test]
fn ids_are_local_to_functions_and_metadata_order_does_not_matter() {
    let mut m = module();
    m.functions[0].value_types.reverse();
    let mut second = m.functions[0].clone();
    second.name = "second".into();
    second.id = ir::FunctionId(1);
    let mut decl = m.declarations[0].clone();
    decl.id = second.id;
    decl.name = second.name.clone();
    m.declarations.push(decl);
    m.functions.push(second);
    assert!(ir::verify_module(&m).is_ok());
}

#[test]
fn derived_result_types_match_builder_metadata() {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let external = builder
        .declare_function(
            "external",
            ir::FunctionSignature::whale(vec![], Type::I32),
            ir::Linkage::External,
            Some("external".into()),
        )
        .unwrap();
    let mut f = builder.begin_function("derived", vec![], Type::Void);
    let value = f.const_i32(1);
    let comparison = f.icmp(ir::ICmpPred::Eq, Type::I32, value, value);
    let checked = f.checked(ir::CheckedOp::SAdd, Type::I32, value, value);
    let tuple_ty = Type::Tuple(vec![Type::I32, Type::Bool]);
    let overflow = f.extract(tuple_ty.clone(), Type::Bool, checked, 1);
    let ptr = f.alloca(Type::I32, 4);
    f.store(Type::I32, value, ptr, 4);
    let loaded = f.load(Type::I32, ptr, 4);
    let called = f
        .call(ir::Callee::Direct(external), vec![])
        .unwrap()
        .unwrap();
    f.ret(None);
    f.finish();
    let module = builder.finish();
    ir::verify_module(&module).unwrap();
    for (id, expected) in [
        (comparison, Type::Bool),
        (checked, tuple_ty),
        (overflow, Type::Bool),
        (ptr, Type::ptr_to(Type::I32)),
        (loaded, Type::I32),
        (called, Type::I32),
    ] {
        let mut malformed = module.clone();
        malformed.functions[0]
            .value_types
            .iter_mut()
            .find(|(v, _)| *v == id)
            .unwrap()
            .1 = Type::I8;
        assert!(
            matches!(ir::verify_module(&malformed), Err(ir::VerifyError::ValueTypeMismatch { value, expected: got, .. }) if value == id && got == expected)
        );
    }
}
