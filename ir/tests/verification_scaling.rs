use ir::*;

fn module(ret: Type, blocks: Vec<BasicBlock>, value_types: Vec<(ValueId, Type)>) -> Module {
    let mut m = Module::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    m.declarations.push(FunctionDecl {
        id: FunctionId(0),
        name: "scale".into(),
        signature: FunctionSignature::whale(vec![], ret.clone()),
        linkage: Linkage::Internal,
        link_name: None,
    });
    m.functions.push(Function {
        id: FunctionId(0),
        name: "scale".into(),
        params: vec![],
        ret_ty: ret,
        entry: BlockId(0),
        blocks,
        value_types,
    });
    m
}

fn moves(count: u32) -> Module {
    let id = |n: u32| ValueId(n * 17 + 7);
    let mut instructions =
        vec![Instruction::Const { dst: id(0), ty: Type::I32, value: ConstValue::I(42) }];
    for n in 1..count {
        instructions.push(Instruction::Mov { dst: id(n), ty: Type::I32, src: id(n - 1) });
    }
    let mut types: Vec<_> = (0..count).map(|n| (id(n), Type::I32)).collect();
    types.reverse();
    module(
        Type::I32,
        vec![BasicBlock {
            id: BlockId(0),
            name: "entry".into(),
            instructions,
            terminator: Some(Terminator::Ret { ty: Type::I32, value: Some(id(count - 1)) }),
        }],
        types,
    )
}

fn chain(count: u32) -> Module {
    let mut blocks = Vec::new();
    for n in 0..count {
        blocks.push(BasicBlock {
            id: BlockId(n),
            name: format!("b{n}"),
            instructions: if n == 0 {
                vec![Instruction::Const {
                    dst: ValueId(7),
                    ty: Type::Bool,
                    value: ConstValue::Bool(true),
                }]
            } else {
                vec![]
            },
            terminator: Some(if n + 1 == count {
                Terminator::Ret { ty: Type::Void, value: None }
            } else {
                Terminator::CBr {
                    cond: ValueId(7),
                    then_bb: BlockId(n + 1),
                    else_bb: BlockId(n + 1),
                }
            }),
        });
    }
    blocks.reverse();
    module(Type::Void, blocks, vec![(ValueId(7), Type::Bool)])
}

fn independent_calls(count: u32) -> Module {
    let mut m = Module::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let signature = FunctionSignature::whale(vec![], Type::Void);
    for n in 0..count {
        let id = FunctionId(n);
        let name = format!("function_{n}");
        m.declarations.push(FunctionDecl {
            id,
            name: name.clone(),
            signature: signature.clone(),
            linkage: Linkage::Internal,
            link_name: None,
        });
        m.functions.push(Function {
            id,
            name,
            params: vec![],
            ret_ty: Type::Void,
            entry: BlockId(0),
            blocks: vec![BasicBlock {
                id: BlockId(0),
                name: "entry".into(),
                instructions: vec![
                    Instruction::FunctionAddr {
                        dst: ValueId(0),
                        function: FunctionId(0),
                        signature: signature.clone(),
                    },
                    Instruction::Call {
                        convention: CallingConvention::Whale,
                        dst: None,
                        ret_ty: Type::Void,
                        callee: Callee::Direct(FunctionId(0)),
                        args: vec![],
                    },
                ],
                terminator: Some(Terminator::Ret { ty: Type::Void, value: None }),
            }],
            value_types: vec![(ValueId(0), Type::FnPtr(Box::new(signature.clone())))],
        });
    }
    m
}

#[test]
fn declaration_and_direct_call_lookup_work_scales_with_function_count() {
    for count in [100, 200, 400, 800] {
        let m = independent_calls(count);
        let stats = verify_module_with_stats(&m, IrLimits::default()).unwrap();
        assert_eq!(stats.declaration_work, 6 * count as usize, "{stats:?}");
    }
}

#[test]
fn lookup_and_dominance_work_grow_linearly_without_reordering_ir() {
    for constructor in [moves, chain] {
        let mut stats = Vec::new();
        for size in [1000, 2000, 4000] {
            let m = constructor(size);
            let before = print_module(&m);
            let s = verify_module_with_stats(&m, IrLimits::default()).unwrap();
            assert_eq!(print_module(&m), before);
            assert!(s.value_type_lookups <= 5 * size as usize, "{s:?}");
            assert!(s.analysis_work <= 20 * size as usize, "{s:?}");
            stats.push(s);
        }
        assert!(stats[2].analysis_work <= 3 * stats[1].analysis_work);
    }
}

#[test]
fn cfg_work_budget_is_shared_and_returns_a_resource_error() {
    let m = chain(100);
    assert!(matches!(
        verify_module_with_limits(&m, IrLimits { max_analysis_work: 100, ..IrLimits::default() }),
        Err(VerifyError::ResourceLimit(LimitError {
            resource: Resource::AnalysisWork,
            limit: 100
        }))
    ));
    let mut m = moves(1);
    for n in 1..20 {
        let mut f = m.functions[0].clone();
        f.id = FunctionId(n);
        f.name = format!("f{n}");
        let mut d = m.declarations[0].clone();
        d.id = f.id;
        d.name = f.name.clone();
        m.functions.push(f);
        m.declarations.push(d);
    }
    assert!(matches!(
        verify_module_with_limits(&m, IrLimits { max_analysis_work: 30, ..IrLimits::default() }),
        Err(VerifyError::ResourceLimit(LimitError { resource: Resource::AnalysisWork, .. }))
    ));
}

fn constants(globals: u32, functions: u32) -> Module {
    let mut m = moves(1);
    m.functions.clear();
    m.declarations.clear();
    for n in 0..globals {
        m.globals.push(Global {
            id: GlobalId(n),
            name: format!("g{n}"),
            ty: Type::I32,
            init: ConstValue::I(42),
            init_expr: ConstExpr::literal(Type::I32, ConstValue::I(42)),
            align: 4,
        });
    }
    for n in 0..functions {
        let mut single = moves(1);
        let f = &mut single.functions[0];
        f.id = FunctionId(n);
        f.name = format!("f{n}");
        f.blocks[0].instructions[0] = Instruction::ConstDecl {
            dst: ValueId(7),
            name: "local".into(),
            expression: ConstExpr {
                ty: Type::I32,
                kind: ConstExprKind::Reference(ConstRef::Global(GlobalId(n % globals))),
            },
            value: ConstValue::I(42),
        };
        let d = &mut single.declarations[0];
        d.id = f.id;
        d.name = f.name.clone();
        m.functions.extend(single.functions);
        m.declarations.extend(single.declarations);
    }
    m
}

#[test]
fn many_functions_share_global_values_without_losing_constant_identity() {
    let m = constants(256, 256);
    let before = print_module(&m);
    verify_module(&m).unwrap();
    assert_eq!(print_module(&m), before);
}

#[test]
#[ignore = "repeatable scaling benchmark; run with --ignored --nocapture"]
fn benchmark_verification_scaling() {
    for (name, constructor) in [("moves", moves as fn(u32) -> Module), ("cfg", chain)] {
        for size in [1000, 2000, 4000, 8000] {
            let m = constructor(size);
            let mut durations = Vec::new();
            let mut stats = VerificationStats::default();
            for _ in 0..3 {
                let start = std::time::Instant::now();
                stats = verify_module_with_stats(&m, IrLimits::default()).unwrap();
                durations.push(start.elapsed());
            }
            durations.sort();
            println!(
                "{name} nodes={size} median_us={} lookups={} dominance={} work={}",
                durations[1].as_micros(),
                stats.value_type_lookups,
                stats.dominance_queries,
                stats.analysis_work
            );
        }
    }
    for size in [100, 250, 500, 1000] {
        let m = constants(size, size);
        let start = std::time::Instant::now();
        verify_module(&m).unwrap();
        println!(
            "constant overlay globals={size} functions={size} elapsed_us={}",
            start.elapsed().as_micros()
        );
    }
}
