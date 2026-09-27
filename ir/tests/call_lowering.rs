#![cfg(feature = "socket")]
use ir::lower_ast::{
    frontend as a,
    interchange::{decode, encode},
    lower_o0, LowerError,
};
use ir::*;
fn ty() -> a::TypeRef {
    a::TypeRef::Int {
        bits: 32,
        signed: true,
    }
}
fn sig(params: Vec<a::TypeRef>, ret: a::TypeRef) -> a::SignatureRef {
    a::SignatureRef {
        params,
        ret,
        convention: CallingConvention::Whale,
        variadic: false,
    }
}
fn decl(name: &str, params: Vec<a::TypeRef>, ret: a::TypeRef) -> a::FunctionDeclaration {
    a::FunctionDeclaration {
        name: name.into(),
        signature: sig(params, ret),
        linkage: Linkage::External,
        link_name: Some(name.into()),
    }
}
fn call(name: &str, args: Vec<a::Expr>) -> a::Expr {
    a::Expr::Call {
        callee: a::CalleeRef::Direct(name.into()),
        args,
    }
}
fn lit() -> a::Expr {
    a::Expr::Lit(a::Lit::Int {
        bits: 32,
        signed: true,
        value: "7".into(),
    })
}
fn function(name: &str, ret: a::TypeRef, body: Vec<a::Stmt>) -> a::Function {
    a::Function {
        name: name.into(),
        parameters: vec![],
        return_type: ret,
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
        functions: vec![function("entry", a::TypeRef::Void, body)],
    }
}
fn lower(p: &a::Program) -> Result<Module, LowerError> {
    lower_o0(p, "x86_64-whale-linux", DataLayout::default_64bit_le())
}

#[test]
fn forward_recursive_and_external_names_resolve_in_the_function_namespace() {
    let mut p = program(vec![
        a::Stmt::VarDecl {
            name: "later".into(),
            ty: ty(),
            init: Some(lit()),
        },
        a::Stmt::ExprStmt(call("later", vec![])),
    ]);
    p.functions.push(function(
        "later",
        a::TypeRef::Void,
        vec![a::Stmt::ExprStmt(call("entry", vec![]))],
    ));
    let mut declared = decl("later", vec![], a::TypeRef::Void);
    declared.linkage = Linkage::Internal;
    declared.link_name = None;
    p.declarations.push(declared);
    let m = lower(&p).unwrap();
    verify_module(&m).unwrap();
    assert_eq!(m.declarations.len(), 2);
    assert!(m.functions[0].blocks[0]
        .instructions
        .iter()
        .any(|i| matches!(
            i,
            Instruction::Call {
                callee: Callee::Direct(FunctionId(0)),
                ..
            }
        )));
    p.declarations[0].signature.ret = ty();
    assert!(matches!(
        lower(&p),
        Err(LowerError::Call(CallError::ConflictingDeclaration(_)))
    ));
    p.functions.pop();
    p.declarations[0].signature.ret = a::TypeRef::Void;
    assert!(matches!(
        lower(&p),
        Err(LowerError::Call(CallError::MissingDefinition(_)))
    ));
}

#[test]
fn callee_then_arguments_are_evaluated_left_to_right_and_unused_results_survive_o0() {
    let callback = a::TypeRef::FnPtr(Box::new(sig(vec![ty(), ty()], ty())));
    let invocation = a::Expr::Call {
        callee: a::CalleeRef::Indirect(Box::new(call("choose", vec![]))),
        args: vec![call("first", vec![]), call("second", vec![])],
    };
    let mut p = program(vec![
        a::Stmt::ExprStmt(invocation.clone()),
        a::Stmt::Return(None),
        a::Stmt::ExprStmt(invocation),
    ]);
    p.declarations = vec![
        decl("choose", vec![], callback),
        decl("first", vec![], ty()),
        decl("second", vec![], ty()),
    ];
    let m = lower(&p).unwrap();
    verify_module(&m).unwrap();
    for block in &m.functions[0].blocks {
        let calls = block
            .instructions
            .iter()
            .filter_map(|i| {
                if let Instruction::Call { dst, callee, .. } = i {
                    Some((dst, callee))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 4);
        for (i, (result, callee)) in calls[..3].iter().enumerate() {
            assert!(result.is_some());
            assert!(matches!(callee, Callee::Direct(id) if id.0 == i as u32));
        }
        assert!(calls[3].0.is_some());
        assert!(matches!(calls[3].1, Callee::Indirect(value) if Some(*value) == *calls[0].0));
    }
}

#[test]
fn unknown_callees_arity_types_void_values_and_nonfunction_values_fail_lowering() {
    let mut p = program(vec![a::Stmt::ExprStmt(call("missing", vec![]))]);
    assert!(matches!(
        lower(&p),
        Err(LowerError::Call(CallError::UnknownFunctionName(_)))
    ));
    p.declarations.push(decl("missing", vec![ty()], ty()));
    assert!(matches!(
        lower(&p),
        Err(LowerError::Call(CallError::ArgumentCount { .. }))
    ));
    p.functions[0].body = vec![a::Stmt::ExprStmt(call(
        "missing",
        vec![a::Expr::Lit(a::Lit::Bool(true))],
    ))];
    assert!(matches!(
        lower(&p),
        Err(LowerError::Call(CallError::ArgumentType { .. }))
    ));
    p.declarations[0].signature = sig(vec![], a::TypeRef::Void);
    p.functions[0].body = vec![a::Stmt::VarDecl {
        name: "value".into(),
        ty: ty(),
        init: Some(call("missing", vec![])),
    }];
    assert!(matches!(lower(&p), Err(LowerError::VoidCallUsedAsValue)));
    p.functions[0].body = vec![a::Stmt::ExprStmt(call("missing", vec![]))];
    verify_module(&lower(&p).unwrap()).unwrap();
    p.functions[0].body = vec![a::Stmt::ExprStmt(a::Expr::Call {
        callee: a::CalleeRef::Indirect(Box::new(lit())),
        args: vec![],
    })];
    assert!(matches!(
        lower(&p),
        Err(LowerError::Call(CallError::NotFunctionPointer(_)))
    ));
}

#[test]
fn function_references_can_be_stored_and_called_but_are_not_numeric_constants() {
    let pointer = a::TypeRef::FnPtr(Box::new(sig(vec![ty()], ty())));
    let mut p = program(vec![
        a::Stmt::VarDecl {
            name: "callback".into(),
            ty: pointer.clone(),
            init: Some(a::Expr::FunctionRef("foreign".into())),
        },
        a::Stmt::ExprStmt(a::Expr::Call {
            callee: a::CalleeRef::Indirect(Box::new(a::Expr::Var("callback".into()))),
            args: vec![lit()],
        }),
    ]);
    p.declarations.push(decl("foreign", vec![ty()], ty()));
    let m = lower(&p).unwrap();
    verify_module(&m).unwrap();
    assert!(print_module(&m).contains("load fnptr<whale (i32) -> i32>"));
    p.functions[0].body[0] = a::Stmt::ConstDecl {
        name: "callback".into(),
        ty: pointer,
        init: a::Expr::FunctionRef("foreign".into()),
    };
    assert!(matches!(lower(&p), Err(LowerError::NonConstExpr)));
    p.functions[0].body = vec![a::Stmt::ExprStmt(a::Expr::Call {
        callee: a::CalleeRef::Indirect(Box::new(a::Expr::NullFunction(sig(
            vec![],
            a::TypeRef::Void,
        )))),
        args: vec![],
    })];
    verify_module(&lower(&p).unwrap()).unwrap();
}

#[test]
fn version_two_calls_fixture_roundtrips_and_matches_printed_ir() {
    let source = include_str!("fixtures/ast-v2-calls.json");
    let canonical = encode(decode(source).unwrap()).unwrap();
    assert_eq!(encode(decode(&canonical).unwrap()).unwrap(), canonical);
    let m = lower(&decode(source).unwrap()).unwrap();
    verify_module(&m).unwrap();
    assert_eq!(print_module(&m), include_str!("fixtures/calls-v2.wir"));
    for (from, to) in [
        ("\"Whale\"", "\"UnknownConvention\""),
        ("\"Internal\"", "\"Weak\""),
        ("\"format_version\": 2", "\"format_version\": 1"),
        (
            "\"convention\": \"Whale\"",
            "\"convention\": \"Whale\", \"convention\": \"Whale\"",
        ),
        ("\"Direct\": \"emit\"", "\"Direct\": \"emit\", \"extra\": 0"),
    ] {
        assert!(decode(&source.replace(from, to)).is_err(), "{from}");
    }
}
