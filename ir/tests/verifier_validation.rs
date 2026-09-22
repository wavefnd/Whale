use ir::{
    BlockId, ConstValue, DataLayout, Module, ModuleBuilder, Terminator, Type, ValueId, VerifyError,
};

fn returning_bool(ret_ty: Type) -> Module {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut function = builder.begin_function("f", vec![], ret_ty);
    let value = function.const_bool(true);
    function.ret(Some(value));
    function.finish();
    builder.finish()
}

#[test]
fn return_checks_the_value_type_not_only_the_annotation() {
    let module = returning_bool(Type::I64);
    assert!(matches!(
        ir::verify_module(&module),
        Err(VerifyError::RetTypeMismatch {
            expected: Type::I64,
            got: Some(Type::I1),
            ..
        })
    ));
    assert!(ir::verify_module(&returning_bool(Type::I1)).is_ok());
}

#[test]
fn void_returns_cannot_carry_a_value_and_nonvoid_returns_require_one() {
    assert!(matches!(
        ir::verify_module(&returning_bool(Type::Void)),
        Err(VerifyError::RetTypeMismatch {
            expected: Type::Void,
            ..
        })
    ));
    let mut module = returning_bool(Type::I1);
    module.functions[0].blocks[0].terminator = Some(Terminator::Ret {
        ty: Type::I1,
        value: None,
    });
    assert!(matches!(
        ir::verify_module(&module),
        Err(VerifyError::RetTypeMismatch { got: None, .. })
    ));
    module.functions[0].ret_ty = Type::Void;
    module.functions[0].blocks[0].terminator = Some(Terminator::Ret {
        ty: Type::Void,
        value: None,
    });
    assert!(ir::verify_module(&module).is_ok());
}

#[test]
fn returned_values_require_type_metadata_and_a_definition() {
    let mut module = returning_bool(Type::I1);
    module.functions[0].value_types.clear();
    assert!(matches!(
        ir::verify_module(&module),
        Err(VerifyError::MissingValueType {
            value: ValueId(0),
            ..
        })
    ));
    module.functions[0].blocks[0].terminator = Some(Terminator::Ret {
        ty: Type::I1,
        value: Some(ValueId(999)),
    });
    assert!(matches!(
        ir::verify_module(&module),
        Err(VerifyError::UseOfUndefinedValue {
            value: ValueId(999),
            ..
        })
    ));
}

#[test]
fn unused_void_parameters_are_rejected() {
    for params in [
        vec![("bad".into(), Type::Void)],
        vec![("ok".into(), Type::I32), ("bad".into(), Type::Void)],
    ] {
        let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        let mut function = builder.begin_function("invalid", params, Type::Void);
        function.ret(None);
        function.finish();

        assert!(matches!(
            ir::verify_module(&builder.finish()),
            Err(VerifyError::VoidParameter { func, param })
                if func == "invalid" && param == "bad"
        ));
    }
}

#[test]
fn zero_argument_void_functions_are_valid() {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut function = builder.begin_function("noop", vec![], Type::Void);
    function.ret(None);
    function.finish();

    assert!(ir::verify_module(&builder.finish()).is_ok());
}

#[test]
fn integer_and_pointer_parameters_are_valid() {
    for ty in [Type::I32, Type::ptr_to(Type::I32), Type::ptr_to(Type::Void)] {
        let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        let mut function =
            builder.begin_function("identity", vec![("value".into(), ty.clone())], ty);
        function.ret(Some(function.param_value(0)));
        function.finish();

        assert!(ir::verify_module(&builder.finish()).is_ok());
    }
}

#[test]
fn valid_parameter_returns_and_wrong_terminator_annotations() {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut function = builder.begin_function("identity", vec![("x".into(), Type::I32)], Type::I32);
    function.ret(Some(function.param_value(0)));
    function.finish();
    let mut module = builder.finish();
    assert!(ir::verify_module(&module).is_ok());
    module.functions[0].blocks[0].terminator = Some(Terminator::Ret {
        ty: Type::I64,
        value: Some(ValueId(0)),
    });
    assert!(matches!(
        ir::verify_module(&module),
        Err(VerifyError::RetTypeMismatch { .. })
    ));
}

#[test]
fn entry_must_exist_in_a_nonempty_function_definition() {
    let mut module = returning_bool(Type::I1);
    module.functions[0].entry = BlockId(999);
    assert!(matches!(
        ir::verify_module(&module),
        Err(VerifyError::InvalidEntryBlock {
            entry: BlockId(999),
            ..
        })
    ));
    module.functions[0].blocks.clear();
    assert!(matches!(
        ir::verify_module(&module),
        Err(VerifyError::InvalidEntryBlock { .. })
    ));
}

#[test]
fn every_branch_and_switch_destination_must_exist() {
    let mut module = returning_bool(Type::I1);
    let here = module.functions[0].entry;
    let missing = BlockId(999);
    for terminator in [
        Terminator::Br { target: missing },
        Terminator::CBr {
            cond: ValueId(0),
            then_bb: missing,
            else_bb: here,
        },
        Terminator::CBr {
            cond: ValueId(0),
            then_bb: here,
            else_bb: missing,
        },
        Terminator::Switch {
            ty: Type::I1,
            value: ValueId(0),
            default_bb: missing,
            cases: vec![],
        },
        Terminator::Switch {
            ty: Type::I1,
            value: ValueId(0),
            default_bb: here,
            cases: vec![(ConstValue::Bool(true), missing)],
        },
    ] {
        module.functions[0].blocks[0].terminator = Some(terminator);
        assert!(matches!(
            ir::verify_module(&module),
            Err(VerifyError::InvalidBranchTarget {
                target: BlockId(999),
                ..
            })
        ));
    }
    module.functions[0].blocks[0].terminator = Some(Terminator::Switch {
        ty: Type::I1,
        value: ValueId(0),
        default_bb: here,
        cases: vec![(ConstValue::Bool(true), here)],
    });
    assert!(ir::verify_module(&module).is_ok());
}

#[test]
fn a_target_in_another_function_is_not_a_local_destination() {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut first = builder.begin_function("first", vec![], Type::Void);
    first.ret(None);
    first.finish();
    let mut second = builder.begin_function("second", vec![], Type::Void);
    let foreign = second.entry_block();
    second.ret(None);
    second.finish();
    let mut module = builder.finish();
    module.functions[0].blocks[0].terminator = Some(Terminator::Br { target: foreign });
    assert!(ir::verify_module(&module).is_err());
    module.functions[0].entry = foreign;
    assert!(ir::verify_module(&module).is_err());
}

#[test]
fn valid_forward_branches_and_backedges_pass() {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut function = builder.begin_function("loop", vec![], Type::Void);
    let entry = function.entry_block();
    let body = function.create_block("body");
    let exit = function.create_block("exit");
    let cond = function.const_bool(true);
    function.cbr(cond, body, exit);
    function.set_insert_point(body);
    function.br(entry);
    function.set_insert_point(exit);
    function.ret(None);
    function.finish();
    assert!(ir::verify_module(&builder.finish()).is_ok());
}
