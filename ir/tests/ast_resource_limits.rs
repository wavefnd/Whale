#![cfg(feature = "socket")]
use ir::lower_ast::{frontend::*, interchange, limits::inspect_program, *};

use ir::{CallingConvention, Linkage, Target};

fn int() -> Expr {
    Expr::Lit(Lit::Int { bits: 32, signed: true, value: "1".into() })
}

fn program(expr: Expr) -> Program {
    Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![Function {
            name: "main".into(),
            convention: CallingConvention::Whale,
            linkage: Linkage::Internal,
            link_name: None,
            parameters: vec![],
            return_type: TypeRef::Int { bits: 32, signed: true },
            body: vec![Stmt::Return(Some(expr))],
        }],
    }
}

fn binary(depth: usize) -> Expr {
    let mut expr = int();
    for _ in 0..depth {
        expr = Expr::Binary { left: Box::new(expr), op: BinOpRef::Add, right: Box::new(int()) };
    }
    expr
}

#[test]
fn direct_ast_guards_depth_nodes_strings_and_output_boundaries() {
    let target = Target::X86_64WhaleLinux;
    let p = program(binary(6));
    let limits = AstLimits { max_depth: 8, ..AstLimits::default() };
    inspect_program(&p, limits).unwrap();
    let m = lower_o0_with_limits(&p, target.name(), target.data_layout(), limits).unwrap();
    ir::verify_module(&m).unwrap();
    let lower = lower_o0_with_limits(
        &p,
        target.name(),
        target.data_layout(),
        AstLimits { max_depth: 7, ..limits },
    );
    assert!(matches!(lower, Err(LowerError::ResourceLimit(AstLimitError::Depth))));
    assert_eq!(
        inspect_program(&program(int()), AstLimits { max_nodes: 4, ..limits }),
        Err(AstLimitError::Nodes)
    );
    assert_eq!(inspect_program(&program(int()), AstLimits { max_nodes: 5, ..limits }), Ok(()));
    assert_eq!(
        inspect_program(&program(int()), AstLimits { max_string_bytes: 4, ..limits }),
        Err(AstLimitError::Strings)
    );
    assert_eq!(
        inspect_program(&program(int()), AstLimits { max_string_bytes: 5, ..limits }),
        Ok(())
    );
    let json = interchange::encode(program(int())).unwrap();
    let size = json.len();
    assert_eq!(
        interchange::encode_with_limits(
            program(int()),
            AstLimits { max_output_bytes: size, ..limits }
        )
        .unwrap(),
        json
    );
    assert!(interchange::encode_with_limits(
        program(int()),
        AstLimits { max_output_bytes: size - 1, ..limits }
    )
    .unwrap_err()
    .contains("output byte limit"));
    interchange::decode(&json).unwrap();
}

#[test]
fn raised_ast_depth_cannot_bypass_safe_ceiling_and_all_tree_kinds_are_checked() {
    let limits = AstLimits { max_depth: usize::MAX, ..AstLimits::default() };
    assert_eq!(inspect_program(&program(binary(MAX_AST_DEPTH)), limits), Err(AstLimitError::Depth));
    let mut p = program(int());
    let mut ty = TypeRef::Bool;
    for _ in 0..MAX_AST_DEPTH {
        ty = TypeRef::Ptr(Box::new(ty));
    }
    p.functions[0].return_type = ty;
    assert_eq!(inspect_program(&p, limits), Err(AstLimitError::Depth));
    let mut body = vec![Stmt::Return(Some(int()))];
    for _ in 0..MAX_AST_DEPTH {
        body = vec![Stmt::While { cond: Expr::Lit(Lit::Bool(true)), body }];
    }
    p = program(int());
    p.functions[0].body = body;
    assert_eq!(inspect_program(&p, limits), Err(AstLimitError::Depth));
    p = program(Expr::Call {
        callee: CalleeRef::Indirect(Box::new(Expr::NullFunction(SignatureRef {
            params: vec![TypeRef::Opaque("oversized".into())],
            ret: TypeRef::Void,
            convention: CallingConvention::Whale,
            variadic: false,
        }))),
        args: vec![],
    });
    assert_eq!(
        inspect_program(&p, AstLimits { max_string_bytes: 4, ..limits }),
        Err(AstLimitError::Strings)
    );
}

#[test]
fn deep_ast_lower_child() {
    if std::env::var_os("WHALE_DEEP_AST_CHILD").is_none() {
        return;
    }
    let p = Box::leak(Box::new(program(binary(20_000))));
    let target = Target::X86_64WhaleLinux;
    assert!(matches!(
        lower_o0(p, target.name(), target.data_layout()),
        Err(LowerError::ResourceLimit(AstLimitError::Depth))
    ));
}

#[test]
fn deep_ast_encode_child() {
    if std::env::var_os("WHALE_DEEP_AST_CHILD").is_none() {
        return;
    }
    let p = program(binary(20_000));
    assert!(interchange::encode(p).unwrap_err().contains("nesting limit"));
    let mut e = int();
    for _ in 0..20_000 {
        e = Expr::BitNot(Box::new(e));
    }
    assert!(interchange::encode(program(e)).unwrap_err().contains("nesting limit"));
    // Also cover rejection disposal for deep statements and type/signature trees.
    let mut p = program(int());
    let mut ty = TypeRef::Bool;
    for _ in 0..20_000 {
        ty = TypeRef::FnPtr(Box::new(SignatureRef {
            params: vec![],
            ret: ty,
            convention: CallingConvention::Whale,
            variadic: false,
        }));
    }
    p.functions[0].return_type = ty;
    assert!(interchange::encode(p).unwrap_err().contains("nesting limit"));
    let mut body = vec![Stmt::Return(Some(int()))];
    for _ in 0..20_000 {
        body = vec![Stmt::While { cond: Expr::Lit(Lit::Bool(true)), body }];
    }
    let mut p = program(int());
    p.functions[0].body = body;
    assert!(interchange::encode(p).unwrap_err().contains("nesting limit"));
}

#[cfg(target_os = "linux")]
fn child(name: &str) {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg("ulimit -c 0; ulimit -v 51200; exec \"$@\"")
        .arg("ast-test")
        .arg(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(name)
        // Locale archives can map over 200 MiB before this 50 MiB AST probe starts.
        .env("LC_ALL", "C")
        .env("MALLOC_ARENA_MAX", "1")
        .env("WHALE_DEEP_AST_CHILD", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
}

#[cfg(target_os = "linux")]
#[test]
fn direct_deep_lowering_is_safe_in_a_capped_child_process() {
    child("deep_ast_lower_child");
}

#[cfg(target_os = "linux")]
#[test]
fn owned_deep_encoding_and_disposal_are_safe_in_a_capped_child_process() {
    child("deep_ast_encode_child");
}

#[test]
fn accepted_depth_boundary_encodes_and_decodes_all_ast_tree_shapes() {
    let expr = binary(MAX_AST_DEPTH - 2); // Return is level 1; its expression starts at 2.
    let p = program(expr);
    inspect_program(&p, AstLimits::default()).unwrap();
    let encoded = interchange::encode(p).unwrap();
    interchange::decode(&encoded).unwrap();
    let mut body = vec![Stmt::Return(Some(int()))];
    for _ in 0..MAX_AST_DEPTH - 2 {
        body = vec![Stmt::While { cond: Expr::Lit(Lit::Bool(false)), body }];
    }
    let mut p = program(int());
    p.functions[0].body = body;
    inspect_program(&p, AstLimits::default()).unwrap();
    let encoded = interchange::encode(p).unwrap();
    let p = interchange::decode(&encoded).unwrap();
    let target = Target::X86_64WhaleLinux;
    let m = lower_o0(&p, target.name(), target.data_layout()).unwrap();
    ir::verify_module(&m).unwrap();
    let mut p = program(int());
    let mut t = TypeRef::Bool;
    for _ in 0..MAX_AST_DEPTH - 1 {
        t = TypeRef::Ptr(Box::new(t));
    }
    p.functions[0].return_type = t;
    inspect_program(&p, AstLimits::default()).unwrap();
    interchange::decode(&interchange::encode(p).unwrap()).unwrap();
    let mut p = program(int());
    let mut t = TypeRef::Bool;
    for _ in 0..(MAX_AST_DEPTH - 1) / 2 {
        t = TypeRef::FnPtr(Box::new(SignatureRef {
            params: vec![TypeRef::Bool],
            ret: t,
            convention: CallingConvention::Whale,
            variadic: false,
        }));
    }
    p.functions[0].return_type = t;
    inspect_program(&p, AstLimits::default()).unwrap();
    interchange::decode(&interchange::encode(p).unwrap()).unwrap();
}
