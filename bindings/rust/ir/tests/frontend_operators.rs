#![cfg(feature = "socket")]
use ast::BinOpRef as B;

use ir::lower_ast::{frontend as ast, interchange};

use ir::*;

fn ty(bits: u16, signed: bool) -> ast::TypeRef {
    ast::TypeRef::Int { bits, signed }
}

fn int(bits: u16, signed: bool, value: impl ToString) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Int { bits, signed, value: value.to_string() })
}

fn boolean(value: bool) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Bool(value))
}

fn bin(op: B, left: ast::Expr, right: ast::Expr) -> ast::Expr {
    ast::Expr::Binary { op, left: Box::new(left), right: Box::new(right) }
}

fn eq(left: ast::Expr, right: ast::Expr) -> ast::Expr {
    ast::Expr::Cmp { op: ast::CmpOpRef::Eq, left: Box::new(left), right: Box::new(right) }
}

fn program(expr: ast::Expr, ret: ast::TypeRef, constant: bool) -> ast::Program {
    let globals = if constant {
        vec![ast::GlobalConst { name: "result".into(), ty: ret.clone(), init: expr.clone() }]
    } else {
        vec![]
    };
    ast::Program {
        declarations: vec![],
        globals,
        functions: vec![ast::Function {
            name: "test".into(),
            convention: CallingConvention::Whale,
            linkage: Linkage::Internal,
            link_name: None,
            parameters: vec![],
            return_type: ret,
            body: vec![ast::Stmt::Return(Some(if constant {
                ast::Expr::Var("result".into())
            } else {
                expr
            }))],
        }],
    }
}

fn lower(p: &ast::Program) -> Result<Module, LowerError> {
    let target = Target::X86_64WhaleLinux;
    lower_o0(p, target.name(), target.data_layout())
}

fn canonical(m: &Module) {
    verify_module(m).unwrap();
    let text = print_module(m);
    assert_eq!(print_module(&parse_module(&text).unwrap()), text);
}

fn same(expr: ast::Expr, ty: ast::TypeRef, expected: ConstValue) {
    for constant in [false, true] {
        let p = program(expr.clone(), ty.clone(), constant);
        let json = interchange::encode(p).unwrap();
        let p = interchange::decode(&json).unwrap();
        let m = lower(&p).unwrap();
        canonical(&m);
        assert_eq!(
            interpret(&m, FunctionId(0), &[]).unwrap().value,
            Some(expected.clone()),
            "constant={constant}, {expr:?}"
        );
    }
}

#[test]
fn short_circuit_call_rhs_executes_once_with_shared_fuel() {
    let build = |left| {
        let call = ast::Expr::Call { callee: ast::CalleeRef::Direct("rhs".into()), args: vec![] };
        let mut p = program(bin(B::LogicalAnd, boolean(left), call), ast::TypeRef::Bool, false);
        p.functions.push(ast::Function {
            name: "rhs".into(),
            convention: CallingConvention::Whale,
            linkage: Linkage::Internal,
            link_name: None,
            parameters: vec![],
            return_type: ast::TypeRef::Bool,
            body: vec![
                ast::Stmt::ExprStmt(bin(B::Add, int(32, true, 1), int(32, true, 2))),
                ast::Stmt::Return(Some(boolean(true))),
            ],
        });
        let m = lower(&p).unwrap();
        canonical(&m);
        m
    };
    let skipped = build(false);
    let taken = build(true);
    let skip = interpret(&skipped, FunctionId(0), &[]).unwrap();
    let run = interpret(&taken, FunctionId(0), &[]).unwrap();
    let rhs = interpret(&taken, FunctionId(1), &[]).unwrap();
    assert_eq!(skip.value, Some(ConstValue::Bool(false)));
    assert_eq!(run.value, Some(ConstValue::Bool(true)));
    // The RHS adds precisely one call, its callee's steps, and its outgoing branch.
    assert_eq!(run.steps, skip.steps + rhs.steps + 2);
    let options = InterpreterOptions { max_steps: skip.steps, ..InterpreterOptions::default() };
    assert!(interpret_with_options(&skipped, FunctionId(0), &[], options).is_ok());
    let options = InterpreterOptions { max_steps: run.steps - 1, ..InterpreterOptions::default() };
    assert!(matches!(
        interpret_with_options(&taken, FunctionId(0), &[], options),
        Err(InterpreterError::StepLimit { .. })
    ));
}

#[test]
fn bitwise_runtime_and_preserved_constants_agree_for_every_integer_width() {
    for bits in [8, 16, 32, 64, 128] {
        let mask = if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 };
        let high = 1u128 << (bits - 1);
        let min = if bits == 128 { i128::MIN } else { -(1i128 << (bits - 1)) };
        let max = if bits == 128 { i128::MAX } else { (1i128 << (bits - 1)) - 1 };
        for (op, a, b, expected) in [
            (B::BitAnd, high, mask, high),
            (B::BitOr, high, 1, high | 1),
            (B::BitXor, mask, 1, mask - 1),
        ] {
            same(
                bin(op, int(bits, false, a), int(bits, false, b)),
                ty(bits, false),
                ConstValue::U(expected),
            );
        }
        for (op, a, b, expected) in
            [(B::BitAnd, min, -1, min), (B::BitOr, min, 1, min + 1), (B::BitXor, min, -1, max)]
        {
            same(
                bin(op, int(bits, true, a), int(bits, true, b)),
                ty(bits, true),
                ConstValue::I(expected),
            );
        }
        same(
            ast::Expr::BitNot(Box::new(int(bits, false, mask))),
            ty(bits, false),
            ConstValue::U(0),
        );
        same(ast::Expr::BitNot(Box::new(int(bits, true, min))), ty(bits, true), ConstValue::I(max));
    }
    same(ast::Expr::BitNot(Box::new(int(1, true, -1))), ty(1, true), ConstValue::I(0));
    same(bin(B::BitXor, int(1, false, 1), int(1, false, 1)), ty(1, false), ConstValue::U(0));
}

#[test]
fn division_remainder_and_zero_traps_follow_typed_width_semantics() {
    for bits in [8, 16, 32, 64, 128] {
        let max = if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 };
        let min = if bits == 128 { i128::MIN } else { -(1i128 << (bits - 1)) };
        for (op, expected) in [(B::Div, ConstValue::I(-2)), (B::Rem, ConstValue::I(-1))] {
            same(bin(op, int(bits, true, -7), int(bits, true, 3)), ty(bits, true), expected);
        }
        same(
            bin(B::Div, int(bits, false, max), int(bits, false, 2)),
            ty(bits, false),
            ConstValue::U(max / 2),
        );
        same(
            bin(B::Rem, int(bits, false, max), int(bits, false, 2)),
            ty(bits, false),
            ConstValue::U(1),
        );
        same(
            bin(B::Div, int(bits, true, min), int(bits, true, -1)),
            ty(bits, true),
            ConstValue::I(min),
        );
        same(
            bin(B::Rem, int(bits, true, min), int(bits, true, -1)),
            ty(bits, true),
            ConstValue::I(0),
        );
        for signed in [false, true] {
            for op in [B::Div, B::Rem] {
                let expr = bin(op, int(bits, signed, 1), int(bits, signed, 0));
                let m = lower(&program(expr.clone(), ty(bits, signed), false)).unwrap();
                canonical(&m);
                assert!(matches!(
                    interpret(&m, FunctionId(0), &[]),
                    Err(InterpreterError::Trap(InterpreterTrap {
                        reason: TrapReason::DivisionByZero | TrapReason::RemainderByZero,
                        ..
                    }))
                ));
                assert!(matches!(
                    lower(&program(expr, ty(bits, signed), true)),
                    Err(LowerError::ConstantEvaluation(
                        ConstEvalError::DivisionByZero | ConstEvalError::RemainderByZero
                    ))
                ));
            }
        }
    }
    same(bin(B::Div, int(1, true, -1), int(1, true, -1)), ty(1, true), ConstValue::I(-1));
    same(bin(B::Rem, int(1, true, -1), int(1, true, -1)), ty(1, true), ConstValue::I(0));
}

#[test]
fn shifts_mask_complete_unsigned_count_patterns_and_select_signed_right_shift() {
    for bits in [8, 16, 32, 64, 128] {
        let high = 1u128 << (bits - 1);
        let min = if bits == 128 { i128::MIN } else { -(1i128 << (bits - 1)) };
        let mask = if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 };
        for count in [0, u128::from(bits - 1), u128::from(bits), u128::from(bits + 5), mask] {
            let shift = (count % u128::from(bits)) as u32;
            same(
                bin(B::ShiftRight, int(bits, false, high), int(bits, false, count)),
                ty(bits, false),
                ConstValue::U(high >> shift),
            );
            same(
                bin(B::ShiftLeft, int(bits, false, mask), int(bits, false, count)),
                ty(bits, false),
                ConstValue::U(mask.wrapping_shl(shift) & mask),
            );
        }
        for count in [0i128, i128::from(bits - 1), -1] {
            let shift = (((count as u128) & mask) % u128::from(bits)) as u32;
            same(
                bin(B::ShiftRight, int(bits, true, min), int(bits, true, count)),
                ty(bits, true),
                ConstValue::I(min >> shift),
            );
        }
    }
    same(
        bin(B::ShiftRight, int(128, false, u128::MAX), int(128, false, (1u128 << 100) + 7)),
        ty(128, false),
        ConstValue::U(u128::MAX >> 7),
    );
    same(bin(B::ShiftLeft, int(1, false, 1), int(1, false, 1)), ty(1, false), ConstValue::U(1));
    same(bin(B::ShiftRight, int(1, true, -1), int(1, true, -1)), ty(1, true), ConstValue::I(-1));
    assert!(lower(&program(
        bin(B::ShiftLeft, int(8, false, 1), int(8, false, -1)),
        ty(8, false),
        false
    ))
    .is_err());
}

#[test]
fn logical_cfg_skips_traps_and_evaluates_the_rhs_exactly_once() {
    let rhs =
        |divisor| eq(bin(B::Div, int(32, true, 12), int(32, true, divisor)), int(32, true, 4));
    for (op, lhs, expected) in [(B::LogicalAnd, false, false), (B::LogicalOr, true, true)] {
        same(bin(op, boolean(lhs), rhs(0)), ast::TypeRef::Bool, ConstValue::Bool(expected));
        let m = lower(&program(bin(op, boolean(lhs), rhs(3)), ast::TypeRef::Bool, false)).unwrap();
        canonical(&m);
        let instructions: Vec<_> =
            m.functions[0].blocks.iter().flat_map(|b| &b.instructions).collect();
        assert_eq!(
            instructions
                .iter()
                .filter(|i| matches!(i, Instruction::Bin { op: BinOp::SDiv, .. }))
                .count(),
            1
        );
        assert_eq!(instructions.iter().filter(|i| matches!(i, Instruction::Phi { .. })).count(), 1);
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().steps, 4, "left, cbr, phi, ret");
    }
    for (op, lhs) in [(B::LogicalAnd, true), (B::LogicalOr, false)] {
        let m = lower(&program(bin(op, boolean(lhs), rhs(3)), ast::TypeRef::Bool, false)).unwrap();
        canonical(&m);
        assert_eq!(
            interpret(&m, FunctionId(0), &[]).unwrap().steps,
            10,
            "RHS: two literals, div, literal, cmp, br, executed once"
        );
        same(bin(op, boolean(lhs), rhs(3)), ast::TypeRef::Bool, ConstValue::Bool(true));
        let m = lower(&program(bin(op, boolean(lhs), rhs(0)), ast::TypeRef::Bool, false)).unwrap();
        assert!(matches!(
            interpret(&m, FunctionId(0), &[]),
            Err(InterpreterError::Trap(InterpreterTrap {
                reason: TrapReason::DivisionByZero,
                steps: 5,
                ..
            }))
        ));
        assert!(matches!(
            lower(&program(bin(op, boolean(lhs), rhs(0)), ast::TypeRef::Bool, true)),
            Err(LowerError::ConstantEvaluation(ConstEvalError::DivisionByZero))
        ));
    }
    let nested = bin(
        B::LogicalOr,
        bin(B::LogicalAnd, boolean(true), boolean(false)),
        bin(B::LogicalAnd, boolean(true), boolean(true)),
    );
    same(nested, ast::TypeRef::Bool, ConstValue::Bool(true));
}

#[test]
fn lower_and_verify_preserve_exactly_one_effectful_rhs_call() {
    let call = ast::Expr::Call { callee: ast::CalleeRef::Direct("effect".into()), args: vec![] };
    let mut p = program(bin(B::LogicalAnd, boolean(false), call), ast::TypeRef::Bool, false);
    p.declarations.push(ast::FunctionDeclaration {
        name: "effect".into(),
        signature: ast::SignatureRef {
            params: vec![],
            ret: ast::TypeRef::Bool,
            convention: CallingConvention::Whale,
            variadic: false,
        },
        linkage: Linkage::External,
        link_name: Some("effect_bool".into()),
    });
    let m = lower(&p).unwrap();
    canonical(&m);
    assert_eq!(
        m.functions[0]
            .blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .filter(|i| matches!(i, Instruction::Call { .. }))
            .count(),
        1
    );
    assert!(m.functions[0].blocks.iter().any(|b| b.name == "logical.rhs"));
}

#[test]
fn operators_reject_wrong_categories_widths_signedness_and_skipped_invalid_ir() {
    for constant in [false, true] {
        for op in [B::BitAnd, B::BitOr, B::BitXor, B::Div, B::Rem, B::ShiftLeft, B::ShiftRight] {
            assert!(lower(&program(
                bin(op, boolean(true), boolean(false)),
                ast::TypeRef::Bool,
                constant
            ))
            .is_err());
            assert!(lower(&program(
                bin(op, int(8, false, 1), int(16, false, 1)),
                ty(8, false),
                constant
            ))
            .is_err());
            assert!(lower(&program(
                bin(op, int(8, false, 1), int(8, true, 1)),
                ty(8, false),
                constant
            ))
            .is_err());
            let float = || ast::Expr::Lit(ast::Lit::Float { bits: 32, value: "0x3f800000".into() });
            if matches!(op, B::Div | B::Rem) {
                assert!(lower(&program(
                    bin(op, float(), float()),
                    ast::TypeRef::Float { bits: 32 },
                    constant
                ))
                .is_ok());
                continue;
            }
            assert!(matches!(
                lower(&program(
                    bin(op, float(), float()),
                    ast::TypeRef::Float { bits: 32 },
                    constant
                )),
                Err(LowerError::UnsupportedExpr)
            ));
        }
        assert!(lower(&program(
            bin(B::LogicalAnd, int(1, false, 1), int(1, false, 1)),
            ty(1, false),
            constant
        ))
        .is_err());
        assert!(lower(&program(
            ast::Expr::BitNot(Box::new(boolean(true))),
            ast::TypeRef::Bool,
            constant
        ))
        .is_err());
        assert!(lower(&program(
            bin(B::LogicalAnd, boolean(false), int(32, true, 1)),
            ast::TypeRef::Bool,
            constant
        ))
        .is_err());
    }
    for op in [B::BitAnd, B::BitOr, B::BitXor, B::Div, B::Rem, B::ShiftLeft, B::ShiftRight] {
        let pointer = ast::TypeRef::Ptr(Box::new(ty(8, false)));
        let mut p = program(
            bin(op, ast::Expr::Var("a".into()), ast::Expr::Var("b".into())),
            pointer.clone(),
            false,
        );
        p.functions[0].parameters = vec![
            ast::Parameter { name: "a".into(), ty: pointer.clone() },
            ast::Parameter { name: "b".into(), ty: pointer },
        ];
        assert!(matches!(lower(&p), Err(LowerError::UnsupportedExpr)));
        p.functions[0].body =
            vec![ast::Stmt::Return(Some(ast::Expr::BitNot(Box::new(ast::Expr::Var("a".into())))))];
        assert!(matches!(lower(&p), Err(LowerError::UnsupportedExpr)));
    }
    let bad_right = ConstExpr { ty: Type::Bool, kind: ConstExprKind::Literal(ConstValue::U(1)) };
    let expr = ConstExpr {
        ty: Type::Bool,
        kind: ConstExprKind::Binary {
            op: ConstBinaryOp::LogicalAnd,
            left: Box::new(ConstExpr::literal(Type::Bool, ConstValue::Bool(false))),
            right: Box::new(bad_right),
        },
    };
    assert_eq!(expr.evaluate(&|_| None), Err(ConstEvalError::InvalidLiteral));
}

#[test]
fn new_ast_and_constant_syntax_requires_its_version_and_old_versions_still_read() {
    let expr = bin(B::BitXor, int(8, false, 5), int(8, false, 3));
    let p = program(expr, ty(8, false), true);
    let json = interchange::encode(p.clone()).unwrap();
    assert!(json.contains("\"format_version\": 5"));
    assert!(interchange::decode(&json.replace("\"format_version\": 5", "\"format_version\": 2"))
        .unwrap_err()
        .contains("require AST format 3"));
    let text = print_module(&lower(&p).unwrap());
    assert!(text.contains("bit_xor(u8 5, u8 3)"));
    for old in [3, 4] {
        assert!(parse_module(&text.replace(
            &format!("format_version {}", IR_FORMAT_VERSION),
            &format!("format_version {old}")
        ))
        .unwrap_err()
        .to_string()
        .contains("require typed IR format 5"));
    }
    let json = interchange::encode(program(int(8, false, 1), ty(8, false), false)).unwrap();
    interchange::decode(&json.replace("\"format_version\": 5", "\"format_version\": 2")).unwrap();
    let text = print_module(&lower(&program(int(8, false, 1), ty(8, false), false)).unwrap());
    for old in [3, 4] {
        canonical(
            &parse_module(&text.replace(
                &format!("format_version {}", IR_FORMAT_VERSION),
                &format!("format_version {old}"),
            ))
            .unwrap(),
        );
    }
}

#[test]
fn new_constant_trees_stay_visible_at_o0_and_all_new_forms_are_version_gated() {
    let zero_divisor = eq(bin(B::Div, int(8, false, 1), int(8, false, 0)), int(8, false, 0));
    let expr = bin(B::LogicalAnd, boolean(false), zero_divisor);
    let mut p = program(boolean(true), ast::TypeRef::Void, false);
    p.functions[0].body = vec![
        ast::Stmt::Return(None),
        ast::Stmt::ConstDecl {
            name: "unreachable_lazy".into(),
            ty: ast::TypeRef::Bool,
            init: expr,
        },
    ];
    let m = lower(&p).unwrap();
    canonical(&m);
    let text = print_module(&m);
    assert!(text.contains("logical_and(bool false, eq(div(u8 1, u8 0), u8 0))"));
    assert!(text.contains("\"unreachable.cont\""));
    for op in [
        B::BitAnd,
        B::BitOr,
        B::BitXor,
        B::Div,
        B::Rem,
        B::ShiftLeft,
        B::ShiftRight,
        B::LogicalAnd,
        B::LogicalOr,
    ] {
        let p = program(bin(op, int(8, false, 2), int(8, false, 1)), ty(8, false), false);
        let json = interchange::encode(p).unwrap();
        assert!(interchange::decode(
            &json.replace("\"format_version\": 5", "\"format_version\": 2")
        )
        .unwrap_err()
        .contains("require AST format 3"));
    }
    let p = program(ast::Expr::BitNot(Box::new(int(8, false, 1))), ty(8, false), true);
    let json = interchange::encode(p.clone()).unwrap();
    assert!(interchange::decode(&json.replace("\"format_version\": 5", "\"format_version\": 2"))
        .unwrap_err()
        .contains("require AST format 3"));
    let text = print_module(&lower(&p).unwrap());
    assert!(text.contains("bit_not(u8 1)"));
    for old in [3, 4] {
        assert!(parse_module(&text.replace(
            &format!("format_version {}", IR_FORMAT_VERSION),
            &format!("format_version {old}")
        ))
        .unwrap_err()
        .to_string()
        .contains("require typed IR format 5"));
    }
}

#[test]
fn preserved_references_resolve_once_per_occurrence_in_source_order() {
    let reference = || ConstExpr {
        ty: Type::I32,
        kind: ConstExprKind::Reference(ConstRef::Global(GlobalId(1))),
    };
    let expression = ConstExpr {
        ty: Type::I32,
        kind: ConstExprKind::Binary {
            op: ConstBinaryOp::Add,
            left: Box::new(reference()),
            right: Box::new(reference()),
        },
    };
    let calls = std::cell::Cell::new(0i128);
    let value = expression
        .evaluate(&|_| {
            let next = calls.get() + 1;
            calls.set(next);
            Some((Type::I32, ConstValue::I(next)))
        })
        .unwrap();
    assert_eq!(value, ConstValue::I(3));
    assert_eq!(calls.get(), 2);
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
