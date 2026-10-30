use ir::*;

fn target(index: usize) -> SourceTarget {
    SourceTarget::Instruction { function: FunctionId(0), block: BlockId(0), index }
}

fn module() -> Module {
    let mut mb = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = mb.begin_function("main", vec![], Type::I32);
    let ptr = f.alloca(Type::I32, 4);
    let value = f.const_int(Type::I32, 7);
    f.store(Type::I32, value, ptr, 4);
    f.const_decl("unused", ConstExpr::literal(Type::I32, ConstValue::I(99)), ConstValue::I(99));
    f.ret(Some(value));
    f.finish();
    let mut m = mb.finish();
    m.sources.files.push(SourceFile { id: 900, name: "경로/main.wave".into() });
    m.sources.originals = vec![
        OriginalNode {
            id: 8000,
            span: Some(SourceSpan { file: 900, line: 3, column: 4, end_line: 3, end_column: 9 }),
        },
        OriginalNode { id: 12, span: None },
    ];
    m.sources.mappings = vec![
        SourceMapping { target: target(2), original: 8000 },
        SourceMapping { target: target(3), original: 12 },
        SourceMapping {
            target: SourceTarget::Terminator { function: FunctionId(0), block: BlockId(0) },
            original: 8000,
        },
    ];
    m
}

#[test]
fn operation_identity_roundtrip_includes_store_unused_constant_and_terminator() {
    let m = module();
    verify_module(&m).unwrap();
    let printed = print_module(&m);
    let decoded = parse_module(&printed).unwrap();
    assert_eq!(m.sources, decoded.sources);
    assert_eq!(print_module(&decoded), printed);
    assert_eq!(
        decoded.sources.location(target(2)).unwrap().to_string(),
        "경로/main.wave:3:4 original @n8000"
    );
    assert_eq!(decoded.sources.location(target(3)).unwrap().to_string(), "original @n12");
    assert!(decoded.sources.location(target(0)).is_none());
    assert!(interpret(&decoded, FunctionId(0), &[]).is_ok());
}

#[test]
fn original_location_escapes_controls_without_changing_source_metadata() {
    let span = SourceSpan { file: 0, line: 3, column: 4, end_line: 3, end_column: 9 };
    for control in ['\n', '\r', '\t', '\x1b'] {
        let name = format!("경로/{control}file.wave");
        let location = OriginalLocation { original: 7, source: Some((name.clone(), span.clone())) };
        let rendered = location.to_string();
        assert!(!rendered.contains(control), "{rendered:?}");
        assert!(rendered.contains(&control.escape_debug().to_string()));
        assert!(rendered.contains(":3:4 original @n7"));
        assert_eq!(location.source.as_ref().unwrap().0, name);
    }
    let location =
        OriginalLocation { original: 7, source: Some(("경로/파일.wave".into(), span)) };
    assert_eq!(location.to_string(), "경로/파일.wave:3:4 original @n7");
}

#[test]
fn required_operation_splitting_preserves_original_ids() {
    let mut m = module();
    m.sources.copy_mapping(target(2), target(1)).unwrap();
    verify_module(&m).unwrap();
    assert_eq!(m.sources.original(target(1)), m.sources.original(target(2)));
    assert!(m.sources.copy_mapping(target(2), target(1)).is_err());
    assert!(m.sources.copy_mapping(target(99), target(0)).is_err());
}

#[test]
fn malformed_metadata_and_old_version_metadata_are_rejected() {
    for defect in 0..7 {
        let mut m = module();
        match defect {
            0 => m.sources.files.push(m.sources.files[0].clone()),
            1 => m.sources.originals.push(m.sources.originals[0].clone()),
            2 => m.sources.originals[0].span.as_mut().unwrap().file = 0,
            3 => m.sources.originals[0].span.as_mut().unwrap().end_line = 1,
            4 => m.sources.mappings[0].target = target(99),
            5 => m.sources.mappings[0].original = 999,
            _ => m.sources.mappings.push(m.sources.mappings[0].clone()),
        }
        assert!(
            matches!(verify_module(&m), Err(VerifyError::SourceMetadata(_))),
            "defect {defect}"
        );
        assert!(parse_module(&print_module(&m)).is_err());
    }
    for version in 3..7 {
        assert!(parse_module(&print_module(&module()).replace(
            &format!("format_version {}", IR_FORMAT_VERSION),
            &format!("format_version {version}")
        ))
        .is_err());
    }
}

#[test]
fn direct_source_metadata_uses_ir_node_and_byte_budgets() {
    let mut m = module();
    m.sources.files[0].name = "x".repeat(1025);
    assert!(matches!(
        verify_module_with_limits(&m, IrLimits { max_input_bytes: 1024, ..Default::default() }),
        Err(VerifyError::ResourceLimit(_))
    ));
    let mut m = module();
    m.sources.originals.resize(200, OriginalNode { id: 0, span: None });
    assert!(matches!(
        verify_module_with_limits(&m, IrLimits { max_nodes: 100, ..Default::default() }),
        Err(VerifyError::ResourceLimit(_))
    ));
}

#[cfg(feature = "socket")]
mod frontend {
    use super::*;

    use ir::lower_ast::{frontend as ast, interchange};

    #[test]
    fn located_lower_error_escapes_filename_but_retains_annotation() {
        use ir::lower_ast::LowerError;
        for control in ['\n', '\r', '\t', '\x1b'] {
            let name = format!("경로/{control}file.wave");
            let annotation = SourceAnnotation {
                id: 7,
                source: Some(FrontendSource {
                    file: name.clone(),
                    line: 3,
                    column: 4,
                    end_line: 3,
                    end_column: 9,
                }),
            };
            let error = LowerError::UnknownVariable("missing".into()).at(&annotation);
            let rendered = error.to_string();
            assert!(!rendered.contains(control), "{rendered:?}");
            assert!(rendered.contains(&control.escape_debug().to_string()));
            assert!(rendered.contains(":3:4: original @n7"));
            assert_eq!(error.annotation(), Some(&annotation));
        }
    }

    fn annotation(id: u64) -> SourceAnnotation {
        SourceAnnotation {
            id,
            source: Some(FrontendSource {
                file: "input.wave".into(),
                line: id as u32,
                column: 2,
                end_line: id as u32,
                end_column: 8,
            }),
        }
    }

    fn lit(n: i32) -> ast::Expr {
        ast::Expr::Lit(ast::Lit::Int { bits: 32, signed: true, value: n.to_string() })
    }

    fn expr(id: u64, value: ast::Expr) -> ast::Expr {
        ast::Expr::Located { annotation: annotation(id), expression: Box::new(value) }
    }

    fn stmt(id: u64, value: ast::Stmt) -> ast::Stmt {
        ast::Stmt::Located { annotation: annotation(id), statement: Box::new(value) }
    }

    fn program(body: Vec<ast::Stmt>) -> ast::Program {
        ast::Program {
            declarations: vec![],
            globals: vec![],
            functions: vec![ast::Function {
                convention: CallingConvention::Whale,
                linkage: Linkage::Internal,
                link_name: None,
                name: "main".into(),
                parameters: vec![],
                return_type: ast::TypeRef::Int { bits: 32, signed: true },
                body,
            }],
        }
    }

    fn lower(p: &ast::Program) -> Result<Module, LowerError> {
        lower_o0(p, "x86_64-whale-linux", DataLayout::default_64bit_le())
    }

    #[test]
    fn lowering_errors_use_the_innermost_original_expression_and_statement() {
        let p = program(vec![stmt(
            1,
            ast::Stmt::Return(Some(expr(
                2,
                ast::Expr::Binary {
                    left: Box::new(lit(1)),
                    op: ast::BinOpRef::Add,
                    right: Box::new(expr(3, ast::Expr::Var("missing".into()))),
                },
            ))),
        )]);
        let e = lower(&p).unwrap_err();
        assert_eq!(e.annotation().unwrap().id, 3);
        assert!(matches!(e.root_cause(),LowerError::UnknownVariable(name) if name=="missing"));
        assert!(e.to_string().starts_with("input.wave:3:2: original @n3"));
        for bad in [ast::Stmt::Break, ast::Stmt::Return(Some(ast::Expr::Lit(ast::Lit::Bool(true))))]
        {
            let e = lower(&program(vec![stmt(4, bad)])).unwrap_err();
            assert_eq!(e.annotation().unwrap().id, 4);
        }
        let p = program(vec![ast::Stmt::Return(Some(ast::Expr::Var("missing".into())))]);
        assert!(lower(&p).unwrap_err().annotation().is_none());
    }

    #[test]
    fn hoisted_allocas_preserve_old_store_mappings_and_child_origins() {
        let t = ast::TypeRef::Int { bits: 32, signed: true };
        let p = program(vec![
            stmt(
                10,
                ast::Stmt::VarDecl {
                    name: "a".into(),
                    ty: t.clone(),
                    init: Some(expr(11, lit(1))),
                },
            ),
            ast::Stmt::VarDecl { name: "b".into(), ty: t.clone(), init: Some(lit(2)) },
            stmt(12, ast::Stmt::ConstDecl { name: "unused".into(), ty: t, init: lit(99) }),
            stmt(13, ast::Stmt::Return(Some(expr(14, ast::Expr::Var("a".into()))))),
        ]);
        let m = lower(&p).unwrap();
        verify_module(&m).unwrap();
        let f = &m.functions[0];
        let b = &f.blocks[0];
        let store =
            b.instructions.iter().position(|i| matches!(i, Instruction::Store { .. })).unwrap();
        let load =
            b.instructions.iter().position(|i| matches!(i, Instruction::Load { .. })).unwrap();
        assert_eq!(m.sources.original(target(store)).unwrap().id, 10);
        assert_eq!(m.sources.original(target(load)).unwrap().id, 14);
        let unused =
            b.instructions.iter().position(|i| matches!(i, Instruction::ConstDecl { .. })).unwrap();
        assert_eq!(m.sources.original(target(unused)).unwrap().id, 12);
        assert!(matches!(b.instructions[0], Instruction::Alloca { .. }));
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(1)));
        assert_eq!(parse_module(&print_module(&m)).unwrap().sources, m.sources);
    }

    #[test]
    fn annotated_ast_is_versioned_bounded_and_rejects_duplicate_identities() {
        let p = program(vec![stmt(1, ast::Stmt::Return(Some(expr(2, lit(5)))))]);
        let json = interchange::encode(p.clone()).unwrap();
        assert!(json.contains("\"format_version\": 5"));
        lower(&interchange::decode(&json).unwrap()).unwrap();
        for old in [2, 3] {
            assert!(interchange::decode(
                &json.replace("\"format_version\": 5", &format!("\"format_version\": {old}"))
            )
            .unwrap_err()
            .contains("require AST format 4"));
        }
        let duplicate = program(vec![stmt(1, ast::Stmt::Return(Some(expr(1, lit(5)))))]);
        assert!(matches!(lower(&duplicate), Err(LowerError::ResourceLimit(AstLimitError::Source))));
        assert!(matches!(
            lower_o0_with_limits(
                &p,
                "x86_64-whale-linux",
                DataLayout::default_64bit_le(),
                AstLimits { max_source_work: 0, ..Default::default() }
            ),
            Err(LowerError::ResourceLimit(AstLimitError::SourceWork))
        ));
    }

    #[test]
    fn constant_initializer_annotations_survive_declarations_without_runtime_uses() {
        let t = ast::TypeRef::Int { bits: 32, signed: true };
        let mut p = program(vec![
            ast::Stmt::ConstDecl { name: "unused".into(), ty: t.clone(), init: expr(9, lit(8)) },
            ast::Stmt::Return(Some(lit(1))),
        ]);
        p.globals.push(ast::GlobalConst { name: "global".into(), ty: t, init: expr(10, lit(7)) });
        let m = lower(&p).unwrap();
        verify_module(&m).unwrap();
        assert_eq!(m.sources.original(target(0)).unwrap().id, 9);
        assert_eq!(m.sources.original(SourceTarget::Global(GlobalId(0))).unwrap().id, 10);
        assert_eq!(parse_module(&print_module(&m)).unwrap().sources, m.sources);
        let p =
            program(vec![ast::Stmt::Return(Some(expr(11, ast::Expr::Lit(ast::Lit::Bool(true)))))]);
        assert_eq!(lower(&p).unwrap_err().annotation().unwrap().id, 11);
    }

    #[test]
    fn located_discarded_void_calls_preserve_execution_and_source_identity() {
        let call = ast::Expr::Call { callee: ast::CalleeRef::Direct("side".into()), args: vec![] };
        let mut p = program(vec![
            ast::Stmt::ExprStmt(expr(12, expr(13, call))),
            ast::Stmt::Return(Some(lit(1))),
        ]);
        p.functions.push(ast::Function {
            convention: CallingConvention::Whale,
            linkage: Linkage::Internal,
            link_name: None,
            name: "side".into(),
            parameters: vec![],
            return_type: ast::TypeRef::Void,
            body: vec![ast::Stmt::Return(None)],
        });
        let m = lower(&p).unwrap();
        verify_module(&m).unwrap();
        assert_eq!(m.sources.original(target(0)).unwrap().id, 13);
        assert!(m.sources.originals.iter().any(|o| o.id == 12));
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(1)));
    }

    fn binary(left: ast::Expr, op: ast::BinOpRef, right: ast::Expr) -> ast::Expr {
        ast::Expr::Binary { left: Box::new(left), op, right: Box::new(right) }
    }

    fn constant_program(init: ast::Expr, ty: ast::TypeRef, global: bool) -> ast::Program {
        let mut p = program(vec![ast::Stmt::Return(Some(lit(1)))]);
        if global {
            p.globals.push(ast::GlobalConst { name: "unused".into(), ty, init });
        } else {
            p.functions[0].body.insert(0, ast::Stmt::ConstDecl { name: "unused".into(), ty, init });
        }
        p
    }

    fn i32_type() -> ast::TypeRef {
        ast::TypeRef::Int { bits: 32, signed: true }
    }

    #[test]
    fn every_nested_constant_annotation_survives_without_inventing_operation_targets() {
        for global in [false, true] {
            let init =
                expr(10, binary(expr(11, lit(1)), ast::BinOpRef::Add, expr(12, expr(13, lit(2)))));
            let m = lower(&constant_program(init, i32_type(), global)).unwrap();
            verify_module(&m).unwrap();
            assert_eq!(
                m.sources.originals.iter().map(|n| n.id).collect::<Vec<_>>(),
                [10, 11, 12, 13]
            );
            assert_eq!(m.sources.mappings.len(), 1);
            let mapped = if global { SourceTarget::Global(GlobalId(0)) } else { target(0) };
            assert_eq!(m.sources.original(mapped).unwrap().id, 10);
            assert_eq!(parse_module(&print_module(&m)).unwrap().sources, m.sources);
            assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::I(1)));

            let init = binary(lit(1), ast::BinOpRef::Add, expr(14, lit(2)));
            let m = lower(&constant_program(init, i32_type(), global)).unwrap();
            verify_module(&m).unwrap();
            assert_eq!(m.sources.originals.iter().map(|n| n.id).collect::<Vec<_>>(), [14]);
            assert!(m.sources.mappings.is_empty());
        }
    }

    #[test]
    fn constant_evaluation_uses_the_first_actual_failing_subexpression_location() {
        for global in [false, true] {
            for (first, expected) in [
                (ast::BinOpRef::Div, ConstEvalError::DivisionByZero),
                (ast::BinOpRef::Rem, ConstEvalError::RemainderByZero),
            ] {
                let init = expr(
                    10,
                    binary(
                        expr(11, expr(12, binary(lit(1), first, lit(0)))),
                        ast::BinOpRef::Add,
                        expr(13, binary(lit(1), ast::BinOpRef::Div, lit(0))),
                    ),
                );
                let e = lower(&constant_program(init, i32_type(), global)).unwrap_err();
                assert_eq!(e.annotation().unwrap().id, 12);
                assert!(
                    matches!(e.root_cause(), LowerError::ConstantEvaluation(error) if error == &expected)
                );
                assert!(e.to_string().starts_with("input.wave:12:2: original @n12"));
            }
            let init = binary(
                lit(1),
                ast::BinOpRef::Add,
                expr(15, binary(lit(1), ast::BinOpRef::Div, lit(0))),
            );
            assert_eq!(
                lower(&constant_program(init, i32_type(), global))
                    .unwrap_err()
                    .annotation()
                    .unwrap()
                    .id,
                15
            );
        }
    }

    #[test]
    fn lazy_constants_keep_skipped_origins_but_validate_skipped_static_errors() {
        for global in [false, true] {
            for (op, left) in [(ast::BinOpRef::LogicalAnd, false), (ast::BinOpRef::LogicalOr, true)]
            {
                let rhs = expr(
                    20,
                    ast::Expr::Cmp {
                        left: Box::new(expr(21, binary(lit(1), ast::BinOpRef::Div, lit(0)))),
                        op: ast::CmpOpRef::Eq,
                        right: Box::new(lit(0)),
                    },
                );
                let init = binary(ast::Expr::Lit(ast::Lit::Bool(left)), op, rhs);
                let m = lower(&constant_program(init, ast::TypeRef::Bool, global)).unwrap();
                verify_module(&m).unwrap();
                assert_eq!(m.sources.originals.iter().map(|n| n.id).collect::<Vec<_>>(), [20, 21]);
                let text = print_module(&m);
                assert!(text.contains("div"));
                assert_eq!(parse_module(&text).unwrap().sources, m.sources);
            }
            let invalid_numeric = expr(
                22,
                ast::Expr::Lit(ast::Lit::Int {
                    bits: 32,
                    signed: true,
                    value: "not a number".into(),
                }),
            );
            let rhs = ast::Expr::Cmp {
                left: Box::new(invalid_numeric),
                op: ast::CmpOpRef::Eq,
                right: Box::new(lit(0)),
            };
            let init =
                binary(ast::Expr::Lit(ast::Lit::Bool(false)), ast::BinOpRef::LogicalAnd, rhs);
            let e = lower(&constant_program(init, ast::TypeRef::Bool, global)).unwrap_err();
            assert_eq!(e.annotation().unwrap().id, 22);
            assert!(matches!(e.root_cause(), LowerError::NumericLiteral(_)));

            // The typed evaluator validates operator categories throughout the
            // preserved tree before lazy arithmetic begins.
            let rhs = expr(
                23,
                ast::Expr::Cmp {
                    left: Box::new(ast::Expr::Lit(ast::Lit::Bool(true))),
                    op: ast::CmpOpRef::Lt,
                    right: Box::new(ast::Expr::Lit(ast::Lit::Bool(false))),
                },
            );
            let init =
                binary(ast::Expr::Lit(ast::Lit::Bool(false)), ast::BinOpRef::LogicalAnd, rhs);
            let e = lower(&constant_program(init, ast::TypeRef::Bool, global)).unwrap_err();
            assert_eq!(e.annotation().unwrap().id, 23);
            assert!(matches!(
                e.root_cause(),
                LowerError::ConstantEvaluation(ConstEvalError::UnsupportedOperation)
            ));
        }
    }

    fn lower_budget(p: &ast::Program, max_source_work: usize) -> Result<Module, LowerError> {
        lower_o0_with_limits(
            p,
            "x86_64-whale-linux",
            DataLayout::default_64bit_le(),
            AstLimits { max_source_work, ..Default::default() },
        )
    }

    #[test]
    fn identity_only_globals_cost_work_and_unannotated_functions_do_not_scan_their_mappings() {
        let mut p = program(vec![ast::Stmt::Return(Some(lit(1)))]);
        for id in 0..2 {
            p.globals.push(ast::GlobalConst {
                name: format!("g{id}"),
                ty: i32_type(),
                init: ast::Expr::Located {
                    annotation: SourceAnnotation { id, source: None },
                    expression: Box::new(lit(1)),
                },
            });
        }
        assert!(matches!(
            lower_budget(&p, 0),
            Err(LowerError::ResourceLimit(AstLimitError::SourceWork))
        ));
        let minimum = (1..256).find(|&work| lower_budget(&p, work).is_ok()).unwrap();
        assert!(matches!(
            lower_budget(&p, minimum - 1),
            Err(LowerError::ResourceLimit(AstLimitError::SourceWork))
        ));
        for id in 1..100 {
            let mut f = p.functions[0].clone();
            f.name = format!("f{id}");
            f.parameters.push(ast::Parameter { name: "arg".into(), ty: i32_type() });
            p.functions.push(f);
        }
        // Only the two annotated globals consume source work. Hoisted slots in
        // the ninety-nine unannotated functions never scan their global maps.
        let m = lower_budget(&p, minimum).unwrap();
        verify_module(&m).unwrap();
        assert_eq!(m.sources.mappings.len(), 2);
    }

    #[test]
    fn final_hoisted_alloca_mappings_are_bounded_and_use_final_physical_indices() {
        let declaration = |name: &str| ast::Stmt::VarDecl {
            name: name.into(),
            ty: i32_type(),
            init: Some(lit(1)),
        };
        let base = program(vec![stmt(10, declaration("a")), ast::Stmt::Return(Some(lit(1)))]);
        let mut late = base;
        late.functions[0].body.insert(1, declaration("b"));
        let minimum = (1..256).find(|&work| lower_budget(&late, work).is_ok()).unwrap();
        assert!(matches!(
            lower_budget(&late, minimum - 1),
            Err(LowerError::ResourceLimit(AstLimitError::SourceWork))
        ));
        let m = lower_budget(&late, minimum).unwrap();
        verify_module(&m).unwrap();
        assert_eq!(m.sources.original(target(3)).unwrap().id, 10);
        assert_eq!(m.sources.original(target(2)).unwrap().id, 10);
        assert!(m.sources.original(target(1)).is_none());
    }
}
