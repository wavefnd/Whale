use ir::*;

const HEADER: &str = "module { format_version 6 semantics_version 1 target \"x86_64-whale-linux\" datalayout { ptr=64, endian=little }";

fn fixture() -> Module {
    parse_module(include_str!("fixtures/runtime-storage-v6.wir")).unwrap()
}

fn module(ret: &str, body: &str) -> Module {
    parse_module(&format!("{HEADER} declare @f0 \"test\": whale () -> {ret}, linkage internal fn @f0 \"test\"() -> {ret}, entry %b0 {{ %b0 \"entry\": {body} }} }}")).unwrap()
}

fn with_storage(storage: &str, ret: &str, body: &str) -> Module {
    parse_module(&format!("{HEADER} {storage} declare @f0 \"test\": whale () -> {ret}, linkage internal fn @f0 \"test\"() -> {ret}, entry %b0 {{ %b0 \"entry\": {body} }} }}")).unwrap()
}

fn trap(m: &Module, id: u32, reason: MemoryTrap) {
    assert!(
        matches!(interpret(m, FunctionId(id), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::Memory(reason))
    );
}

fn canonical(m: &Module) {
    let printed = print_module(m);
    assert_eq!(print_module(&parse_module(&printed).unwrap()), printed);
}

#[test]
fn scalar_globals_are_shared_by_calls_and_every_public_run_starts_fresh() {
    let m = fixture();
    canonical(&m);
    for _ in 0..2 {
        assert_eq!(interpret(&m, FunctionId(1), &[]).unwrap().value, Some(ConstValue::U(12)));
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(11)));
        assert_eq!(interpret(&m, FunctionId(3), &[]).unwrap().value, Some(ConstValue::U(42)));
    }
    let printed = print_module(&m);
    assert!(printed.contains("link_name \"public_answer\""));
    assert_eq!(m.global_storage[1].link_name.as_deref(), Some("public_answer"));
}

#[test]
fn read_only_and_uninitialized_globals_have_distinct_defined_failures() {
    let m = fixture();
    trap(&m, 4, MemoryTrap::Uninitialized { offset: 0 });
    trap(&m, 5, MemoryTrap::PermissionDenied);
    let m = with_storage("storage @g0 \"g\": u32 = uninitialized, align 4, access readwrite, linkage internal", "u32", "%v0: ptr<u32> = global_addr @g0 %v1: u32 = const u32 42 store u32 %v1, ptr<u32> %v0, align 4 %v2: u32 = load u32, ptr<u32> %v0, align 4 ret u32 %v2");
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(42)));
}

#[test]
fn global_initialization_limits_are_reported_before_any_entry_instruction() {
    let m = fixture();
    for memory_limits in [
        MemoryLimits { max_bytes: 3, ..Default::default() },
        MemoryLimits { max_allocations: 0, ..Default::default() },
        MemoryLimits { max_work: 0, ..Default::default() },
    ] {
        assert!(matches!(
            interpret_with_options(
                &m,
                FunctionId(3),
                &[],
                InterpreterOptions { memory_limits, ..Default::default() }
            ),
            Err(InterpreterError::GlobalMemoryLimit { global: GlobalId(0), .. })
        ));
    }
}

#[test]
fn globals_preserve_all_integer_widths_and_bool_without_scalar_category_changes() {
    for (ty, literal, value) in [
        ("i1", "-1", ConstValue::I(-1)),
        ("u1", "1", ConstValue::U(1)),
        ("i8", "-128", ConstValue::I(-128)),
        ("u8", "255", ConstValue::U(255)),
        ("i16", "-32768", ConstValue::I(-32768)),
        ("u16", "65535", ConstValue::U(65535)),
        ("i32", "-2147483648", ConstValue::I(-2147483648)),
        ("u32", "4294967295", ConstValue::U(4294967295)),
        ("i64", "-9223372036854775808", ConstValue::I(i64::MIN.into())),
        ("u64", "18446744073709551615", ConstValue::U(u64::MAX.into())),
        ("i128", "-170141183460469231731687303715884105728", ConstValue::I(i128::MIN)),
        ("u128", "340282366920938463463374607431768211455", ConstValue::U(u128::MAX)),
        ("bool", "true", ConstValue::Bool(true)),
    ] {
        let m = with_storage(&format!("storage @g0 \"g\": {ty} = value {ty} {literal}, align 1, access readonly, linkage internal"), ty, &format!("%v0: ptr<{ty}> = global_addr @g0 %v1: {ty} = load {ty}, ptr<{ty}> %v0, align 1 ret {ty} %v1"));
        canonical(&m);
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(value));
    }
}

#[test]
fn ended_allocations_invalidate_stored_and_gep_aliases_and_return_retirement_stays_idempotent() {
    trap(&fixture(), 6, MemoryTrap::ExpiredAllocation);
    let body = "%v0: ptr<u32> = alloca u32, align 4 %v1: u64 = const u64 1 %v2: ptr<u32> = gep %v0, %v1 lifetime_end %v0 %v3: bool = icmp eq ptr<u32> %v2, %v0 ret bool %v3";
    trap(&module("bool", body), 0, MemoryTrap::ExpiredAllocation);
    assert_eq!(
        interpret(
            &module("void", "%v0: ptr<u32> = alloca u32, align 4 lifetime_end %v0 ret void"),
            FunctionId(0),
            &[]
        )
        .unwrap()
        .value,
        None
    );
}

#[test]
fn lifetime_end_rejects_nonbase_raw_null_global_caller_owned_and_repeated_requests() {
    trap(&module("void", "%v0: ptr<u32> = alloca u32, align 4 %v1: u64 = const u64 1 %v2: ptr<u32> = gep %v0, %v1 lifetime_end %v2 ret void"), 0, MemoryTrap::InvalidLifetimeEnd);
    trap(&module("void", "%v0: u64 = const u64 4096 %v1: ptr<u32> = inttoptr u64 %v0 to ptr<u32> lifetime_end %v1 ret void"), 0, MemoryTrap::MissingProvenance);
    trap(&module("void", "%v0: u64 = const u64 0 %v1: ptr<u32> = inttoptr u64 %v0 to ptr<u32> lifetime_end %v1 ret void"), 0, MemoryTrap::NullPointer);
    trap(
        &module(
            "void",
            "%v0: ptr<u32> = alloca u32, align 4 lifetime_end %v0 lifetime_end %v0 ret void",
        ),
        0,
        MemoryTrap::ExpiredAllocation,
    );
    trap(
        &with_storage(
            "storage @g0 \"g\": u32 = uninitialized, align 4, access readwrite, linkage internal",
            "void",
            "%v0: ptr<u32> = global_addr @g0 lifetime_end %v0 ret void",
        ),
        0,
        MemoryTrap::InvalidLifetimeEnd,
    );
    let source = format!("{HEADER} declare @f0 \"caller\": whale () -> void, linkage internal declare @f1 \"borrower\": whale (ptr<u32>) -> void, linkage internal fn @f0 \"caller\"() -> void, entry %b0 {{ %b0 \"entry\": %v0: ptr<u32> = alloca u32, align 4 call whale void @f1(%v0) ret void }} fn @f1 \"borrower\"(%v0 \"p\": ptr<u32>) -> void, entry %b0 {{ %b0 \"entry\": lifetime_end %v0 ret void }} }}");
    let m = parse_module(&source).unwrap();
    assert!(
        matches!(interpret(&m, FunctionId(0), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::Memory(MemoryTrap::InvalidLifetimeEnd) && t.site.function == FunctionId(1))
    );
}

#[test]
fn zero_length_byte_operations_remain_no_access_after_lifetime_end() {
    let body = "%v0: ptr<u8> = alloca u8, align 1 lifetime_end %v0 %v1: u64 = const u64 0 %v2: u8 = const u8 0 memcpy ptr<u8> %v0, ptr<u8> %v0, u64 %v1, align 1 memset ptr<u8> %v0, u8 %v2, u64 %v1, align 1 ret void";
    assert_eq!(interpret(&module("void", body), FunctionId(0), &[]).unwrap().value, None);
}

#[test]
fn byte_difference_is_signed_and_ordering_uses_live_allocation_offsets() {
    let m = fixture();
    assert_eq!(interpret(&m, FunctionId(7), &[]).unwrap().value, Some(ConstValue::I(16)));
    let prefix = "%v0: ptr<array<u32, 4>> = alloca array<u32, 4>, align 4 %v1: u64 = const u64 0 %v2: u64 = const u64 3 %v3: ptr<u32> = gep %v0, %v1, %v1 %v4: ptr<u32> = gep %v0, %v1, %v2";
    for (lhs, rhs, value) in [("%v3", "%v4", -12), ("%v4", "%v3", 12), ("%v4", "%v4", 0)] {
        let m = module("i128", &format!("{prefix} %v5: i128 = ptrdiff {lhs}, {rhs} ret i128 %v5"));
        canonical(&m);
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(value)));
    }
    for pred in ["slt", "sle", "sgt", "sge", "ult", "ule", "ugt", "uge"] {
        let expected = matches!(pred, "slt" | "sle" | "ult" | "ule");
        for op in ["cmp", "icmp"] {
            let m = module(
                "bool",
                &format!("{prefix} %v5: bool = {op} {pred} ptr<u32> %v3, %v4 ret bool %v5"),
            );
            canonical(&m);
            assert_eq!(
                interpret(&m, FunctionId(0), &[]).unwrap().value,
                Some(ConstValue::Bool(expected))
            );
        }
    }
}

#[test]
fn pointer_difference_and_ordering_reject_different_raw_null_and_expired_allocations() {
    let prefix = "%v0: ptr<u32> = alloca u32, align 4 %v1: ptr<u32> = alloca u32, align 4";
    for (ret, operation) in [
        ("i128", "%v2: i128 = ptrdiff %v0, %v1 ret i128 %v2"),
        ("bool", "%v2: bool = icmp ult ptr<u32> %v0, %v1 ret bool %v2"),
    ] {
        trap(&module(ret, &format!("{prefix} {operation}")), 0, MemoryTrap::DifferentAllocation);
    }
    let m = module("bool", &format!("{prefix} %v2: bool = icmp eq ptr<u32> %v0, %v1 ret bool %v2"));
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::Bool(false)));
    trap(&module("i128","%v0: u64 = const u64 4096 %v1: ptr<u32> = inttoptr u64 %v0 to ptr<u32> %v2: i128 = ptrdiff %v1, %v1 ret i128 %v2"),0,MemoryTrap::MissingProvenance);
    trap(&module("bool","%v0: u64 = const u64 0 %v1: ptr<u32> = inttoptr u64 %v0 to ptr<u32> %v2: bool = icmp ult ptr<u32> %v1, %v1 ret bool %v2"),0,MemoryTrap::NullPointer);
    trap(&module("i128","%v0: ptr<u32> = alloca u32, align 4 lifetime_end %v0 %v1: i128 = ptrdiff %v0, %v0 ret i128 %v1"),0,MemoryTrap::ExpiredAllocation);
}

#[test]
fn storage_declarations_and_global_addresses_reject_invalid_identity_types_modes_and_names() {
    for edit in [
        "unknown",
        "pointee",
        "duplicate_id",
        "duplicate_name",
        "constant_id",
        "readonly_uninitialized",
        "initializer",
        "aggregate",
        "alignment",
        "missing_link",
        "internal_link",
        "nul_link",
        "link_collision",
    ] {
        let mut m = fixture();
        match edit {
            "unknown" => {
                let Instruction::GlobalAddr { global, .. } =
                    &mut m.functions[0].blocks[0].instructions[0]
                else {
                    unreachable!()
                };
                *global = GlobalId(1000);
            },
            "pointee" => {
                let Instruction::GlobalAddr { ty, .. } =
                    &mut m.functions[0].blocks[0].instructions[0]
                else {
                    unreachable!()
                };
                *ty = Type::I32;
                m.functions[0].value_types[0].1 = Type::ptr_to(Type::I32);
            },
            "duplicate_id" => m.global_storage[1].id = GlobalId(0),
            "duplicate_name" => m.global_storage[1].name = "counter".into(),
            "constant_id" => m.global_storage[0].id = GlobalId(90),
            "readonly_uninitialized" => m.global_storage[1].init = GlobalStorageInit::Uninitialized,
            "initializer" => {
                m.global_storage[0].init = GlobalStorageInit::Value(ConstValue::Bool(true))
            },
            "aggregate" => m.global_storage[0].ty = Type::Array(Box::new(Type::U32), 2),
            "alignment" => m.global_storage[0].align = 3,
            "missing_link" => m.global_storage[1].link_name = None,
            "internal_link" => m.global_storage[0].link_name = Some("unexpected".into()),
            "nul_link" => m.global_storage[1].link_name = Some("bad\0name".into()),
            "link_collision" => {
                m.declarations[0].linkage = Linkage::External;
                m.declarations[0].link_name = Some("public_answer".into());
            },
            _ => unreachable!(),
        }
        assert!(verify_module(&m).is_err(), "{edit}");
    }
    let mut m = fixture();
    m.global_storage[0].name = m.functions[0].name.clone();
    verify_module(&m).unwrap();
    // Compile-time names/identities do not silently become addressable values.
    let mut m = fixture();
    let Instruction::GlobalAddr { global, .. } = &mut m.functions[0].blocks[0].instructions[0]
    else {
        unreachable!()
    };
    *global = GlobalId(90);
    assert!(matches!(
        verify_module(&m),
        Err(VerifyError::GlobalStorage { reason: GlobalStorageError::UnknownGlobal, .. })
    ));
}

#[test]
fn checked_storage_and_pointer_builders_reject_errors_without_publishing_partial_ir() {
    let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let id = mb
        .try_add_global_storage(GlobalStorage::internal(
            "g",
            Type::U32,
            GlobalStorageInit::Value(ConstValue::U(42)),
            true,
            4,
        ))
        .unwrap();
    assert!(matches!(
        mb.try_add_global_storage(GlobalStorage::internal(
            "g",
            Type::U32,
            GlobalStorageInit::Uninitialized,
            false,
            4
        )),
        Err(GlobalStorageError::DuplicateName)
    ));
    assert!(matches!(
        mb.try_add_global_storage(GlobalStorage::internal(
            "bad",
            Type::U32,
            GlobalStorageInit::Uninitialized,
            true,
            4
        )),
        Err(GlobalStorageError::ReadOnlyUninitialized)
    ));
    let mut fb = mb.begin_function("f", vec![], Type::I128);
    assert!(matches!(fb.try_global_addr(GlobalId(999)), Err(BuilderError::UnknownGlobal(_))));
    let p = fb.try_global_addr(id).unwrap();
    let n = fb.const_uint(Type::U64, 0);
    assert!(matches!(fb.try_lifetime_end(n), Err(BuilderError::InvalidPointerOperands)));
    assert!(matches!(fb.try_ptrdiff(p, n), Err(BuilderError::InvalidPointerOperands)));
    let result = fb.try_ptrdiff(p, p).unwrap();
    fb.ret(Some(result));
    assert!(matches!(fb.try_lifetime_end(p), Err(BuilderError::TerminatedBlock(_))));
    fb.finish();
    let m = mb.finish();
    verify_module(&m).unwrap();
    assert_eq!(m.global_storage.len(), 1);
    assert_eq!(m.functions[0].blocks[0].instructions.len(), 3);
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(0)));
}

#[test]
fn new_operations_have_exact_result_operands_and_are_gated_from_all_prior_formats() {
    let m = fixture();
    let text = print_module(&m);
    for version in [3, 4, 5] {
        assert!(parse_module(&text.replace(
            &format!("format_version {}", IR_FORMAT_VERSION),
            &format!("format_version {version}")
        ))
        .unwrap_err()
        .to_string()
        .contains("requires typed IR format 6"));
        for body in [
            "%v0: ptr<u32> = alloca u32, align 4 lifetime_end %v0 ret void",
            "%v0: ptr<u32> = alloca u32, align 4 %v1: i128 = ptrdiff %v0, %v0 ret void",
            "%v0: ptr<u32> = alloca u32, align 4 %v1: bool = icmp ult ptr<u32> %v0, %v0 ret void",
        ] {
            let text = print_module(&module("void", body));
            assert!(parse_module(&text.replace(
                &format!("format_version {}", IR_FORMAT_VERSION),
                &format!("format_version {version}")
            ))
            .is_err());
        }
    }
    for source in [
        format!("{HEADER} declare @f0 \"f\": whale () -> void, linkage internal fn @f0 \"f\"() -> void, entry %b0 {{ %b0 \"entry\": %v0: u32 = const u32 0 lifetime_end %v0 ret void }} }}"),
        format!("{HEADER} declare @f0 \"f\": whale () -> void, linkage internal fn @f0 \"f\"() -> void, entry %b0 {{ %b0 \"entry\": %v0: ptr<u32> = alloca u32, align 4 %v1: ptr<u8> = bitcast ptr<u32> %v0 to ptr<u8> %v2: i128 = ptrdiff %v0, %v1 ret void }} }}"),
        text.replace("%v5: i128 = ptrdiff","%v5: i64 = ptrdiff"),
        text.replace("lifetime_end %v0","%v99: u32 = lifetime_end %v0"),
    ] {assert!(parse_module(&source).is_err());}
}

#[test]
fn lifetime_end_is_preserved_in_unreachable_o0_blocks_without_being_executed() {
    let source=format!("{HEADER} declare @f0 \"f\": whale () -> void, linkage internal fn @f0 \"f\"() -> void, entry %b0 {{ %b0 \"entry\": ret void %b1 \"dead\": %v0: u64 = const u64 0 %v1: ptr<u32> = inttoptr u64 %v0 to ptr<u32> lifetime_end %v1 ret void }} }}");
    let mut m = parse_module(&source).unwrap();
    let before = print_module(&m);
    zero::pass::run_zero_pass(&mut m);
    assert_eq!(print_module(&m), before);
    assert!(before.contains("lifetime_end %v1"));
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, None);
}

#[test]
fn retiring_then_allocating_never_reuses_an_address_and_budgets_remain_cumulative() {
    let body="%v0: ptr<u32> = alloca u32, align 4 %v1: u64 = ptrtoint ptr<u32> %v0 to u64 lifetime_end %v0 %v2: ptr<u32> = alloca u32, align 4 %v3: u64 = ptrtoint ptr<u32> %v2 to u64 %v4: bool = icmp ne u64 %v1, %v3 ret bool %v4";
    let m = module("bool", body);
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::Bool(true)));
    assert!(matches!(
        interpret_with_options(
            &m,
            FunctionId(0),
            &[],
            InterpreterOptions {
                memory_limits: MemoryLimits { max_allocations: 1, ..Default::default() },
                ..Default::default()
            }
        ),
        Err(InterpreterError::MemoryLimit { resource: MemoryResource::Allocations, limit: 1, .. })
    ));
    assert!(matches!(
        interpret_with_options(
            &m,
            FunctionId(0),
            &[],
            InterpreterOptions {
                memory_limits: MemoryLimits { max_bytes: 7, ..Default::default() },
                ..Default::default()
            }
        ),
        Err(InterpreterError::MemoryLimit { resource: MemoryResource::Bytes, limit: 7, .. })
    ));
}

#[test]
fn global_pointer_cells_keep_expired_capabilities_and_globals_are_not_const_references() {
    let source=format!("{HEADER} storage @g0 \"cell\": ptr<u32> = uninitialized, align 8, access readwrite, linkage internal declare @f0 \"caller\": whale () -> u32, linkage internal declare @f1 \"store_local\": whale () -> void, linkage internal fn @f0 \"caller\"() -> u32, entry %b0 {{ %b0 \"entry\": call whale void @f1() %v0: ptr<ptr<u32>> = global_addr @g0 %v1: ptr<u32> = load ptr<u32>, ptr<ptr<u32>> %v0, align 8 %v2: u32 = load u32, ptr<u32> %v1, align 4 ret u32 %v2 }} fn @f1 \"store_local\"() -> void, entry %b0 {{ %b0 \"entry\": %v0: ptr<u32> = alloca u32, align 4 %v1: ptr<ptr<u32>> = global_addr @g0 store ptr<u32> %v0, ptr<ptr<u32>> %v1, align 8 ret void }} }}");
    trap(&parse_module(&source).unwrap(), 0, MemoryTrap::ExpiredAllocation);
    let source=format!("{HEADER} storage @g0 \"g\": u32 = value u32 42, align 4, access readonly, linkage internal global @g1 \"c\": u32 = const u32 42, align 4, init_expr u32 @g0 }}");
    assert!(
        matches!(parse_module(&source).unwrap_err().kind,ParseErrorKind::Verification(error) if matches!(*error,VerifyError::InvalidConstExpression {reason:ConstEvalError::UnknownReference(ConstRef::Global(GlobalId(0))),..}))
    );
}

#[test]
fn global_addresses_keep_identity_through_indirect_calls_and_returns() {
    let source=format!("{HEADER} storage @g0 \"counter\": u32 = value u32 0, align 4, access readwrite, linkage internal declare @f0 \"entry\": whale () -> u32, linkage internal declare @f1 \"increment\": whale () -> u32, linkage internal fn @f0 \"entry\"() -> u32, entry %b0 {{ %b0 \"entry\": %v0: ptr<u32> = global_addr @g0 %v1: fnptr<whale () -> u32> = function_addr @f1 %v2: u32 = call whale u32 indirect %v1() %v3: u32 = call whale u32 indirect %v1() %v4: u32 = load u32, ptr<u32> %v0, align 4 ret u32 %v4 }} fn @f1 \"increment\"() -> u32, entry %b0 {{ %b0 \"entry\": %v0: ptr<u32> = global_addr @g0 %v1: u32 = load u32, ptr<u32> %v0, align 4 %v2: u32 = const u32 1 %v3: u32 = add u32 %v1, %v2 store u32 %v3, ptr<u32> %v0, align 4 ret u32 %v3 }} }}");
    let m = parse_module(&source).unwrap();
    canonical(&m);
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(2)));
}

#[test]
fn floating_storage_instantiates_without_losing_payload_bits() {
    let m=with_storage("storage @g0 \"bits\": f32 = value f32 0xffc01234, align 4, access readonly, linkage internal", "void", "ret void");
    canonical(&m);
    assert!(print_module(&m).contains("0xffc01234"));
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, None);
}

// Existing semantic assertions intentionally inspect the preserved leaf cause.
fn interpret(
    module: &Module,
    function: FunctionId,
    args: &[ConstValue],
) -> Result<InterpreterResult, InterpreterError> {
    ir::interpret(module, function, args).map_err(InterpreterError::into_root_cause)
}

fn interpret_with_options(
    module: &Module,
    function: FunctionId,
    args: &[ConstValue],
    options: InterpreterOptions,
) -> Result<InterpreterResult, InterpreterError> {
    ir::interpret_with_options(module, function, args, options)
        .map_err(InterpreterError::into_root_cause)
}
