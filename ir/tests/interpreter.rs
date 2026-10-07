use ir::*;

const HEADER: &str = "module { format_version 3 semantics_version 1 target \"x86_64-whale-linux\" datalayout { ptr=64, endian=little }";
fn program(types: &[&str], ret: &str, blocks: &str) -> Module {
    let params = types
        .iter()
        .enumerate()
        .map(|(i, t)| format!("%v{i} \"p{i}\": {t}"))
        .collect::<Vec<_>>()
        .join(", ");
    parse_module(&format!("{HEADER} declare @f7 \"test\": whale ({}) -> {ret}, linkage internal fn @f7 \"test\"({params}) -> {ret}, entry %b11 {{ {blocks} }} }}", types.join(", "))).unwrap()
}
fn run(
    types: &[&str],
    ret: &str,
    body: &str,
    args: &[ConstValue],
) -> Result<InterpreterResult, InterpreterError> {
    interpret(
        &program(types, ret, &format!("%b11 \"entry\": {body}")),
        FunctionId(7),
        args,
    )
}
fn arithmetic(ty: &str, op: &str, a: ConstValue, b: ConstValue) -> ConstValue {
    run(
        &[ty, ty],
        ty,
        &format!("%v8: {ty} = {op} {ty} %v0, %v1 ret {ty} %v8"),
        &[a, b],
    )
    .unwrap()
    .value
    .unwrap()
}
fn pair(ty: &str, op: &str, a: ConstValue, b: ConstValue) -> (ConstValue, bool) {
    let body = format!("%v8: tuple<{ty}, bool> = {op} {ty} %v0, %v1 %v9: {ty} = extract %v8, 0 %v10: bool = extract %v8, 1");
    let args = [a, b];
    let value = run(&[ty, ty], ty, &format!("{body} ret {ty} %v9"), &args)
        .unwrap()
        .value
        .unwrap();
    let overflow = run(&[ty, ty], "bool", &format!("{body} ret bool %v10"), &args)
        .unwrap()
        .value
        .unwrap();
    (value, overflow == ConstValue::Bool(true))
}
fn literal(value: &ConstValue) -> String {
    match value {
        ConstValue::I(v) => v.to_string(),
        ConstValue::U(v) => v.to_string(),
        _ => unreachable!(),
    }
}
#[test]
fn every_integer_width_wraps_and_checked_overflow_agrees_with_constant_evaluation() {
    for bits in [1, 8, 16, 32, 64, 128] {
        let max = if bits == 128 {
            u128::MAX
        } else {
            (1u128 << bits) - 1
        };
        let ty = format!("u{bits}");
        let cases = [
            (
                "add",
                "uadd_chk",
                ConstValue::U(max),
                ConstValue::U(1),
                ConstValue::U(0),
                true,
            ),
            (
                "sub",
                "usub_chk",
                ConstValue::U(0),
                ConstValue::U(1),
                ConstValue::U(max),
                true,
            ),
            (
                "mul",
                "umul_chk",
                ConstValue::U(max),
                ConstValue::U(max),
                ConstValue::U(1),
                bits > 1,
            ),
            (
                "mul",
                "umul_chk",
                ConstValue::U(max),
                ConstValue::U(0),
                ConstValue::U(0),
                false,
            ),
        ];
        check_cases(&ty, &cases);
        let min = if bits == 128 {
            i128::MIN
        } else {
            -(1i128 << (bits - 1))
        };
        let max = if bits == 128 {
            i128::MAX
        } else {
            (1i128 << (bits - 1)) - 1
        };
        let ty = format!("i{bits}");
        let cases = [
            (
                "add",
                "sadd_chk",
                ConstValue::I(min),
                ConstValue::I(-1),
                ConstValue::I(max),
                true,
            ),
            (
                "sub",
                "ssub_chk",
                ConstValue::I(max),
                ConstValue::I(-1),
                ConstValue::I(min),
                true,
            ),
            (
                "mul",
                "smul_chk",
                ConstValue::I(min),
                ConstValue::I(-1),
                ConstValue::I(min),
                true,
            ),
            (
                "mul",
                "smul_chk",
                ConstValue::I(min),
                ConstValue::I(0),
                ConstValue::I(0),
                false,
            ),
        ];
        check_cases(&ty, &cases);
    }
}
type Case = (
    &'static str,
    &'static str,
    ConstValue,
    ConstValue,
    ConstValue,
    bool,
);
fn check_cases(ty: &str, cases: &[Case]) {
    for (op, checked, a, b, expected, overflow) in cases {
        assert_eq!(
            arithmetic(ty, op, a.clone(), b.clone()),
            *expected,
            "{ty} {op}"
        );
        assert_eq!(
            pair(ty, checked, a.clone(), b.clone()),
            (expected.clone(), *overflow),
            "{ty} {checked}"
        );
        // The verifier independently evaluates the original typed declaration.
        let body = format!("%v8: {ty} = const_decl \"boundary\" {op}({ty} {}, {ty} {}) => const {ty} {} ret {ty} %v8", literal(a), literal(b), literal(expected));
        assert_eq!(
            run(&[], ty, &body, &[]).unwrap().value,
            Some(expected.clone())
        );
    }
}
#[test]
fn division_remainder_and_full_width_shift_counts_never_use_host_edge_behavior() {
    for bits in [1, 8, 16, 32, 64, 128] {
        let ty = format!("i{bits}");
        let min = if bits == 128 {
            i128::MIN
        } else {
            -(1i128 << (bits - 1))
        };
        assert_eq!(
            arithmetic(&ty, "sdiv", ConstValue::I(min), ConstValue::I(-1)),
            ConstValue::I(min)
        );
        assert_eq!(
            arithmetic(&ty, "srem", ConstValue::I(min), ConstValue::I(-1)),
            ConstValue::I(0)
        );
        let shifted = arithmetic(&ty, "shl", ConstValue::I(-1), ConstValue::I(-1));
        assert_eq!(shifted, ConstValue::I(min));
        assert_eq!(
            arithmetic(&ty, "lshr", ConstValue::I(-1), ConstValue::I(-1)),
            ConstValue::I(if bits == 1 { -1 } else { 1 })
        );
        assert_eq!(
            arithmetic(&ty, "ashr", ConstValue::I(min), ConstValue::I(-1)),
            ConstValue::I(-1)
        );
        if bits > 1 {
            assert_eq!(
                arithmetic(&ty, "shl", ConstValue::I(-1), ConstValue::I(min)),
                ConstValue::I(-1)
            );
        }
        for (ty, div, rem, zero) in [
            (ty, "sdiv", "srem", ConstValue::I(0)),
            (format!("u{bits}"), "udiv", "urem", ConstValue::U(0)),
        ] {
            for (op, reason) in [
                (div, TrapReason::DivisionByZero),
                (rem, TrapReason::RemainderByZero),
            ] {
                let error = run(
                    &[&ty, &ty],
                    &ty,
                    &format!("%v8: {ty} = {op} {ty} %v0, %v1 ret {ty} %v8"),
                    &[zero.clone(), zero.clone()],
                )
                .unwrap_err();
                match error {
                    InterpreterError::Trap(t) => {
                        assert_eq!(t.reason, reason);
                        assert_eq!(t.steps, 1);
                        assert_eq!(t.site.value, Some(ValueId(8)));
                    }
                    _ => panic!("{error:?}"),
                }
            }
        }
    }
    assert_eq!(
        arithmetic("i8", "sdiv", ConstValue::I(-7), ConstValue::I(3)),
        ConstValue::I(-2)
    );
    assert_eq!(
        arithmetic("i8", "srem", ConstValue::I(-7), ConstValue::I(3)),
        ConstValue::I(-1)
    );
    assert_eq!(
        arithmetic("u8", "ashr", ConstValue::U(128), ConstValue::U(1)),
        ConstValue::U(192)
    );
    assert_eq!(
        arithmetic(
            "u128",
            "lshr",
            ConstValue::U(u128::MAX),
            ConstValue::U(1u128 << 100)
        ),
        ConstValue::U(u128::MAX)
    );
}
#[test]
fn bitwise_comparisons_and_integer_casts_observe_bit_patterns_and_bool_category() {
    for (op, expected) in [("and", 0), ("or", 255), ("xor", 255)] {
        assert_eq!(
            arithmetic("u8", op, ConstValue::U(170), ConstValue::U(85)),
            ConstValue::U(expected)
        );
    }
    assert_eq!(
        run(
            &["i8"],
            "i8",
            "%v8: i8 = not i8 %v0 ret i8 %v8",
            &[ConstValue::I(127)]
        )
        .unwrap()
        .value,
        Some(ConstValue::I(-128))
    );
    for (op, src, dst, input, expected) in [
        ("sext", "u8", "i32", ConstValue::U(255), ConstValue::I(-1)),
        ("zext", "i8", "i32", ConstValue::I(-1), ConstValue::I(255)),
        (
            "sext",
            "i1",
            "u128",
            ConstValue::I(-1),
            ConstValue::U(u128::MAX),
        ),
        (
            "trunc",
            "u128",
            "i1",
            ConstValue::U(u128::MAX),
            ConstValue::I(-1),
        ),
        (
            "bitcast",
            "u128",
            "i128",
            ConstValue::U(1u128 << 127),
            ConstValue::I(i128::MIN),
        ),
        (
            "zext",
            "bool",
            "u1",
            ConstValue::Bool(true),
            ConstValue::U(1),
        ),
    ] {
        assert_eq!(
            run(
                &[src],
                dst,
                &format!("%v8: {dst} = {op} {src} %v0 to {dst} ret {dst} %v8"),
                &[input]
            )
            .unwrap()
            .value,
            Some(expected)
        );
    }
    for (ty, a, b, predicates) in [
        (
            "i8",
            ConstValue::I(-1),
            ConstValue::I(1),
            [
                ("eq", false),
                ("ne", true),
                ("slt", true),
                ("sle", true),
                ("sgt", false),
                ("sge", false),
            ],
        ),
        (
            "u8",
            ConstValue::U(255),
            ConstValue::U(1),
            [
                ("eq", false),
                ("ne", true),
                ("ult", false),
                ("ule", false),
                ("ugt", true),
                ("uge", true),
            ],
        ),
    ] {
        for opcode in ["cmp", "icmp"] {
            for (pred, expected) in predicates {
                assert_eq!(
                    run(
                        &[ty, ty],
                        "bool",
                        &format!("%v8: bool = {opcode} {pred} {ty} %v0, %v1 ret bool %v8"),
                        &[a.clone(), b.clone()]
                    )
                    .unwrap()
                    .value,
                    Some(ConstValue::Bool(expected))
                );
            }
        }
    }
    assert_eq!(
        run(
            &["bool", "bool"],
            "bool",
            "%v8: bool = icmp ne bool %v0, %v1 ret bool %v8",
            &[ConstValue::Bool(true), ConstValue::Bool(false)]
        )
        .unwrap()
        .value,
        Some(ConstValue::Bool(true))
    );
}
#[test]
fn phi_cycles_snapshot_predecessor_values_and_block_order_is_irrelevant() {
    let m = parse_module(include_str!("fixtures/interpreter-loop-v3.wir")).unwrap();
    let before = print_module(&m);
    for (iterations, expected) in [(0, 11), (1, 22), (2, 11), (3, 22)] {
        assert_eq!(
            interpret(&m, FunctionId(7), &[ConstValue::U(iterations)])
                .unwrap()
                .value,
            Some(ConstValue::I(expected))
        );
    }
    assert_eq!(print_module(&m), before);
    let m = program(&["bool"], "i32", "%b90 \"merge\": %v8: i32 = phi i32 [ %v5, %b20 ], [ %v6, %b30 ] ret i32 %v8 %b30 \"false\": %v6: i32 = const i32 -1 br label %b90 %b11 \"entry\": cbr bool %v0, label %b20, label %b30 %b20 \"true\": %v5: i32 = const i32 1 br label %b90");
    for (input, expected) in [(true, 1), (false, -1)] {
        assert_eq!(
            interpret(&m, FunctionId(7), &[ConstValue::Bool(input)])
                .unwrap()
                .value,
            Some(ConstValue::I(expected))
        );
    }
}
#[test]
fn switch_distinct_predecessors_and_checked_pairs_can_flow_through_select_and_phi() {
    let m = program(&["u8", "bool"], "bool", "%b11 \"entry\": %v2: u8 = const u8 255 %v3: u8 = const u8 1 %v4: tuple<u8, bool> = uadd_chk u8 %v2, %v3 %v5: tuple<u8, bool> = usub_chk u8 %v2, %v3 %v6: tuple<u8, bool> = select bool %v1, tuple<u8, bool> %v4, tuple<u8, bool> %v5 switch u8 %v0, label %b20 [ 1: %b20, 2: %b20 ] %b20 \"join\": %v8: tuple<u8, bool> = phi tuple<u8, bool> [ %v6, %b11 ] %v9: tuple<u8, bool> = mov tuple<u8, bool> %v8 %v10: bool = extract %v9, 1 ret bool %v10");
    for input in [0, 1, 2] {
        for flag in [true, false] {
            assert_eq!(
                interpret(
                    &m,
                    FunctionId(7),
                    &[ConstValue::U(input), ConstValue::Bool(flag)]
                )
                .unwrap()
                .value,
                Some(ConstValue::Bool(flag))
            );
        }
    }
}
#[test]
fn fuel_counts_instructions_phis_and_terminators_and_stops_empty_loops() {
    let m = program(
        &[],
        "i32",
        "%b11 \"entry\": %v8: i32 = const i32 42 ret i32 %v8",
    );
    for limit in [0, 1, 2] {
        let result = interpret_with_options(
            &m,
            FunctionId(7),
            &[],
            InterpreterOptions {
                max_steps: limit,
                ..Default::default()
            },
        );
        if limit == 2 {
            assert_eq!(result.unwrap().steps, 2);
        } else {
            assert!(
                matches!(result, Err(InterpreterError::StepLimit { site, limit: l }) if l == limit && site.instruction == limit as usize)
            );
        }
    }
    let m = program(
        &[],
        "void",
        "%b11 \"entry\": br label %b20 %b20 \"loop\": br label %b20",
    );
    assert!(
        matches!(interpret_with_options(&m, FunctionId(7), &[], InterpreterOptions { max_steps: 10, ..Default::default() }), Err(InterpreterError::StepLimit { site, limit: 10 }) if site.block == BlockId(20))
    );
    let m = parse_module(include_str!("fixtures/interpreter-loop-v3.wir")).unwrap();
    // Entry: 4 constants + branch. Header: 3 phis + compare + branch.
    assert!(
        matches!(interpret_with_options(&m, FunctionId(7), &[ConstValue::U(1)], InterpreterOptions { max_steps: 6, ..Default::default() }), Err(InterpreterError::StepLimit { site, .. }) if site.instruction == 1 && site.value == Some(ValueId(6)))
    );
}
#[test]
fn unused_arithmetic_traps_in_o0_order_and_select_does_not_hide_its_operands() {
    let m = program(&["bool"], "i32", "%b11 \"entry\": %v1: i32 = const i32 0 %v2: i32 = sdiv i32 %v1, %v1 %v3: i32 = select bool %v0, i32 %v1, i32 %v2 ret i32 %v1");
    for flag in [true, false] {
        assert!(
            matches!(interpret(&m, FunctionId(7), &[ConstValue::Bool(flag)]), Err(InterpreterError::Trap(t)) if t.site.value == Some(ValueId(2)) && t.steps == 2)
        );
    }
    let body = "%b11 \"entry\": cbr bool %v0, label %b20, label %b30 %b20 \"yes\": trap_if bool %v0, reason=\"first\" trap reason=\"later\" %b30 \"no\": ret void";
    let m = program(&["bool"], "void", body);
    assert!(
        matches!(interpret(&m, FunctionId(7), &[ConstValue::Bool(true)]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::Explicit("first".into()) && t.steps == 2)
    );
    assert_eq!(
        interpret(&m, FunctionId(7), &[ConstValue::Bool(false)])
            .unwrap()
            .value,
        None
    );
    let m = program(&[], "void", "%b11 \"entry\": trap reason=\"stop\"");
    assert!(
        matches!(interpret(&m, FunctionId(7), &[]), Err(InterpreterError::Trap(t)) if t.steps == 1 && t.site.value.is_none())
    );
}
#[test]
fn invalid_arguments_modules_and_unsupported_dead_instructions_fail_before_execution() {
    let mut m = program(&["i1"], "i1", "%b11 \"entry\": ret i1 %v0");
    assert!(matches!(
        interpret(&m, FunctionId(7), &[]),
        Err(InterpreterError::ArgumentCount { .. })
    ));
    for value in [ConstValue::Bool(false), ConstValue::I(1), ConstValue::U(0)] {
        assert!(matches!(
            interpret(&m, FunctionId(7), &[value]),
            Err(InterpreterError::ArgumentType { index: 0, .. })
        ));
    }
    assert!(matches!(
        interpret(&m, FunctionId(9), &[]),
        Err(InterpreterError::UnknownFunction(FunctionId(9)))
    ));
    assert!(matches!(
        interpret_with_options(
            &m,
            FunctionId(7),
            &[ConstValue::I(0)],
            InterpreterOptions {
                ir_limits: IrLimits {
                    max_nodes: 0,
                    ..Default::default()
                },
                ..Default::default()
            }
        ),
        Err(InterpreterError::Verification(_))
    ));
    m.functions[0].blocks[0].terminator = None;
    assert!(matches!(
        interpret(&m, FunctionId(7), &[ConstValue::I(0)]),
        Err(InterpreterError::Verification(_))
    ));
    for (body, operation) in [
        ("%v8: i32 = undef i32", "undef"),
        ("%v8: ptr<i32> = alloca i32, align 4", "alloca"),
        ("%v8: f32 = const f32 0x00000000", "non-integer constant"),
    ] {
        let m = program(
            &[],
            "void",
            &format!("%b11 \"entry\": ret void %b90 \"dead\": {body} ret void"),
        );
        assert!(
            matches!(interpret(&m, FunctionId(7), &[]), Err(InterpreterError::UnsupportedInstruction { site, operation: op }) if site.block == BlockId(90) && op == operation)
        );
    }
    let m = program(&["f32"], "f32", "%b11 \"entry\": ret f32 %v0");
    assert!(matches!(
        interpret(&m, FunctionId(7), &[ConstValue::F(FloatBits::F32(0))]),
        Err(InterpreterError::UnsupportedSignature(_))
    ));
    let m = parse_module(include_str!("fixtures/calls-v3.wir")).unwrap();
    assert!(matches!(
        interpret(&m, m.functions[0].id, &[]),
        Err(InterpreterError::UnsupportedInstruction { .. })
            | Err(InterpreterError::ArgumentCount { .. })
    ));
}

#[test]
fn published_integer_fixture_returns_values_and_explicit_overflow_trap() {
    let m = parse_module(include_str!("fixtures/integer-operations-v3.wir")).unwrap();
    for (id, expected) in [
        (0, ConstValue::U(0)),
        (2, ConstValue::I(-128)),
        (3, ConstValue::I(0)),
        (4, ConstValue::I(-128)),
    ] {
        assert_eq!(
            interpret(&m, FunctionId(id), &[]).unwrap().value,
            Some(expected)
        );
    }
    assert!(
        matches!(interpret(&m, FunctionId(1), &[]), Err(InterpreterError::Trap(t)) if t.reason == TrapReason::Explicit("integer overflow".into()) && t.steps == 6 && t.site.instruction == 5)
    );
}

#[test]
fn verification_checks_other_functions_but_execution_restricts_only_the_selected_body() {
    let mut m = program(&[], "void", "%b11 \"entry\": ret void");
    let other = program(&[], "void", "%b11 \"entry\": %v8: i32 = undef i32 ret void");
    let mut function = other.functions[0].clone();
    function.id = FunctionId(8);
    function.name = "other".into();
    let mut declaration = other.declarations[0].clone();
    declaration.id = FunctionId(8);
    declaration.name = "other".into();
    m.functions.push(function);
    m.declarations.push(declaration);
    assert_eq!(interpret(&m, FunctionId(7), &[]).unwrap().value, None);
    assert!(matches!(
        interpret(&m, FunctionId(8), &[]),
        Err(InterpreterError::UnsupportedInstruction {
            operation: "undef",
            ..
        })
    ));
    m.functions[1].blocks[0].terminator = None;
    assert!(matches!(
        interpret(&m, FunctionId(7), &[]),
        Err(InterpreterError::Verification(_))
    ));
    let source = format!("{HEADER} declare @f8 \"external\": sysv64 () -> void, linkage external, link_name \"external\" declare @f7 \"test\": whale () -> void, linkage internal fn @f7 \"test\"() -> void, entry %b11 {{ %b11 \"entry\": call sysv64 void @f8() ret void }} }}");
    let m = parse_module(&source).unwrap();
    assert!(matches!(
        interpret(&m, FunctionId(7), &[]),
        Err(InterpreterError::UnsupportedInstruction {
            operation: "call",
            ..
        })
    ));
    let m = program(&[], "void", "%b11 \"entry\": ret void");
    let mut m = m;
    m.declarations[0].signature.convention = CallingConvention::SysV64;
    assert!(matches!(
        interpret(&m, FunctionId(7), &[]),
        Err(InterpreterError::UnsupportedSignature(_))
    ));
}
