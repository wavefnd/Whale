use ir::*;

use diagnostic::{Context, ToDiagnostic};

#[test]
fn parser_and_verifier_keep_typed_categories_and_available_locations() {
    let e = parse_module("not a module").unwrap_err();
    let d = e.diagnostic();
    assert_eq!(d.code, "ir.parse.syntax");
    assert!(d
        .contexts
        .iter()
        .any(|c| matches!(c,Context::Source(s)if s.line==1&&s.column==1&&s.start_byte==Some(0))));
    let e = VerifyError::NonDominatingValue {
        func: "provided-function".into(),
        value: ValueId(9),
        block: BlockId(7),
    };
    let d = e.diagnostic();
    assert_eq!(d.code, "ir.verify.non-dominating-value");
    assert!(d.contexts.contains(&Context::Symbol("provided-function".into())));
    assert!(d.contexts.contains(&Context::Block(7)));
}

#[test]
fn runtime_conversion_keeps_original_source_and_leaf_operation_from_same_cause() {
    let module = parse_module(include_str!("fixtures/runtime-diagnostics-v7.wir")).unwrap();
    let error = interpret(&module, FunctionId(0), &[]).unwrap_err();
    let d = ToDiagnostic::diagnostic(&error);
    assert_eq!(d.code, "ir.run.trap");
    assert!(d.contexts.contains(&Context::Function(2)));
    assert!(d.contexts.contains(&Context::Block(0)));
    assert!(d.contexts.contains(&Context::Instruction(1)));
    assert!(d.contexts.contains(&Context::OriginalNode(300)));
    assert!(d.contexts.iter().any(|c|matches!(c,Context::Source(s)if s.source=="original.wave"&&s.line==11&&s.column==13&&s.end_line==Some(11)&&s.end_column==Some(18))));
    assert_eq!(d.message, error.to_string());
    assert!(
        matches!(error.root_cause(),InterpreterError::Trap(t)if t.reason==TrapReason::DivisionByZero)
    );
}

#[cfg(feature = "socket")]
#[test]
fn located_frontend_cause_retains_source_and_original_identity() {
    use ir::lower_ast::{frontend as ast, lower_o0};
    let p = ast::Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![ast::Function {
            name: "f".into(),
            parameters: vec![],
            return_type: ast::TypeRef::Bool,
            body: vec![ast::Stmt::Return(Some(ast::Expr::Located {
                annotation: SourceAnnotation {
                    id: 9,
                    source: Some(FrontendSource {
                        file: "f.wave".into(),
                        line: 3,
                        column: 2,
                        end_line: 3,
                        end_column: 8,
                    }),
                },
                expression: Box::new(ast::Expr::Var("missing".into())),
            }))],
            convention: CallingConvention::Whale,
            linkage: Linkage::Internal,
            link_name: None,
        }],
    };
    let e = lower_o0(&p, "x86_64-whale-linux", DataLayout::default_64bit_le()).unwrap_err();
    let d = e.diagnostic();
    assert_eq!(d.code, "ir.lower.unknown-variable");
    assert!(d.contexts.contains(&Context::OriginalNode(9)));
    assert!(d
        .contexts
        .iter()
        .any(|c| matches!(c,Context::Source(s)if s.source=="f.wave"&&s.line==3&&s.column==2)));
}
