use ir::*;

fn gep(base: Type, indices: Vec<(Type, Option<ConstValue>)>, result: Type) -> Module {
    let mut params = vec![("base".into(), base)];
    params.extend(
        indices
            .iter()
            .enumerate()
            .map(|(i, (ty, _))| (format!("index{i}"), ty.clone())),
    );
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = builder.begin_function("address", params, Type::Void);
    f.ret(None);
    f.finish();
    let mut m = builder.finish();
    let f = &mut m.functions[0];
    for (i, (ty, constant)) in indices.iter().enumerate() {
        if let Some(value) = constant {
            let id = ValueId(i as u32 + 1);
            f.params.retain(|p| p.id != id);
            f.blocks[0].instructions.push(Instruction::Const {
                dst: id,
                ty: ty.clone(),
                value: value.clone(),
            });
        }
    }
    f.value_types.push((ValueId(100), result.clone()));
    f.blocks[0].instructions.push(Instruction::Gep {
        dst: ValueId(100),
        dst_ty: result,
        base_ptr: ValueId(0),
        indices: (0..indices.len()).map(|i| ValueId(i as u32 + 1)).collect(),
    });
    m.declarations[0].signature.params = f.params.iter().map(|p| p.ty.clone()).collect();
    m
}

fn literal(value: i128) -> (Type, Option<ConstValue>) {
    (Type::I64, Some(ConstValue::I(value)))
}
fn index() -> (Type, Option<ConstValue>) {
    (Type::I64, None)
}

#[test]
fn element_offsets_preserve_the_pointee_and_require_integer_indices() {
    let ptr = Type::ptr_to(Type::I32);
    for indices in [
        vec![],
        vec![index()],
        vec![literal(2)],
        vec![(Type::U64, None)],
    ] {
        verify_module(&gep(ptr.clone(), indices, ptr.clone())).unwrap();
    }
    for (base, indices, result) in [
        (Type::I32, vec![index()], ptr.clone()),
        (ptr.clone(), vec![(Type::Bool, None)], ptr.clone()),
        (ptr.clone(), vec![(Type::F64, None)], ptr.clone()),
        (ptr.clone(), vec![index()], Type::I32),
        (ptr.clone(), vec![index()], Type::ptr_to(Type::U8)),
        (
            Type::ptr_to(Type::Void),
            vec![index()],
            Type::ptr_to(Type::Void),
        ),
        (ptr.clone(), vec![index(), index()], ptr),
    ] {
        assert!(verify_module(&gep(base, indices, result)).is_err());
    }
}

#[test]
fn nested_aggregate_paths_derive_the_selected_field_type() {
    let record = Type::Struct(vec![Type::I32, Type::Array(Box::new(Type::Bool), 4)]);
    let base = Type::ptr_to(record.clone());
    verify_module(&gep(base.clone(), vec![index()], base.clone())).unwrap();
    verify_module(&gep(
        base.clone(),
        vec![literal(0), literal(0)],
        Type::ptr_to(Type::I32),
    ))
    .unwrap();
    verify_module(&gep(
        base.clone(),
        vec![literal(0), literal(1), index()],
        Type::ptr_to(Type::Bool),
    ))
    .unwrap();
    verify_module(&gep(
        Type::ptr_to(Type::Tuple(vec![Type::Bool, record])),
        vec![literal(0), literal(1), literal(0)],
        Type::ptr_to(Type::I32),
    ))
    .unwrap();
    assert!(verify_module(&gep(
        base,
        vec![literal(0), literal(1), index()],
        Type::ptr_to(Type::I32)
    ))
    .is_err());
}

#[test]
fn fields_require_representable_literal_ordinals_and_gep_never_loads_pointers() {
    let base = Type::ptr_to(Type::Struct(vec![Type::I32, Type::Bool]));
    for field in [
        index(),
        literal(-1),
        literal(2),
        (Type::U128, Some(ConstValue::U(u128::MAX))),
    ] {
        assert!(verify_module(&gep(
            base.clone(),
            vec![literal(0), field],
            Type::ptr_to(Type::I32)
        ))
        .is_err());
    }
    assert!(verify_module(&gep(
        Type::ptr_to(Type::ptr_to(Type::I32)),
        vec![literal(0), index()],
        Type::ptr_to(Type::I32)
    ))
    .is_err());
}
