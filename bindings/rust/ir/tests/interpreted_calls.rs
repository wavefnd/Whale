use ir::*;

const HEADER: &str = "module { format_version 4 semantics_version 1 target \"x86_64-whale-linux\" datalayout { ptr=64, endian=little }";

fn fixture() -> Module {
    parse_module(include_str!("fixtures/interpreted-calls-v4.wir")).unwrap()
}

fn append(module: &mut Module, id: u32, params: &[&str], ret: &str, blocks: &str) {
    let params_text = params
        .iter()
        .enumerate()
        .map(|(i, t)| format!("%v{i} \"p{i}\": {t}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("{HEADER} declare @f{id} \"extra{id}\": whale ({}) -> {ret}, linkage internal fn @f{id} \"extra{id}\"({params_text}) -> {ret}, entry %b0 {{ {blocks} }} }}", params.join(", "));
    let mut extra = parse_module(&source).unwrap();
    module.declarations.append(&mut extra.declarations);
    module.functions.append(&mut extra.functions);
}

fn append_using_fixture(id: u32, params: &[&str], ret: &str, blocks: &str) -> Module {
    // Parse together so forward calls to fixture declarations resolve.
    let source = include_str!("fixtures/interpreted-calls-v4.wir");
    let params_text = params
        .iter()
        .enumerate()
        .map(|(i, t)| format!("%v{i} \"p{i}\": {t}"))
        .collect::<Vec<_>>()
        .join(", ");
    let extra = format!("declare @f{id} \"extra{id}\": whale ({}) -> {ret}, linkage internal fn @f{id} \"extra{id}\"({params_text}) -> {ret}, entry %b0 {{ {blocks} }} }}", params.join(", "));
    parse_module(&format!("{} {extra}", source.trim().strip_suffix('}').unwrap())).unwrap()
}

fn memory_trap(module: &Module, id: u32, reason: MemoryTrap) {
    assert!(
        matches!(interpret(module, FunctionId(id), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::Memory(reason))
    );
}

#[test]
fn direct_nested_void_bool_unused_results_and_multiple_returns_execute_in_o0_order() {
    let m = append_using_fixture(
        8,
        &[],
        "void",
        "%b0 \"entry\": %v0: i32 = const i32 41 %v1: i32 = call whale i32 @f0(%v0) ret void",
    );
    assert_eq!(interpret(&m, FunctionId(8), &[]).unwrap().value, None);
    let m = append_using_fixture(
        8,
        &["bool"],
        "bool",
        "%b0 \"entry\": %v1: bool = call whale bool @f101(%v0) ret bool %v1",
    );
    for flag in [true, false] {
        assert_eq!(
            interpret(&m, FunctionId(8), &[ConstValue::Bool(flag)]).unwrap().value,
            Some(ConstValue::Bool(flag))
        );
    }
    let m = append_using_fixture(
        8,
        &[],
        "u32",
        "%b0 \"entry\": %v0: u32 = call whale u32 @f3() ret u32 %v0",
    );
    assert_eq!(interpret(&m, FunctionId(8), &[]).unwrap().value, Some(ConstValue::U(42)));
    let m = append_using_fixture(8, &[], "u32", "%b0 \"entry\": %v0: ptr<u32> = alloca u32, align 4 call whale void @f100(%v0) %v1: u32 = load u32, ptr<u32> %v0, align 4 ret u32 %v1");
    assert_eq!(interpret(&m, FunctionId(8), &[]).unwrap().value, Some(ConstValue::U(42)));
}

#[test]
fn caller_owned_pointers_survive_nested_return_storage_select_phi_and_indirect_dispatch() {
    let m = fixture();
    assert_eq!(interpret(&m, FunctionId(3), &[]).unwrap().value, Some(ConstValue::U(42)));
    let body = "%b0 \"entry\": %v0: ptr<u32> = alloca u32, align 4 %v1: bool = const bool false %v2: ptr<u32> = call whale ptr<u32> @f2(%v0, %v1) %v3: fnptr<whale (ptr<u32>) -> ptr<u32>> = function_addr @f1 %v4: ptr<u32> = call whale ptr<u32> indirect %v3(%v2) %v5: u32 = load u32, ptr<u32> %v0, align 4 ret u32 %v5";
    let m = append_using_fixture(8, &[], "u32", body);
    assert_eq!(interpret(&m, FunctionId(8), &[]).unwrap().value, Some(ConstValue::U(42)));
}

#[test]
fn returning_a_callee_local_pointer_expires_it_and_integer_addresses_never_gain_authority() {
    let m = append_using_fixture(8, &[], "u32", "%b0 \"entry\": %v0: ptr<u32> = call whale ptr<u32> @f4() %v1: u32 = load u32, ptr<u32> %v0, align 4 ret u32 %v1");
    memory_trap(&m, 8, MemoryTrap::ExpiredAllocation);
    let m = append_using_fixture(8, &[], "u32", "%b0 \"entry\": %v0: ptr<u32> = alloca u32, align 4 %v1: u64 = ptrtoint ptr<u32> %v0 to u64 %v2: ptr<u32> = inttoptr u64 %v1 to ptr<u32> %v3: ptr<u32> = call whale ptr<u32> @f1(%v2) %v4: u32 = load u32, ptr<u32> %v3, align 4 ret u32 %v4");
    memory_trap(&m, 8, MemoryTrap::MissingProvenance);
}

#[test]
fn direct_and_indirect_recursion_share_fuel_depth_and_total_frame_budgets() {
    let m = fixture();
    for (id, value) in [(5, ConstValue::I(4)), (6, ConstValue::U(4))] {
        let result = interpret(&m, FunctionId(id), std::slice::from_ref(&value)).unwrap();
        assert_eq!(result.value, Some(value.clone()));
        for (options, expected) in [
            (InterpreterOptions { max_call_depth: 4, ..Default::default() }, CallResource::Depth),
            (InterpreterOptions { max_frames: 4, ..Default::default() }, CallResource::Frames),
        ] {
            assert!(
                matches!(interpret_with_options(&m, FunctionId(id), std::slice::from_ref(&value), options), Err(InterpreterError::CallLimit { site, resource, limit: 4 }) if resource == expected && site.function == FunctionId(id) && site.value.is_some())
            );
        }
        assert!(
            matches!(interpret_with_options(&m, FunctionId(id), std::slice::from_ref(&value), InterpreterOptions { max_steps: result.steps - 1, ..Default::default() }), Err(InterpreterError::StepLimit { limit, .. }) if limit == result.steps - 1)
        );
        assert!(matches!(
            interpret_with_options(
                &m,
                FunctionId(id),
                &[value],
                InterpreterOptions { max_call_depth: 0, ..Default::default() }
            ),
            Err(InterpreterError::CallLimit { resource: CallResource::Depth, limit: 0, .. })
        ));
    }
}

#[test]
fn reachable_unsupported_callees_are_preflighted_before_an_earlier_runtime_trap() {
    let source = format!("{HEADER} declare @f0 \"entry\": whale () -> void, linkage internal declare @f1 \"bad\": whale () -> void, linkage internal fn @f0 \"entry\"() -> void, entry %b0 {{ %b0 \"entry\": trap reason=\"must not execute\" %b1 \"dead\": call whale void @f1() ret void }} fn @f1 \"bad\"() -> void, entry %b0 {{ %b0 \"entry\": %v0: ptr<array<u8, 1>> = alloca array<u8, 1>, align 1 %v1: array<u8, 1> = load array<u8, 1>, ptr<array<u8, 1>> %v0, align 1 ret void }} }}");
    let m = parse_module(&source).unwrap();
    assert!(
        matches!(interpret(&m, FunctionId(0), &[]), Err(InterpreterError::UnsupportedInstruction { site, operation: "load" }) if site.function == FunctionId(1))
    );
    let source = source.replace(
        "call whale void @f1()",
        "%v0: fnptr<whale () -> void> = function_addr @f1 call whale void indirect %v0()",
    );
    let m = parse_module(&source).unwrap();
    assert!(
        matches!(interpret(&m, FunctionId(0), &[]), Err(InterpreterError::UnsupportedInstruction { site, operation: "load" }) if site.function == FunctionId(1))
    );
}

#[test]
fn checked_library_boundary_preserves_signatures_and_rejects_foreign_pointer_resurrection() {
    let mut m = fixture();
    append(
        &mut m,
        8,
        &["ptr<u32>"],
        "u32",
        "%b0 \"entry\": %v1: u32 = load u32, ptr<u32> %v0, align 4 ret u32 %v1",
    );
    let returned = interpret_values(&m, FunctionId(4), &[]).unwrap().value.unwrap();
    assert_eq!(returned.ty(), &Type::ptr_to(Type::U32));
    assert!(returned.pointer_address().is_some());
    assert!(
        matches!(interpret_values(&m, FunctionId(8), &[returned]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::Memory(MemoryTrap::ExpiredAllocation))
    );
    let raw = RuntimeValue::raw_pointer(Type::ptr_to(Type::U32), 0x1000).unwrap();
    assert!(
        matches!(interpret_values(&m, FunctionId(8), &[raw]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::Memory(MemoryTrap::MissingProvenance))
    );
    assert!(RuntimeValue::scalar(Type::Bool, ConstValue::U(1)).is_err());
    assert!(RuntimeValue::scalar(Type::U8, ConstValue::U(256)).is_err());
    assert!(RuntimeValue::raw_pointer(Type::U64, 0).is_err());
    let m = append_using_fixture(
        9,
        &["fnptr<whale (i32) -> i32>", "i32"],
        "i32",
        "%b0 \"entry\": %v2: i32 = call whale i32 indirect %v0(%v1) ret i32 %v2",
    );
    let target = interpret_values(&m, FunctionId(7), &[]).unwrap().value.unwrap();
    assert_eq!(target.function_target(), Some(FunctionId(0)));
    let input = RuntimeValue::scalar(Type::I32, ConstValue::I(41)).unwrap();
    assert_eq!(
        interpret_values(&m, FunctionId(9), &[target.clone(), input.clone()])
            .unwrap()
            .value
            .unwrap()
            .scalar_value(),
        Some(&ConstValue::I(42))
    );
    assert!(matches!(
        interpret_values(
            &m,
            FunctionId(9),
            &[RuntimeValue::function(&m, FunctionId(6)).unwrap(), input]
        ),
        Err(InterpreterError::ArgumentType { index: 0, .. })
    ));
    let mut changed = m.clone();
    changed.declarations.iter_mut().find(|d| d.id == FunctionId(0)).unwrap().signature.convention =
        CallingConvention::SysV64;
    let input = RuntimeValue::scalar(Type::I32, ConstValue::I(41)).unwrap();
    assert!(matches!(
        interpret_values(&changed, FunctionId(9), &[target, input]),
        Err(InterpreterError::Verification(_))
            | Err(InterpreterError::ArgumentType { index: 0, .. })
    ));
}

#[test]
fn null_function_storage_and_raw_callable_bits_have_defined_failures() {
    let m = append_using_fixture(8, &[], "i32", "%b0 \"entry\": %v0: fnptr<whale (i32) -> i32> = null_function %v1: i32 = const i32 41 %v2: i32 = call whale i32 indirect %v0(%v1) ret i32 %v2");
    assert!(
        matches!(interpret(&m, FunctionId(8), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::NullFunction && t.site.value == Some(ValueId(2)))
    );
    let m = append_using_fixture(8, &[], "i32", "%b0 \"entry\": %v0: ptr<u64> = alloca u64, align 8 %v1: u64 = const u64 1 store u64 %v1, ptr<u64> %v0, align 8 %v2: ptr<fnptr<whale (i32) -> i32>> = bitcast ptr<u64> %v0 to ptr<fnptr<whale (i32) -> i32>> %v3: fnptr<whale (i32) -> i32> = load fnptr<whale (i32) -> i32>, ptr<fnptr<whale (i32) -> i32>> %v2, align 8 %v4: i32 = const i32 41 %v5: i32 = call whale i32 indirect %v3(%v4) ret i32 %v5");
    assert!(
        matches!(interpret(&m, FunctionId(8), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::InvalidFunctionPointer)
    );
}

fn split_callable_body() -> &'static str {
    "%b0 \"entry\":
    %v0: ptr<fnptr<whale (i32) -> i32>> = alloca fnptr<whale (i32) -> i32>, align 8
    %v1: ptr<fnptr<whale (i32) -> i32>> = alloca fnptr<whale (i32) -> i32>, align 8
    %v2: fnptr<whale (i32) -> i32> = call whale fnptr<whale (i32) -> i32> @f7()
    store fnptr<whale (i32) -> i32> %v2, ptr<fnptr<whale (i32) -> i32>> %v0, align 8
    %v3: ptr<u8> = bitcast ptr<fnptr<whale (i32) -> i32>> %v0 to ptr<u8>
    %v4: ptr<u8> = bitcast ptr<fnptr<whale (i32) -> i32>> %v1 to ptr<u8>
    %v5: u64 = const u64 4
    memcpy ptr<u8> %v4, ptr<u8> %v3, u64 %v5, align 1
    %v6: ptr<u8> = gep %v3, %v5
    %v7: ptr<u8> = gep %v4, %v5
    memcpy ptr<u8> %v7, ptr<u8> %v6, u64 %v5, align 1
    %v8: fnptr<whale (i32) -> i32> = load fnptr<whale (i32) -> i32>, ptr<fnptr<whale (i32) -> i32>> %v1, align 8
    %v9: fnptr<whale (i32) -> i32> = mov fnptr<whale (i32) -> i32> %v8
    %v10: bool = const bool true
    %v11: fnptr<whale (i32) -> i32> = null_function
    %v12: fnptr<whale (i32) -> i32> = select bool %v10, fnptr<whale (i32) -> i32> %v9, fnptr<whale (i32) -> i32> %v11
    br label %b1
    %b1 \"call\":
    %v13: fnptr<whale (i32) -> i32> = phi fnptr<whale (i32) -> i32> [ %v12, %b0 ]
    %v14: i32 = const i32 41
    %v15: i32 = call whale i32 indirect %v13(%v14)
    ret i32 %v15"
}

#[test]
fn function_pointer_identity_survives_return_mov_select_phi_and_split_byte_copies() {
    let body = split_callable_body();
    let m = append_using_fixture(8, &[], "i32", body);
    assert_eq!(interpret(&m, FunctionId(8), &[]).unwrap().value, Some(ConstValue::I(42)));
    let incomplete = body.replace("memcpy ptr<u8> %v7, ptr<u8> %v6, u64 %v5, align 1", "");
    memory_trap(
        &append_using_fixture(8, &[], "i32", &incomplete),
        8,
        MemoryTrap::Uninitialized { offset: 4 },
    );
    // Writing the same physical zero byte removes that fragment's authority.
    let corrupted = body.replace("%v8: fnptr", "%v16: u64 = const u64 7 %v17: ptr<u8> = gep %v4, %v16 %v18: u8 = const u8 0 store u8 %v18, ptr<u8> %v17, align 1 %v8: fnptr");
    let m = append_using_fixture(8, &[], "i32", &corrupted);
    assert!(
        matches!(interpret(&m, FunctionId(8), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::InvalidFunctionPointer)
    );
}

#[test]
fn storage_views_cannot_change_callable_signature_convention_or_data_pointer_kind() {
    let prefix = "%b0 \"entry\": %v0: ptr<fnptr<whale (i32) -> i32>> = alloca fnptr<whale (i32) -> i32>, align 8 %v1: fnptr<whale (i32) -> i32> = function_addr @f0 store fnptr<whale (i32) -> i32> %v1, ptr<fnptr<whale (i32) -> i32>> %v0, align 8";
    let body = format!("{prefix} %v2: ptr<fnptr<whale (u32) -> u32>> = bitcast ptr<fnptr<whale (i32) -> i32>> %v0 to ptr<fnptr<whale (u32) -> u32>> %v3: fnptr<whale (u32) -> u32> = load fnptr<whale (u32) -> u32>, ptr<fnptr<whale (u32) -> u32>> %v2, align 8 %v4: u32 = const u32 41 %v5: u32 = call whale u32 indirect %v3(%v4) ret u32 %v5");
    let m = append_using_fixture(8, &[], "u32", &body);
    assert!(
        matches!(interpret(&m, FunctionId(8), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::InvalidFunctionPointer)
    );
    let body = format!("{prefix} %v2: ptr<ptr<u32>> = bitcast ptr<fnptr<whale (i32) -> i32>> %v0 to ptr<ptr<u32>> %v3: ptr<u32> = load ptr<u32>, ptr<ptr<u32>> %v2, align 8 %v4: u32 = load u32, ptr<u32> %v3, align 4 ret u32 %v4");
    memory_trap(&append_using_fixture(8, &[], "u32", &body), 8, MemoryTrap::MissingProvenance);
    let m = append_using_fixture(
        8,
        &["fnptr<whale (i32) -> i32>", "i32"],
        "i32",
        "%b0 \"entry\": %v2: i32 = call whale i32 indirect %v0(%v1) ret i32 %v2",
    );
    let mut signature = FunctionSignature::whale(vec![Type::I32], Type::I32);
    signature.convention = CallingConvention::SysV64;
    let target = RuntimeValue::null_function(signature).unwrap();
    let input = RuntimeValue::scalar(Type::I32, ConstValue::I(41)).unwrap();
    assert!(matches!(
        interpret_values(&m, FunctionId(8), &[target, input]),
        Err(InterpreterError::ArgumentType { index: 0, .. })
    ));
}

#[test]
fn indirect_preflight_ignores_unrelated_external_and_unsupported_same_signature_bodies() {
    let mut m = append_using_fixture(8, &[], "i32", "%b0 \"entry\": %v0: fnptr<whale (i32) -> i32> = function_addr @f0 %v1: i32 = const i32 41 %v2: i32 = call whale i32 indirect %v0(%v1) ret i32 %v2");
    append(
        &mut m,
        9,
        &["i32"],
        "i32",
        "%b0 \"entry\": %v1: ptr<array<u8, 1>> = alloca array<u8, 1>, align 1 %v2: array<u8, 1> = load array<u8, 1>, ptr<array<u8, 1>> %v1, align 1 ret i32 %v0",
    );
    m.declarations.push(FunctionDecl {
        id: FunctionId(10),
        name: "external".into(),
        signature: FunctionSignature::whale(vec![Type::I32], Type::I32),
        linkage: Linkage::External,
        link_name: Some("external".into()),
    });
    assert_eq!(interpret(&m, FunctionId(8), &[]).unwrap().value, Some(ConstValue::I(42)));
    // A matching address in any retained block of a reachable function makes it a candidate.
    let fun = m.functions.iter_mut().find(|f| f.id == FunctionId(8)).unwrap();
    fun.blocks.push(BasicBlock {
        id: BlockId(2),
        name: "dead".into(),
        instructions: vec![Instruction::FunctionAddr {
            dst: ValueId(3),
            function: FunctionId(9),
            signature: FunctionSignature::whale(vec![Type::I32], Type::I32),
        }],
        terminator: Some(Terminator::Ret { ty: Type::I32, value: Some(ValueId(4)) }),
    });

    fun.blocks.last_mut().unwrap().instructions.push(Instruction::Const {
        dst: ValueId(4),
        ty: Type::I32,
        value: ConstValue::I(0),
    });

    fun.value_types.extend([
        (ValueId(3), Type::FnPtr(Box::new(FunctionSignature::whale(vec![Type::I32], Type::I32)))),
        (ValueId(4), Type::I32),
    ]);
    assert!(
        matches!(interpret(&m, FunctionId(8), &[]), Err(InterpreterError::UnsupportedInstruction { site, operation: "load" }) if site.function == FunctionId(9))
    );
}

#[test]
fn unused_results_execute_callee_traps_and_retired_frames_count_toward_total_budget() {
    let mut m = append_using_fixture(8, &[], "i32", "%b0 \"entry\": %v0: i32 = const i32 0 %v1: i32 = call whale i32 @f0(%v0) %v2: i32 = call whale i32 @f0(%v1) ret i32 %v2");
    assert_eq!(
        interpret_with_options(
            &m,
            FunctionId(8),
            &[],
            InterpreterOptions { max_call_depth: 2, ..Default::default() }
        )
        .unwrap()
        .value,
        Some(ConstValue::I(2))
    );
    assert!(
        matches!(interpret_with_options(&m, FunctionId(8), &[], InterpreterOptions { max_frames: 2, ..Default::default() }), Err(InterpreterError::CallLimit { site, resource: CallResource::Frames, limit: 2 }) if site.function == FunctionId(8) && site.value == Some(ValueId(2)))
    );
    let fun = m.functions.iter_mut().find(|f| f.id == FunctionId(0)).unwrap();
    fun.blocks[0].instructions.insert(
        0,
        Instruction::Const { dst: ValueId(90), ty: Type::Bool, value: ConstValue::Bool(true) },
    );
    fun.blocks[0].instructions.insert(
        1,
        Instruction::TrapIf { cond: ValueId(90), reason: "callee effects execute".into() },
    );
    fun.value_types.push((ValueId(90), Type::Bool));
    assert!(
        matches!(interpret(&m, FunctionId(8), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::Explicit("callee effects execute".into()) && t.site.function == FunctionId(0) && t.steps == 4)
    );
}

#[test]
fn callee_pointer_checks_preserve_one_past_bounds_and_expired_values_in_caller_storage() {
    let m = append_using_fixture(8, &[], "u32", "%b0 \"entry\": %v0: ptr<u32> = alloca u32, align 4 %v1: u64 = const u64 1 %v2: ptr<u32> = gep %v0, %v1 %v3: ptr<u32> = call whale ptr<u32> @f1(%v2) %v4: u32 = load u32, ptr<u32> %v0, align 4 ret u32 %v4");
    assert!(
        matches!(interpret(&m, FunctionId(8), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::Memory(MemoryTrap::OutOfBounds) && t.site.function == FunctionId(1))
    );
    let source = format!("{HEADER} declare @f0 \"local_store\": whale (ptr<ptr<u32>>) -> void, linkage internal declare @f1 \"entry\": whale () -> u32, linkage internal fn @f0 \"local_store\"(%v0 \"out\": ptr<ptr<u32>>) -> void, entry %b0 {{ %b0 \"entry\": %v1: ptr<u32> = alloca u32, align 4 store ptr<u32> %v1, ptr<ptr<u32>> %v0, align 8 ret void }} fn @f1 \"entry\"() -> u32, entry %b0 {{ %b0 \"entry\": %v0: ptr<ptr<u32>> = alloca ptr<u32>, align 8 call whale void @f0(%v0) %v1: ptr<u32> = load ptr<u32>, ptr<ptr<u32>> %v0, align 8 %v2: u32 = load u32, ptr<u32> %v1, align 4 ret u32 %v2 }} }}");
    memory_trap(&parse_module(&source).unwrap(), 1, MemoryTrap::ExpiredAllocation);
}

#[test]
fn recursive_stack_allocations_belong_to_each_invocation_and_share_memory_limits() {
    let source = format!("{HEADER} declare @f0 \"sum\": whale (u32) -> u32, linkage internal fn @f0 \"sum\"(%v0 \"n\": u32) -> u32, entry %b0 {{ %b0 \"entry\": %v1: ptr<u32> = alloca u32, align 4 store u32 %v0, ptr<u32> %v1, align 4 %v2: u32 = const u32 0 %v3: bool = icmp eq u32 %v0, %v2 cbr bool %v3, label %b1, label %b2 %b1 \"base\": ret u32 %v2 %b2 \"recursive\": %v4: u32 = const u32 1 %v5: u32 = sub u32 %v0, %v4 %v6: u32 = call whale u32 @f0(%v5) %v7: u32 = load u32, ptr<u32> %v1, align 4 %v8: u32 = add u32 %v6, %v7 ret u32 %v8 }} }}");
    let m = parse_module(&source).unwrap();
    assert_eq!(
        interpret(&m, FunctionId(0), &[ConstValue::U(4)]).unwrap().value,
        Some(ConstValue::U(10))
    );
    assert!(matches!(
        interpret_with_options(
            &m,
            FunctionId(0),
            &[ConstValue::U(4)],
            InterpreterOptions {
                memory_limits: MemoryLimits { max_bytes: 19, ..Default::default() },
                ..Default::default()
            }
        ),
        Err(InterpreterError::MemoryLimit { resource: MemoryResource::Bytes, limit: 19, .. })
    ));
    // More frames than the default host stack could safely hold still fail at the explicit budget.
    assert!(matches!(
        interpret_with_options(
            &m,
            FunctionId(0),
            &[ConstValue::U(10000)],
            InterpreterOptions { max_call_depth: 1024, ..Default::default() }
        ),
        Err(InterpreterError::CallLimit { resource: CallResource::Depth, limit: 1024, .. })
    ));
}

#[test]
fn invocation_value_budget_rejects_large_live_environments_before_entering_a_callee() {
    let m = fixture();
    // The recursive body reserves seven SSA slots, independently of its chosen path.
    let options = InterpreterOptions { max_live_values: 13, ..Default::default() };
    assert!(
        matches!(interpret_with_options(&m, FunctionId(5), &[ConstValue::I(2)], options), Err(InterpreterError::CallLimit { site, resource: CallResource::Values, limit: 13 }) if site.function == FunctionId(5) && site.value == Some(ValueId(5)))
    );
    assert!(
        matches!(interpret_with_options(&m, FunctionId(5), &[ConstValue::I(0)], InterpreterOptions { max_live_values: 6, ..Default::default() }), Err(InterpreterError::CallLimit { site, resource: CallResource::Values, limit: 6 }) if site.instruction == 0)
    );
    let m = append_using_fixture(8, &[], "i32", "%b0 \"entry\": %v0: i32 = const i32 0 %v1: i32 = call whale i32 @f0(%v0) %v2: i32 = call whale i32 @f0(%v1) ret i32 %v2");
    // Retiring the first callee releases its SSA reservation for the second call.
    assert_eq!(
        interpret_with_options(
            &m,
            FunctionId(8),
            &[],
            InterpreterOptions { max_live_values: 8, ..Default::default() }
        )
        .unwrap()
        .value,
        Some(ConstValue::I(2))
    );
}

#[test]
fn callable_library_values_cannot_rebind_equal_ids_and_signatures_in_another_module() {
    let m = append_using_fixture(
        8,
        &["fnptr<whale (i32) -> i32>", "i32"],
        "i32",
        "%b0 \"entry\": %v2: i32 = call whale i32 indirect %v0(%v1) ret i32 %v2",
    );
    let target = RuntimeValue::function(&m, FunctionId(0)).unwrap();
    let returned_target = interpret_values(&m, FunctionId(7), &[]).unwrap().value.unwrap();
    let input = RuntimeValue::scalar(Type::I32, ConstValue::I(41)).unwrap();
    let mut other = m.clone();
    let fun = other.functions.iter_mut().find(|f| f.id == FunctionId(0)).unwrap();
    let Instruction::Const { value, .. } = &mut fun.blocks[0].instructions[3] else {
        unreachable!()
    };
    *value = ConstValue::I(2);
    for target in [target, returned_target] {
        assert_eq!(
            interpret_values(&m, FunctionId(8), &[target.clone(), input.clone()])
                .unwrap()
                .value
                .unwrap()
                .scalar_value(),
            Some(&ConstValue::I(42))
        );
        assert!(matches!(
            interpret_values(&other, FunctionId(8), &[target, input.clone()]),
            Err(InterpreterError::ArgumentType { index: 0, .. })
        ));
    }
    // An explicit constructor for the other module refers to that module's own function.
    let target = RuntimeValue::function(&other, FunctionId(0)).unwrap();
    assert_eq!(
        interpret_values(&other, FunctionId(8), &[target, input])
            .unwrap()
            .value
            .unwrap()
            .scalar_value(),
        Some(&ConstValue::I(43))
    );
}

#[test]
fn callable_storage_preserves_null_initialization_and_bool_representation_checks() {
    let body = "%b0 \"entry\": %v0: ptr<fnptr<whale (i32) -> i32>> = alloca fnptr<whale (i32) -> i32>, align 8 %v1: fnptr<whale (i32) -> i32> = null_function store fnptr<whale (i32) -> i32> %v1, ptr<fnptr<whale (i32) -> i32>> %v0, align 8 %v2: fnptr<whale (i32) -> i32> = load fnptr<whale (i32) -> i32>, ptr<fnptr<whale (i32) -> i32>> %v0, align 8 %v3: i32 = const i32 41 %v4: i32 = call whale i32 indirect %v2(%v3) ret i32 %v4";
    let m = append_using_fixture(8, &[], "i32", body);
    assert!(
        matches!(interpret(&m, FunctionId(8), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::NullFunction)
    );
    let uninitialized = body.replace(
        "store fnptr<whale (i32) -> i32> %v1, ptr<fnptr<whale (i32) -> i32>> %v0, align 8",
        "",
    );
    memory_trap(
        &append_using_fixture(8, &[], "i32", &uninitialized),
        8,
        MemoryTrap::Uninitialized { offset: 0 },
    );
    let m = append_using_fixture(8, &[], "bool", "%b0 \"entry\": %v0: ptr<fnptr<whale (ptr<u32>) -> ptr<u32>>> = alloca fnptr<whale (ptr<u32>) -> ptr<u32>>, align 8 %v1: fnptr<whale (ptr<u32>) -> ptr<u32>> = function_addr @f1 store fnptr<whale (ptr<u32>) -> ptr<u32>> %v1, ptr<fnptr<whale (ptr<u32>) -> ptr<u32>>> %v0, align 8 %v2: ptr<bool> = bitcast ptr<fnptr<whale (ptr<u32>) -> ptr<u32>>> %v0 to ptr<bool> %v3: bool = load bool, ptr<bool> %v2, align 1 ret bool %v3");
    memory_trap(&m, 8, MemoryTrap::InvalidBool { byte: 2 });
}

#[test]
fn callable_metadata_and_copied_fragments_use_the_shared_memory_budget() {
    let m = append_using_fixture(8, &[], "i32", split_callable_body());
    assert!(
        matches!(interpret_with_options(&m, FunctionId(8), &[], InterpreterOptions { memory_limits: MemoryLimits { max_pointer_fragments: 15, ..Default::default() }, ..Default::default() }), Err(InterpreterError::MemoryLimit { site, resource: MemoryResource::PointerFragments, limit: 15 }) if site.function == FunctionId(8) && site.instruction == 10)
    );
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

fn interpret_values<'a>(
    module: &'a Module,
    function: FunctionId,
    args: &[RuntimeValue<'_>],
) -> Result<RuntimeInterpreterResult<'a>, InterpreterError> {
    ir::interpret_values(module, function, args).map_err(InterpreterError::into_root_cause)
}
