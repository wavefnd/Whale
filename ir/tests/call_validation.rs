use ir::*;

fn builder() -> ModuleBuilder {
    ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le())
}
fn external(m: &mut ModuleBuilder, name: &str, sig: FunctionSignature) -> FunctionId {
    m.declare_function(name, sig, Linkage::External, Some(name.into()))
        .unwrap()
}
fn signature() -> FunctionSignature {
    FunctionSignature::whale(vec![Type::I32], Type::I32)
}
fn module(indirect: bool) -> Module {
    let mut m = builder();
    let target = external(&mut m, "target", signature());
    let mut f = m.begin_function("caller", vec![], Type::Void);
    let x = f.const_i32(7);
    let callee = if indirect {
        Callee::Indirect(f.function_addr(target).unwrap())
    } else {
        Callee::Direct(target)
    };
    f.call(callee, vec![x]).unwrap(); // unused nonvoid result is retained
    f.ret(None);
    f.finish();
    m.finish()
}
fn call(m: &mut Module) -> &mut Instruction {
    m.functions[0].blocks[0]
        .instructions
        .iter_mut()
        .find(|i| matches!(i, Instruction::Call { .. }))
        .unwrap()
}
fn reason(m: &Module) -> CallError {
    match verify_module(m).unwrap_err() {
        VerifyError::Call { reason, .. } => reason,
        e => panic!("unexpected error: {e:?}"),
    }
}

#[test]
fn direct_and_indirect_calls_retain_signatures_results_and_printed_identity() {
    for indirect in [false, true] {
        let m = module(indirect);
        verify_module(&m).unwrap();
        let text = print_module(&m);
        assert!(text.contains(
            "declare @f0 \"target\": whale (i32) -> i32, linkage external, link_name \"target\""
        ));
        assert!(text.contains("= call whale i32"));
        assert!(text.contains(if indirect { "indirect %v" } else { "@f0(%v" }));
        let mut reordered = m.clone();
        reordered.declarations.reverse();
        verify_module(&reordered).unwrap();
    }
}

#[test]
fn verifier_rejects_unknown_callee_arity_type_return_and_convention() {
    for indirect in [false, true] {
        let mut m = module(indirect);
        if let Instruction::Call { args, .. } = call(&mut m) {
            args.clear();
        }
        assert_eq!(
            reason(&m),
            CallError::ArgumentCount {
                expected: 1,
                got: 0
            }
        );
        let mut m = module(indirect);
        let wrong = ValueId(99);
        m.functions[0].value_types.push((wrong, Type::Bool));
        m.functions[0].blocks[0].instructions.insert(
            0,
            Instruction::Const {
                dst: wrong,
                ty: Type::Bool,
                value: ConstValue::Bool(true),
            },
        );
        if let Instruction::Call { args, .. } = call(&mut m) {
            args[0] = wrong;
        }
        assert!(matches!(
            reason(&m),
            CallError::ArgumentType { index: 0, .. }
        ));
        let mut m = module(indirect);
        let result = if let Instruction::Call { ret_ty, dst, .. } = call(&mut m) {
            *ret_ty = Type::Bool;
            dst.unwrap()
        } else {
            unreachable!()
        };
        m.functions[0]
            .value_types
            .iter_mut()
            .find(|(v, _)| *v == result)
            .unwrap()
            .1 = Type::Bool;
        assert!(matches!(reason(&m), CallError::ReturnType { .. }));
        let mut m = module(indirect);
        if let Instruction::Call { convention, .. } = call(&mut m) {
            *convention = CallingConvention::SysV64;
        }
        assert_eq!(reason(&m), CallError::ConventionMismatch);
    }
    let mut m = module(false);
    if let Instruction::Call { callee, .. } = call(&mut m) {
        *callee = Callee::Direct(FunctionId(900));
    }
    assert_eq!(reason(&m), CallError::UnknownFunction(FunctionId(900)));
    let mut m = module(false);
    if let Instruction::Call { callee, .. } = call(&mut m) {
        *callee = Callee::Indirect(ValueId(0));
    }
    assert_eq!(reason(&m), CallError::NotFunctionPointer(ValueId(0)));
}

#[test]
fn void_and_nonvoid_destination_presence_is_mandatory() {
    let mut m = module(false);
    let result = if let Instruction::Call { dst, .. } = call(&mut m) {
        dst.take().unwrap()
    } else {
        unreachable!()
    };
    m.functions[0].value_types.retain(|(v, _)| *v != result);
    assert_eq!(reason(&m), CallError::ResultPresence);
    let mut b = builder();
    let id = external(&mut b, "void", FunctionSignature::whale(vec![], Type::Void));
    let mut f = b.begin_function("caller", vec![], Type::Void);
    assert_eq!(f.call(Callee::Direct(id), vec![]).unwrap(), None);
    f.ret(None);
    f.finish();
    let mut m = b.finish();
    verify_module(&m).unwrap();
    if let Instruction::Call { dst, .. } = call(&mut m) {
        *dst = Some(ValueId(99));
    }
    m.functions[0].value_types.push((ValueId(99), Type::Void));
    assert_eq!(reason(&m), CallError::ResultPresence);
}

#[test]
fn checked_builders_do_not_emit_partial_instructions_on_error() {
    let mut b = builder();
    let id = external(&mut b, "callee", signature());
    let mut f = b.begin_function("caller", vec![], Type::Void);
    let x = f.const_bool(true);
    assert!(matches!(
        f.call(Callee::Direct(id), vec![x]),
        Err(CallError::ArgumentType { .. })
    ));
    assert!(matches!(
        f.call(Callee::Direct(id), vec![]),
        Err(CallError::ArgumentCount { .. })
    ));
    assert!(matches!(
        f.call(Callee::Indirect(x), vec![]),
        Err(CallError::NotFunctionPointer(_))
    ));
    assert!(matches!(
        f.function_addr(FunctionId(99)),
        Err(CallError::UnknownFunction(_))
    ));
    f.ret(None);
    f.finish();
    let m = b.finish();
    verify_module(&m).unwrap();
    assert_eq!(m.functions[0].blocks[0].instructions.len(), 1);
}

#[test]
fn declaration_definition_identity_and_explicit_link_names_are_checked() {
    let mut b = builder();
    let id = b
        .declare_function("same", signature(), Linkage::Internal, None)
        .unwrap();
    assert_eq!(
        id,
        b.declare_function("same", signature(), Linkage::Internal, None)
            .unwrap()
    );
    assert!(matches!(
        b.declare_function(
            "same",
            FunctionSignature::whale(vec![], Type::Void),
            Linkage::Internal,
            None
        ),
        Err(CallError::ConflictingDeclaration(_))
    ));
    assert!(b
        .declare_function("foreign", signature(), Linkage::External, None)
        .is_err());
    assert!(b
        .declare_function("foreign", signature(), Linkage::External, Some("".into()))
        .is_err());
    external(&mut b, "symbol", signature());
    assert!(matches!(
        b.declare_function(
            "another",
            signature(),
            Linkage::External,
            Some("symbol".into())
        ),
        Err(CallError::LinkNameCollision(_))
    ));
    assert!(b.begin_declared_function(id, vec![]).is_err());
    let mut f = b.begin_declared_function(id, vec!["x".into()]).unwrap();
    let x = f.param_value(0);
    f.ret(Some(x));
    f.finish();
    assert!(matches!(
        b.begin_declared_function(id, vec!["x".into()]),
        Err(CallError::DuplicateDefinition(_))
    ));
    let m = b.finish();
    verify_module(&m).unwrap();
    let mut broken = m.clone();
    broken.functions[0].id = FunctionId(99);
    assert!(verify_module(&broken).is_err());
    let mut broken = m.clone();
    broken.declarations[0].signature.ret = Type::Bool;
    assert!(matches!(reason(&broken), CallError::DefinitionMismatch(_)));
    let mut broken = m.clone();
    broken.functions.clear();
    assert!(matches!(reason(&broken), CallError::MissingDefinition(_)));
    let mut broken = m.clone();
    broken.declarations.push(broken.declarations[0].clone());
    assert!(verify_module(&broken).is_err());
    let mut exported = m.clone();
    exported.declarations[0].linkage = Linkage::External;
    exported.declarations[0].link_name = Some("exported_same".into());
    verify_module(&exported).unwrap();
    let mut broken = m;
    broken.declarations[0].link_name = Some("hidden".into());
    assert!(verify_module(&broken).is_err());
}

#[test]
fn variadic_void_parameters_and_foreign_aggregates_are_explicit_errors() {
    for sig in [
        FunctionSignature {
            variadic: true,
            ..signature()
        },
        FunctionSignature::whale(vec![Type::Void], Type::Void),
        FunctionSignature {
            convention: CallingConvention::SysV64,
            params: vec![Type::Struct(vec![Type::I32])],
            ..signature()
        },
    ] {
        assert!(matches!(
            builder().declare_function("bad", sig.clone(), Linkage::External, Some("bad".into())),
            Err(CallError::InvalidSignature(_))
        ));
        let mut m = module(false);
        m.declarations[0].signature = sig;
        assert!(matches!(reason(&m), CallError::InvalidSignature(_)));
    }
}

#[test]
fn function_pointer_storage_parameters_returns_select_and_null_preserve_types() {
    let mut b = builder();
    let target = external(&mut b, "target", signature());
    let ty = Type::FnPtr(Box::new(signature()));
    let mut f = b.begin_function("pass", vec![("callback".into(), ty.clone())], ty.clone());
    let value = f.param_value(0);
    f.ret(Some(value));
    f.finish();
    let pass = b.function_id("pass").unwrap();
    let mut f = b.begin_function("caller", vec![], Type::Void);
    let pointer = f.function_addr(target).unwrap();
    let pointer = f
        .call(Callee::Direct(pass), vec![pointer])
        .unwrap()
        .unwrap();
    let slot = f.alloca(ty.clone(), 8);
    f.store(ty.clone(), pointer, slot, 8);
    let loaded = f.load(ty, slot, 8);
    let x = f.const_i32(1);
    f.call(Callee::Indirect(loaded), vec![x]).unwrap();
    let null = f.null_function(signature()).unwrap();
    // A well-typed null call is valid IR with a defined runtime trap, not UB.
    f.call(Callee::Indirect(null), vec![x]).unwrap();
    f.ret(None);
    f.finish();
    let m = b.finish();
    verify_module(&m).unwrap();
    assert!(print_module(&m).contains("null_function"));
    assert_eq!(
        allocation_align(
            &Type::FnPtr(Box::new(signature())),
            Target::lookup("x86_64-whale-linux").unwrap()
        )
        .unwrap(),
        8
    );
}

#[test]
fn pointer_signature_and_indirect_callee_dominance_cannot_be_forged() {
    let mut m = module(true);
    m.functions[0].blocks[0].instructions.swap(1, 2);
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::NonDominatingValue { .. })
    ));
    let mut m = module(true);
    if let Instruction::Call { callee, .. } = call(&mut m) {
        *callee = Callee::Indirect(ValueId(99));
    }
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::UseOfUndefinedValue { .. })
    ));
    let mut m = module(true);
    let altered = FunctionSignature::whale(vec![Type::Bool], Type::I32);
    if let Instruction::FunctionAddr { signature, .. } =
        &mut m.functions[0].blocks[0].instructions[1]
    {
        *signature = altered.clone();
    }
    m.functions[0].value_types[1].1 = Type::FnPtr(Box::new(altered));
    assert_eq!(reason(&m), CallError::AddressTypeMismatch);
    for lies in [false, true] {
        let mut m = module(true);
        let pointer_ty = m.functions[0].value_types[1].1.clone();
        m.functions[0].value_types.push((ValueId(99), Type::U64));
        m.functions[0].blocks[0]
            .instructions
            .push(Instruction::Cast {
                dst: ValueId(99),
                op: CastOp::PtrToInt,
                src_ty: if lies { Type::I64 } else { pointer_ty },
                src: ValueId(1),
                dst_ty: Type::U64,
            });
        assert_eq!(reason(&m), CallError::ForbiddenFunctionPointerCast);
    }
}

#[test]
fn phi_select_and_aggregate_extract_preserve_full_pointer_signature() {
    let mut b = builder();
    let id = external(&mut b, "target", signature());
    let ty = Type::FnPtr(Box::new(signature()));
    let mut f = b.begin_function("choose", vec![("cond".into(), Type::Bool)], Type::Void);
    let cond = f.param_value(0);
    let pointer = f.function_addr(id).unwrap();
    let null = f.null_function(signature()).unwrap();
    let left = f.create_block("left");
    let right = f.create_block("right");
    let merge = f.create_block("merge");
    f.cbr(cond, left, right);
    f.set_insert_point(left);
    f.br(merge);
    f.set_insert_point(right);
    f.br(merge);
    f.set_insert_point(merge);
    let selected = f.undef(ty.clone());
    let x = f.const_i32(1);
    f.call(Callee::Indirect(selected), vec![x]).unwrap();
    f.ret(None);
    f.finish();
    let mut m = b.finish();
    m.functions[0].blocks[3].instructions[0] = Instruction::Phi {
        dst: selected,
        ty: ty.clone(),
        incomings: vec![(pointer, left), (null, right)],
    };
    verify_module(&m).unwrap();
    m.functions[0].blocks[3].instructions[0] = Instruction::Select {
        dst: selected,
        ty: ty.clone(),
        cond,
        on_true: pointer,
        on_false: null,
    };
    verify_module(&m).unwrap();
    let mut b = builder();
    let tuple_ty = Type::Tuple(vec![ty.clone()]);
    let mut f = b.begin_function(
        "unpack",
        vec![("tuple".into(), tuple_ty.clone())],
        Type::Void,
    );
    let tuple = f.param_value(0);
    let pointer = f.extract(tuple_ty, ty.clone(), tuple, 0);
    let x = f.const_i32(1);
    f.call(Callee::Indirect(pointer), vec![x]).unwrap();
    f.ret(None);
    f.finish();
    let mut m = b.finish();
    verify_module(&m).unwrap();
    if let Instruction::Extract { index, .. } = &mut m.functions[0].blocks[0].instructions[0] {
        *index = 1;
    }
    assert!(verify_module(&m).is_err());
    if let Instruction::Extract { index, dst_ty, .. } =
        &mut m.functions[0].blocks[0].instructions[0]
    {
        *index = 0;
        *dst_ty = Type::U64;
    }
    m.functions[0].value_types[1].1 = Type::U64;
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::InvalidInstructionType {
            operation: "extract",
            ..
        })
    ));
}

#[test]
fn unused_malformed_pointer_signature_is_rejected() {
    let mut b = builder();
    let mut f = b.begin_function("bad", vec![], Type::Void);
    f.undef(Type::FnPtr(Box::new(FunctionSignature {
        variadic: true,
        ..signature()
    })));
    f.ret(None);
    f.finish();
    assert!(matches!(
        reason(&b.finish()),
        CallError::InvalidSignature(_)
    ));
}

#[test]
fn checked_address_builder_rejects_an_invalid_legacy_signature() {
    let mut b = builder();
    let mut invalid = b.begin_function("bad", vec![("void".into(), Type::Void)], Type::Void);
    invalid.ret(None);
    invalid.finish();
    let id = b.function_id("bad").unwrap();
    let mut caller = b.begin_function("caller", vec![], Type::Void);
    assert!(matches!(
        caller.function_addr(id),
        Err(CallError::InvalidSignature(_))
    ));
}
