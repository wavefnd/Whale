use ir::*;

#[test]
fn public_signature_and_global_policy_preserve_existing_diagnostics() {
    let valid = FunctionSignature::whale(vec![Type::U64], Type::Void);
    validate_signature_with_limits(&valid, IrLimits::default()).unwrap();
    let invalid = FunctionSignature::whale(vec![Type::Void], Type::Void);
    assert!(matches!(
        validate_signature_with_limits(&invalid, IrLimits::default()),
        Err(CallError::InvalidSignature(_))
    ));
    let mut global = GlobalStorage::internal(
        "value",
        Type::U8,
        GlobalStorageInit::Value(ConstValue::U(42)),
        false,
        1,
    );
    global.validate_with_limits(IrLimits::default()).unwrap();
    global.align = 3;
    assert_eq!(
        global.validate_with_limits(IrLimits::default()),
        Err(GlobalStorageError::InvalidAlignment)
    );
    global.align = 1;
    global.init = GlobalStorageInit::Value(ConstValue::U(256));
    assert_eq!(
        global.validate_with_limits(IrLimits::default()),
        Err(GlobalStorageError::InvalidInitializer)
    );
}

#[test]
fn wave_public_boundaries_apply_the_native_analysis_work_ceiling() {
    if !IR_WAVE_ENABLED {
        return;
    }
    let limits = IrLimits { max_analysis_work: 0, ..Default::default() };
    let signature = FunctionSignature::whale(vec![], Type::Void);
    assert!(matches!(
        validate_signature_with_limits(&signature, limits),
        Err(CallError::ResourceLimit(LimitError { resource: Resource::AnalysisWork, limit: 0 }))
    ));
    let global = GlobalStorage::internal(
        "value",
        Type::U8,
        GlobalStorageInit::Value(ConstValue::U(42)),
        false,
        1,
    );
    assert_eq!(
        global.validate_with_limits(limits),
        Err(GlobalStorageError::ResourceLimit(LimitError {
            resource: Resource::AnalysisWork,
            limit: 0
        }))
    );
}
