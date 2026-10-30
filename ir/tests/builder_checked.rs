use ir::*;

#[cfg(whale_wave_ir)]
#[test]
fn complete_body_resource_failures_preserve_values_and_terminator_state() {
    let mut module = ModuleBuilder::with_limits(
        "x86_64-whale-linux",
        DataLayout::default_64bit_le(),
        IrLimits { max_input_bytes: 16, ..IrLimits::default() },
    )
    .unwrap();
    let mut function = module.begin_function("f", vec![], Type::Void);
    let condition = function.try_const_bool(true).unwrap();
    let expected =
        BuilderError::ResourceLimit(LimitError { resource: Resource::InputBytes, limit: 16 });
    assert_eq!(function.try_trap("x".repeat(17)), Err(expected.clone()));
    assert_eq!(function.try_trap_if(condition, "x".repeat(17)), Err(expected.clone()));
    assert_eq!(
        function.try_const_decl(
            "x".repeat(17),
            ConstExpr::literal(Type::I32, ConstValue::I(4)),
            ConstValue::I(4)
        ),
        Err(expected)
    );
    assert_eq!(function.try_const_i32(42), Ok(ValueId(condition.0 + 1)));
    function.try_ret(None).unwrap();
    function.finish();
    let module = module.finish();
    verify_module(&module).unwrap();
    assert_eq!(module.functions[0].blocks[0].instructions.len(), 2);
}

#[cfg(whale_wave_ir)]
#[test]
fn rejected_constant_body_expression_is_discarded_iteratively() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut expression = ConstExpr::literal(Type::I32, ConstValue::I(0));
            for _ in 0..5000 {
                expression =
                    ConstExpr { ty: Type::I32, kind: ConstExprKind::Not(Box::new(expression)) };
            }
            let mut module =
                ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
            let mut function = module.begin_function("f", vec![], Type::Void);
            assert!(matches!(
                function.try_const_decl("deep", expression, ConstValue::I(0)),
                Err(BuilderError::ResourceLimit(LimitError {
                    resource: Resource::ConstantDepth,
                    ..
                }))
            ));
            assert_eq!(function.try_const_i32(42), Ok(ValueId(0)));
            function.try_ret(None).unwrap();
            function.finish();
            verify_module(&module.finish()).unwrap();
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn checked_entry_allocation_cannot_mutate_a_terminated_entry() {
    let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = mb.begin_function("entry_allocation", vec![], Type::Void);
    let entry = f.entry_block();
    let next = f.create_block("next");
    f.try_br(next).unwrap();
    f.try_set_insert_point(next).unwrap();
    assert!(
        matches!(f.try_alloca_in_entry(Type::I32,4),Err(BuilderError::TerminatedBlock(id)) if id==entry)
    );
    f.try_ret(None).unwrap();
    f.finish();
    let m = mb.finish();
    assert!(m.functions[0].blocks.iter().find(|b| b.id == entry).unwrap().instructions.is_empty());
    verify_module(&m).unwrap();
}

#[test]
fn checked_ids_and_failed_insertions_preserve_the_program() {
    let target = Target::X86_64WhaleLinux;
    let mut module = ModuleBuilder::new(target.name(), target.data_layout());
    let mut f = module.begin_function("checked", vec![("arg".into(), Type::I32)], Type::I32);
    let entry = f.entry_block();
    assert_eq!(f.try_param_value(1), Err(BuilderError::UnknownParameter { index: 1, count: 1 }));
    assert_eq!(f.try_set_insert_point(BlockId(999)), Err(BuilderError::UnknownBlock(BlockId(999))));
    let arg = f.try_param_value(0).unwrap();
    assert_eq!(
        f.try_add(Type::I32, arg, ValueId(999)),
        Err(BuilderError::UnknownValue(ValueId(999)))
    );
    assert_eq!(f.try_br(BlockId(999)), Err(BuilderError::UnknownBlock(BlockId(999))));
    let result = f.try_add(Type::I32, arg, arg).unwrap();
    f.try_ret(Some(result)).unwrap();
    assert_eq!(f.try_ret(Some(arg)), Err(BuilderError::TerminatedBlock(entry)));
    assert_eq!(f.try_const_i32(2), Err(BuilderError::TerminatedBlock(entry)));
    assert_eq!(f.try_store(Type::I32, arg, arg, 4), Err(BuilderError::TerminatedBlock(entry)));
    assert_eq!(f.try_trap("replace"), Err(BuilderError::TerminatedBlock(entry)));
    f.finish();
    let module = module.finish();
    verify_module(&module).unwrap();
    let f = &module.functions[0];
    assert_eq!(f.value_types.len(), 2, "failed operations cannot consume value IDs");
    assert_eq!(f.blocks[0].instructions.len(), 1);
    assert!(
        matches!(f.blocks[0].terminator, Some(Terminator::Ret { value: Some(v), .. }) if v == result)
    );
}

#[test]
fn checked_calls_report_terminated_blocks_and_can_build_neighbors() {
    let target = Target::X86_64WhaleLinux;
    let mut m = ModuleBuilder::new(target.name(), target.data_layout());
    let signature = FunctionSignature::whale(vec![], Type::Void);
    let id = m
        .declare_function("external", signature.clone(), Linkage::External, Some("external".into()))
        .unwrap();
    let mut f = m.begin_function("caller", vec![], Type::Void);
    f.call(Callee::Direct(id), vec![]).unwrap();
    let entry = f.entry_block();
    f.try_ret(None).unwrap();
    assert_eq!(
        f.call(Callee::Direct(id), vec![]),
        Err(CallError::Builder(BuilderError::TerminatedBlock(entry)))
    );
    assert_eq!(
        f.null_function(signature),
        Err(CallError::Builder(BuilderError::TerminatedBlock(entry)))
    );
    let next = f.create_block("unreachable");
    f.try_set_insert_point(next).unwrap();
    f.try_trap("unreachable").unwrap();
    f.finish();
    verify_module(&m.finish()).unwrap();
}

#[test]
fn function_and_storage_link_collisions_fail_atomically_in_both_orders() {
    fn storage(link: &str) -> GlobalStorage {
        let mut g = GlobalStorage::internal(
            "data",
            Type::U32,
            GlobalStorageInit::Value(ConstValue::U(0)),
            false,
            4,
        );
        g.linkage = Linkage::External;
        g.link_name = Some(link.into());
        g
    }

    fn declare(
        m: &mut ModuleBuilder,
        name: &str,
        link: &str,
        with_limits: bool,
    ) -> Result<FunctionId, CallError> {
        let signature = FunctionSignature::whale(vec![], Type::Void);
        if with_limits {
            m.declare_function_with_limits(
                name,
                signature,
                Linkage::External,
                Some(link.into()),
                IrLimits::default(),
            )
        } else {
            m.declare_function(name, signature, Linkage::External, Some(link.into()))
        }
    }
    for with_limits in [false, true] {
        let mut m = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        assert_eq!(m.try_add_global_storage(storage("exported")), Ok(GlobalId(0)));
        assert_eq!(
            declare(&mut m, "function", "exported", with_limits),
            Err(CallError::LinkNameCollision("exported".into()))
        );
        assert_eq!(declare(&mut m, "function", "different", with_limits), Ok(FunctionId(0)));
        assert_eq!(declare(&mut m, "function", "different", with_limits), Ok(FunctionId(0)));
        assert_eq!(declare(&mut m, "other", "another", with_limits), Ok(FunctionId(1)));
        let m = m.finish();
        assert_eq!(m.declarations.len(), 2);
        assert_eq!(m.global_storage.len(), 1);
        verify_module(&m).unwrap();

        let mut m = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        assert_eq!(declare(&mut m, "function", "exported", with_limits), Ok(FunctionId(0)));
        assert_eq!(
            m.try_add_global_storage(storage("exported")),
            Err(GlobalStorageError::LinkNameCollision)
        );
        assert_eq!(m.try_add_global_storage(storage("different")), Ok(GlobalId(0)));
        let m = m.finish();
        assert_eq!(m.declarations.len(), 1);
        assert_eq!(m.global_storage.len(), 1);
        verify_module(&m).unwrap();
    }
}

#[test]
fn legacy_entry_hoisting_preserves_a_preexisting_terminator() {
    let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = mb.begin_function("hoist_after_branch", vec![], Type::Void);
    let entry = f.entry_block();
    let next = f.create_block("body");
    f.br(next);
    f.set_insert_point(next);
    let slot = f.alloca_in_entry(Type::I32, 4);
    assert_eq!(f.try_alloca_in_entry(Type::I32, 4), Err(BuilderError::TerminatedBlock(entry)));
    let value = f.const_i32(7);
    f.store(Type::I32, value, slot, 4);
    f.ret(None);
    f.finish();
    let module = mb.finish();
    assert!(matches!(
        module.functions[0].blocks[0].instructions.as_slice(),
        [Instruction::Alloca { .. }]
    ));
    assert!(
        matches!(module.functions[0].blocks[0].terminator, Some(Terminator::Br { target }) if target == next)
    );
    verify_module(&module).unwrap();
}

#[test]
fn deep_rejected_builder_type_is_discarded_before_clone_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut ty = Type::I32;
            for _ in 0..50_000 {
                ty = Type::ptr_to(ty);
            }
            let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
            let mut f = mb.begin_function("bounded_type", vec![], Type::I32);
            assert!(matches!(
                f.try_const_int(ty, 1),
                Err(BuilderError::ResourceLimit(LimitError { resource: Resource::TypeDepth, .. }))
            ));
            let value = f.try_const_i32(9).unwrap();
            assert_eq!(value, ValueId(0), "rejected type cannot consume a value identity");
            f.ret(Some(value));
            f.finish();
            verify_module(&mb.finish()).unwrap();
        })
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(whale_wave_ir)]
#[test]
fn checked_function_admission_reserves_all_parameter_storage_before_publication() {
    let mut mb = ModuleBuilder::with_limits(
        "x86_64-whale-linux",
        DataLayout::default_64bit_le(),
        IrLimits { max_nodes: 10, ..IrLimits::default() },
    )
    .unwrap();
    let signature = FunctionSignature::whale(vec![Type::I32; 5], Type::Void);
    let id = mb.declare_function("f", signature, Linkage::Internal, None).unwrap();
    assert!(matches!(
        mb.begin_declared_function(id, vec![String::new(); 5]),
        Err(CallError::ResourceLimit(LimitError { resource: Resource::Nodes, .. }))
    ));
    let module = mb.finish();
    assert!(module.functions.is_empty());
    assert_eq!(module.declarations.len(), 1);
}
