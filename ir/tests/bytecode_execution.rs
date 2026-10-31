use ir::{bytecode::*, *};

fn module(source: &str) -> Module {
    parse_module(&source.replace("x86_64-whale-linux", "whale-bytecode-v1")).unwrap()
}

fn compile_program(module: &Module) -> BytecodeProgram {
    let bytes = compile(module, BytecodeLimits::default()).unwrap();
    let repeated = compile(module, BytecodeLimits::default()).unwrap();
    assert_eq!(bytes, repeated);
    BytecodeProgram::load(&bytes, BytecodeLimits::default()).unwrap()
}

#[test]
fn native_bytecode_execution_matches_calls_phi_pointer_and_float_fixtures() {
    if !available() {
        return;
    }
    let fixtures = [
        (
            include_str!("fixtures/interpreted-calls-v4.wir"),
            vec![
                (0, vec![ConstValue::I(41)]),
                (3, vec![]),
                (5, vec![ConstValue::I(4)]),
                (6, vec![ConstValue::U(4)]),
            ],
        ),
        (include_str!("fixtures/tracked-memory-v4.wir"), vec![(0, vec![])]),
        (
            include_str!("fixtures/runtime-storage-v6.wir"),
            vec![(0, vec![]), (1, vec![]), (3, vec![]), (7, vec![])],
        ),
        (
            include_str!("fixtures/float-storage-v7.wir"),
            vec![
                (0, vec![ConstValue::F(FloatBits::F16(0x7d55))]),
                (1, vec![ConstValue::F(FloatBits::F32(0xff80_0123))]),
                (2, vec![ConstValue::F(FloatBits::F64(0x7ff0_0000_0000_0123))]),
                (3, vec![]),
            ],
        ),
    ];
    for (fixture, cases) in fixtures {
        let module = module(fixture);
        let mut program = compile_program(&module);
        for (id, arguments) in cases {
            let reference = interpret(&module, FunctionId(id), &arguments).unwrap();
            for _ in 0..2 {
                let result =
                    program.run(FunctionId(id), &arguments, InterpreterOptions::default()).unwrap();
                assert_eq!(
                    result.value.as_ref().and_then(BytecodeValue::scalar_value),
                    reference.value.as_ref()
                );
                assert_eq!(result.steps, reference.steps);
            }
        }
    }
}

#[test]
fn bytecode_preserves_source_sites_call_trace_and_runtime_limits() {
    if !available() {
        return;
    }
    let module = module(include_str!("fixtures/runtime-diagnostics-v7.wir"));
    let mut program = compile_program(&module);
    let reference = interpret(&module, FunctionId(0), &[]).unwrap_err();
    let BytecodeError::Interpreter(actual) =
        program.run(FunctionId(0), &[], InterpreterOptions::default()).unwrap_err()
    else {
        panic!("expected runtime trap")
    };
    assert_eq!(actual.to_string(), reference.to_string());
    let BytecodeError::Interpreter(actual) = program
        .run(FunctionId(0), &[], InterpreterOptions { max_steps: 0, ..Default::default() })
        .unwrap_err()
    else {
        panic!("expected step limit")
    };
    assert!(matches!(actual.root_cause(), InterpreterError::StepLimit { limit: 0, .. }));
}

#[test]
fn bytecode_preserves_sparse_parameter_and_definition_ids_and_return_bits() {
    if !available() {
        return;
    }
    let source = r#"module {
        format_version 8
        semantics_version 1
        target "whale-bytecode-v1"
        datalayout { ptr=64, endian=little }
        declare @f2147483647 "sparse": whale (u128) -> u128, linkage internal
        fn @f2147483647 "sparse"(%v2147483647 "argument": u128) -> u128, entry %b2147483647 {
        %b2147483647 "entry":
            %v4294967295: u128 = const u128 1
            %v42: u128 = add u128 %v2147483647, %v4294967295
            ret u128 %v42
        }
    }"#;
    let module = parse_module(source).unwrap();
    let mut program = compile_program(&module);
    let info = program.function(FunctionId(2147483647)).unwrap().unwrap();
    assert_eq!(info.params, vec![Type::U128]);
    let result =
        program.run(info.id, &[ConstValue::U(u128::MAX)], InterpreterOptions::default()).unwrap();
    assert_eq!(result.value.unwrap().scalar_value(), Some(&ConstValue::U(0)));
}

#[test]
fn bytecode_rejects_truncation_unknown_tables_bad_indices_and_limits_before_execution() {
    if !available() {
        return;
    }
    let module = module(include_str!("fixtures/runtime-diagnostics-v7.wir"));
    let bytes = compile(&module, BytecodeLimits::default()).unwrap();
    for length in [0, 7, 63, 575, bytes.len() - 1] {
        assert!(BytecodeProgram::load(&bytes[..length], BytecodeLimits::default()).is_err());
    }
    for offset in [0, 8, 16, 24, 32, 40, 48, 56, 64, 88] {
        let mut corrupt = bytes.clone();
        corrupt[offset] ^= 0x80;
        assert!(
            BytecodeProgram::load(&corrupt, BytecodeLimits::default()).is_err(),
            "offset {offset}"
        );
    }
    for limits in [
        BytecodeLimits { max_input_bytes: bytes.len() as u64 - 1, ..Default::default() },
        BytecodeLimits { max_decoded_bytes: 0, ..Default::default() },
        BytecodeLimits { max_records: 0, ..Default::default() },
        BytecodeLimits { max_work: 0, ..Default::default() },
        BytecodeLimits { max_workspace_bytes: 0, ..Default::default() },
    ] {
        assert!(BytecodeProgram::load(&bytes, limits).is_err());
    }
}

#[test]
fn reference_build_reports_explicit_unavailability() {
    if available() {
        return;
    }
    assert!(matches!(
        BytecodeProgram::load(&[], BytecodeLimits::default()),
        Err(BytecodeError::Unavailable)
    ));
}

#[test]
fn bytecode_callable_origins_and_raw_addresses_preserve_authority() {
    if !available() {
        return;
    }
    let fixture =
        include_str!("fixtures/interpreted-calls-v4.wir").trim().strip_suffix('}').unwrap();
    let text = format!(
        r#"{fixture}
        declare @f200 "apply": whale (fnptr<whale (i32) -> i32>) -> i32, linkage internal
        fn @f200 "apply"(%v0 "callback": fnptr<whale (i32) -> i32>) -> i32, entry %b0 {{
        %b0 "entry":
            %v1: i32 = const i32 41
            %v2: i32 = call whale i32 indirect %v0(%v1)
            ret i32 %v2
        }}
    }}"#
    );
    let module = module(&text);
    let bytes = compile(&module, BytecodeLimits::default()).unwrap();
    let mut first = BytecodeProgram::load(&bytes, BytecodeLimits::default()).unwrap();
    let mut second = BytecodeProgram::load(&bytes, BytecodeLimits::default()).unwrap();
    let callable =
        first.run(FunctionId(7), &[], InterpreterOptions::default()).unwrap().value.unwrap();
    assert_eq!(callable.function_target(), Some(FunctionId(0)));
    let result = first
        .run_values(FunctionId(200), std::slice::from_ref(&callable), InterpreterOptions::default())
        .unwrap();
    assert_eq!(result.value.unwrap().scalar_value(), Some(&ConstValue::I(42)));
    let error =
        second.run_values(FunctionId(200), &[callable], InterpreterOptions::default()).unwrap_err();
    assert!(matches!(
        error,
        BytecodeError::Interpreter(InterpreterError::ArgumentType { index: 0, .. })
    ));
    let raw = BytecodeValue::raw_pointer(Type::ptr_to(Type::U32), 0x1000).unwrap();
    let BytecodeError::Interpreter(error) =
        first.run_values(FunctionId(1), &[raw], InterpreterOptions::default()).unwrap_err()
    else {
        panic!()
    };
    assert!(
        matches!(error.root_cause(), InterpreterError::Trap(trap) if trap.reason == TrapReason::Memory(MemoryTrap::MissingProvenance))
    );
}

#[test]
fn generated_streams_match_independent_integer_memory_and_branch_oracle() {
    if !available() {
        return;
    }
    let mut text = String::from("module { format_version 8 semantics_version 1 target \"whale-bytecode-v1\" datalayout {ptr=64,endian=little} ");
    for id in 0..64 {
        text.push_str(&format!(
            "declare @f{id} \"generated{id}\": whale (u64, bool) -> u64, linkage internal "
        ));
    }
    let mut random = 0x81e2_3945_6ad0_7c31_u64;
    let mut seeds = Vec::new();
    for id in 0..64 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        seeds.push(random);
        text.push_str(&format!(
            r#"
            fn @f{id} "generated{id}"(%v0 "input": u64, %v1 "condition": bool) -> u64, entry %b0 {{
            %b0 "entry":
                %v2: u64 = const u64 {random}
                %v3: u64 = xor u64 %v0, %v2
                %v4: ptr<u64> = alloca u64, align 8
                volatile_store u64 %v3, ptr<u64> %v4, align 8
                %v5: u64 = volatile_load u64, ptr<u64> %v4, align 8
                cbr bool %v1, label %b1, label %b2
            %b1 "left":
                %v6: u64 = add u64 %v5, %v2
                br label %b3
            %b2 "right":
                %v7: u64 = mul u64 %v5, %v2
                br label %b3
            %b3 "join":
                %v8: u64 = phi u64 [%v6, %b1], [%v7, %b2]
                ret u64 %v8
            }}
        "#
        ));
    }
    text.push('}');
    let module = parse_module(&text).unwrap();
    let mut program = compile_program(&module);
    for (id, salt) in seeds.iter().enumerate() {
        for input in [0_u64, u64::MAX, salt.rotate_left(19)] {
            for condition in [false, true] {
                let expected = if condition {
                    (input ^ salt).wrapping_add(*salt)
                } else {
                    (input ^ salt).wrapping_mul(*salt)
                };
                let result = program
                    .run(
                        FunctionId(id as u32),
                        &[ConstValue::U(input.into()), ConstValue::Bool(condition)],
                        InterpreterOptions::default(),
                    )
                    .unwrap();
                assert_eq!(
                    result.value.unwrap().scalar_value(),
                    Some(&ConstValue::U(expected.into()))
                );
                assert_eq!(result.steps, 10);
            }
        }
    }
}

#[test]
fn empty_images_and_unused_declarations_are_valid_but_referenced_imports_are_rejected() {
    if !available() {
        return;
    }
    let empty = Module::new("whale-bytecode-v1", DataLayout::default_64bit_le());
    let mut program = compile_program(&empty);
    assert!(program.verify().unwrap().functions.is_empty());
    let text = r#"module {
        format_version 8
        semantics_version 1
        target "whale-bytecode-v1"
        datalayout { ptr=64, endian=little }
        declare @f0 "entry": whale () -> u32, linkage internal
        declare @f1 "external": whale () -> u32, linkage external, link_name "external"
        fn @f0 "entry"() -> u32, entry %b0 {
        %b0 "entry":
            %v0: u32 = const u32 42
            ret u32 %v0
        }
    }"#;
    let module = parse_module(text).unwrap();
    let mut program = compile_program(&module);
    let result = program.run(FunctionId(0), &[], InterpreterOptions::default()).unwrap();
    assert_eq!(result.value.unwrap().scalar_value(), Some(&ConstValue::U(42)));
    let called = parse_module(&text.replace("const u32 42", "call whale u32 @f1()")).unwrap();
    assert!(matches!(
        compile(&called, BytecodeLimits::default()),
        Err(BytecodeError::Invalid { status: 5, .. })
    ));
}

#[test]
fn compile_budget_preflight_precedes_semantic_verification_and_packet_copies() {
    if !available() {
        return;
    }
    let mut module = Module::new("whale-bytecode-v1", DataLayout::default_64bit_le());
    module.sources.files.push(SourceFile { id: 0, name: "x".repeat(64 * 1024) });
    module.sources.files.push(SourceFile { id: 0, name: "duplicate".into() });
    for limits in [
        BytecodeLimits { max_workspace_bytes: 0, ..Default::default() },
        BytecodeLimits { max_workspace_bytes: 2048, ..Default::default() },
        BytecodeLimits { max_decoded_bytes: 0, ..Default::default() },
        BytecodeLimits { max_input_bytes: 575, ..Default::default() },
        BytecodeLimits { max_work: 1, ..Default::default() },
    ] {
        assert!(matches!(
            compile(&module, limits),
            Err(BytecodeError::Invalid { status: 2, code: 4, .. })
        ));
    }
}

#[test]
fn typed_argument_admission_is_bounded_and_does_not_mutate_the_verified_packet() {
    if !available() {
        return;
    }
    let module = parse_module(
        r#"module {
        format_version 8
        semantics_version 1
        target "whale-bytecode-v1"
        datalayout { ptr=64, endian=little }
        declare @f0 "ignore": whale (ptr<u8>) -> void, linkage internal
        fn @f0 "ignore"(%v0 "pointer": ptr<u8>) -> void, entry %b0 {
        %b0 "entry":
            ret void
        }
    }"#,
    )
    .unwrap();
    let bytes = compile(&module, BytecodeLimits::default()).unwrap();
    let limits = BytecodeLimits { max_workspace_bytes: 64 * 1024, ..Default::default() };
    let mut program = BytecodeProgram::load(&bytes, limits).unwrap();
    let initial_bytes = program.verify().unwrap().decoded_bytes;
    let oversized =
        BytecodeValue::raw_pointer(Type::ptr_to(Type::Struct(vec![Type::U8; 256])), 1).unwrap();
    assert!(matches!(
        program.run_values(FunctionId(0), &[oversized], InterpreterOptions::default()),
        Err(BytecodeError::Invalid { status: 2, .. })
    ));
    assert_eq!(program.verify().unwrap().decoded_bytes, initial_bytes);
    for _ in 0..16 {
        let value = BytecodeValue::raw_pointer(Type::ptr_to(Type::U8), 1).unwrap();
        assert!(program
            .run_values(FunctionId(0), &[value], InterpreterOptions::default())
            .unwrap()
            .value
            .is_none());
        assert_eq!(program.verify().unwrap().decoded_bytes, initial_bytes);
    }
}
