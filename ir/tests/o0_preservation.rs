#![cfg(feature = "socket")]

use ir::lower_ast::{frontend as ast, lower_o0, LowerError};
use ir::{ConstValue, DataLayout, Instruction, Module, Terminator};

fn literal(value: i128) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Int {
        bits: 32,
        signed: true,
        value: value.to_string(),
    })
}
fn expression(value: i128) -> ast::Stmt {
    ast::Stmt::ExprStmt(ast::Expr::Binary {
        left: Box::new(literal(value)),
        op: ast::BinOpRef::Add,
        right: Box::new(literal(1)),
    })
}
fn lower(body: Vec<ast::Stmt>) -> Result<Module, LowerError> {
    lower_o0(
        &ast::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![ast::Function {
                convention: ir::CallingConvention::Whale,
                linkage: ir::Linkage::Internal,
                link_name: None,
                name: "preserve".into(),
                parameters: vec![],
                return_type: ast::TypeRef::Void,
                body,
            }],
        },
        "x86_64-whale-linux",
        DataLayout::default_64bit_le(),
    )
}
fn has_literal(m: &Module, value: i128) -> bool {
    m.functions[0]
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .any(|i| matches!(i,Instruction::Const {value:ConstValue::I(actual),..} if *actual==value))
}
fn check_unchanged(m: &Module) {
    let before = format!("{m:?}");
    ir::verify_module(m).unwrap();
    assert_eq!(before, format!("{m:?}"));
}

#[test]
fn statements_after_return_remain_in_disconnected_blocks() {
    let m = lower(vec![
        expression(10),
        ast::Stmt::Return(None),
        expression(20),
        ast::Stmt::Return(None),
        expression(30),
    ])
    .unwrap();
    check_unchanged(&m);
    for value in [10, 20, 30] {
        assert!(has_literal(&m, value));
    }
    assert_eq!(m.functions[0].blocks.len(), 3);
    assert!(m.functions[0]
        .blocks
        .iter()
        .all(|b| matches!(b.terminator, Some(Terminator::Ret { .. }))));
    assert_eq!(
        m.functions[0]
            .blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .filter(|i| matches!(i, Instruction::Bin { .. }))
            .count(),
        3
    );
}

#[test]
fn constant_branches_and_dead_loop_tails_are_not_simplified() {
    for jump in [
        ast::Stmt::Break,
        ast::Stmt::Continue,
        ast::Stmt::Return(None),
    ] {
        let m = lower(vec![
            ast::Stmt::If {
                cond: ast::Expr::Lit(ast::Lit::Bool(false)),
                then_body: vec![expression(40)],
                else_body: vec![expression(50)],
            },
            ast::Stmt::While {
                cond: ast::Expr::Lit(ast::Lit::Bool(false)),
                body: vec![jump, expression(60)],
            },
            ast::Stmt::Return(None),
        ])
        .unwrap();
        check_unchanged(&m);
        for value in [40, 50, 60] {
            assert!(has_literal(&m, value));
        }
        assert_eq!(
            m.functions[0]
                .blocks
                .iter()
                .filter(|b| matches!(b.terminator, Some(Terminator::CBr { .. })))
                .count(),
            2
        );
    }
}

#[test]
fn invalid_unreachable_statements_are_diagnosed_instead_of_discarded() {
    assert!(matches!(lower(vec![ast::Stmt::Return(None),
        ast::Stmt::ExprStmt(ast::Expr::Var("missing".into()))]),
        Err(LowerError::UnknownVariable(name)) if name=="missing"));
    assert!(matches!(
        lower(vec![ast::Stmt::Return(None), ast::Stmt::Break]),
        Err(LowerError::BreakOutsideLoop)
    ));
}
