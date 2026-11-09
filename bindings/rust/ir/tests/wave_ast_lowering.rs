// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "socket")]
//! Independent frontend fixtures exercise the complete production lowerer against
//! its explicit conformance reference, including identities and source ordering.
use ir::lower_ast::{
    frontend as a, lower_o0, lower_o0_reference, lower_o0_reference_with_limits,
    lower_o0_with_limits, AstLimits,
};

use ir::*;

const TARGET: &str = "x86_64-whale-linux";

fn ty() -> a::TypeRef {
    a::TypeRef::Int { bits: 32, signed: true }
}

fn lit(value: i32) -> a::Expr {
    a::Expr::Lit(a::Lit::Int { bits: 32, signed: true, value: value.to_string() })
}

fn var(name: &str) -> a::Expr {
    a::Expr::Var(name.into())
}

fn bin(left: a::Expr, op: a::BinOpRef, right: a::Expr) -> a::Expr {
    a::Expr::Binary { left: Box::new(left), op, right: Box::new(right) }
}

fn cmp(left: a::Expr, op: a::CmpOpRef, right: a::Expr) -> a::Expr {
    a::Expr::Cmp { left: Box::new(left), op, right: Box::new(right) }
}

fn assign(name: &str, value: a::Expr) -> a::Stmt {
    a::Stmt::Assign { name: name.into(), value }
}

fn func(name: &str, parameters: Vec<a::Parameter>, body: Vec<a::Stmt>) -> a::Function {
    a::Function {
        name: name.into(),
        parameters,
        return_type: ty(),
        body,
        convention: CallingConvention::Whale,
        linkage: Linkage::Internal,
        link_name: None,
    }
}

fn program(body: Vec<a::Stmt>) -> a::Program {
    a::Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![func("entry", vec![], body)],
    }
}

fn compare(p: &a::Program) -> Module {
    let actual = lower_o0(p, TARGET, DataLayout::default_64bit_le()).unwrap();
    let expected = lower_o0_reference(p, TARGET, DataLayout::default_64bit_le()).unwrap();
    assert_eq!(actual, expected, "production/reference full typed module differs");
    verify_module(&actual).unwrap();
    let text = print_module(&actual);
    assert_eq!(text, print_module(&parse_module(&text).unwrap()));
    actual
}

fn compare_error(p: &a::Program, limits: AstLimits) {
    let a = lower_o0_with_limits(p, TARGET, DataLayout::default_64bit_le(), limits).unwrap_err();
    let b = lower_o0_reference_with_limits(p, TARGET, DataLayout::default_64bit_le(), limits)
        .unwrap_err();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    assert_eq!(a.to_string(), b.to_string());
}

fn located(id: u64, e: a::Expr) -> a::Expr {
    a::Expr::Located { annotation: annotation(id), expression: Box::new(e) }
}

fn statement(id: u64, s: a::Stmt) -> a::Stmt {
    a::Stmt::Located { annotation: annotation(id), statement: Box::new(s) }
}

fn annotation(id: u64) -> SourceAnnotation {
    SourceAnnotation {
        id,
        source: Some(FrontendSource {
            file: if id % 2 == 0 { "a.wave" } else { "b.wave" }.into(),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 4,
        }),
    }
}

#[test]
fn generated_integer_control_flow_matches_reference_and_independent_execution() {
    for seed in 0i32..128 {
        let offset = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let op = if seed & 1 == 0 { a::BinOpRef::BitXor } else { a::BinOpRef::Add };
        let body = vec![
            a::Stmt::ConstDecl { name: "bias".into(), ty: ty(), init: lit(offset) },
            a::Stmt::VarDecl {
                name: "x".into(),
                ty: ty(),
                init: Some(bin(var("p"), op, var("bias"))),
            },
            a::Stmt::If {
                cond: cmp(var("x"), a::CmpOpRef::Lt, lit(0)),
                then_body: vec![assign("x", bin(var("x"), a::BinOpRef::Add, lit(3)))],
                else_body: vec![assign("x", bin(var("x"), a::BinOpRef::Sub, lit(5)))],
            },
            a::Stmt::VarDecl { name: "i".into(), ty: ty(), init: Some(lit(0)) },
            a::Stmt::While {
                cond: cmp(var("i"), a::CmpOpRef::Lt, lit(5)),
                body: vec![
                    assign("i", bin(var("i"), a::BinOpRef::Add, lit(1))),
                    a::Stmt::If {
                        cond: cmp(var("i"), a::CmpOpRef::Eq, lit(1)),
                        then_body: vec![a::Stmt::Continue],
                        else_body: vec![],
                    },
                    a::Stmt::If {
                        cond: cmp(var("i"), a::CmpOpRef::Eq, lit(4)),
                        then_body: vec![a::Stmt::Break],
                        else_body: vec![],
                    },
                    assign("x", bin(var("x"), a::BinOpRef::Add, lit(1))),
                ],
            },
            a::Stmt::Return(Some(a::Expr::Neg(Box::new(a::Expr::BitNot(Box::new(var("x"))))))),
        ];
        let p = a::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![func("entry", vec![a::Parameter { name: "p".into(), ty: ty() }], body)],
        };
        let m = compare(&p);
        for input in [i32::MIN, -31, 0, 7, i32::MAX] {
            let x = if seed & 1 == 0 { input ^ offset } else { input.wrapping_add(offset) };
            let x = if x < 0 { x.wrapping_add(3) } else { x.wrapping_sub(5) };
            let expected = (!x.wrapping_add(2)).wrapping_neg();
            assert_eq!(
                interpret(&m, FunctionId(0), &[ConstValue::I(input.into())]).unwrap().value,
                Some(ConstValue::I(expected.into()))
            );
        }
    }
}

#[test]
fn globals_local_constants_calls_and_shared_identity_order_match() {
    let signature = a::SignatureRef {
        params: vec![ty()],
        ret: ty(),
        convention: CallingConvention::Whale,
        variadic: false,
    };
    let helper = func(
        "helper",
        vec![a::Parameter { name: "p".into(), ty: ty() }],
        vec![a::Stmt::Return(Some(bin(var("p"), a::BinOpRef::Add, var("global"))))],
    );
    let pointer = a::TypeRef::FnPtr(Box::new(signature.clone()));
    let mut p = program(vec![
        a::Stmt::ConstDecl {
            name: "local".into(),
            ty: ty(),
            init: bin(var("global"), a::BinOpRef::Mul, lit(2)),
        },
        a::Stmt::VarDecl {
            name: "f".into(),
            ty: pointer,
            init: Some(a::Expr::FunctionRef("helper".into())),
        },
        a::Stmt::Return(Some(a::Expr::Call {
            callee: a::CalleeRef::Indirect(Box::new(var("f"))),
            args: vec![var("local")],
        })),
    ]);
    p.globals = vec![a::GlobalConst {
        name: "global".into(),
        ty: ty(),
        init: bin(lit(11), a::BinOpRef::Add, lit(3)),
    }];
    p.functions.push(helper);
    let m = compare(&p);
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(42)));
}

#[test]
fn aggregates_pointer_access_and_volatile_storage_match() {
    let array = a::TypeRef::Array { elem: Box::new(ty()), len: 3 };
    let index = |i| a::Expr::Index { base: Box::new(var("array")), index: Box::new(lit(i)) };
    let ptr = || a::Expr::AddressOf(Box::new(index(1)));
    let body = vec![
        a::Stmt::VarDecl {
            name: "array".into(),
            ty: array.clone(),
            init: Some(a::Expr::Array { ty: array, elements: vec![lit(1), lit(2), lit(3)] }),
        },
        a::Stmt::VolatileStore { pointer: ptr(), value: lit(42) },
        a::Stmt::Return(Some(a::Expr::VolatileLoad(Box::new(ptr())))),
    ];
    let m = compare(&program(body));
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(42)));
    let structure = a::TypeRef::Struct {
        fields: vec![
            a::StructField { name: "left".into(), ty: ty() },
            a::StructField { name: "right".into(), ty: ty() },
        ],
    };
    let p = program(vec![
        a::Stmt::VarDecl {
            name: "s".into(),
            ty: structure.clone(),
            init: Some(a::Expr::Struct {
                ty: structure.clone(),
                fields: vec![
                    a::FieldInit { name: "right".into(), value: lit(40) },
                    a::FieldInit { name: "left".into(), value: lit(2) },
                ],
            }),
        },
        a::Stmt::Return(Some(a::Expr::Field {
            base: Box::new(var("s")),
            layout: structure,
            name: "right".into(),
        })),
    ]);
    let m = compare(&p);
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(40)));
}

#[test]
fn nested_source_annotations_preserve_original_order_mappings_and_hoisting() {
    let mut p = program(vec![
        statement(
            1,
            a::Stmt::ConstDecl {
                name: "k".into(),
                ty: ty(),
                init: located(
                    2,
                    bin(located(3, lit(4)), a::BinOpRef::Add, located(4, var("global"))),
                ),
            },
        ),
        statement(
            5,
            a::Stmt::If {
                cond: located(6, cmp(var("k"), a::CmpOpRef::Gt, lit(0))),
                then_body: vec![
                    statement(
                        7,
                        a::Stmt::VarDecl {
                            name: "x".into(),
                            ty: ty(),
                            init: Some(located(8, var("k"))),
                        },
                    ),
                    statement(9, a::Stmt::Return(Some(located(10, var("x"))))),
                ],
                else_body: vec![statement(11, a::Stmt::Return(Some(located(12, lit(0)))))],
            },
        ),
    ]);
    p.globals.push(a::GlobalConst {
        name: "global".into(),
        ty: ty(),
        init: located(13, bin(located(14, lit(2)), a::BinOpRef::Add, located(15, lit(3)))),
    });
    let m = compare(&p);
    assert!(!m.sources.mappings.is_empty());
    m.sources.validate(&m).unwrap();
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(9)));
}

#[test]
fn invalid_frontend_operations_and_attributed_failures_match() {
    for body in [
        vec![a::Stmt::Break],
        vec![a::Stmt::Continue],
        vec![a::Stmt::ExprStmt(var("missing"))],
        vec![a::Stmt::ConstDecl { name: "x".into(), ty: ty(), init: lit(1) }, assign("x", lit(2))],
        vec![a::Stmt::Return(Some(a::Expr::Lit(a::Lit::Bool(true))))],
        vec![a::Stmt::ConstDecl {
            name: "x".into(),
            ty: ty(),
            init: bin(lit(1), a::BinOpRef::Div, lit(0)),
        }],
    ] {
        compare_error(&program(body), AstLimits::default());
    }
    let p = program(vec![statement(101, a::Stmt::Return(Some(located(102, var("missing")))))]);
    compare_error(&p, AstLimits::default());
    let p = program(vec![statement(
        103,
        a::Stmt::ConstDecl {
            name: "bad".into(),
            ty: ty(),
            init: located(104, bin(located(105, lit(1)), a::BinOpRef::Div, located(106, lit(0)))),
        },
    )]);
    compare_error(&p, AstLimits::default());
    let p = program(vec![a::Stmt::VarDecl {
        name: "bad".into(),
        ty: a::TypeRef::Int { bits: 7, signed: true },
        init: None,
    }]);
    compare_error(&p, AstLimits::default());
    let mut p = program(vec![]);
    p.functions[0].parameters = vec![
        a::Parameter { name: "same".into(), ty: ty() },
        a::Parameter { name: "same".into(), ty: ty() },
    ];
    compare_error(&p, AstLimits::default());
}

#[test]
fn resource_boundaries_reject_before_publishing_partial_modules() {
    let plain = program(vec![a::Stmt::Return(Some(lit(42)))]);
    for limits in [
        AstLimits { max_nodes: 1, ..AstLimits::default() },
        AstLimits { max_string_bytes: 1, ..AstLimits::default() },
        AstLimits { max_depth: 0, ..AstLimits::default() },
    ] {
        compare_error(&plain, limits)
    }
    let minimal = AstLimits { max_source_work: 0, ..AstLimits::default() };
    assert!(lower_o0_with_limits(&plain, TARGET, DataLayout::default_64bit_le(), minimal).is_ok());
    let located = program(vec![statement(1, a::Stmt::Return(Some(lit(42))))]);
    assert!(matches!(
        lower_o0_with_limits(&located, TARGET, DataLayout::default_64bit_le(), minimal),
        Err(ir::lower_ast::LowerError::ResourceLimit(ir::lower_ast::AstLimitError::SourceWork))
    ));
}

#[test]
fn declaration_call_literal_and_aggregate_diagnostics_match_complete_reference() {
    let mut cases = Vec::new();
    let signature = a::SignatureRef {
        params: vec![ty()],
        ret: ty(),
        convention: CallingConvention::Whale,
        variadic: false,
    };
    let declaration = a::FunctionDeclaration {
        name: "external".into(),
        signature: signature.clone(),
        linkage: Linkage::External,
        link_name: Some("external".into()),
    };
    cases.push(program(vec![a::Stmt::ExprStmt(a::Expr::Call {
        callee: a::CalleeRef::Direct("missing".into()),
        args: vec![],
    })]));
    cases.push(program(vec![a::Stmt::ExprStmt(a::Expr::FunctionRef("missing".into()))]));
    cases.push(program(vec![a::Stmt::ExprStmt(a::Expr::Call {
        callee: a::CalleeRef::Indirect(Box::new(lit(0))),
        args: vec![],
    })]));
    for args in [vec![], vec![a::Expr::Lit(a::Lit::Bool(true))]] {
        let mut p = program(vec![a::Stmt::ExprStmt(a::Expr::Call {
            callee: a::CalleeRef::Direct("external".into()),
            args,
        })]);
        p.declarations.push(declaration.clone());
        cases.push(p);
    }
    let mut p = program(vec![]);
    let mut d = declaration.clone();
    d.linkage = Linkage::Internal;
    d.link_name = None;
    p.declarations.push(d);
    cases.push(p);
    let mut p = program(vec![]);
    let mut d = declaration.clone();
    d.link_name = None;
    p.declarations.push(d);
    cases.push(p);
    let mut p = program(vec![]);
    let mut d = declaration.clone();
    d.link_name = Some("bad\0symbol".into());
    p.declarations.push(d);
    cases.push(p);
    let mut p = program(vec![]);
    let mut other = declaration.clone();
    other.name = "other".into();
    p.declarations = vec![declaration.clone(), other];
    cases.push(p);
    let mut p = program(vec![]);
    let mut other = declaration.clone();
    other.signature.ret = a::TypeRef::Bool;
    p.declarations = vec![declaration, other];
    cases.push(p);
    let structure =
        a::TypeRef::Struct { fields: vec![a::StructField { name: "field".into(), ty: ty() }] };
    for fields in [vec![], vec![a::FieldInit { name: "unknown".into(), value: lit(1) }]] {
        cases.push(program(vec![a::Stmt::ExprStmt(a::Expr::Struct {
            ty: structure.clone(),
            fields,
        })]));
    }
    cases.push(program(vec![
        a::Stmt::VarDecl { name: "s".into(), ty: structure.clone(), init: None },
        a::Stmt::ExprStmt(a::Expr::Field {
            base: Box::new(var("s")),
            layout: structure,
            name: "absent".into(),
        }),
    ]));
    for (bits, signed, value) in [
        (8, true, "128"),
        (8, false, "256"),
        (32, true, "+1"),
        (32, false, "-1"),
        (128, true, "170141183460469231731687303715884105728"),
        (128, false, "340282366920938463463374607431768211456"),
    ] {
        cases.push(program(vec![a::Stmt::ExprStmt(a::Expr::Lit(a::Lit::Int {
            bits,
            signed,
            value: value.into(),
        }))]));
    }
    for value in ["1.0", "0x123", "0xgggggggg"] {
        cases.push(program(vec![a::Stmt::ExprStmt(a::Expr::Lit(a::Lit::Float {
            bits: 32,
            value: value.into(),
        }))]));
    }
    cases.push(program(vec![a::Stmt::ConstDecl {
        name: "bad".into(),
        ty: a::TypeRef::Bool,
        init: bin(
            a::Expr::Lit(a::Lit::Bool(true)),
            a::BinOpRef::BitAnd,
            a::Expr::Lit(a::Lit::Bool(false)),
        ),
    }]));
    for (ordinal, p) in cases.iter().enumerate() {
        let actual = lower_o0(p, TARGET, DataLayout::default_64bit_le()).unwrap_err();
        let expected = lower_o0_reference(p, TARGET, DataLayout::default_64bit_le()).unwrap_err();
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "diagnostic case {ordinal}");
    }
}
