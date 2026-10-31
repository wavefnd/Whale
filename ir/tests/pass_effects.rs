use ir::*;

use ir::pass::*;

fn module(trap: bool) -> Module {
    let mut b = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = b.begin_function("test", vec![], Type::U32);
    let ptr = f.alloca(Type::U32, 4);
    let initial = f.const_uint(Type::U32, 42);
    f.volatile_store(Type::U32, initial, ptr, 4);
    let dead = f.const_uint(Type::U32, 7);
    assert_eq!(dead, ValueId(2));
    let _a = f.volatile_load(Type::U32, ptr, 4);
    let result = f.volatile_load(Type::U32, ptr, 4);
    let condition = f.const_bool(trap);
    f.trap_if(condition, "reachable trap");
    f.ret(Some(result));
    f.finish();
    b.finish()
}

/// Test-only transform: O0 defaults never invoke it. The candidate is known
/// unused; the shared effect model must also permit its deletion.
struct RemoveCandidate(ValueId);

impl Pass for RemoveCandidate {
    fn name(&self) -> &str {
        "remove-unused-pure-candidate"
    }

    fn run(&mut self, m: &mut Module) -> Result<PassChange, String> {
        let mut changed = false;
        for f in &mut m.functions {
            for b in &mut f.blocks {
                b.instructions.retain(|i| {
                    let candidate = match i {
                        Instruction::Const { dst, .. } | Instruction::VolatileLoad { dst, .. } => {
                            *dst == self.0
                        },
                        _ => false,
                    };
                    let remove = candidate && i.effects().permits_unused_removal();
                    changed |= remove;
                    !remove
                });
            }
            if changed {
                f.value_types.retain(|(id, _)| *id != self.0);
            }
        }
        Ok(PassChange {
            changed,
            invalidates: if changed { Invalidates::ALL } else { Invalidates::default() },
        })
    }
}

#[test]
fn effect_aware_pass_preserves_volatile_count_order_return_values_and_traps() {
    for trap in [false, true] {
        let mut m = module(trap);
        let before = m.clone();
        let mut state = AnalysisState::default();
        let cfg = AnalysisCache::new(AnalysisKind::Cfg, &state, "old cfg");
        assert!(cfg.get(&AnalysisState::default()).is_none());
        let values = AnalysisCache::new(AnalysisKind::Values, &state, 42);
        let effects = AnalysisCache::new(AnalysisKind::Effects, &state, vec![1, 2]);
        assert!(run_pass(&mut m, &mut RemoveCandidate(ValueId(2)), &mut state).unwrap().changed);
        assert!(
            cfg.get(&state).is_none()
                && values.get(&state).is_none()
                && effects.get(&state).is_none()
        );
        let volatile = |m: &Module| {
            m.functions[0].blocks[0]
                .instructions
                .iter()
                .filter(|i| i.effects().volatile)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(volatile(&m), volatile(&before));
        assert_eq!(volatile(&m).len(), 3);
        let stable = AnalysisCache::new(AnalysisKind::Effects, &state, 3);
        assert!(!run_pass(&mut m, &mut RemoveCandidate(ValueId(3)), &mut state).unwrap().changed);
        assert_eq!(stable.get(&state), Some(&3));
        let outcomes = [interpret(&before, FunctionId(0), &[]), interpret(&m, FunctionId(0), &[])];
        if trap {
            for outcome in outcomes {
                assert!(
                    matches!(outcome,Err(InterpreterError::Trap(t)) if t.reason==TrapReason::Explicit("reachable trap".into()))
                );
            }
        } else {
            for outcome in outcomes {
                assert_eq!(outcome.unwrap().value, Some(ConstValue::U(42)));
            }
        }
        let text = print_module(&m);
        assert_eq!(print_module(&parse_module(&text).unwrap()), text);
        assert!(parse_module(&text.replace("format_version 8", "format_version 7")).is_err());
    }
}

struct InvalidPass(u8);

impl Pass for InvalidPass {
    fn name(&self) -> &str {
        "invalid-test-pass"
    }

    fn run(&mut self, m: &mut Module) -> Result<PassChange, String> {
        match self.0 {
            0 => m.functions[0].blocks[0].terminator = None,
            1 => {
                m.functions[0].name.push_str("-edited");
                return Err("transform diagnostic".into());
            },
            _ => m.functions[0].blocks[0].name.push_str("-edited"),
        }
        Ok(PassChange::default())
    }
}

#[test]
fn invalid_transform_reports_context_and_rolls_back_module_and_analysis_state() {
    for (case, stage) in
        [(0, PassStage::After), (1, PassStage::Transform), (2, PassStage::Contract)]
    {
        let mut m = module(false);
        let original = m.clone();
        let mut state = AnalysisState::default();
        let cache = AnalysisCache::new(AnalysisKind::Values, &state, 42);
        let error = run_pass(&mut m, &mut InvalidPass(case), &mut state).unwrap_err();
        assert_eq!(error.stage, stage);
        assert_eq!(error.pass, "invalid-test-pass");
        assert_eq!(error.target, original.target);
        assert_eq!(m, original);
        assert_eq!(cache.get(&state), Some(&42));
        assert!(error.to_string().contains("invalid-test-pass"));
    }
    let mut m = module(false);
    m.functions[0].blocks[0].terminator = None;
    let before = m.clone();
    let error = run_pass(&mut m, &mut InvalidPass(1), &mut AnalysisState::default()).unwrap_err();
    assert_eq!(error.stage, PassStage::Before);
    assert_eq!(m, before);
}

#[test]
fn ordinary_checks_and_trapping_arithmetic_cannot_be_discarded_as_unused() {
    let m = module(true);
    for i in &m.functions[0].blocks[0].instructions {
        if matches!(
            i,
            Instruction::Alloca { .. }
                | Instruction::VolatileLoad { .. }
                | Instruction::VolatileStore { .. }
                | Instruction::TrapIf { .. }
        ) {
            assert!(!i.effects().permits_unused_removal());
        }
    }
    for op in [BinOp::UDiv, BinOp::SDiv, BinOp::URem, BinOp::SRem] {
        let i = Instruction::Bin {
            dst: ValueId(0),
            op,
            ty: Type::U32,
            lhs: ValueId(1),
            rhs: ValueId(2),
        };
        assert!(i.effects().may_trap);
    }
    let mut unchanged = module(false);
    let before = unchanged.clone();
    let mut state = AnalysisState::default();
    assert!(!run_pass(&mut unchanged, &mut zero::pass::ZeroPass, &mut state).unwrap().changed);
    assert_eq!(unchanged, before);
}

#[cfg(feature = "socket")]
#[test]
fn volatile_frontend_storage_roundtrips_and_rejects_aggregate_access() {
    use ir::lower_ast::{frontend as a, interchange};
    let ty = a::TypeRef::Int { bits: 32, signed: false };
    let address = || a::Expr::AddressOf(Box::new(a::Expr::Var("x".into())));
    let p = a::Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![a::Function {
            name: "test".into(),
            convention: CallingConvention::Whale,
            linkage: Linkage::Internal,
            link_name: None,
            parameters: vec![],
            return_type: ty.clone(),
            body: vec![
                a::Stmt::VarDecl { name: "x".into(), ty: ty.clone(), init: None },
                a::Stmt::VolatileStore {
                    pointer: address(),
                    value: a::Expr::Lit(a::Lit::Int {
                        bits: 32,
                        signed: false,
                        value: "42".into(),
                    }),
                },
                a::Stmt::Return(Some(a::Expr::VolatileLoad(Box::new(address())))),
            ],
        }],
    };
    let json = interchange::encode(p).unwrap();
    assert!(interchange::decode(&json.replace("\"format_version\": 5", "\"format_version\": 4"))
        .is_err());
    let p = interchange::decode(&json).unwrap();
    let m = lower_o0(&p, "x86_64-whale-linux", DataLayout::default_64bit_le()).unwrap();
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(42)));
    let mut b = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = b.begin_function("bad", vec![], Type::Void);
    let ty = Type::Array(Box::new(Type::U8), 2);
    let ptr = f.alloca(ty.clone(), 1);
    f.volatile_load(ty, ptr, 1);
    f.ret(None);
    f.finish();
    assert!(verify_module(&b.finish()).is_err());
}

#[test]
fn rollback_retains_nonempty_original_source_diagnostics() {
    let mut m = parse_module(include_str!("fixtures/runtime-diagnostics-v7.wir")).unwrap();
    assert!(!m.sources.originals.is_empty());
    let before = m.clone();
    assert_eq!(
        run_pass(&mut m, &mut InvalidPass(0), &mut AnalysisState::default()).unwrap_err().stage,
        PassStage::After
    );
    assert_eq!(m, before);
}

#[test]
fn volatile_function_pointer_loads_retain_the_callable_signature() {
    let source="module { format_version 8 semantics_version 1 target \"x86_64-whale-linux\" datalayout {ptr=64,endian=little} declare @f0 \"entry\": whale () -> u32, linkage internal declare @f1 \"answer\": whale () -> u32, linkage internal fn @f1 \"answer\"() -> u32, entry %b0 { %b0 \"entry\": %v0: u32 = const u32 42 ret u32 %v0 } fn @f0 \"entry\"() -> u32, entry %b0 { %b0 \"entry\": %v0: ptr<fnptr<whale () -> u32>> = alloca fnptr<whale () -> u32>, align 8 %v1: fnptr<whale () -> u32> = function_addr @f1 volatile_store fnptr<whale () -> u32> %v1, ptr<fnptr<whale () -> u32>> %v0, align 8 %v2: fnptr<whale () -> u32> = volatile_load fnptr<whale () -> u32>, ptr<fnptr<whale () -> u32>> %v0, align 8 %v3: u32 = call whale u32 indirect %v2() ret u32 %v3 } }";
    let m = parse_module(source).unwrap();
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(42)));
}

struct ReverseValueTable(bool);

impl Pass for ReverseValueTable {
    fn name(&self) -> &str {
        "reverse-value-table"
    }

    fn run(&mut self, module: &mut Module) -> Result<PassChange, String> {
        module.functions[0].value_types.reverse();
        Ok(PassChange {
            changed: self.0,
            invalidates: if self.0 { Invalidates::ALL } else { Invalidates::default() },
        })
    }
}

#[test]
fn exact_pass_identity_includes_unprinted_value_table_order() {
    let mut module = module(false);
    let original = module.clone();
    let text = print_module(&module);
    let mut state = AnalysisState::default();
    assert_eq!(
        run_pass(&mut module, &mut ReverseValueTable(false), &mut state).unwrap_err().stage,
        PassStage::Contract
    );
    assert_eq!(module, original);
    assert!(run_pass(&mut module, &mut ReverseValueTable(true), &mut state).unwrap().changed);
    assert_eq!(print_module(&module), text);
    assert_ne!(module, original);
    assert_eq!(state.epoch(AnalysisKind::Values), 1);
}
