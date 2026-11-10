use ir::*;

fn fixture() -> Module {
    parse_module(include_str!("fixtures/runtime-diagnostics-v7.wir")).unwrap()
}

fn single(ret: &str, body: &str) -> Module {
    parse_module(&format!("module {{ format_version 7 semantics_version 1 target \"x86_64-whale-linux\" datalayout {{ ptr=64, endian=little }} declare @f0 \"test\": whale () -> {ret}, linkage internal fn @f0 \"test\"() -> {ret}, entry %b0 {{ %b0 \"entry\": {body} }} }}")).unwrap()
}

fn annotate(m: &mut Module, target: SourceTarget, id: u64, line: u32) {
    if m.sources.files.is_empty() {
        m.sources.files.push(SourceFile { id: 0, name: "provided.wave".into() });
    }
    m.sources.originals.push(OriginalNode {
        id,
        span: Some(SourceSpan { file: 0, line, column: 7, end_line: line, end_column: 12 }),
    });
    m.sources.mappings.push(SourceMapping { target, original: id });
}

#[test]
fn direct_and_indirect_failures_preserve_leaf_source_and_ordered_caller_chain() {
    let m = fixture();
    let error = interpret(&m, FunctionId(0), &[]).unwrap_err();
    let InterpreterError::Trap(trap) = error.root_cause() else { panic!("{error:?}") };
    assert_eq!(trap.reason, TrapReason::DivisionByZero);
    assert_eq!(trap.steps, 5);
    let d = error.diagnostic().unwrap();
    assert_eq!(d.site, Some(trap.site));
    assert_eq!(d.original.as_ref().unwrap().original, 300);
    let (file, span) = d.original.as_ref().unwrap().source.as_ref().unwrap();
    assert_eq!(file, "original.wave");
    assert_eq!((span.line, span.column), (11, 13));
    assert_eq!(d.calls.len(), 2);
    assert_eq!((d.calls[0].caller, d.calls[0].callee), (FunctionId(0), FunctionId(1)));
    assert_eq!((d.calls[1].caller, d.calls[1].callee), (FunctionId(1), FunctionId(2)));
    assert_eq!(d.calls[0].original.as_ref().unwrap().original, 100);
    assert_eq!(d.calls[1].original.as_ref().unwrap().original, 200);
    let human = error.to_string();
    assert!(human.contains("original.wave:11:13"));
    assert!(human.contains("@f2 %b0 instruction 1"));
    assert_eq!(human.matches("called from").count(), 2);
    assert_eq!(d.omitted_call_frames, 0);
    assert_eq!(d.omitted_source_details, 0);
}

#[test]
fn source_mapping_roundtrip_never_changes_runtime_cause_site_or_step_order() {
    let m = fixture();
    let printed = print_module(&m);
    let parsed = parse_module(&printed).unwrap();
    assert_eq!(print_module(&parsed), printed);
    let mut plain = m.clone();
    plain.sources = SourceMetadata::default();
    let mapped = interpret(&parsed, FunctionId(0), &[]).unwrap_err();
    let raw = interpret(&plain, FunctionId(0), &[]).unwrap_err();
    let (InterpreterError::Trap(mapped), InterpreterError::Trap(raw)) =
        (mapped.root_cause(), raw.root_cause())
    else {
        panic!()
    };
    assert_eq!(mapped, raw);
}

#[test]
fn initialization_bounds_and_explicit_instruction_traps_report_the_original_operation() {
    for (ret,body,index,expected) in [
        ("u32","%v0: ptr<u32> = alloca u32, align 4 %v1: u32 = load u32, ptr<u32> %v0, align 4 ret u32 %v1",1,TrapReason::Memory(MemoryTrap::Uninitialized {offset:0})),
        ("u32","%v0: ptr<u32> = alloca u32, align 4 %v1: u64 = const u64 1 %v2: ptr<u32> = gep %v0, %v1 %v3: u32 = load u32, ptr<u32> %v2, align 4 ret u32 %v3",3,TrapReason::Memory(MemoryTrap::OutOfBounds)),
        ("void","%v0: bool = const bool true trap_if bool %v0, reason=\"provided reason\" ret void",1,TrapReason::Explicit("provided reason".into())),
    ] {
        let mut m=single(ret,body);annotate(&mut m,SourceTarget::Instruction {function:FunctionId(0),block:BlockId(0),index},500,24);
        let error=interpret(&m,FunctionId(0),&[]).unwrap_err();assert!(matches!(error.root_cause(),InterpreterError::Trap(t) if t.reason==expected));
        let d=error.diagnostic().unwrap();assert_eq!(d.original.as_ref().unwrap().original,500);assert!(d.calls.is_empty());assert!(error.to_string().contains("provided.wave:24:7"));
    }
}

#[test]
fn terminators_have_their_own_source_identity_including_explicit_traps_and_fuel() {
    let mut m = fixture();
    let f = m.functions.iter_mut().find(|f| f.id == FunctionId(2)).unwrap();
    let Instruction::Bin { op, .. } = &mut f.blocks[0].instructions[1] else { panic!() };
    *op = BinOp::Add;
    f.blocks[0].terminator = Some(Terminator::Trap { reason: "terminator stop".into() });
    let error = interpret(&m, FunctionId(0), &[]).unwrap_err();
    assert!(matches!(error.root_cause(),InterpreterError::Trap(t) if t.site.instruction==2));
    assert_eq!(error.diagnostic().unwrap().original.as_ref().unwrap().original, 301);
    assert!(error.to_string().contains("original.wave:12:1"));
    let error = interpret_with_options(
        &m,
        FunctionId(0),
        &[],
        InterpreterOptions { max_steps: 5, ..Default::default() },
    )
    .unwrap_err();
    assert!(
        matches!(error.root_cause(),InterpreterError::StepLimit {site,..} if site.instruction==2)
    );
    assert_eq!(error.diagnostic().unwrap().original.as_ref().unwrap().original, 301);
}

#[test]
fn fuel_depth_frame_and_live_value_failures_keep_bounded_structured_context() {
    let m = fixture();
    for options in [
        InterpreterOptions { max_steps: 4, ..Default::default() },
        InterpreterOptions { max_call_depth: 2, ..Default::default() },
        InterpreterOptions { max_frames: 2, ..Default::default() },
        InterpreterOptions { max_live_values: 3, ..Default::default() },
    ] {
        let error = interpret_with_options(&m, FunctionId(0), &[], options).unwrap_err();
        let d = error.diagnostic().unwrap();
        assert!(matches!(
            error.root_cause(),
            InterpreterError::StepLimit { .. } | InterpreterError::CallLimit { .. }
        ));
        assert!(!d.calls.is_empty());
        assert!(d.calls.len() <= 64);
        assert!(d.original.is_some());
    }
    let error = interpret_with_options(
        &m,
        FunctionId(0),
        &[],
        InterpreterOptions { max_call_depth: 0, ..Default::default() },
    )
    .unwrap_err();
    assert_eq!(error.diagnostic().unwrap().original.as_ref().unwrap().original, 100);
}

#[test]
fn unannotated_inputs_keep_ir_sites_and_never_infer_a_source_from_names_or_canonical_lines() {
    let m = single("u32", "%v0: u32 = const u32 0 %v1: u32 = udiv u32 %v0, %v0 ret u32 %v1");
    let error = interpret(&m, FunctionId(0), &[]).unwrap_err();
    assert!(error.diagnostic().is_none());
    assert!(matches!(error, InterpreterError::Trap(_)));
    assert!(!error.to_string().contains(".wave:"));
    let mut m = fixture();
    m.sources = SourceMetadata::default();
    let error = interpret(&m, FunctionId(0), &[]).unwrap_err();
    let d = error.diagnostic().unwrap();
    assert!(d.original.is_none());
    assert!(d.calls.iter().all(|call| call.original.is_none()));
    assert!(error.to_string().contains("@f2 %b0"));
}

#[test]
fn recursion_trace_limits_truncate_outer_frames_and_the_hard_cap_cannot_be_bypassed() {
    for (body, instruction) in [("call whale void @f0() ret void", 0), ("%v0: fnptr<whale () -> void> = function_addr @f0 call whale void indirect %v0() ret void", 1)] {
        let m = single("void", body);
        for (bound, retained) in [(0, 0), (3, 3), (64, 64), (usize::MAX, MAX_RUNTIME_TRACE_FRAMES)] {
            let error = interpret_with_options(&m, FunctionId(0), &[], InterpreterOptions { max_call_depth: 400, max_trace_frames: bound, ..Default::default() }).unwrap_err();
            assert!(matches!(error.root_cause(), InterpreterError::CallLimit { resource: CallResource::Depth, limit: 400, .. }));
            let d = error.diagnostic().unwrap(); assert_eq!(d.calls.len(), retained); assert_eq!(d.omitted_call_frames, 399 - retained);
            assert!(d.calls.iter().all(|call| call.caller == FunctionId(0) && call.callee == FunctionId(0) && call.site.instruction == instruction));
        }
        let error = interpret_with_options(&m, FunctionId(0), &[], InterpreterOptions { max_call_depth: 1000, max_frames: 400, ..Default::default() }).unwrap_err();
        assert!(matches!(error.root_cause(), InterpreterError::CallLimit { resource: CallResource::Frames, limit: 400, .. }));
        assert_eq!(error.diagnostic().unwrap().calls.len(), 64);
        let error = interpret_with_options(&m, FunctionId(0), &[], InterpreterOptions { max_call_depth: 1000, max_steps: 400, ..Default::default() }).unwrap_err();
        assert!(matches!(error.root_cause(), InterpreterError::StepLimit { limit: 400, .. }));
        assert_eq!(error.diagnostic().unwrap().calls.len(), 64);
    }
}

#[test]
fn source_detail_byte_limits_are_explicit_and_keep_original_and_ir_ids() {
    let mut m = fixture();
    m.sources.files[0].name = "x".repeat(MAX_RUNTIME_DIAGNOSTIC_SOURCE_BYTES + 1);
    let error = interpret(&m, FunctionId(0), &[]).unwrap_err();
    let d = error.diagnostic().unwrap();
    assert_eq!(d.omitted_source_details, 3);
    assert_eq!(d.original.as_ref().unwrap().original, 300);
    assert!(d.original.as_ref().unwrap().source.is_none());
    assert!(error.to_string().len() < 1000);
    m.sources.files[0].name = "x".repeat(MAX_RUNTIME_DIAGNOSTIC_SOURCE_BYTES);
    let error = interpret(&m, FunctionId(0), &[]).unwrap_err();
    let d = error.diagnostic().unwrap();
    assert_eq!(d.omitted_source_details, 2);
    let copied = d
        .original
        .iter()
        .chain(d.calls.iter().filter_map(|call| call.original.as_ref()))
        .filter_map(|origin| origin.source.as_ref())
        .map(|(file, _)| file.len())
        .sum::<usize>();
    assert_eq!(copied, MAX_RUNTIME_DIAGNOSTIC_SOURCE_BYTES);
}

#[test]
fn global_initialization_failures_report_global_original_metadata_without_a_fabricated_function_site(
) {
    let mut m = single("void", "ret void");
    m.global_storage.push(GlobalStorage::internal(
        "g",
        Type::U32,
        GlobalStorageInit::Value(ConstValue::U(42)),
        true,
        4,
    ));
    annotate(&mut m, SourceTarget::Global(GlobalId(0)), 900, 33);
    verify_module(&m).unwrap();
    let error = interpret_with_options(
        &m,
        FunctionId(0),
        &[],
        InterpreterOptions {
            memory_limits: MemoryLimits { max_bytes: 0, ..Default::default() },
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error.root_cause(),
        InterpreterError::GlobalMemoryLimit { global: GlobalId(0), .. }
    ));
    let d = error.diagnostic().unwrap();
    assert_eq!(d.global, Some(GlobalId(0)));
    assert!(d.site.is_none());
    assert_eq!(d.original.as_ref().unwrap().original, 900);
    assert!(error.to_string().contains("provided.wave:33:7"));
}

#[test]
fn malformed_metadata_is_rejected_and_unrelated_metadata_does_not_label_a_failure() {
    let mut m = fixture();
    m.sources.mappings[0].target =
        SourceTarget::Instruction { function: FunctionId(99), block: BlockId(0), index: 0 };
    assert!(matches!(interpret(&m, FunctionId(0), &[]), Err(InterpreterError::Verification(_))));
    let mut m = single("u32", "%v0: u32 = const u32 0 %v1: u32 = udiv u32 %v0, %v0 ret u32 %v1");
    annotate(
        &mut m,
        SourceTarget::Instruction { function: FunctionId(0), block: BlockId(0), index: 0 },
        800,
        20,
    );
    assert!(interpret(&m, FunctionId(0), &[]).unwrap_err().diagnostic().is_none());
}

#[test]
fn mapped_memory_limits_and_identity_only_origins_keep_the_exact_leaf_cause() {
    let mut m = single("void", "%v0: ptr<u32> = alloca u32, align 4 ret void");
    m.sources.originals.push(OriginalNode { id: 777, span: None });
    m.sources.mappings.push(SourceMapping {
        target: SourceTarget::Instruction { function: FunctionId(0), block: BlockId(0), index: 0 },
        original: 777,
    });
    let error = interpret_with_options(
        &m,
        FunctionId(0),
        &[],
        InterpreterOptions {
            memory_limits: MemoryLimits { max_bytes: 0, ..Default::default() },
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error.root_cause(),
        InterpreterError::MemoryLimit { resource: MemoryResource::Bytes, limit: 0, .. }
    ));
    let origin = error.diagnostic().unwrap().original.as_ref().unwrap();
    assert_eq!(origin.original, 777);
    assert!(origin.source.is_none());
    assert!(error.to_string().contains("original @n777"));
    assert!(error.to_string().contains("@f0 %b0 instruction 0"));
}
