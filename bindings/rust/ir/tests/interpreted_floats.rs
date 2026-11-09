use ir::*;

const HEADER: &str = "module { format_version 7 semantics_version 1 target \"x86_64-whale-linux\" datalayout { ptr=64, endian=little }";

fn program(types: &[&str], ret: &str, body: &str) -> Module {
    let params = types
        .iter()
        .enumerate()
        .map(|(i, t)| format!("%v{i} \"p{i}\": {t}"))
        .collect::<Vec<_>>()
        .join(", ");
    parse_module(&format!("{HEADER} declare @f0 \"test\": whale ({}) -> {ret}, linkage internal fn @f0 \"test\"({params}) -> {ret}, entry %b0 {{ {body} }} }}", types.join(", "))).unwrap()
}

fn execute(types: &[&str], ret: &str, body: &str, args: &[ConstValue]) -> ConstValue {
    interpret(&program(types, ret, body), FunctionId(0), args).unwrap().value.unwrap()
}

fn vectors() -> Vec<(&'static str, FloatBits)> {
    [
        ("f16", vec![0, 0x8000, 1, 0x8001, 0x3c00, 0x7bff, 0x7c00, 0xfc00, 0x7c01, 0x7e01, 0xffff]),
        (
            "f32",
            vec![
                0, 0x80000000, 1, 0x80000001, 0x3f800000, 0x7f7fffff, 0x7f800000, 0xff800000,
                0x7f800001, 0x7fc01234, 0xffffffff,
            ],
        ),
        (
            "f64",
            vec![
                0,
                0x8000000000000000,
                1,
                0x8000000000000001,
                0x3ff0000000000000,
                0x7fefffffffffffff,
                0x7ff0000000000000,
                0xfff0000000000000,
                0x7ff0000000000001,
                0x7ff8123456789abc,
                0xffffffffffffffff,
            ],
        ),
    ]
    .into_iter()
    .flat_map(|(ty, bits)| {
        bits.into_iter().map(move |b| {
            (
                ty,
                match ty {
                    "f16" => FloatBits::F16(b as u16),
                    "f32" => FloatBits::F32(b as u32),
                    _ => FloatBits::F64(b),
                },
            )
        })
    })
    .collect()
}

#[test]
fn exact_constants_declarations_arguments_and_control_flow_retain_storage_bits() {
    for (ty, bits) in vectors() {
        let value = ConstValue::F(bits);
        for source in [
            format!("const {ty} {bits}"),
            format!("const_decl \"bits\" {ty} {bits} => const {ty} {bits}"),
        ] {
            assert_eq!(
                execute(&[], ty, &format!("%b0 \"entry\": %v1: {ty} = {source} ret {ty} %v1"), &[]),
                value
            );
        }
        let body=format!("%b0 \"entry\": %v2: {ty} = mov {ty} %v0 cbr bool %v1, label %b1, label %b2 %b1 \"true\": br label %b3 %b2 \"false\": br label %b3 %b3 \"join\": %v3: {ty} = phi {ty} [ %v0, %b1 ], [ %v2, %b2 ] %v4: {ty} = select bool %v1, {ty} %v3, {ty} %v2 ret {ty} %v4");
        for condition in [false, true] {
            assert_eq!(
                execute(&[ty, "bool"], ty, &body, &[value.clone(), ConstValue::Bool(condition)]),
                value
            );
        }
        let runtime = RuntimeValue::scalar(
            match bits {
                FloatBits::F16(_) => Type::F16,
                FloatBits::F32(_) => Type::F32,
                FloatBits::F64(_) => Type::F64,
            },
            value.clone(),
        )
        .unwrap();
        let module = program(&[ty], ty, &format!("%b0 \"entry\": ret {ty} %v0"));
        let result = interpret_values(&module, FunctionId(0), &[runtime]).unwrap();
        assert_eq!(result.value.unwrap().scalar_value(), Some(&value));
    }
}

#[test]
fn signed_unsigned_bitcasts_and_storage_roundtrips_preserve_every_edge_pattern() {
    for (ty, bits) in vectors() {
        let width = bits.width();
        for int in [format!("i{width}"), format!("u{width}")] {
            let body=format!("%b0 \"entry\": %v1: {int} = bitcast {ty} %v0 to {int} %v2: ptr<{int}> = alloca {int}, align 1 store {int} %v1, ptr<{int}> %v2, align 1 %v3: {int} = load {int}, ptr<{int}> %v2, align 1 %v4: {ty} = bitcast {int} %v3 to {ty} %v5: ptr<{ty}> = alloca {ty}, align 1 store {ty} %v4, ptr<{ty}> %v5, align 1 %v6: {ty} = load {ty}, ptr<{ty}> %v5, align 1 ret {ty} %v6");
            assert_eq!(execute(&[ty], ty, &body, &[ConstValue::F(bits)]), ConstValue::F(bits));
            // Independently check the integer result, including its sign bit.
            let result = execute(
                &[ty],
                &int,
                &format!("%b0 \"entry\": %v1: {int} = bitcast {ty} %v0 to {int} ret {int} %v1"),
                &[ConstValue::F(bits)],
            );
            let expected = if int.starts_with('u') {
                ConstValue::U(bits.bits().into())
            } else {
                let shift = 128 - u32::from(width);
                ConstValue::I(((i128::from(bits.bits())) << shift) >> shift)
            };
            assert_eq!(result, expected);
        }
    }
}

#[test]
fn tracked_float_globals_and_split_copies_preserve_bits() {
    for (ty, bits) in vectors() {
        let n = bits.width() / 8;
        let source=format!("{HEADER} storage @g0 \"bits\": {ty} = value {ty} {bits}, align 1, access readwrite, linkage internal declare @f0 \"test\": whale () -> {ty}, linkage internal fn @f0 \"test\"() -> {ty}, entry %b0 {{ %b0 \"entry\": %v0: ptr<{ty}> = global_addr @g0 %v1: ptr<{ty}> = alloca {ty}, align 1 %v2: ptr<u8> = bitcast ptr<{ty}> %v0 to ptr<u8> %v3: ptr<u8> = bitcast ptr<{ty}> %v1 to ptr<u8> %v4: u64 = const u64 1 memcpy ptr<u8> %v3, ptr<u8> %v2, u64 %v4, align 1 %v5: ptr<u8> = gep %v2, %v4 %v6: ptr<u8> = gep %v3, %v4 %v7: u64 = const u64 {} memcpy ptr<u8> %v6, ptr<u8> %v5, u64 %v7, align 1 %v8: {ty} = load {ty}, ptr<{ty}> %v1, align 1 ret {ty} %v8 }} }}",n-1);
        let module = parse_module(&source).unwrap();
        for _ in 0..2 {
            assert_eq!(
                interpret(&module, FunctionId(0), &[]).unwrap().value,
                Some(ConstValue::F(bits))
            );
        }
    }
}

#[test]
fn floating_fields_skip_padding_but_value_bytes_require_initialization() {
    for (ty, bits) in vectors() {
        let body=format!("%b0 \"entry\": %v0: ptr<struct{{u8, {ty}}}> = alloca struct{{u8, {ty}}}, align 8 %v1: u64 = const u64 0 %v2: u64 = const u64 1 %v3: ptr<{ty}> = gep %v0, %v1, %v2 %v4: {ty} = const {ty} {bits} store {ty} %v4, ptr<{ty}> %v3, align 1 %v5: {ty} = load {ty}, ptr<{ty}> %v3, align 1 ret {ty} %v5");
        assert_eq!(execute(&[], ty, &body, &[]), ConstValue::F(bits));
        let unset = body.replace(
            &format!("store {ty} %v4, ptr<{ty}> %v3, align 1"),
            &format!("uninit {ty}, ptr<{ty}> %v3, align 1"),
        );
        assert!(matches!(
            interpret(&program(&[], ty, &unset), FunctionId(0), &[]),
            Err(InterpreterError::Trap(InterpreterTrap {
                reason: TrapReason::Memory(MemoryTrap::Uninitialized { .. }),
                ..
            }))
        ));
    }
    let body="%b0 \"entry\": %v0: ptr<array<u8, 16>> = alloca array<u8, 16>, align 8 %v1: ptr<u8> = bitcast ptr<array<u8, 16>> %v0 to ptr<u8> %v2: u64 = const u64 1 %v3: ptr<u8> = gep %v1, %v2 %v4: ptr<f64> = bitcast ptr<u8> %v3 to ptr<f64> %v5: f64 = load f64, ptr<f64> %v4, align 8 ret f64 %v5";
    assert!(matches!(
        interpret(&program(&[], "f64", body), FunctionId(0), &[]),
        Err(InterpreterError::Trap(InterpreterTrap {
            reason: TrapReason::Memory(MemoryTrap::Misaligned { alignment: 8, .. }),
            ..
        }))
    ));
}

#[test]
fn partial_float_copy_transfers_initialization_masks() {
    let body="%b0 \"entry\": %v0: ptr<f64> = alloca f64, align 8 %v1: ptr<f64> = alloca f64, align 8 %v2: ptr<u8> = bitcast ptr<f64> %v0 to ptr<u8> %v3: ptr<u8> = bitcast ptr<f64> %v1 to ptr<u8> %v4: u8 = const u8 255 store u8 %v4, ptr<u8> %v2, align 1 %v5: u64 = const u64 8 memcpy ptr<u8> %v3, ptr<u8> %v2, u64 %v5, align 8 %v6: f64 = load f64, ptr<f64> %v1, align 8 ret f64 %v6";
    assert!(matches!(
        interpret(&program(&[], "f64", body), FunctionId(0), &[]),
        Err(InterpreterError::Trap(InterpreterTrap {
            reason: TrapReason::Memory(MemoryTrap::Uninitialized { offset: 1 }),
            ..
        }))
    ));
}

#[test]
fn float_bits_do_not_recreate_pointer_authority() {
    let body="%b0 \"entry\": %v0: ptr<u32> = alloca u32, align 4 %v1: u64 = ptrtoint ptr<u32> %v0 to u64 %v2: f64 = bitcast u64 %v1 to f64 %v3: u64 = bitcast f64 %v2 to u64 %v4: ptr<u32> = inttoptr u64 %v3 to ptr<u32> %v5: u32 = load u32, ptr<u32> %v4, align 4 ret u32 %v5";
    assert!(matches!(
        interpret(&program(&[], "u32", body), FunctionId(0), &[]),
        Err(InterpreterError::Trap(InterpreterTrap {
            reason: TrapReason::Memory(MemoryTrap::MissingProvenance),
            ..
        }))
    ));
    // A floating store clears pointer metadata even when all address bits agree.
    let body=body.replace("%v4: ptr<u32> = inttoptr u64 %v3 to ptr<u32>","%v6: ptr<ptr<u32>> = alloca ptr<u32>, align 8 store ptr<u32> %v0, ptr<ptr<u32>> %v6, align 8 %v7: ptr<f64> = bitcast ptr<ptr<u32>> %v6 to ptr<f64> store f64 %v2, ptr<f64> %v7, align 8 %v4: ptr<u32> = load ptr<u32>, ptr<ptr<u32>> %v6, align 8");
    assert!(matches!(
        interpret(&program(&[], "u32", &body), FunctionId(0), &[]),
        Err(InterpreterError::Trap(InterpreterTrap {
            reason: TrapReason::Memory(MemoryTrap::MissingProvenance),
            ..
        }))
    ));
}

#[test]
fn direct_and_indirect_float_calls_share_storage_without_rounding() {
    for (ty, bits) in vectors() {
        let source=format!("{HEADER} declare @f0 \"entry\": whale ({ty}) -> {ty}, linkage internal declare @f1 \"identity\": whale ({ty}) -> {ty}, linkage internal fn @f0 \"entry\"(%v0 \"value\": {ty}) -> {ty}, entry %b0 {{ %b0 \"entry\": %v1: {ty} = call whale {ty} @f1(%v0) %v2: fnptr<whale ({ty}) -> {ty}> = function_addr @f1 %v3: {ty} = call whale {ty} indirect %v2(%v1) ret {ty} %v3 }} fn @f1 \"identity\"(%v0 \"value\": {ty}) -> {ty}, entry %b0 {{ %b0 \"entry\": %v1: ptr<{ty}> = alloca {ty}, align 1 store {ty} %v0, ptr<{ty}> %v1, align 1 %v2: {ty} = load {ty}, ptr<{ty}> %v1, align 1 ret {ty} %v2 }} }}");
        assert_eq!(
            interpret(&parse_module(&source).unwrap(), FunctionId(0), &[ConstValue::F(bits)])
                .unwrap()
                .value,
            Some(ConstValue::F(bits))
        );
    }
}

#[test]
fn invalid_bitcasts_remain_rejected_and_float_arithmetic_executes() {
    for (src, dst, value) in [
        ("f16", "u32", "0x7e01"),
        ("bool", "f16", "true"),
        ("f64", "ptr<u8>", "0x0000000000000000"),
    ] {
        let source=format!("{HEADER} declare @f0 \"bad\": whale () -> {dst}, linkage internal fn @f0 \"bad\"() -> {dst}, entry %b0 {{ %b0 \"entry\": %v0: {src} = const {src} {value} %v1: {dst} = bitcast {src} %v0 to {dst} ret {dst} %v1 }} }}");
        assert!(parse_module(&source).is_err());
    }
    let module =
        program(&["f32"], "f32", "%b0 \"entry\": %v1: f32 = fadd f32 %v0, %v0 ret f32 %v1");
    assert_eq!(
        interpret(&module, FunctionId(0), &[ConstValue::F(FloatBits::F32(0))]).unwrap().value,
        Some(ConstValue::F(FloatBits::F32(0)))
    );
}
