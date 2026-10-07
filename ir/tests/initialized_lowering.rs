#![cfg(feature = "socket")]
use ir::lower_ast::{frontend as ast, lower_o0};
use ir::*;
fn int() -> ast::TypeRef {
    ast::TypeRef::Int {
        bits: 32,
        signed: true,
    }
}
fn lit(n: i32) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Int {
        bits: 32,
        signed: true,
        value: n.to_string(),
    })
}
fn var(name: &str) -> ast::Expr {
    ast::Expr::Var(name.into())
}
fn decl(name: &str, init: Option<ast::Expr>) -> ast::Stmt {
    ast::Stmt::VarDecl {
        name: name.into(),
        ty: int(),
        init,
    }
}
fn assign(name: &str, value: ast::Expr) -> ast::Stmt {
    ast::Stmt::Assign {
        name: name.into(),
        value,
    }
}
fn lower(body: Vec<ast::Stmt>, params: Vec<ast::Parameter>) -> Module {
    let m = lower_o0(
        &ast::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![ast::Function {
                name: "initialize".into(),
                convention: CallingConvention::Whale,
                linkage: Linkage::Internal,
                link_name: None,
                parameters: params,
                return_type: int(),
                body,
            }],
        },
        "x86_64-whale-linux",
        DataLayout::default_64bit_le(),
    )
    .unwrap();
    verify_module(&m).unwrap();
    assert!(!m.functions[0]
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .any(|i| matches!(i, Instruction::Undef { .. })));
    let s = print_module(&m);
    assert_eq!(s, print_module(&parse_module(&s).unwrap()));
    m
}
fn uninitialized(m: &Module, args: &[ConstValue]) {
    assert!(matches!(
        interpret(m, FunctionId(0), args),
        Err(InterpreterError::Trap(InterpreterTrap {
            reason: TrapReason::Memory(MemoryTrap::Uninitialized { .. }),
            ..
        }))
    ));
}
#[test]
fn no_some_and_all_predecessor_initialization_trap_only_on_actual_reads() {
    for (then_init, else_init) in [(false, false), (true, false), (true, true)] {
        let m = lower(
            vec![
                decl("x", None),
                ast::Stmt::If {
                    cond: var("c"),
                    then_body: if then_init {
                        vec![assign("x", lit(42))]
                    } else {
                        vec![]
                    },
                    else_body: if else_init {
                        vec![assign("x", lit(43))]
                    } else {
                        vec![]
                    },
                },
                ast::Stmt::Return(Some(var("x"))),
            ],
            vec![ast::Parameter {
                name: "c".into(),
                ty: ast::TypeRef::Bool,
            }],
        );
        for (flag, initialized, expected) in [(true, then_init, 42), (false, else_init, 43)] {
            let args = [ConstValue::Bool(flag)];
            if initialized {
                assert_eq!(
                    interpret(&m, FunctionId(0), &args).unwrap().value,
                    Some(ConstValue::I(expected))
                );
            } else {
                uninitialized(&m, &args);
            }
        }
    }
}
#[test]
fn loop_redeclaration_resets_hoisted_slot_without_zero_initialization() {
    let m = lower(
        vec![
            decl("i", Some(lit(0))),
            ast::Stmt::While {
                cond: ast::Expr::Cmp {
                    left: Box::new(var("i")),
                    op: ast::CmpOpRef::Lt,
                    right: Box::new(lit(2)),
                },
                body: vec![
                    decl("x", None),
                    ast::Stmt::If {
                        cond: ast::Expr::Cmp {
                            left: Box::new(var("i")),
                            op: ast::CmpOpRef::Eq,
                            right: Box::new(lit(0)),
                        },
                        then_body: vec![assign("x", lit(42))],
                        else_body: vec![ast::Stmt::Return(Some(var("x")))],
                    },
                    assign(
                        "i",
                        ast::Expr::Binary {
                            left: Box::new(var("i")),
                            op: ast::BinOpRef::Add,
                            right: Box::new(lit(1)),
                        },
                    ),
                ],
            },
            ast::Stmt::Return(Some(lit(0))),
        ],
        vec![],
    );
    uninitialized(&m, &[]);
    let f = &m.functions[0];
    assert!(f.blocks.iter().any(|b| b.id != f.entry
        && b.instructions
            .iter()
            .any(|i| matches!(i, Instruction::Uninit { .. }))));
}
#[test]
fn discarded_read_traps_but_retained_unreachable_read_does_not_execute() {
    let m = lower(
        vec![
            decl("x", None),
            ast::Stmt::ExprStmt(var("x")),
            ast::Stmt::Return(Some(lit(42))),
        ],
        vec![],
    );
    uninitialized(&m, &[]);
    let m = lower(
        vec![
            decl("x", None),
            ast::Stmt::Return(Some(lit(42))),
            ast::Stmt::ExprStmt(var("x")),
        ],
        vec![],
    );
    assert!(print_module(&m).contains("load i32"));
    assert_eq!(
        interpret(&m, FunctionId(0), &[]).unwrap().value,
        Some(ConstValue::I(42))
    );
}
