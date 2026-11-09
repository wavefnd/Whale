// SPDX-License-Identifier: MPL-2.0
use ir::pass::*;

use ir::*;

#[cfg(whale_wave_ir)]
fn integer_module(ty: Type, a: ConstValue, b: ConstValue, op: BinOp) -> Module {
    let mut module = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut function = module.begin_function("fold", vec![], ty.clone());
    let literal = |f: &mut FunctionBuilder<'_>, value: ConstValue| match value {
        ConstValue::I(value) => f.const_int(ty.clone(), value),
        ConstValue::U(value) => f.const_uint(ty.clone(), value),
        _ => unreachable!(),
    };
    let a = literal(&mut function, a);
    let b = literal(&mut function, b);
    let result = function.bin(op, ty, a, b);
    function.ret(Some(result));
    function.finish();
    module.finish()
}

#[cfg(whale_wave_ir)]
#[test]
fn all_integer_widths_fold_modular_arithmetic_without_changing_ids_or_sources() {
    let cases = [
        (Type::I1, ConstValue::I(-1), ConstValue::I(-1), ConstValue::I(0)),
        (Type::U1, ConstValue::U(1), ConstValue::U(1), ConstValue::U(0)),
        (Type::I8, ConstValue::I(127), ConstValue::I(1), ConstValue::I(-128)),
        (Type::I16, ConstValue::I(32767), ConstValue::I(1), ConstValue::I(-32768)),
        (
            Type::I32,
            ConstValue::I(i32::MAX.into()),
            ConstValue::I(1),
            ConstValue::I(i32::MIN.into()),
        ),
        (
            Type::I64,
            ConstValue::I(i64::MAX.into()),
            ConstValue::I(1),
            ConstValue::I(i64::MIN.into()),
        ),
        (Type::I128, ConstValue::I(i128::MAX), ConstValue::I(1), ConstValue::I(i128::MIN)),
        (Type::U8, ConstValue::U(255), ConstValue::U(1), ConstValue::U(0)),
        (Type::U16, ConstValue::U(65535), ConstValue::U(1), ConstValue::U(0)),
        (Type::U32, ConstValue::U(u32::MAX.into()), ConstValue::U(1), ConstValue::U(0)),
        (Type::U64, ConstValue::U(u64::MAX.into()), ConstValue::U(1), ConstValue::U(0)),
        (Type::U128, ConstValue::U(u128::MAX), ConstValue::U(1), ConstValue::U(0)),
    ];
    for (ty, a, b, expected) in cases {
        let mut module = integer_module(ty, a, b, BinOp::Add);
        module.sources.originals.push(OriginalNode { id: 118, span: None });
        module.sources.mappings.push(SourceMapping {
            target: SourceTarget::Instruction {
                function: FunctionId(0),
                block: BlockId(0),
                index: 2,
            },
            original: 118,
        });
        let original = module.clone();
        let mut state = AnalysisState::default();
        assert!(
            run_pass(&mut module, &mut IntegerConstantFold::default(), &mut state).unwrap().changed
        );
        assert_eq!(module.functions[0].value_types, original.functions[0].value_types);
        assert_eq!(module.sources, original.sources);
        assert_eq!(module.functions[0].blocks[0].instructions.len(), 3);
        assert!(
            matches!(&module.functions[0].blocks[0].instructions[2], Instruction::Const { dst: ValueId(2), value, .. } if value == &expected)
        );
        assert_eq!(interpret(&original, FunctionId(0), &[]).unwrap().value, Some(expected.clone()));
        assert_eq!(interpret(&module, FunctionId(0), &[]).unwrap().value, Some(expected));
        let before_second = module.clone();
        assert!(
            !run_pass(&mut module, &mut IntegerConstantFold::default(), &mut state)
                .unwrap()
                .changed
        );
        assert_eq!(module, before_second);
        assert_eq!(state.epoch(AnalysisKind::Values), 1);
    }
}

#[cfg(whale_wave_ir)]
#[test]
fn trapping_arithmetic_and_memory_calls_remain_at_their_original_ordinals() {
    for op in [BinOp::UDiv, BinOp::SDiv, BinOp::URem, BinOp::SRem] {
        let mut module = if matches!(op, BinOp::UDiv | BinOp::URem) {
            integer_module(Type::U128, ConstValue::U(u128::MAX), ConstValue::U(0), op)
        } else {
            integer_module(Type::I128, ConstValue::I(i128::MIN), ConstValue::I(0), op)
        };
        let before = module.clone();
        assert!(
            !run_pass(
                &mut module,
                &mut IntegerConstantFold::default(),
                &mut AnalysisState::default()
            )
            .unwrap()
            .changed
        );
        assert_eq!(module, before);
        let before_error = interpret(&before, FunctionId(0), &[]).unwrap_err();
        let after_error = interpret(&module, FunctionId(0), &[]).unwrap_err();
        assert_eq!(format!("{before_error:?}"), format!("{after_error:?}"));
    }
    let mut module = parse_module(include_str!("fixtures/runtime-diagnostics-v7.wir")).unwrap();
    let before = module.clone();
    assert!(
        !run_pass(&mut module, &mut IntegerConstantFold::default(), &mut AnalysisState::default())
            .unwrap()
            .changed
    );
    assert_eq!(module, before);
}

#[cfg(whale_wave_ir)]
#[test]
fn propagation_handles_comparison_select_and_physically_reordered_sparse_scoped_ids() {
    let text = r#"module { format_version 8 semantics_version 1 target "x86_64-whale-linux"
        datalayout {ptr=64,endian=little}
        declare @f4294967295 "test": whale () -> i128, linkage internal
        declare @f7 "other": whale () -> u8, linkage internal
        fn @f4294967295 "test"() -> i128, entry %b1 {
        %b2 "exit":
            %v4294967295: i128 = add i128 %v4000000000, %v3000000000
            %v12: bool = icmp eq i128 %v4294967295, %v4000000000
            %v13: i128 = select bool %v12, i128 %v3000000000, i128 %v4294967295
            ret i128 %v13
        %b1 "entry":
            %v4000000000: i128 = const i128 42
            %v3000000000: i128 = const i128 0
            br label %b2
        }
        fn @f7 "other"() -> u8, entry %b0 {
        %b0 "entry":
            %v4000000000: u8 = const u8 7
            ret u8 %v4000000000
        }}"#;
    let mut module = parse_module(text).unwrap();
    let original = module.clone();
    let mut pass = IntegerConstantFold::default();
    assert!(run_pass(&mut module, &mut pass, &mut AnalysisState::default()).unwrap().changed);
    for instruction in &module.functions[0].blocks[0].instructions {
        assert!(matches!(instruction, Instruction::Const { .. }));
    }
    assert_eq!(module.functions[1], original.functions[1]);
    assert_eq!(
        interpret(&original, FunctionId(u32::MAX), &[]).unwrap().value,
        Some(ConstValue::I(0))
    );
    assert_eq!(
        interpret(&module, FunctionId(u32::MAX), &[]).unwrap().value,
        Some(ConstValue::I(0))
    );
}

#[cfg(whale_wave_ir)]
#[test]
fn named_constant_declarations_and_global_expressions_are_preserved() {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    builder.add_global_const(
        "global",
        ConstExpr::literal(Type::I32, ConstValue::I(40)),
        ConstValue::I(40),
        4,
    );
    let mut function = builder.begin_function("entry", vec![], Type::I32);
    let a = function.const_decl(
        "answer",
        ConstExpr::literal(Type::I32, ConstValue::I(40)),
        ConstValue::I(40),
    );
    let b = function.const_i32(2);
    let result = function.add(Type::I32, a, b);
    function.ret(Some(result));
    function.finish();
    let mut module = builder.finish();
    let before = module.clone();
    assert!(
        run_pass(&mut module, &mut IntegerConstantFold::default(), &mut AnalysisState::default())
            .unwrap()
            .changed
    );
    assert_eq!(module.globals, before.globals);
    assert_eq!(
        module.functions[0].blocks[0].instructions[0],
        before.functions[0].blocks[0].instructions[0]
    );
    assert_eq!(interpret(&module, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(42)));
}

#[cfg(whale_wave_ir)]
#[test]
fn failed_fold_work_budget_preserves_module_and_cache() {
    let mut module = integer_module(Type::U32, ConstValue::U(40), ConstValue::U(2), BinOp::Add);
    let before = module.clone();
    let mut state = AnalysisState::default();
    let cache = AnalysisCache::new(AnalysisKind::Values, &state, 42);
    let mut pass =
        IntegerConstantFold { limits: IrLimits { max_analysis_work: 1, ..IrLimits::default() } };
    assert!(run_pass(&mut module, &mut pass, &mut state).is_err());
    assert_eq!(module, before);
    assert_eq!(cache.get(&state), Some(&42));
}

#[cfg(not(whale_wave_ir))]
#[test]
fn integer_folding_is_explicitly_unavailable_without_a_wave_module() {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut function = builder.begin_function("entry", vec![], Type::Void);
    function.ret(None);
    function.finish();
    let mut module = builder.finish();
    let original = module.clone();
    let error =
        run_pass(&mut module, &mut IntegerConstantFold::default(), &mut AnalysisState::default())
            .unwrap_err();
    assert_eq!(error.stage, PassStage::Transform);
    assert!(error.to_string().contains("requires the Wave"));
    assert_eq!(module, original);
}
