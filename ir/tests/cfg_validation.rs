use ir::*;

fn block(id: u32, instructions: Vec<Instruction>, terminator: Terminator) -> BasicBlock {
    BasicBlock {
        id: BlockId(id),
        name: format!("b{id}"),
        instructions,
        terminator: Some(terminator),
    }
}

fn module(blocks: Vec<BasicBlock>, values: &[(u32, Type)]) -> Module {
    let mut m = Module::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    m.declarations.push(FunctionDecl {
        id: FunctionId(0),
        name: "flow".into(),
        signature: FunctionSignature::whale(vec![Type::Bool], Type::Void),
        linkage: Linkage::Internal,
        link_name: None,
    });
    m.functions.push(Function {
        id: FunctionId(0),
        name: "flow".into(),
        params: vec![Param {
            name: "condition".into(),
            id: ValueId(0),
            ty: Type::Bool,
        }],
        ret_ty: Type::Void,
        blocks,
        entry: BlockId(0),
        value_types: std::iter::once((ValueId(0), Type::Bool))
            .chain(values.iter().map(|(id, ty)| (ValueId(*id), ty.clone())))
            .collect(),
    });
    m
}

fn constant(id: u32) -> Instruction {
    Instruction::Const {
        dst: ValueId(id),
        ty: Type::I32,
        value: ConstValue::I(7),
    }
}
fn mov(dst: u32, src: u32) -> Instruction {
    Instruction::Mov {
        dst: ValueId(dst),
        ty: Type::I32,
        src: ValueId(src),
    }
}
fn br(id: u32) -> Terminator {
    Terminator::Br {
        target: BlockId(id),
    }
}
fn cbr(a: u32, b: u32) -> Terminator {
    Terminator::CBr {
        cond: ValueId(0),
        then_bb: BlockId(a),
        else_bb: BlockId(b),
    }
}
fn ret() -> Terminator {
    Terminator::Ret {
        ty: Type::Void,
        value: None,
    }
}
fn phi(dst: u32, incomings: &[(u32, u32)]) -> Instruction {
    Instruction::Phi {
        dst: ValueId(dst),
        ty: Type::I32,
        incomings: incomings
            .iter()
            .map(|(v, b)| (ValueId(*v), BlockId(*b)))
            .collect(),
    }
}

#[test]
fn sibling_values_do_not_leak_and_block_order_does_not_matter() {
    let mut bad = module(
        vec![
            block(0, vec![], cbr(1, 2)),
            block(1, vec![constant(10)], ret()),
            block(2, vec![mov(11, 10)], ret()),
        ],
        &[(10, Type::I32), (11, Type::I32)],
    );
    for _ in 0..3 {
        assert!(verify_module(&bad).is_err(), "sibling use accepted");
        bad.functions[0].blocks.rotate_left(1);
    }
    let mut good = module(
        vec![
            block(0, vec![constant(10)], br(1)),
            block(1, vec![mov(11, 10)], br(2)),
            block(2, vec![mov(12, 11)], ret()),
        ],
        &[(10, Type::I32), (11, Type::I32), (12, Type::I32)],
    );
    for _ in 0..3 {
        verify_module(&good).unwrap();
        good.functions[0].blocks.rotate_left(1);
    }
}

#[test]
fn same_block_forward_and_self_uses_fail() {
    for instructions in [
        vec![mov(11, 10), constant(10)],
        vec![mov(10, 10), constant(11)],
    ] {
        assert!(verify_module(&module(
            vec![block(0, instructions, ret())],
            &[(10, Type::I32), (11, Type::I32)]
        ))
        .is_err());
    }
}

fn diamond() -> Module {
    module(
        vec![
            block(0, vec![], cbr(1, 2)),
            block(1, vec![constant(10)], br(3)),
            block(2, vec![constant(11)], br(3)),
            block(3, vec![phi(12, &[(10, 1), (11, 2)])], ret()),
        ],
        &[(10, Type::I32), (11, Type::I32), (12, Type::I32)],
    )
}

#[test]
fn phi_inputs_match_predecessors_and_are_available_on_their_edges() {
    let mut good = diamond();
    for _ in 0..4 {
        verify_module(&good).unwrap();
        good.functions[0].blocks.rotate_left(1);
    }
    for incomings in [
        vec![(10, 1)],
        vec![(10, 1), (11, 1)],
        vec![(10, 1), (11, 0)],
        vec![(11, 1), (10, 2)],
        vec![(0, 1), (11, 2)],
        vec![(999, 1), (11, 2)],
        vec![(10, 1), (11, 99)],
    ] {
        let mut bad = diamond();
        bad.functions[0].blocks[3].instructions[0] = phi(12, &incomings);
        assert!(verify_module(&bad).is_err(), "accepted {incomings:?}");
    }
}

#[test]
fn loop_phi_accepts_backedge_values_stored_later_and_self_edges() {
    let mut m = module(
        vec![
            block(0, vec![constant(10)], br(1)),
            block(1, vec![phi(11, &[(10, 0), (12, 2)])], cbr(2, 3)),
            block(2, vec![mov(12, 11)], br(1)),
            block(3, vec![mov(13, 11)], ret()),
        ],
        &[
            (10, Type::I32),
            (11, Type::I32),
            (12, Type::I32),
            (13, Type::I32),
        ],
    );
    for _ in 0..4 {
        verify_module(&m).unwrap();
        m.functions[0].blocks.reverse();
        m.functions[0].blocks.rotate_left(1);
    }
    verify_module(&module(
        vec![
            block(0, vec![constant(10)], br(1)),
            block(1, vec![phi(11, &[(10, 0), (11, 1)])], cbr(1, 2)),
            block(2, vec![], ret()),
        ],
        &[(10, Type::I32), (11, Type::I32)],
    ))
    .unwrap();
}

#[test]
fn phis_are_a_typed_prefix_and_cannot_supply_an_entry_value() {
    let mut bad = diamond();
    bad.functions[0].value_types.push((ValueId(13), Type::I32));
    bad.functions[0].blocks[3]
        .instructions
        .insert(0, constant(13));
    assert!(verify_module(&bad).is_err());
    let mut bad = diamond();
    if let Instruction::Phi { ty, .. } = &mut bad.functions[0].blocks[3].instructions[0] {
        *ty = Type::Void;
    }
    bad.functions[0]
        .value_types
        .iter_mut()
        .find(|(v, _)| *v == ValueId(12))
        .unwrap()
        .1 = Type::Void;
    assert!(verify_module(&bad).is_err());
    let bad = module(
        vec![block(0, vec![phi(10, &[(10, 0)])], br(0))],
        &[(10, Type::I32)],
    );
    assert!(verify_module(&bad).is_err());
    let bad = module(
        vec![block(0, vec![phi(10, &[])], ret())],
        &[(10, Type::I32)],
    );
    assert!(verify_module(&bad).is_err());
}

#[test]
fn repeated_successor_edges_use_one_input_per_predecessor_block() {
    let mut m = module(
        vec![
            block(0, vec![constant(10)], cbr(1, 1)),
            block(1, vec![phi(11, &[(10, 0)])], ret()),
        ],
        &[(10, Type::I32), (11, Type::I32)],
    );
    verify_module(&m).unwrap();
    m.functions[0].blocks[0].terminator = Some(Terminator::Switch {
        ty: Type::Bool,
        value: ValueId(0),
        default_bb: BlockId(1),
        cases: vec![(ConstValue::Bool(true), BlockId(1))],
    });
    verify_module(&m).unwrap();
}

#[test]
fn unreachable_blocks_are_preserved_and_checked_without_leaking_values() {
    let mut m = module(
        vec![
            block(0, vec![constant(10)], ret()),
            block(1, vec![mov(11, 12)], br(2)),
            block(2, vec![constant(12)], br(1)),
        ],
        &[(10, Type::I32), (11, Type::I32), (12, Type::I32)],
    );
    let before = format!("{m:?}");
    verify_module(&m).unwrap();
    assert_eq!(before, format!("{m:?}"));
    m.functions[0].blocks[0].instructions.push(mov(13, 12));
    m.functions[0].value_types.push((ValueId(13), Type::I32));
    assert!(verify_module(&m).is_err());
    m.functions[0].blocks[0].instructions.pop();
    m.functions[0].value_types.pop();
    m.functions[0].blocks[1].instructions[0] = mov(11, 999);
    assert!(verify_module(&m).is_err());
    m.functions[0].blocks[1].instructions[0] = mov(11, 0);
    assert!(
        verify_module(&m).is_err(),
        "unreachable code still requires valid types"
    );
}

#[test]
fn unreachable_predecessors_do_not_destroy_reachable_dominance() {
    verify_module(&module(
        vec![
            block(0, vec![constant(10)], br(1)),
            block(1, vec![mov(11, 10)], ret()),
            block(2, vec![], br(1)),
        ],
        &[(10, Type::I32), (11, Type::I32)],
    ))
    .unwrap();
}

#[test]
fn entry_rejects_every_kind_of_incoming_edge_even_from_unreachable_blocks() {
    for source in 0..3 {
        for terminator in [
            br(0),
            cbr(0, 2),
            cbr(2, 0),
            Terminator::Switch {
                ty: Type::Bool,
                value: ValueId(0),
                default_bb: BlockId(0),
                cases: vec![(ConstValue::Bool(true), BlockId(2))],
            },
            Terminator::Switch {
                ty: Type::Bool,
                value: ValueId(0),
                default_bb: BlockId(2),
                cases: vec![(ConstValue::Bool(true), BlockId(0))],
            },
        ] {
            // b0 is entry, b1 is reachable, and b2 is initially unreachable.
            let mut m = module(
                vec![
                    block(0, vec![], br(1)),
                    block(1, vec![], ret()),
                    block(2, vec![], ret()),
                ],
                &[],
            );
            m.functions[0].blocks[source].terminator = Some(terminator);
            let before = format!("{m:?}");
            for _ in 0..3 {
                assert!(matches!(
                    verify_module(&m),
                    Err(VerifyError::EntryHasPredecessor {
                        entry: BlockId(0), predecessor, ..
                    }) if predecessor == BlockId(source as u32)
                ));
                m.functions[0].blocks.rotate_left(1);
            }
            assert_eq!(before, format!("{m:?}"));
        }
    }
}

#[test]
fn dominance_agrees_with_path_removal_on_small_cyclic_graphs() {
    // Independent oracle: D dominates U iff removing D removes every entry-to-U
    // path. Enumerate all directed graphs on three blocks without self edges;
    // the structural contract independently rejects edges to the entry block.
    fn reaches(edges: &[Vec<u32>], target: u32, removed: Option<u32>) -> bool {
        let mut todo = vec![0];
        let mut seen = [false; 3];
        while let Some(node) = todo.pop() {
            if Some(node) == removed || std::mem::replace(&mut seen[node as usize], true) {
                continue;
            }
            if node == target {
                return true;
            }
            todo.extend(&edges[node as usize]);
        }
        false
    }
    let pairs = [(0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1)];
    for mask in 0..64 {
        let mut edges = vec![vec![]; 3];
        for (bit, &(from, to)) in pairs.iter().enumerate() {
            if mask & (1 << bit) != 0 {
                edges[from].push(to);
            }
        }
        for definition in 0..3 {
            for usage in 0..3 {
                let blocks = (0..3)
                    .map(|id| {
                        let mut instructions = Vec::new();
                        if id == definition {
                            instructions.push(constant(10));
                        }
                        if id == usage {
                            instructions.push(mov(11, 10));
                        }
                        let end = match edges[id as usize].as_slice() {
                            [] => ret(),
                            [one] => br(*one),
                            [a, b] => cbr(*a, *b),
                            _ => unreachable!(),
                        };
                        block(id, instructions, end)
                    })
                    .collect();
                let mut m = module(blocks, &[(10, Type::I32), (11, Type::I32)]);
                let expected = !edges.iter().any(|targets| targets.contains(&0))
                    && (!reaches(&edges, usage, None) || !reaches(&edges, usage, Some(definition)));
                for _ in 0..2 {
                    assert_eq!(
                        verify_module(&m).is_ok(),
                        expected,
                        "edges={edges:?}, definition={definition}, usage={usage}"
                    );
                    m.functions[0].blocks.reverse();
                }
            }
        }
    }
}
