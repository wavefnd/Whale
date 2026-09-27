#![cfg(feature = "socket")]

use ir::lower_ast::{frontend as ast, lower_o0};
use ir::{DataLayout, Instruction};

fn int(value: i128) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Int {
        bits: 32,
        signed: true,
        value: value.to_string(),
    })
}
fn sum(left: ast::Expr, right: ast::Expr) -> ast::Expr {
    ast::Expr::Binary {
        left: Box::new(left),
        op: ast::BinOpRef::Add,
        right: Box::new(right),
    }
}
fn ty() -> ast::TypeRef {
    ast::TypeRef::Int {
        bits: 32,
        signed: true,
    }
}

#[test]
fn unused_and_unreachable_constant_declarations_keep_their_expressions() {
    let program = ast::Program {
        declarations: vec![],
        globals: vec![ast::GlobalConst {
            name: "global_sum".into(),
            ty: ty(),
            init: sum(int(1), int(2)),
        }],
        functions: vec![ast::Function {
            convention: ir::CallingConvention::Whale,
            linkage: ir::Linkage::Internal,
            link_name: None,
            name: "f".into(),
            parameters: vec![],
            return_type: ast::TypeRef::Void,
            body: vec![
                ast::Stmt::ConstDecl {
                    name: "unused".into(),
                    ty: ty(),
                    init: sum(int(4), int(5)),
                },
                ast::Stmt::Return(None),
                ast::Stmt::ConstDecl {
                    name: "dead".into(),
                    ty: ty(),
                    init: sum(ast::Expr::Var("unused".into()), int(6)),
                },
            ],
        }],
    };
    let m = lower_o0(
        &program,
        "x86_64-whale-linux",
        DataLayout::default_64bit_le(),
    )
    .unwrap();
    ir::verify_module(&m).unwrap();
    let text = ir::print_module(&m);
    assert!(text.contains("const_decl \"unused\""), "{text}");
    assert!(text.contains("const_decl \"dead\""), "{text}");
    assert!(text.contains("add(i32 1, i32 2)"), "{text}");
    assert!(text.contains("add(i32 4, i32 5)"), "{text}");
    assert!(
        !m.functions[0]
            .blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .any(|i| matches!(i, Instruction::Bin { .. })),
        "compile-time expressions must not become runtime arithmetic"
    );
}

#[test]
fn shadowed_names_keep_distinct_declarations_and_resolved_references() {
    let program = ast::Program {
        declarations: vec![],
        globals: vec![ast::GlobalConst {
            name: "value".into(),
            ty: ty(),
            init: int(10),
        }],
        functions: vec![ast::Function {
            convention: ir::CallingConvention::Whale,
            linkage: ir::Linkage::Internal,
            link_name: None,
            name: "f".into(),
            parameters: vec![],
            return_type: ast::TypeRef::Bool,
            body: vec![
                ast::Stmt::ConstDecl {
                    name: "value".into(),
                    ty: ty(),
                    init: sum(ast::Expr::Var("value".into()), int(1)),
                },
                ast::Stmt::ConstDecl {
                    name: "value".into(),
                    ty: ty(),
                    init: sum(ast::Expr::Var("value".into()), int(1)),
                },
                ast::Stmt::ConstDecl {
                    name: "equal".into(),
                    ty: ast::TypeRef::Bool,
                    init: ast::Expr::Cmp {
                        left: Box::new(ast::Expr::Var("value".into())),
                        op: ast::CmpOpRef::Eq,
                        right: Box::new(int(12)),
                    },
                },
                ast::Stmt::Return(Some(ast::Expr::Var("equal".into()))),
            ],
        }],
    };
    let m = lower_o0(
        &program,
        "x86_64-whale-linux",
        DataLayout::default_64bit_le(),
    )
    .unwrap();
    ir::verify_module(&m).unwrap();
    let declarations = m.functions[0].blocks[0]
        .instructions
        .iter()
        .filter_map(|i| match i {
            Instruction::ConstDecl {
                dst,
                expression,
                value,
                ..
            } => Some((*dst, expression, value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 3);
    assert_eq!(
        declarations[0].1.references(),
        vec![ir::ConstRef::Global(m.globals[0].id)]
    );
    assert_eq!(
        declarations[1].1.references(),
        vec![ir::ConstRef::Local(declarations[0].0)]
    );
    assert_eq!(
        declarations[2].1.references(),
        vec![ir::ConstRef::Local(declarations[1].0)]
    );
    assert_eq!(declarations[0].2, &ir::ConstValue::I(11));
    assert_eq!(declarations[1].2, &ir::ConstValue::I(12));
    assert_eq!(declarations[2].2, &ir::ConstValue::Bool(true));
    assert!(
        matches!(m.functions[0].blocks[0].terminator,Some(ir::Terminator::Ret {value:Some(value),..}) if value==declarations[2].0)
    );
}
