// SPDX-License-Identifier: MPL-2.0

pub mod block;
pub mod builder;
pub mod const_expr;
mod constant;
pub mod float;
pub mod function;
pub mod instr;
pub mod layout;
pub mod module;
pub mod printer;
pub mod types;
pub mod value;
pub mod verifier;
pub mod zero;

#[cfg(feature = "socket")]
pub mod lower_ast;

// re-export는 socket일 때만
#[cfg(feature = "socket")]
pub use lower_ast::*;

pub use block::*;
pub use builder::*;
pub use const_expr::*;
pub use float::*;
pub use function::*;

/// Independently versioned printed typed IR and shared execution semantics.
pub const IR_FORMAT_VERSION: u32 = 3;
pub const SEMANTICS_VERSION: u32 = 1;
pub use instr::*;
pub use layout::*;
pub use module::*;
pub use printer::*;
pub use types::*;
pub use value::*;
pub use verifier::*;

#[cfg(test)]
mod core_tests {
    use super::*;

    #[test]
    fn smoke_build_print_verify() {
        let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());

        let mut fb = mb.begin_function(
            "add_i32",
            vec![("a".into(), Type::I32), ("b".into(), Type::I32)],
            Type::I32,
        );

        let a = fb.param_value(0);
        let b = fb.param_value(1);

        let t0 = fb.add(Type::I32, a, b);
        fb.ret(Some(t0));
        fb.finish();

        let mut module = mb.finish();
        zero::pass::run_zero_pass(&mut module);

        verifier::verify_module(&module).unwrap();

        let s = printer::print_module(&module);
        assert!(s.contains("fn @f0 \"add_i32\""));
    }
}

#[cfg(all(test, feature = "socket"))]
mod tests {
    use super::*;

    #[test]
    fn smoke_lower_socket_o0_global_const() {
        use crate::lower_ast::{frontend as s, lower_o0};

        let i32s = s::TypeRef::Int {
            bits: 32,
            signed: true,
        };

        let program = s::Program {
            declarations: vec![],
            globals: vec![s::GlobalConst {
                name: "A".into(),
                ty: i32s.clone(),
                init: s::Expr::Lit(s::Lit::Int {
                    bits: 32,
                    signed: true,
                    value: (123).to_string(),
                }),
            }],
            functions: vec![s::Function {
                convention: crate::CallingConvention::Whale,
                linkage: crate::Linkage::Internal,
                link_name: None,
                name: "main".into(),
                parameters: vec![],
                return_type: i32s.clone(),
                body: vec![s::Stmt::Return(Some(s::Expr::Var("A".into())))],
            }],
        };

        let module = lower_o0(
            &program,
            "x86_64-whale-linux",
            DataLayout::default_64bit_le(),
        )
        .unwrap();
        crate::verifier::verify_module(&module).unwrap();

        let txt = crate::printer::print_module(&module);
        assert!(txt.contains("global @g0 \"A\": i32 = const i32 123, align 4"));
        assert!(txt.contains("fn @f0 \"main\""));
        assert!(txt.contains("ret i32"));
    }

    #[test]
    fn smoke_lower_socket_o0_add() {
        use crate::lower_ast::{frontend as s, lower_o0};

        let program = s::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![s::Function {
                convention: crate::CallingConvention::Whale,
                linkage: crate::Linkage::Internal,
                link_name: None,
                name: "add".into(),
                parameters: vec![
                    s::Parameter {
                        name: "a".into(),
                        ty: s::TypeRef::Int {
                            bits: 32,
                            signed: true,
                        },
                    },
                    s::Parameter {
                        name: "b".into(),
                        ty: s::TypeRef::Int {
                            bits: 32,
                            signed: true,
                        },
                    },
                ],
                return_type: s::TypeRef::Int {
                    bits: 32,
                    signed: true,
                },
                body: vec![s::Stmt::Return(Some(s::Expr::Binary {
                    left: Box::new(s::Expr::Var("a".into())),
                    op: s::BinOpRef::Add,
                    right: Box::new(s::Expr::Var("b".into())),
                }))],
            }],
        };

        let module = lower_o0(
            &program,
            "x86_64-whale-linux",
            DataLayout::default_64bit_le(),
        )
        .unwrap();
        verifier::verify_module(&module).unwrap();

        let s = printer::print_module(&module);
        assert!(s.contains("fn @f0 \"add\""));
        assert!(s.contains("add i32"));
        assert!(s.contains("ret i32"));
    }

    #[test]
    fn smoke_lower_socket_o0_if_max() {
        use crate::lower_ast::{frontend as s, lower_o0};

        let i32s = s::TypeRef::Int {
            bits: 32,
            signed: true,
        };

        let program = s::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![s::Function {
                convention: crate::CallingConvention::Whale,
                linkage: crate::Linkage::Internal,
                link_name: None,
                name: "max".into(),
                parameters: vec![
                    s::Parameter {
                        name: "a".into(),
                        ty: i32s.clone(),
                    },
                    s::Parameter {
                        name: "b".into(),
                        ty: i32s.clone(),
                    },
                ],
                return_type: i32s.clone(),
                body: vec![
                    s::Stmt::VarDecl {
                        name: "x".into(),
                        ty: i32s.clone(),
                        init: Some(s::Expr::Lit(s::Lit::Int {
                            bits: 32,
                            signed: true,
                            value: (0).to_string(),
                        })),
                    },
                    s::Stmt::If {
                        cond: s::Expr::Cmp {
                            left: Box::new(s::Expr::Var("a".into())),
                            op: s::CmpOpRef::Gt,
                            right: Box::new(s::Expr::Var("b".into())),
                        },
                        then_body: vec![s::Stmt::Assign {
                            name: "x".into(),
                            value: s::Expr::Var("a".into()),
                        }],
                        else_body: vec![s::Stmt::Assign {
                            name: "x".into(),
                            value: s::Expr::Var("b".into()),
                        }],
                    },
                    s::Stmt::Return(Some(s::Expr::Var("x".into()))),
                ],
            }],
        };

        let module = lower_o0(
            &program,
            "x86_64-whale-linux",
            DataLayout::default_64bit_le(),
        )
        .unwrap();
        verifier::verify_module(&module).unwrap();

        let s = printer::print_module(&module);
        assert!(s.contains("fn @f0 \"max\""));
        assert!(s.contains("cmp sgt i32"));
        assert!(s.contains("cbr bool"));
        assert!(s.contains("ret i32"));
    }
    #[test]
    fn smoke_lower_socket_o0_while_sum() {
        use crate::lower_ast::{frontend as s, lower_o0};

        let i32s = s::TypeRef::Int {
            bits: 32,
            signed: true,
        };
        let lit_i32 = |v: i128| {
            s::Expr::Lit(s::Lit::Int {
                bits: 32,
                signed: true,
                value: (v).to_string(),
            })
        };

        let program = s::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![s::Function {
                convention: crate::CallingConvention::Whale,
                linkage: crate::Linkage::Internal,
                link_name: None,
                name: "sum_to_n".into(),
                parameters: vec![s::Parameter {
                    name: "n".into(),
                    ty: i32s.clone(),
                }],
                return_type: i32s.clone(),
                body: vec![
                    s::Stmt::VarDecl {
                        name: "i".into(),
                        ty: i32s.clone(),
                        init: Some(lit_i32(0)),
                    },
                    s::Stmt::VarDecl {
                        name: "sum".into(),
                        ty: i32s.clone(),
                        init: Some(lit_i32(0)),
                    },
                    s::Stmt::While {
                        cond: s::Expr::Cmp {
                            left: Box::new(s::Expr::Var("i".into())),
                            op: s::CmpOpRef::Lt,
                            right: Box::new(s::Expr::Var("n".into())),
                        },
                        body: vec![
                            s::Stmt::Assign {
                                name: "sum".into(),
                                value: s::Expr::Binary {
                                    left: Box::new(s::Expr::Var("sum".into())),
                                    op: s::BinOpRef::Add,
                                    right: Box::new(s::Expr::Var("i".into())),
                                },
                            },
                            s::Stmt::Assign {
                                name: "i".into(),
                                value: s::Expr::Binary {
                                    left: Box::new(s::Expr::Var("i".into())),
                                    op: s::BinOpRef::Add,
                                    right: Box::new(lit_i32(1)),
                                },
                            },
                        ],
                    },
                    s::Stmt::Return(Some(s::Expr::Var("sum".into()))),
                ],
            }],
        };

        let module = lower_o0(
            &program,
            "x86_64-whale-linux",
            DataLayout::default_64bit_le(),
        )
        .unwrap();
        verifier::verify_module(&module).unwrap();

        let txt = printer::print_module(&module);
        assert!(txt.contains("\"while.cond\":"));
        assert!(txt.contains("\"while.body\":"));
        assert!(txt.contains("\"while.exit\":"));
        assert!(txt.contains("cbr bool"));
        assert!(txt.contains("ret i32"));
    }
    #[test]
    fn smoke_lower_socket_o0_while_break() {
        use crate::lower_ast::{frontend as s, lower_o0};
        use crate::DataLayout;

        let program = s::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![s::Function {
                convention: crate::CallingConvention::Whale,
                linkage: crate::Linkage::Internal,
                link_name: None,
                name: "main".into(),
                parameters: vec![],
                return_type: s::TypeRef::Int {
                    bits: 32,
                    signed: true,
                },
                body: vec![
                    s::Stmt::VarDecl {
                        name: "x".into(),
                        ty: s::TypeRef::Int {
                            bits: 32,
                            signed: true,
                        },
                        init: Some(s::Expr::Lit(s::Lit::Int {
                            bits: 32,
                            signed: true,
                            value: (0).to_string(),
                        })),
                    },
                    s::Stmt::While {
                        cond: s::Expr::Lit(s::Lit::Bool(true)),
                        body: vec![
                            s::Stmt::Assign {
                                name: "x".into(),
                                value: s::Expr::Lit(s::Lit::Int {
                                    bits: 32,
                                    signed: true,
                                    value: (1).to_string(),
                                }),
                            },
                            s::Stmt::Break,
                            s::Stmt::Assign {
                                name: "x".into(),
                                value: s::Expr::Lit(s::Lit::Int {
                                    bits: 32,
                                    signed: true,
                                    value: (2).to_string(),
                                }),
                            },
                        ],
                    },
                    s::Stmt::Return(Some(s::Expr::Var("x".into()))),
                ],
            }],
        };

        let module = lower_o0(
            &program,
            "x86_64-whale-linux",
            DataLayout::default_64bit_le(),
        )
        .unwrap();
        crate::verifier::verify_module(&module).unwrap();

        let s = crate::printer::print_module(&module);
        assert!(s.contains("while.cond"));
        assert!(s.contains("while.exit"));
        assert!(s.contains("br label"));
    }
}
