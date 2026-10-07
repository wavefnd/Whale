use ir::*;

fn source(ret: &str, body: &str) -> String {
    format!("module {{ format_version 4 semantics_version 1 target \"x86_64-whale-linux\" datalayout {{ ptr=64, endian=little }} declare @f7 \"memory\": whale () -> {ret}, linkage internal fn @f7 \"memory\"() -> {ret}, entry %b0 {{ %b0 \"entry\": {body} }} }}")
}
fn run(ret: &str, body: &str) -> Result<InterpreterResult, InterpreterError> {
    interpret(
        &parse_module(&source(ret, body)).unwrap(),
        FunctionId(7),
        &[],
    )
}
fn trap(ret: &str, body: &str, reason: MemoryTrap) {
    match run(ret, body).unwrap_err() {
        InterpreterError::Trap(t) => {
            assert_eq!(t.reason, TrapReason::Memory(reason));
            assert_eq!(t.site.function, FunctionId(7));
            assert_eq!(t.site.block, BlockId(0));
            assert!(t.steps > 0);
        }
        e => panic!("{e:?}"),
    }
}
#[test]
fn scalar_storage_roundtrips_every_integer_width_and_bool() {
    for (ty, value, expected) in [
        ("i1", "-1", ConstValue::I(-1)),
        ("u1", "1", ConstValue::U(1)),
        ("i8", "-128", ConstValue::I(-128)),
        ("u8", "255", ConstValue::U(255)),
        ("i16", "-32768", ConstValue::I(-32768)),
        ("u16", "65535", ConstValue::U(65535)),
        ("i32", "-2147483648", ConstValue::I(-2147483648)),
        ("u32", "4294967295", ConstValue::U(4294967295)),
        (
            "i64",
            "-9223372036854775808",
            ConstValue::I(i64::MIN.into()),
        ),
        (
            "u64",
            "18446744073709551615",
            ConstValue::U(u64::MAX.into()),
        ),
        (
            "i128",
            "-170141183460469231731687303715884105728",
            ConstValue::I(i128::MIN),
        ),
        (
            "u128",
            "340282366920938463463374607431768211455",
            ConstValue::U(u128::MAX),
        ),
        ("bool", "true", ConstValue::Bool(true)),
    ] {
        let b = format!("%v0: ptr<{ty}> = alloca {ty}, align 1 %v1: {ty} = const {ty} {value} store {ty} %v1, ptr<{ty}> %v0, align 1 %v2: {ty} = load {ty}, ptr<{ty}> %v0, align 1 ret {ty} %v2");
        assert_eq!(run(ty, &b).unwrap().value, Some(expected));
        trap(
            ty,
            &b.replace(&format!("store {ty} %v1, ptr<{ty}> %v0, align 1"), ""),
            MemoryTrap::Uninitialized { offset: 0 },
        );
    }
}
#[test]
fn physical_zero_uninit_reset_partial_store_and_copy_have_distinct_states() {
    let b = "%v0: ptr<u32> = alloca u32, align 4 %v1: u32 = const u32 42 store u32 %v1, ptr<u32> %v0, align 4 uninit u32, ptr<u32> %v0, align 4 %v2: u32 = load u32, ptr<u32> %v0, align 4 ret u32 %v2";
    trap("u32", b, MemoryTrap::Uninitialized { offset: 0 });
    let b = "%v0: ptr<u32> = alloca u32, align 4 %v1: ptr<u32> = alloca u32, align 4 %v2: ptr<u8> = bitcast ptr<u32> %v0 to ptr<u8> %v3: ptr<u8> = bitcast ptr<u32> %v1 to ptr<u8> %v4: u8 = const u8 42 store u8 %v4, ptr<u8> %v2, align 1 %v5: u64 = const u64 4 memcpy ptr<u8> %v3, ptr<u8> %v2, u64 %v5, align 1 %v6: u32 = load u32, ptr<u32> %v1, align 4 ret u32 %v6";
    trap("u32", b, MemoryTrap::Uninitialized { offset: 1 });
    assert_eq!(
        run(
            "u8",
            &b.replace(
                "%v6: u32 = load u32, ptr<u32> %v1, align 4 ret u32 %v6",
                "%v6: u8 = load u8, ptr<u8> %v3, align 1 ret u8 %v6"
            )
        )
        .unwrap()
        .value,
        Some(ConstValue::U(42))
    );
}
#[test]
fn bounds_alignment_one_past_and_address_overflow_trap_at_execution() {
    let b = "%v0: ptr<u32> = alloca u32, align 4 %v1: i64 = const i64 1 %v2: ptr<u32> = gep %v0, %v1 %v3: u32 = load u32, ptr<u32> %v2, align 4 ret u32 %v3";
    trap("u32", b, MemoryTrap::OutOfBounds);
    // One-past creation and comparison succeed; access does not.
    assert_eq!(
        run(
            "bool",
            &b.replace(
                "%v3: u32 = load u32, ptr<u32> %v2, align 4 ret u32 %v3",
                "%v3: bool = icmp ne ptr<u32> %v0, %v2 ret bool %v3"
            )
        )
        .unwrap()
        .value,
        Some(ConstValue::Bool(true))
    );
    trap(
        "u32",
        &b.replace("const i64 1", "const i64 -1"),
        MemoryTrap::OutOfBounds,
    );
    let b = "%v0: ptr<array<u8, 8>> = alloca array<u8, 8>, align 8 %v1: ptr<u8> = bitcast ptr<array<u8, 8>> %v0 to ptr<u8> %v2: i64 = const i64 1 %v3: ptr<u8> = gep %v1, %v2 %v4: u8 = load u8, ptr<u8> %v3, align 2 ret u8 %v4";
    trap(
        "u8",
        b,
        MemoryTrap::Misaligned {
            address: 0x1001,
            alignment: 2,
        },
    );
    let b = "%v0: ptr<u32> = alloca u32, align 4 %v1: u128 = const u128 340282366920938463463374607431768211455 %v2: ptr<u32> = gep %v0, %v1 ret void";
    trap("void", b, MemoryTrap::AddressOverflow);
}
#[test]
fn integer_roundtrip_never_recovers_pointer_authority() {
    let b = "%v0: ptr<u32> = alloca u32, align 4 %v1: u64 = ptrtoint ptr<u32> %v0 to u64 %v2: ptr<u32> = inttoptr u64 %v1 to ptr<u32> %v3: u32 = load u32, ptr<u32> %v2, align 4 ret u32 %v3";
    trap("u32", b, MemoryTrap::MissingProvenance);
    trap(
        "u32",
        &b.replace("ptrtoint ptr<u32> %v0 to u64", "const u64 0"),
        MemoryTrap::NullPointer,
    );
}
#[test]
fn stored_and_copied_pointers_keep_capabilities_without_forging_them() {
    let b = "%v0: ptr<u32> = alloca u32, align 4 %v1: u32 = const u32 42 store u32 %v1, ptr<u32> %v0, align 4 %v2: ptr<ptr<u32>> = alloca ptr<u32>, align 8 %v3: ptr<ptr<u32>> = alloca ptr<u32>, align 8 store ptr<u32> %v0, ptr<ptr<u32>> %v2, align 8 %v4: ptr<u8> = bitcast ptr<ptr<u32>> %v2 to ptr<u8> %v5: ptr<u8> = bitcast ptr<ptr<u32>> %v3 to ptr<u8> %v6: u64 = const u64 8 memcpy ptr<u8> %v5, ptr<u8> %v4, u64 %v6, align 8 %v7: ptr<u32> = load ptr<u32>, ptr<ptr<u32>> %v3, align 8 %v8: u32 = load u32, ptr<u32> %v7, align 4 ret u32 %v8";
    assert_eq!(run("u32", b).unwrap().value, Some(ConstValue::U(42)));
    let overwritten = b.replace(
        "%v7: ptr<u32>",
        "%v9: u8 = const u8 0 store u8 %v9, ptr<u8> %v5, align 1 %v7: ptr<u32>",
    );
    // The low byte was already zero; identical bits do not reconstruct metadata.
    trap("u32", &overwritten, MemoryTrap::MissingProvenance);
}
#[test]
fn pointer_phi_select_mov_and_typed_array_gep_preserve_identity() {
    let s = source("u32", "%v0: ptr<array<u32, 2>> = alloca array<u32, 2>, align 4 %v1: u64 = const u64 0 %v2: u64 = const u64 1 %v3: ptr<u32> = gep %v0, %v1, %v2 %v4: u32 = const u32 42 store u32 %v4, ptr<u32> %v3, align 4 %v5: bool = const bool true br label %b1 %b1 \"join\": %v6: ptr<u32> = phi ptr<u32> [ %v3, %b0 ] %v7: ptr<u32> = select bool %v5, ptr<u32> %v6, ptr<u32> %v3 %v8: ptr<u32> = mov ptr<u32> %v7 %v9: u32 = load u32, ptr<u32> %v8, align 4 ret u32 %v9");
    assert_eq!(
        interpret(&parse_module(&s).unwrap(), FunctionId(7), &[])
            .unwrap()
            .value,
        Some(ConstValue::U(42))
    );
}
#[test]
fn memcpy_overlap_zero_length_memset_and_bool_representation_are_defined() {
    let b = "%v0: ptr<u8> = alloca u8, align 1 %v1: u64 = const u64 1 memcpy ptr<u8> %v0, ptr<u8> %v0, u64 %v1, align 1 ret void";
    trap("void", b, MemoryTrap::OverlappingCopy);
    assert!(run("void", &b.replace("const u64 1", "const u64 0")).is_ok());
    let zero = "%v0: u64 = const u64 0 %v1: ptr<u8> = inttoptr u64 %v0 to ptr<u8> %v2: u8 = const u8 255 memcpy ptr<u8> %v1, ptr<u8> %v1, u64 %v0, align 16 memset ptr<u8> %v1, u8 %v2, u64 %v0, align 16 ret void";
    assert!(run("void", zero).is_ok());
    let b = "%v0: ptr<bool> = alloca bool, align 1 %v1: ptr<u8> = bitcast ptr<bool> %v0 to ptr<u8> %v2: u8 = const u8 2 %v3: u64 = const u64 1 memset ptr<u8> %v1, u8 %v2, u64 %v3, align 1 %v4: bool = load bool, ptr<bool> %v0, align 1 ret bool %v4";
    trap("bool", b, MemoryTrap::InvalidBool { byte: 2 });
    assert_eq!(
        run("bool", &b.replace("const u8 2", "const u8 0"))
            .unwrap()
            .value,
        Some(ConstValue::Bool(false))
    );
}
#[test]
fn typed_fields_checked_pair_reads_skip_uninitialized_padding() {
    let b = "%v0: ptr<tuple<u32, bool>> = alloca tuple<u32, bool>, align 4 %v1: u32 = const u32 4294967295 %v2: u32 = const u32 1 %v3: tuple<u32, bool> = uadd_chk u32 %v1, %v2 store tuple<u32, bool> %v3, ptr<tuple<u32, bool>> %v0, align 4 %v4: tuple<u32, bool> = load tuple<u32, bool>, ptr<tuple<u32, bool>> %v0, align 4 %v5: bool = extract %v4, 1 ret bool %v5";
    assert_eq!(run("bool", b).unwrap().value, Some(ConstValue::Bool(true)));
    let bytes = b.replace("%v4: tuple<u32, bool> = load tuple<u32, bool>, ptr<tuple<u32, bool>> %v0, align 4 %v5: bool = extract %v4, 1 ret bool %v5", "%v4: ptr<u8> = bitcast ptr<tuple<u32, bool>> %v0 to ptr<u8> %v5: u64 = const u64 5 %v6: ptr<u8> = gep %v4, %v5 %v7: u8 = load u8, ptr<u8> %v6, align 1 ret u8 %v7");
    trap("u8", &bytes, MemoryTrap::Uninitialized { offset: 5 });
    let fields = "%v0: ptr<struct{u8, u32}> = alloca struct{u8, u32}, align 4 %v1: u64 = const u64 0 %v2: u64 = const u64 1 %v3: ptr<u32> = gep %v0, %v1, %v2 %v4: u32 = const u32 42 store u32 %v4, ptr<u32> %v3, align 4 %v5: u32 = load u32, ptr<u32> %v3, align 4 ret u32 %v5";
    assert_eq!(run("u32", fields).unwrap().value, Some(ConstValue::U(42)));
}
#[test]
fn memory_budgets_are_separate_from_ir_fuel() {
    let m = parse_module(&source(
        "void",
        "%v0: ptr<u32> = alloca u32, align 4 ret void",
    ))
    .unwrap();
    for (limits, resource) in [
        (
            MemoryLimits {
                max_bytes: 3,
                ..MemoryLimits::default()
            },
            MemoryResource::Bytes,
        ),
        (
            MemoryLimits {
                max_allocations: 0,
                ..MemoryLimits::default()
            },
            MemoryResource::Allocations,
        ),
        (
            MemoryLimits {
                max_work: 8,
                ..MemoryLimits::default()
            },
            MemoryResource::Work,
        ),
    ] {
        assert!(
            matches!(interpret_with_options(&m, FunctionId(7), &[], InterpreterOptions { memory_limits: limits, ..InterpreterOptions::default() }), Err(InterpreterError::MemoryLimit { resource: got, site: ExecutionSite { instruction: 0, .. }, .. }) if got==resource)
        );
    }
    let m = parse_module(&source("void", "%v0: ptr<u8> = alloca u8, align 1 %v1: ptr<ptr<u8>> = alloca ptr<u8>, align 8 store ptr<u8> %v0, ptr<ptr<u8>> %v1, align 8 ret void")).unwrap();
    assert!(matches!(
        interpret_with_options(
            &m,
            FunctionId(7),
            &[],
            InterpreterOptions {
                memory_limits: MemoryLimits {
                    max_pointer_fragments: 7,
                    ..MemoryLimits::default()
                },
                ..InterpreterOptions::default()
            }
        ),
        Err(InterpreterError::MemoryLimit {
            resource: MemoryResource::PointerFragments,
            ..
        })
    ));
}
#[test]
fn old_wave_integer_memory_executes_and_call_body_stays_unsupported() {
    let m = parse_module(include_str!("fixtures/wave-control-v3.wir")).unwrap();
    assert_eq!(
        interpret(&m, FunctionId(0), &[ConstValue::I(41)])
            .unwrap()
            .value,
        Some(ConstValue::I(42))
    );
    assert!(matches!(
        interpret(&m, FunctionId(1), &[]),
        Err(InterpreterError::UnsupportedInstruction {
            operation: "call",
            ..
        })
    ));
}
#[test]
fn published_memory_fixtures_match_results_trap_sites_and_canonical_output() {
    let source = include_str!("fixtures/tracked-memory-v4.wir");
    let m = parse_module(source).unwrap();
    assert_eq!(print_module(&m), source);
    assert_eq!(
        interpret(&m, FunctionId(0), &[]).unwrap().value,
        Some(ConstValue::U(42))
    );
    assert!(matches!(
        interpret(&m, FunctionId(1), &[]),
        Err(InterpreterError::Trap(InterpreterTrap {
            steps: 3,
            site: ExecutionSite {
                block: BlockId(0),
                instruction: 2,
                ..
            },
            reason: TrapReason::Memory(MemoryTrap::Uninitialized { offset: 0 }),
            ..
        }))
    ));
    let source = include_str!("fixtures/initialization-loop-v4.wir");
    let m = parse_module(source).unwrap();
    assert_eq!(print_module(&m), source);
    assert!(matches!(
        interpret(&m, FunctionId(0), &[]),
        Err(InterpreterError::Trap(InterpreterTrap {
            steps: 17,
            site: ExecutionSite {
                block: BlockId(3),
                instruction: 0,
                ..
            },
            reason: TrapReason::Memory(MemoryTrap::Uninitialized { offset: 0 }),
            ..
        }))
    ));
}
#[test]
fn storage_overflow_undef_and_versioned_uninit_are_rejected_precisely() {
    for b in [
        "%v0: ptr<array<u64, 18446744073709551615>> = alloca array<u64, 18446744073709551615>, align 8 ret void",
        "%v0: u64 = const u64 0 %v1: ptr<array<u64, 18446744073709551615>> = inttoptr u64 %v0 to ptr<array<u64, 18446744073709551615>> %v2: ptr<array<u64, 18446744073709551615>> = gep %v1, %v0 ret void",
        "%v0: u64 = const u64 0 %v1: ptr<array<u64, 18446744073709551615>> = inttoptr u64 %v0 to ptr<array<u64, 18446744073709551615>> %v2: array<u64, 18446744073709551615> = load array<u64, 18446744073709551615>, ptr<array<u64, 18446744073709551615>> %v1, align 8 ret void",
    ] {
        assert!(matches!(parse_module(&source("void", b)).unwrap_err().kind, ParseErrorKind::Verification(e) if matches!(*e, VerifyError::InvalidMemoryLayout { reason: LayoutError::Overflow, .. })));
    }
    assert!(matches!(
        parse_module(&source("u32", "%v0: u32 = undef u32 ret u32 %v0"))
            .unwrap_err()
            .kind,
        ParseErrorKind::Verification(e) if matches!(*e, VerifyError::ForbiddenUndef { .. })
    ));
    let s = source(
        "void",
        "%v0: ptr<u32> = alloca u32, align 4 uninit u32, ptr<u32> %v0, align 4 ret void",
    );
    let m = parse_module(&s).unwrap();
    let printed = print_module(&m);
    assert!(printed.contains("format_version 4"));
    assert_eq!(printed, print_module(&parse_module(&printed).unwrap()));
    assert!(format!(
        "{}",
        parse_module(&s.replace("format_version 4", "format_version 3")).unwrap_err()
    )
    .contains("uninit requires typed IR format 4"));
    for invalid in ["align 0", "align 3"] {
        assert!(parse_module(&s.replace(
            "uninit u32, ptr<u32> %v0, align 4",
            &format!("uninit u32, ptr<u32> %v0, {invalid}")
        ))
        .is_err());
    }
    assert!(parse_module(&s.replace("uninit u32", "%v9: u32 = uninit u32")).is_err());
    assert!(parse_module(&s.replace("uninit u32, ptr<u32>", "uninit u8, ptr<u8>")).is_err());
}
