use ir::*;
use std::process::Command;
fn ty(depth: usize, mode: &str) -> Type {
    let mut t = Type::I32;
    for n in 0..depth {
        t = match mode {
            "fnptr" => Type::FnPtr(Box::new(FunctionSignature::whale(vec![t], Type::Void))),
            "mixed" => match n % 4 {
                0 => Type::ptr_to(t),
                1 => Type::Array(Box::new(t), 1),
                2 => Type::Struct(vec![t]),
                _ => Type::Tuple(vec![t]),
            },
            _ => Type::ptr_to(t),
        };
    }
    t
}
fn expr(depth: usize, right_nested: bool) -> ConstExpr {
    let mut e = ConstExpr::literal(Type::I32, ConstValue::I(0));
    for _ in 0..depth {
        let one = ConstExpr::literal(Type::I32, ConstValue::I(1));
        let (left, right) = if right_nested { (one, e) } else { (e, one) };
        e = ConstExpr {
            ty: Type::I32,
            kind: ConstExprKind::Binary {
                op: ConstBinaryOp::Add,
                left: Box::new(left),
                right: Box::new(right),
            },
        };
    }
    e
}
fn module() -> Module {
    Module::new("x86_64-whale-linux", DataLayout::default_64bit_le())
}
fn declaration(t: Type) -> FunctionDecl {
    FunctionDecl {
        id: FunctionId(0),
        name: "external".into(),
        signature: FunctionSignature::whale(vec![t], Type::Void),
        linkage: Linkage::External,
        link_name: Some("external".into()),
    }
}
#[test]
fn subprocess_helper() {
    let Ok(mode) = std::env::var("WHALE_LIMIT_TEST_MODE") else {
        return;
    };
    let mut m = module();
    match mode.as_str() {
        "owned" => {
            let mut b = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
            assert!(matches!(
                b.declare_function(
                    "deep",
                    FunctionSignature::whale(vec![ty(50_000, "fnptr")], Type::Void),
                    Linkage::External,
                    Some("deep".into())
                ),
                Err(CallError::ResourceLimit(_))
            ));
            return;
        }
        "null" => {
            let mut b = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
            let mut f = b.begin_function("main", vec![], Type::Void);
            assert!(matches!(
                f.null_function(FunctionSignature::whale(
                    vec![ty(50_000, "mixed")],
                    Type::Void
                )),
                Err(CallError::ResourceLimit(_))
            ));
            f.ret(None);
            f.finish();
            verify_module(&b.finish()).unwrap();
            return;
        }
        "left" | "right" => {
            let e = expr(50_000, mode == "right");
            assert!(matches!(
                e.evaluate(&|_| None),
                Err(ConstEvalError::ResourceLimit(_))
            ));
            // Move the tree into the module without cloning it.
            m.globals.push(Global {
                id: GlobalId(0),
                name: "deep".into(),
                ty: Type::I32,
                init: ConstValue::I(0),
                init_expr: e,
                align: 4,
            });
        }
        _ => m.declarations.push(declaration(ty(50_000, &mode))),
    }
    // The API borrows caller-owned trees. Exclude Rust's recursive Drop from
    // this verification regression, as in the issue's original reproducer.
    let m = Box::leak(Box::new(m));
    assert!(matches!(
        verify_module(m),
        Err(VerifyError::ResourceLimit(_))
    ));
}
#[test]
fn adversarial_api_inputs_return_errors_without_process_abort() {
    for mode in [
        "pointer", "mixed", "fnptr", "left", "right", "owned", "null",
    ] {
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "subprocess_helper", "--nocapture"])
            .env("WHALE_LIMIT_TEST_MODE", mode)
            .output()
            .unwrap();
        assert!(result.status.success(), "{mode}: {result:?}");
    }
}
#[test]
fn type_and_signature_depth_boundaries_and_configuration() {
    for mode in ["pointer", "mixed", "fnptr"] {
        let mut m = module();
        m.declarations.push(declaration(ty(3, mode)));
        let limits = IrLimits {
            max_type_depth: 3,
            ..IrLimits::default()
        };
        verify_module_with_limits(&m, limits).unwrap();
        assert!(matches!(
            verify_module_with_limits(
                &m,
                IrLimits {
                    max_type_depth: 2,
                    ..limits
                }
            ),
            Err(VerifyError::ResourceLimit(LimitError {
                resource: Resource::TypeDepth,
                ..
            }))
        ));
        validate_signature_with_limits(&m.declarations[0].signature, limits).unwrap();
    }
    assert!(matches!(
        verify_module_with_limits(
            &module(),
            IrLimits {
                max_const_depth: MAX_IR_NESTING + 1,
                ..IrLimits::default()
            }
        ),
        Err(VerifyError::ResourceLimit(LimitError {
            resource: Resource::Configuration,
            ..
        }))
    ));
    assert!(matches!(
        ConstExpr::literal(Type::I32, ConstValue::I(1)).evaluate_with_limits(
            &|_| None,
            IrLimits {
                max_nodes: 1,
                ..IrLimits::default()
            }
        ),
        Err(ConstEvalError::ResourceLimit(LimitError {
            resource: Resource::Nodes,
            ..
        }))
    ));
}
#[test]
fn constant_global_local_boundaries_preserve_wrapping_references_and_results() {
    for right in [false, true] {
        let expression = expr(3, right);
        let limits = IrLimits {
            max_const_depth: 3,
            ..IrLimits::default()
        };
        assert_eq!(
            expression.evaluate_with_limits(&|_| None, limits).unwrap(),
            ConstValue::I(3)
        );
        assert!(matches!(
            expression.evaluate_with_limits(
                &|_| None,
                IrLimits {
                    max_const_depth: 2,
                    ..limits
                }
            ),
            Err(ConstEvalError::ResourceLimit(_))
        ));
        let mut m = module();
        m.globals.push(Global {
            id: GlobalId(4),
            name: "sum".into(),
            ty: Type::I32,
            init: ConstValue::I(3),
            init_expr: expression.clone(),
            align: 4,
        });
        let mut b = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        {
            let mut f = b.begin_function("local", vec![], Type::Void);
            f.const_decl("sum", expression, ConstValue::I(3));
            f.ret(None);
            f.finish();
        }
        let local = b.finish();
        for input in [&m, &local] {
            verify_module_with_limits(input, limits).unwrap();
            assert!(matches!(
                verify_module_with_limits(
                    input,
                    IrLimits {
                        max_const_depth: 2,
                        ..limits
                    }
                ),
                Err(VerifyError::ResourceLimit(_))
            ));
        }
    }
    let wrapping = ConstExpr {
        ty: Type::I8,
        kind: ConstExprKind::Binary {
            op: ConstBinaryOp::Add,
            left: Box::new(ConstExpr::literal(Type::I8, ConstValue::I(127))),
            right: Box::new(ConstExpr {
                ty: Type::I8,
                kind: ConstExprKind::Reference(ConstRef::Global(GlobalId(0))),
            }),
        },
    };
    assert_eq!(
        wrapping
            .evaluate(&|_| Some((Type::I8, ConstValue::I(1))))
            .unwrap(),
        ConstValue::I(-128)
    );
}

#[test]
fn accepted_depth_ceiling_and_wide_work_budget() {
    for depth in [128, MAX_IR_NESTING] {
        let limits = IrLimits {
            max_type_depth: depth,
            max_const_depth: depth,
            ..IrLimits::default()
        };
        let mut m = module();
        m.declarations.push(declaration(ty(depth, "fnptr")));
        verify_module_with_limits(&m, limits).unwrap();
        let text = print_module(&m);
        let parsed = parse_module_with_limits(&text, limits).unwrap();
        assert_eq!(print_module(&parsed), text);
        let mut constant_module = module();
        constant_module.globals.push(Global {
            id: GlobalId(0),
            name: "deep".into(),
            ty: Type::I32,
            init: ConstValue::I(depth as i128),
            init_expr: expr(depth, false),
            align: 4,
        });
        let text = print_module(&constant_module);
        assert_eq!(
            print_module(&parse_module_with_limits(&text, limits).unwrap()),
            text
        );
        assert_eq!(
            expr(depth, false)
                .evaluate_with_limits(&|_| None, limits)
                .unwrap(),
            ConstValue::I(depth as i128)
        );
    }
    let mut m = module();
    m.declarations
        .push(declaration(Type::Struct(vec![Type::I32; 10_000])));
    assert!(matches!(
        verify_module_with_limits(
            &m,
            IrLimits {
                max_nodes: 20,
                ..IrLimits::default()
            }
        ),
        Err(VerifyError::ResourceLimit(LimitError {
            resource: Resource::Nodes,
            ..
        }))
    ));
    let expression = ConstExpr {
        ty: Type::I32,
        kind: ConstExprKind::Binary {
            op: ConstBinaryOp::Add,
            left: Box::new(ConstExpr {
                ty: Type::I32,
                kind: ConstExprKind::Reference(ConstRef::Global(GlobalId(0))),
            }),
            right: Box::new(ConstExpr {
                ty: Type::I32,
                kind: ConstExprKind::Reference(ConstRef::Global(GlobalId(1))),
            }),
        },
    };
    let order = std::cell::RefCell::new(Vec::new());
    assert_eq!(
        expression
            .evaluate(&|reference| {
                order.borrow_mut().push(reference);
                Some((Type::I32, ConstValue::I(1)))
            })
            .unwrap(),
        ConstValue::I(2)
    );
    assert_eq!(
        *order.borrow(),
        [ConstRef::Global(GlobalId(0)), ConstRef::Global(GlobalId(1))]
    );
    assert!(matches!(
        expression.evaluate(&|_| Some((ty(50_000, "pointer"), ConstValue::I(1)))),
        Err(ConstEvalError::ResourceLimit(_))
    ));
}
