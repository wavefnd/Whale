#![cfg(feature = "socket")]
use ir::lower_ast::{frontend as a, lower_o0, LowerError};

use ir::{CallingConvention, ConstEvalError, DataLayout, Linkage};

fn integer(value: &str) -> a::Expr {
    a::Expr::Lit(a::Lit::Int { bits: 32, signed: true, value: value.into() })
}

fn ty() -> a::TypeRef {
    a::TypeRef::Int { bits: 32, signed: true }
}

fn zero_division() -> a::Expr {
    a::Expr::Binary {
        left: Box::new(integer("1")),
        op: a::BinOpRef::Div,
        right: Box::new(integer("0")),
    }
}

fn program(body: Vec<a::Stmt>) -> a::Program {
    a::Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![a::Function {
            name: "main".into(),
            convention: CallingConvention::Whale,
            linkage: Linkage::Internal,
            link_name: None,
            parameters: vec![],
            return_type: a::TypeRef::Void,
            body,
        }],
    }
}

fn error(p: &a::Program) -> LowerError {
    lower_o0(p, "x86_64-whale-linux", DataLayout::default_64bit_le()).unwrap_err()
}

fn assert_division(error: LowerError) {
    assert!(
        matches!(
            error.root_cause(),
            LowerError::ConstantEvaluation(ConstEvalError::DivisionByZero)
        ),
        "{error:?}"
    );
}

#[test]
fn global_constant_arithmetic_precedes_later_declarations_and_its_type_comparison() {
    for declared in [ty(), a::TypeRef::Bool] {
        let mut p = program(vec![a::Stmt::ExprStmt(a::Expr::Var("missing".into()))]);
        p.globals.push(a::GlobalConst {
            name: "first".into(),
            ty: declared,
            init: zero_division(),
        });
        p.globals.push(a::GlobalConst {
            name: "later".into(),
            ty: ty(),
            init: a::Expr::Var("absent".into()),
        });
        assert_division(error(&p));
    }
    let mut p = program(vec![]);
    p.globals = vec![
        a::GlobalConst { name: "same".into(), ty: ty(), init: zero_division() },
        a::GlobalConst { name: "same".into(), ty: ty(), init: integer("0") },
    ];
    assert_division(error(&p));
}

#[test]
fn local_constant_arithmetic_precedes_later_statements_and_its_type_comparison() {
    for declared in [ty(), a::TypeRef::Bool] {
        let p = program(vec![
            a::Stmt::ConstDecl { name: "first".into(), ty: declared, init: zero_division() },
            a::Stmt::ExprStmt(a::Expr::Var("missing".into())),
        ]);
        assert_division(error(&p));
    }
}

#[test]
fn complete_expression_binding_validation_precedes_its_arithmetic() {
    let expr = a::Expr::Binary {
        left: Box::new(zero_division()),
        op: a::BinOpRef::Add,
        right: Box::new(a::Expr::Var("inside".into())),
    };
    let p = program(vec![a::Stmt::ConstDecl { name: "first".into(), ty: ty(), init: expr }]);
    let e = error(&p);
    assert!(
        matches!(e.root_cause(), LowerError::UnknownVariable(name) if name == "inside"),
        "{e:?}"
    );
}

#[test]
fn lazy_constant_evaluation_does_not_execute_a_skipped_division() {
    let comparison = a::Expr::Cmp {
        left: Box::new(zero_division()),
        op: a::CmpOpRef::Eq,
        right: Box::new(integer("0")),
    };
    let expr = a::Expr::Binary {
        left: Box::new(a::Expr::Lit(a::Lit::Bool(false))),
        op: a::BinOpRef::LogicalAnd,
        right: Box::new(comparison),
    };
    let p = program(vec![
        a::Stmt::ConstDecl { name: "first".into(), ty: a::TypeRef::Bool, init: expr },
        a::Stmt::ExprStmt(a::Expr::Var("later".into())),
    ]);
    let e = error(&p);
    assert!(
        matches!(e.root_cause(), LowerError::UnknownVariable(name) if name == "later"),
        "{e:?}"
    );
}

#[test]
fn earlier_constant_arithmetic_retains_the_innermost_original_identity() {
    let expr = a::Expr::Located {
        annotation: ir::SourceAnnotation { id: 71, source: None },
        expression: Box::new(a::Expr::Located {
            annotation: ir::SourceAnnotation { id: 83, source: None },
            expression: Box::new(zero_division()),
        }),
    };
    let p = program(vec![
        a::Stmt::ConstDecl { name: "first".into(), ty: ty(), init: expr },
        a::Stmt::ExprStmt(a::Expr::Var("later".into())),
    ]);
    let e = error(&p);
    assert_eq!(e.annotation().map(|a| a.id), Some(83), "{e:?}");
    assert_division(e);
}
