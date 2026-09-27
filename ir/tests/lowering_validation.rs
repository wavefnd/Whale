#![cfg(feature = "socket")]

use ir::lower_ast::{frontend as ast, lower_o0, LowerError};
use ir::{ConstValue, DataLayout, Type};

fn function(name: &str, ty: ast::TypeRef, expr: Option<ast::Expr>) -> ast::Function {
    ast::Function {
        convention: ir::CallingConvention::Whale,
        linkage: ir::Linkage::Internal,
        link_name: None,
        name: name.into(),
        parameters: vec![],
        return_type: ty,
        body: vec![ast::Stmt::Return(expr)],
    }
}

fn lower(program: ast::Program) -> Result<ir::Module, LowerError> {
    lower_o0(
        &program,
        "x86_64-whale-linux",
        DataLayout::default_64bit_le(),
    )
}

fn int(bits: u16, signed: bool, value: i128) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Int {
        bits,
        signed,
        value: value.to_string(),
    })
}

#[test]
fn names_are_unique_within_their_own_namespaces() {
    let f = function("same", ast::TypeRef::Void, None);
    assert!(
        matches!(lower(ast::Program { declarations: vec![], globals: vec![], functions: vec![f.clone(), f.clone()] }), Err(LowerError::DuplicateFunction(name)) if name == "same")
    );
    let global = ast::GlobalConst {
        name: "same".into(),
        ty: ast::TypeRef::Int {
            bits: 32,
            signed: true,
        },
        init: int(32, true, 1),
    };
    assert!(
        matches!(lower(ast::Program { declarations: vec![], globals: vec![global.clone(), global.clone()], functions: vec![] }), Err(LowerError::DuplicateGlobal(name)) if name == "same")
    );
    let m = lower(ast::Program {
        declarations: vec![],
        globals: vec![global],
        functions: vec![f, function("other", ast::TypeRef::Void, None)],
    })
    .unwrap();
    ir::verify_module(&m).unwrap();
    assert_eq!(m.globals[0].name, m.functions[0].name);
}

#[test]
fn duplicate_parameters_fail_before_the_body_is_lowered() {
    let p = ast::Parameter {
        name: "x".into(),
        ty: ast::TypeRef::Int {
            bits: 32,
            signed: true,
        },
    };
    let mut f = function(
        "f",
        ast::TypeRef::Int {
            bits: 32,
            signed: true,
        },
        Some(ast::Expr::Var("missing".into())),
    );
    f.parameters = vec![p.clone(), p];
    assert!(
        matches!(lower(ast::Program { declarations: vec![], globals: vec![], functions: vec![f.clone()] }), Err(LowerError::DuplicateParameter { func, param }) if func == "f" && param == "x")
    );
    f.parameters[1].name = "y".into();
    for (index, name) in [(0, "x"), (1, "y")] {
        f.body = vec![ast::Stmt::Return(Some(ast::Expr::Var(name.into())))];
        let m = lower(ast::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![f.clone()],
        })
        .unwrap();
        ir::verify_module(&m).unwrap();
        let f = &m.functions[0];
        let ir::Terminator::Ret {
            value: Some(result),
            ..
        } = f.blocks[0].terminator.as_ref().unwrap()
        else {
            panic!("missing return")
        };
        let pointer = f.blocks[0]
            .instructions
            .iter()
            .find_map(|i| match i {
                ir::Instruction::Load { dst, ptr, .. } if dst == result => Some(*ptr),
                _ => None,
            })
            .unwrap();
        assert!(f.blocks[0].instructions.iter().any(|i| matches!(i, ir::Instruction::Store { ptr, value, .. } if *ptr == pointer && *value == f.params[index].id)));
    }
}

#[test]
fn void_returns_never_discard_an_expression() {
    for expr in [int(32, true, 1), ast::Expr::Var("missing".into())] {
        assert!(matches!(
            lower(ast::Program {
                declarations: vec![],
                globals: vec![],
                functions: vec![function("f", ast::TypeRef::Void, Some(expr))]
            }),
            Err(LowerError::ValueReturnedFromVoid)
        ));
    }
    let m = lower(ast::Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![function("f", ast::TypeRef::Void, None)],
    })
    .unwrap();
    ir::verify_module(&m).unwrap();
    assert!(matches!(
        lower(ast::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![function(
                "f",
                ast::TypeRef::Int {
                    bits: 32,
                    signed: true
                },
                Some(ast::Expr::Lit(ast::Lit::Bool(true)))
            )]
        }),
        Err(LowerError::TypeMismatch { .. })
    ));
}

#[test]
fn integer_literals_have_the_same_range_in_globals_and_function_bodies() {
    for (bits, signed, value, valid) in [
        (1, true, -1, true),
        (1, true, 0, true),
        (1, true, 1, false),
        (1, true, -2, false),
        (1, false, 0, true),
        (1, false, 1, true),
        (1, false, 2, false),
        (1, false, -1, false),
        (8, true, -128, true),
        (8, true, 127, true),
        (8, true, -129, false),
        (8, true, 128, false),
        (8, false, 0, true),
        (8, false, 255, true),
        (8, false, 256, false),
        (8, false, -1, false),
        (32, true, 1, true),
        (128, true, i128::MIN, true),
        (128, true, i128::MAX, true),
        (128, false, i128::MAX, true),
    ] {
        let ty = ast::TypeRef::Int { bits, signed };
        let expr = int(bits, signed, value);
        let programs = [
            ast::Program {
                declarations: vec![],
                globals: vec![],
                functions: vec![function("f", ty.clone(), Some(expr.clone()))],
            },
            ast::Program {
                declarations: vec![],
                globals: vec![ast::GlobalConst {
                    name: "g".into(),
                    ty,
                    init: expr,
                }],
                functions: vec![],
            },
        ];
        for program in programs {
            match lower(program) {
                Ok(m) => {
                    assert!(valid, "accepted bits={bits} signed={signed} value={value}");
                    ir::verify_module(&m).unwrap();
                }
                Err(e) => {
                    assert!(!valid, "{e:?}");
                    assert!(matches!(
                        e,
                        LowerError::InvalidLiteral { .. } | LowerError::NumericLiteral(_)
                    ));
                }
            }
        }
    }
}

#[test]
fn bool_and_one_bit_integer_types_remain_distinct_in_printed_ir() {
    let m = lower(ast::Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![
            function(
                "boolean",
                ast::TypeRef::Bool,
                Some(ast::Expr::Lit(ast::Lit::Bool(true))),
            ),
            function(
                "signed",
                ast::TypeRef::Int {
                    bits: 1,
                    signed: true,
                },
                Some(int(1, true, -1)),
            ),
            function(
                "unsigned",
                ast::TypeRef::Int {
                    bits: 1,
                    signed: false,
                },
                Some(int(1, false, 1)),
            ),
        ],
    })
    .unwrap();
    ir::verify_module(&m).unwrap();
    assert_eq!(
        m.functions
            .iter()
            .map(|f| f.ret_ty.clone())
            .collect::<Vec<_>>(),
        vec![Type::Bool, Type::I1, Type::U1]
    );
    let text = ir::print_module(&m);
    for expected in ["const bool true", "const i1 -1", "const u1 1"] {
        assert!(text.contains(expected), "{text}");
    }
}

#[test]
fn integer_arithmetic_wrapping_is_unchanged_by_literal_validation() {
    let m = lower(ast::Program {
        declarations: vec![],
        globals: vec![ast::GlobalConst {
            name: "g".into(),
            ty: ast::TypeRef::Int {
                bits: 8,
                signed: true,
            },
            init: ast::Expr::Binary {
                left: Box::new(int(8, true, 127)),
                op: ast::BinOpRef::Add,
                right: Box::new(int(8, true, 1)),
            },
        }],
        functions: vec![],
    })
    .unwrap();
    assert_eq!(m.globals[0].init, ConstValue::I(-128));
    ir::verify_module(&m).unwrap();
}

#[test]
fn boolean_equality_is_supported_but_integer_arithmetic_is_not_boolean_arithmetic() {
    for runtime in [false, true] {
        for equality in [false, true] {
            let left = Box::new(ast::Expr::Lit(ast::Lit::Bool(true)));
            let right = Box::new(ast::Expr::Lit(ast::Lit::Bool(false)));
            let expr = if equality {
                ast::Expr::Cmp {
                    left,
                    op: ast::CmpOpRef::Eq,
                    right,
                }
            } else {
                ast::Expr::Binary {
                    left,
                    op: ast::BinOpRef::Add,
                    right,
                }
            };
            let program = if runtime {
                ast::Program {
                    declarations: vec![],
                    globals: vec![],
                    functions: vec![function("f", ast::TypeRef::Bool, Some(expr))],
                }
            } else {
                ast::Program {
                    declarations: vec![],
                    globals: vec![ast::GlobalConst {
                        name: "g".into(),
                        ty: ast::TypeRef::Bool,
                        init: expr,
                    }],
                    functions: vec![],
                }
            };
            let result = lower(program);
            if equality {
                ir::verify_module(&result.unwrap()).unwrap();
            } else {
                assert!(matches!(result, Err(LowerError::UnsupportedExpr)));
            }
        }
    }
}

#[test]
fn one_bit_integer_conditions_need_explicit_conversion() {
    for condition in [int(1, true, -1), int(1, false, 1)] {
        let mut f = function("f", ast::TypeRef::Void, None);
        f.body.insert(
            0,
            ast::Stmt::If {
                cond: condition,
                then_body: vec![],
                else_body: vec![],
            },
        );
        assert!(matches!(
            lower(ast::Program {
                declarations: vec![],
                globals: vec![],
                functions: vec![f]
            }),
            Err(LowerError::TypeMismatch {
                expected: Type::Bool,
                ..
            })
        ));
    }
}

#[test]
fn o0_keeps_function_arithmetic_visible_in_ir() {
    let expr = ast::Expr::Binary {
        left: Box::new(int(32, true, 1)),
        op: ast::BinOpRef::Add,
        right: Box::new(int(32, true, 2)),
    };
    let m = lower(ast::Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![function(
            "f",
            ast::TypeRef::Int {
                bits: 32,
                signed: true,
            },
            Some(expr),
        )],
    })
    .unwrap();
    ir::verify_module(&m).unwrap();
    let instructions = &m.functions[0].blocks[0].instructions;
    assert!(matches!(
        instructions.as_slice(),
        [
            ir::Instruction::Const {
                value: ConstValue::I(1),
                ..
            },
            ir::Instruction::Const {
                value: ConstValue::I(2),
                ..
            },
            ir::Instruction::Bin {
                op: ir::BinOp::Add,
                ..
            }
        ]
    ));
}
