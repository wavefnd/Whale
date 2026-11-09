use ir::*;

fn header() -> String {
    format!("module {{ format_version {} semantics_version 1 target \"x86_64-whale-linux\" datalayout {{ptr=64,endian=little}}",IR_FORMAT_VERSION)
}

#[test]
fn memory_instruction_corpus_is_also_bounded_by_shared_ir_fuel() {
    let text=format!("{} declare @f0 \"bounded\": whale () -> void, linkage internal fn @f0 \"bounded\"() -> void, entry %b0 {{ %b0 \"entry\": %v0: ptr<u8> = alloca u8, align 1 %v1: u8 = const u8 42 br label %b1 %b1 \"loop\": store u8 %v1, ptr<u8> %v0, align 1 %v2: u8 = load u8, ptr<u8> %v0, align 1 uninit u8, ptr<u8> %v0, align 1 br label %b1 }} }}",header());
    let module = parse_module(&text).unwrap();
    for fuel in [0, 1, 3, 7, 31] {
        assert!(
            matches!(interpret_with_options(&module,FunctionId(0),&[],InterpreterOptions{max_steps:fuel,memory_limits:MemoryLimits{max_allocations:1,max_bytes:1,max_pointer_fragments:0,max_work:4096},..Default::default()}),Err(InterpreterError::StepLimit{limit,..})if limit==fuel)
        );
    }
}

#[test]
fn explicit_lifetime_end_invalidates_generated_stored_and_gep_aliases() {
    // Batch5 introduces the operation in format6. This fixture is intentionally
    // dormant on older feature branches and executes after that batch lands.
    for alias in ["%v0", "%v3"] {
        let text=format!("{} declare @f0 \"retired\": whale () -> u8, linkage internal fn @f0 \"retired\"() -> u8, entry %b0 {{ %b0 \"entry\": %v0: ptr<u8> = alloca u8, align 1 %v1: ptr<ptr<u8>> = alloca ptr<u8>, align 8 store ptr<u8> %v0, ptr<ptr<u8>> %v1, align 8 %v2: i32 = const i32 0 %v3: ptr<u8> = gep %v0, %v2 lifetime_end {alias} %v4: ptr<u8> = load ptr<u8>, ptr<ptr<u8>> %v1, align 8 %v5: u8 = load u8, ptr<u8> %v4, align 1 ret u8 %v5 }} }}",header());
        parse_module(&text.replace(&format!("lifetime_end {alias}"), "")).unwrap();
        if IR_FORMAT_VERSION < 6 {
            continue;
        }
        let module = parse_module(&text).unwrap();
        let error = interpret(&module, FunctionId(0), &[]).unwrap_err();
        assert!(error.to_string().contains("allocation lifetime has ended"), "{error}");
    }
}
