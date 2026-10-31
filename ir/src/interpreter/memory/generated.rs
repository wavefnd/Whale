//! Independent byte-cell/token model: no production fragment or access helper
//! participates in expected results. Bounds, initialization and token authority
//! are computed from the operation stream before invoking the real memory API.
use super::*;

const SIZE: usize = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Cell {
    byte: u8,
    initialized: bool,
    token: Option<(u8, u8)>,
}

#[derive(Clone, Debug)]
struct Model {
    cells: [[Cell; SIZE]; 3],
    alive: [bool; 3],
    data_alive: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self { cells: [[Cell::default(); SIZE]; 3], alive: [true; 3], data_alive: true }
    }
}

type Op = [u8; 6];

impl Model {
    fn access(
        &self,
        a: u8,
        offset: usize,
        n: usize,
        write: bool,
    ) -> std::result::Result<(), MemoryTrap> {
        if !self.alive[usize::from(a) % 3] {
            return Err(MemoryTrap::ExpiredAllocation);
        }
        let upper = if a & 64 != 0 { 16 } else { SIZE };
        if offset > upper || n > upper - offset {
            return Err(MemoryTrap::OutOfBounds);
        }
        if (write && a & 128 != 0) || (!write && a & 32 != 0) {
            return Err(MemoryTrap::PermissionDenied);
        }
        Ok(())
    }

    fn load(&self, a: u8, offset: usize, n: usize) -> std::result::Result<u64, MemoryTrap> {
        self.access(a, offset, n, false)?;
        let cells = &self.cells[usize::from(a) % 3][offset..offset + n];
        if let Some(at) = cells.iter().position(|c| !c.initialized) {
            return Err(MemoryTrap::Uninitialized { offset: (offset + at) as u64 });
        }
        Ok(cells.iter().enumerate().map(|(i, c)| u64::from(c.byte) << (8 * i)).sum())
    }

    fn apply(&mut self, op: Op, bits: &[u64; 6]) -> std::result::Result<Option<u64>, MemoryTrap> {
        let [code, a, offset, b, source, n] = op;
        let offset = usize::from(offset) % 25;
        let source = usize::from(source) % 25;
        let n = usize::from(n) % 25;
        let dst = usize::from(a) % 3;
        match code % 10 {
            0 => {
                self.access(a, offset, 1, true)?;
                self.cells[dst][offset] = Cell { byte: op[5], initialized: true, token: None };
            },
            1 => {
                self.access(a, offset, 8, true)?;
                let token = b % 6;
                for i in 0..8 {
                    self.cells[dst][offset + i] = Cell {
                        byte: (bits[usize::from(token)] >> (8 * i)) as u8,
                        initialized: true,
                        token: Some((token, i as u8)),
                    };
                }
            },
            2 => {
                if n == 0 {
                    return Ok(None);
                }
                self.access(b, source, n, false)?;
                self.access(a, offset, n, true)?;
                let src = usize::from(b) % 3;
                if src == dst && offset < source + n && source < offset + n {
                    return Err(MemoryTrap::OverlappingCopy);
                }
                let copy = self.cells[src][source..source + n].to_vec();
                self.cells[dst][offset..offset + n].copy_from_slice(&copy);
            },
            3 => {
                if n == 0 {
                    return Ok(None);
                }
                self.access(a, offset, n, true)?;
                for cell in &mut self.cells[dst][offset..offset + n] {
                    *cell = Cell { byte: b, initialized: true, token: None };
                }
            },
            4 => {
                self.access(a, offset, n, true)?;
                for cell in &mut self.cells[dst][offset..offset + n] {
                    cell.initialized = false;
                    cell.token = None;
                }
            },
            5 => {
                self.access(a, 0, 0, false).or_else(|e| {
                    if e == MemoryTrap::PermissionDenied {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })?;
                let delta = i128::from(op[2]) - 2;
                let upper = if a & 64 != 0 { 16 } else { SIZE };
                if delta < 0 || delta > upper as i128 {
                    return Err(MemoryTrap::OutOfBounds);
                }
                return Ok(Some(delta as u64));
            },
            6 => return self.load(a, offset, if b & 1 == 0 { 1 } else { 8 }).map(Some),
            7 => {
                self.alive = [false; 3];
            },
            9 => {
                // Retire an exact base belonging to the selected frame. Allocation
                // IDs here are test model identities, independent of real caps.
                let (id, size, alive) = match b % 5 {
                    0 => (2 + dst, SIZE, self.alive[dst]),
                    1 => (0, 1, self.data_alive),
                    2 => (1, 1, true), // globals never belong to a stack frame
                    3 => return Err(MemoryTrap::NullPointer),
                    _ => return Err(MemoryTrap::MissingProvenance),
                };
                if !alive {
                    return Err(MemoryTrap::ExpiredAllocation);
                }
                let lower = usize::from(a & 16 != 0);
                let upper = size - usize::from(a & 64 != 0);
                if offset < lower || offset > upper {
                    return Err(MemoryTrap::OutOfBounds);
                }
                if id < source % 6 || b % 5 == 2 || offset != 0 || lower != 0 || upper != size {
                    return Err(MemoryTrap::InvalidLifetimeEnd);
                }
                if b % 5 == 0 {
                    self.alive[dst] = false;
                } else {
                    self.data_alive = false;
                }
            },
            _ => {
                if n != 0 {
                    return Err(MemoryTrap::NullPointer);
                }
            },
        }
        Ok(None)
    }
}

fn pointer(base: Pointer, a: u8, offset: usize) -> Pointer {
    let mut p = base;
    let cap = p.capability.as_mut().unwrap();
    cap.offset = offset as u64;
    p.address += offset as u64;
    cap.read = a & 32 == 0;
    cap.write = a & 128 == 0;
    if a & 64 != 0 {
        cap.upper = 16;
    }
    p
}

fn actual(
    memory: &mut Memory,
    bases: [Pointer; 3],
    tokens: &[Value; 6],
    types: &[Type; 6],
    global: Pointer,
    op: Op,
) -> Result<Option<u64>> {
    let [code, a, offset, b, source, n] = op;
    let offset = usize::from(offset) % 25;
    let source = usize::from(source) % 25;
    let n = u64::from(n % 25);
    let dst = pointer(bases[usize::from(a) % 3], a, offset);
    match code % 10 {
        0 => memory.store(dst, &Type::U8, &Value::Scalar(ConstValue::U(u128::from(op[5]))), 1)?,
        1 => memory.store(dst, &types[usize::from(b) % 6], &tokens[usize::from(b) % 6], 1)?,
        2 => memory.copy(dst, pointer(bases[usize::from(b) % 3], b, source), n, 1)?,
        3 => memory.set(dst, b, n, 1)?,
        4 => memory.uninit(dst, n, 1)?,
        5 => {
            let p = memory.gep(
                pointer(bases[usize::from(a) % 3], a, 0),
                &[GepStep::Stride(1)],
                &[ConstValue::I(i128::from(op[2]) - 2)],
            )?;
            return Ok(Some(p.address - bases[usize::from(a) % 3].address));
        },
        6 => {
            let ty = if b & 1 == 0 { Type::U8 } else { Type::U64 };
            let Value::Scalar(ConstValue::U(value)) = memory.load(dst, &ty, 1)? else {
                panic!("scalar read")
            };
            return Ok(Some(value as u64));
        },
        7 => memory.retire_from(2)?,
        9 => {
            let base = match b % 5 {
                0 => bases[usize::from(a) % 3],
                1 => tokens[0].pointer(),
                2 => global,
                3 => Pointer::raw(0),
                _ => Pointer::raw(4096),
            };
            let mut selected = base;
            if let Some(cap) = selected.capability.as_mut() {
                cap.offset = offset as u64;
                cap.lower = u64::from(a & 16 != 0);
                cap.upper -= u64::from(a & 64 != 0);
                cap.read = a & 32 == 0;
                cap.write = a & 128 == 0;
                selected.address += offset as u64;
            }
            memory.retire_stack(selected, source % 6)?;
        },
        _ => memory.copy(Pointer::raw(0), Pointer::raw(0), n, 1)?,
    }
    Ok(None)
}

fn outcome<T>(actual: Result<T>, expected: std::result::Result<T, MemoryTrap>)
where
    T: PartialEq + std::fmt::Debug,
{
    match (actual, expected) {
        (Ok(a), Ok(b)) => assert_eq!(a, b),
        (Err(Fault::Trap(a)), Err(b)) => assert_eq!(a, b),
        (a, b) => panic!("model mismatch actual={a:?} expected={b:?}"),
    }
}

fn check_state(
    memory: &mut Memory,
    bases: [Pointer; 3],
    tokens: &[Value; 6],
    global: Pointer,
    model: &Model,
    signature: &Arc<crate::FunctionSignature>,
) {
    // A failed end must leave caller/global storage untouched. A successful
    // end invalidates the data target, including every stored token alias.
    let data = memory.load(tokens[0].pointer(), &Type::U8, 1).map(|v| {
        let Value::Scalar(ConstValue::U(value)) = v else { panic!("data byte") };
        Some(value as u64)
    });
    outcome(
        data,
        if model.data_alive { Ok(Some(0x5a)) } else { Err(MemoryTrap::ExpiredAllocation) },
    );
    let Value::Scalar(ConstValue::U(value)) = memory.load(global, &Type::U8, 1).unwrap() else {
        panic!("global byte")
    };
    assert_eq!(value, 0xa5);
    outcome(memory.set(global, 0, 1, 1), Err(MemoryTrap::PermissionDenied));
    for (slot, base) in bases.iter().enumerate() {
        for at in 0..SIZE {
            let value = memory.load(pointer(*base, 0, at), &Type::U8, 1).map(|v| match v {
                Value::Scalar(ConstValue::U(v)) => Some(v as u64),
                _ => panic!("byte"),
            });
            outcome(value, model.load(slot as u8, at, 1).map(Some));
        }
        if !model.alive[slot] {
            continue;
        }
        for at in [0, 8, 16] {
            let Ok(bits) = model.load(slot as u8, at, 8) else {
                continue;
            };
            let cells = &model.cells[slot][at..at + 8];
            let token = cells[0]
                .token
                .filter(|&(t, index)| {
                    index == 0 && (0..8).all(|i| cells[i].token == Some((t, i as u8)))
                })
                .map(|t| t.0);
            let Value::Pointer(p) =
                memory.load(pointer(*base, 0, at), &Type::ptr_to(Type::U8), 1).unwrap()
            else {
                panic!("data pointer")
            };
            if let Some(t @ 0..=1) = token {
                let Value::Pointer(expected) = tokens[usize::from(t)] else { unreachable!() };
                assert_eq!(p, expected);
                let access = memory.gep(p, &[GepStep::Stride(1)], &[ConstValue::I(0)]).map(|_| ());
                outcome(
                    access,
                    if model.data_alive { Ok(()) } else { Err(MemoryTrap::ExpiredAllocation) },
                );
            } else {
                assert_eq!(p.address, bits);
                assert!(
                    p.capability.is_none(),
                    "bytes or callable tags cannot grant data authority"
                );
            }
            let ty = Type::FnPtr(Box::new((**signature).clone()));
            let Value::FunctionPointer(p) = memory.load(pointer(*base, 0, at), &ty, 1).unwrap()
            else {
                panic!("function pointer")
            };
            let target = if bits == 0 {
                FunctionTarget::Null
            } else if matches!(token, Some(2 | 3)) {
                let Value::FunctionPointer(expected) = &tokens[usize::from(token.unwrap())] else {
                    unreachable!()
                };
                expected.target
            } else {
                FunctionTarget::InvalidBits(bits)
            };
            assert_eq!(p.signature, *signature);
            assert_eq!(
                p.target, target,
                "data, mismatched signature or partial tags cannot grant callable authority"
            );
        }
    }
}

fn run(ops: &[Op]) {
    assert!(ops.len() <= 1024);
    let mut memory = Memory::new(MemoryLimits {
        max_allocations: 5,
        max_bytes: 96,
        max_pointer_fragments: 128,
        max_work: 4_000_000,
    });
    let data = memory.allocate(1, 1, 1).unwrap();
    memory.set(data, 0x5a, 1, 1).unwrap();
    let global = memory.allocate_global(1, 1, 1).unwrap();
    memory.set(global, 0xa5, 1, 1).unwrap();
    let global = memory.seal_global(global, true).unwrap();
    let mut readonly = data;
    readonly.capability.as_mut().unwrap().write = false;
    let signature = Arc::new(crate::FunctionSignature::whale(vec![], Type::Void));
    let wrong = Arc::new(crate::FunctionSignature::whale(vec![Type::U8], Type::Void));
    let tokens = [
        Value::Pointer(data),
        Value::Pointer(readonly),
        Value::FunctionPointer(FunctionPointer {
            signature: signature.clone(),
            target: FunctionTarget::Defined(crate::FunctionId(7)),
        }),
        Value::FunctionPointer(FunctionPointer {
            signature: signature.clone(),
            target: FunctionTarget::Defined(crate::FunctionId(8)),
        }),
        Value::FunctionPointer(FunctionPointer {
            signature: wrong.clone(),
            target: FunctionTarget::Defined(crate::FunctionId(7)),
        }),
        Value::FunctionPointer(FunctionPointer {
            signature: signature.clone(),
            target: FunctionTarget::Null,
        }),
    ];
    let bits = tokens.each_ref().map(|v| match v {
        Value::Pointer(p) => p.address,
        Value::FunctionPointer(p) => p.bits(),
        _ => unreachable!(),
    });
    let types = [
        Type::ptr_to(Type::U8),
        Type::ptr_to(Type::U8),
        Type::FnPtr(Box::new((*signature).clone())),
        Type::FnPtr(Box::new((*signature).clone())),
        Type::FnPtr(Box::new((*wrong).clone())),
        Type::FnPtr(Box::new((*signature).clone())),
    ];
    let bases = [
        memory.allocate(24, 8, 8).unwrap(),
        memory.allocate(24, 8, 8).unwrap(),
        memory.allocate(24, 8, 8).unwrap(),
    ];
    let mut model = Model::default();
    for (i, &op) in ops.iter().enumerate() {
        let expected = model.apply(op, &bits);
        let result = actual(&mut memory, bases, &tokens, &types, global, op);
        if let (Err(Fault::Trap(actual)), Err(expected)) = (&result, &expected) {
            assert_eq!(actual, expected, "operation {i}: {op:?}");
        }
        outcome(result, expected);
        check_state(&mut memory, bases, &tokens, global, &model, &signature);
        assert!(memory.work <= 4_000_000, "step {i}");
    }
}

fn parse(source: &str) -> Vec<Op> {
    assert!(source.len() <= 8192);
    source
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<u8> = line.split_whitespace().map(|v| v.parse().unwrap()).collect();
            fields.try_into().expect("six operation bytes")
        })
        .collect()
}

#[test]
fn retained_memory_sequences_match_independent_cells_and_authority_tokens() {
    run(&parse(include_str!("../../../../fuzz/corpus/memory/authority.ops")));
    run(&parse(include_str!("../../../../fuzz/corpus/memory/lifetime.ops")));
    run(&parse(include_str!("../../../../fuzz/corpus/memory/explicit-lifetime.ops")));
}

fn generated(seed: u64, count: usize) -> Vec<Op> {
    let mut state = seed.max(1);
    (0..count)
        .map(|index| {
            let mut bytes = [0; 6];
            for b in &mut bytes {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *b = state as u8;
            }
            // Preserve long data/authority interactions while introducing
            // selective ends in every stream; retained cases exercise frame exit.
            bytes[0] %= 10;
            if bytes[0] == 7 {
                bytes[0] = 6;
            }
            if bytes[0] == 9 {
                bytes[2] %= 3;
                bytes[3] %= 5;
                bytes[4] %= 6;
            }
            if index % 32 == 24 {
                bytes = [9, bytes[1] % 3, 0, 0, 2, 0];
            }
            bytes
        })
        .collect()
}

#[test]
fn generated_memory_sequences_match_independent_model() {
    for seed in 1..=16 {
        run(&generated(seed, 64));
    }
}

#[test]
#[ignore = "bounded generated operation campaign; set WHALE_MEMORY_SEED/WHALE_MEMORY_CASES or WHALE_MEMORY_REPLAY"]
fn generated_memory_campaign() {
    if let Some(path) = std::env::var_os("WHALE_MEMORY_REPLAY") {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path).unwrap().take(8193).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 8192);
        run(&parse(std::str::from_utf8(&bytes).unwrap()));
        return;
    }
    let seed =
        std::env::var("WHALE_MEMORY_SEED").unwrap_or_else(|_| "1".into()).parse::<u64>().unwrap();
    let count = std::env::var("WHALE_MEMORY_CASES")
        .unwrap_or_else(|_| "256".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=4096).contains(&count));
    for case in 0..count {
        let ops = generated(seed.wrapping_add(case as u64), 128);
        if std::panic::catch_unwind(|| run(&ops)).is_err() {
            let mut minimal = ops;
            let mut at = 0;
            while at < minimal.len() {
                let mut trial = minimal.clone();
                trial.remove(at);
                if std::panic::catch_unwind(|| run(&trial)).is_err() {
                    minimal = trial;
                } else {
                    at += 1;
                }
            }
            let dir = std::env::var_os("WHALE_FUZZ_ARTIFACT_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("whale-fuzz"));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("memory-{seed}-{case}.ops"));
            let text = minimal
                .iter()
                .map(|op| op.iter().map(u8::to_string).collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&path, text).unwrap();
            panic!("seed {seed} case {case}; minimized replay {}", path.display());
        }
    }
}

#[test]
fn memory_resource_rejections_never_publish_partial_destination_changes() {
    for fragments in [0, 7, 8, 15, 16] {
        let mut m = Memory::new(MemoryLimits {
            max_allocations: 3,
            max_bytes: 17,
            max_pointer_fragments: fragments,
            max_work: 10000,
        });
        let target = m.allocate(1, 1, 1).unwrap();
        let src = m.allocate(8, 8, 8).unwrap();
        let dst = m.allocate(8, 8, 8).unwrap();
        let before = m.allocations.iter().map(|a| a.bytes.clone()).collect::<Vec<_>>();
        assert!(matches!(
            m.allocate(1, 1, 1),
            Err(Fault::Limit { resource: MemoryResource::Allocations, .. })
        ));
        assert_eq!(before, m.allocations.iter().map(|a| a.bytes.clone()).collect::<Vec<_>>());
        m.set(dst, 0xa5, 8, 1).unwrap();
        let before = m.allocations[2].bytes.clone();
        if fragments < 8 {
            assert!(matches!(
                m.store(src, &Type::ptr_to(Type::U8), &Value::Pointer(target), 1),
                Err(Fault::Limit { resource: MemoryResource::PointerFragments, .. })
            ));
        } else {
            m.store(src, &Type::ptr_to(Type::U8), &Value::Pointer(target), 1).unwrap();
            let result = m.copy(dst, src, 8, 1);
            if fragments < 16 {
                assert!(matches!(
                    result,
                    Err(Fault::Limit { resource: MemoryResource::PointerFragments, .. })
                ));
                assert_eq!(m.allocations[2].bytes, before);
            } else {
                result.unwrap();
                assert_eq!(m.load(dst, &Type::ptr_to(Type::U8), 1).unwrap().pointer(), target);
            }
        }
        let before = m.test_snapshot(2);
        m.limits.max_work = m.work;
        assert!(matches!(
            m.retire_stack(dst, 0),
            Err(Fault::Limit { resource: MemoryResource::Work, .. })
        ));
        assert!(m.test_alive(2));
        assert_eq!(m.test_snapshot(2), before);
        assert!(matches!(
            m.set(dst, 0, 8, 1),
            Err(Fault::Limit { resource: MemoryResource::Work, .. })
        ));
        assert_eq!(m.test_snapshot(2), before);
    }
    let mut m = Memory::new(MemoryLimits { max_bytes: 3, ..Default::default() });
    assert!(matches!(
        m.allocate(4, 1, 1),
        Err(Fault::Limit { resource: MemoryResource::Bytes, .. })
    ));
    assert_eq!(m.allocation_count(), 0);
}
