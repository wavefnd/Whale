// SPDX-License-Identifier: MPL-2.0
//! Lossless enum/ID transport and allocation for the Wave integer folder.
use super::*;

use crate::{BinOp, CmpOp, ConstValue, ICmpPred, Instruction, Type, ValueId};

#[repr(C)]
#[derive(Default)]
struct Node {
    op: u64,
    sub: u64,
    scope: u64,
    id: u64,
    kind: u64,
    literal: u64,
    low: u64,
    high: u64,
    first: u64,
    count: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Value {
    known: u64,
    kind: u64,
    low: u64,
    high: u64,
    replace: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Slot {
    key: u64,
    index: u64,
    present: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct User {
    index: u64,
    next: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Pending {
    head: u64,
    queued: u64,
}

#[repr(C)]
struct Input {
    nodes: *const Node,
    count: u64,
    references: *const u64,
    reference_count: u64,
    values: *mut Value,
    slots: *mut Slot,
    capacity: u64,
    users: *mut User,
    pending: *mut Pending,
    queue: *mut u64,
    max_work: u64,
}

#[repr(C)]
#[derive(Default)]
struct Output {
    status: u64,
    changed: u64,
    folded: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_fold_integer(input: *const Input, output: *mut Output) -> i32;
}

fn kind(ty: &Type) -> u64 {
    match ty {
        Type::Bool => 1,
        Type::I1 => 2,
        Type::U1 => 3,
        Type::I8 => 4,
        Type::I16 => 5,
        Type::I32 => 6,
        Type::I64 => 7,
        Type::I128 => 8,
        Type::U8 => 9,
        Type::U16 => 10,
        Type::U32 => 11,
        Type::U64 => 12,
        Type::U128 => 13,
        _ => 0,
    }
}

fn binary(op: &BinOp) -> Option<u64> {
    Some(match op {
        BinOp::Add => 0,
        BinOp::Sub => 1,
        BinOp::Mul => 2,
        BinOp::UDiv => 3,
        BinOp::SDiv => 4,
        BinOp::URem => 5,
        BinOp::SRem => 6,
        BinOp::And => 12,
        BinOp::Or => 13,
        BinOp::Xor => 14,
        BinOp::Shl => 15,
        BinOp::LShr => 16,
        BinOp::AShr => 17,
        _ => return None,
    })
}

fn predicate(op: &ICmpPred) -> u64 {
    match op {
        ICmpPred::Eq => 0,
        ICmpPred::Ne => 1,
        ICmpPred::Ult => 2,
        ICmpPred::Ule => 3,
        ICmpPred::Ugt => 4,
        ICmpPred::Uge => 5,
        ICmpPred::Slt => 6,
        ICmpPred::Sle => 7,
        ICmpPred::Sgt => 8,
        ICmpPred::Sge => 9,
    }
}

fn comparison(op: &CmpOp) -> Option<u64> {
    Some(match op {
        CmpOp::Eq => 0,
        CmpOp::Ne => 1,
        CmpOp::ULt => 2,
        CmpOp::ULe => 3,
        CmpOp::UGt => 4,
        CmpOp::UGe => 5,
        CmpOp::SLt => 6,
        CmpOp::SLe => 7,
        CmpOp::SGt => 8,
        CmpOp::SGe => 9,
        _ => return None,
    })
}

fn literal(node: &mut Node, value: &ConstValue) {
    let bits = match value {
        ConstValue::I(value) => {
            node.literal = 1;
            *value as u128
        },
        ConstValue::U(value) => {
            node.literal = 2;
            *value
        },
        ConstValue::Bool(value) => {
            node.literal = 3;
            u128::from(*value)
        },
        _ => return,
    };
    node.low = bits as u64;
    node.high = (bits >> 64) as u64;
}

fn allocate<T: Clone + Default>(count: usize) -> Result<Vec<T>, String> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| "constant folder allocation failed")?;
    values.resize(count, T::default());
    Ok(values)
}

fn result_id(i: &Instruction) -> Option<ValueId> {
    use Instruction::*;
    Some(match i {
        GlobalAddr { dst, .. }
        | PtrDiff { dst, .. }
        | ConstDecl { dst, .. }
        | Const { dst, .. }
        | Undef { dst, .. }
        | Mov { dst, .. }
        | Bin { dst, .. }
        | Neg { dst, .. }
        | Not { dst, .. }
        | Cmp { dst, .. }
        | ICmp { dst, .. }
        | FCmp { dst, .. }
        | Select { dst, .. }
        | Cast { dst, .. }
        | Phi { dst, .. }
        | Extract { dst, .. }
        | Checked { dst, .. }
        | Alloca { dst, .. }
        | VolatileLoad { dst, .. }
        | Load { dst, .. }
        | Gep { dst, .. }
        | NullFunction { dst, .. }
        | FunctionAddr { dst, .. } => *dst,
        Call { dst, .. } => return *dst,
        LifetimeEnd { .. }
        | VolatileStore { .. }
        | Uninit { .. }
        | Store { .. }
        | Memcpy { .. }
        | Memset { .. }
        | TrapIf { .. } => return None,
    })
}

fn marshal(i: &Instruction, node: &mut Node, references: &mut Vec<u64>) {
    use Instruction::*;
    let mut operands = |ids: &[ValueId]| references.extend(ids.iter().map(|id| u64::from(id.0)));
    match i {
        Const { ty, value, .. } => {
            node.op = 4;
            node.kind = kind(ty);
            literal(node, value);
        },
        ConstDecl { expression, value, .. } => {
            node.op = 3;
            node.kind = kind(&expression.ty);
            literal(node, value);
        },
        Mov { ty, src, .. } | Neg { ty, src, .. } | Not { ty, src, .. } => {
            node.op = match i {
                Mov { .. } => 6,
                Neg { .. } => 8,
                _ => 9,
            };
            node.kind = kind(ty);
            operands(&[*src]);
        },
        Bin { op, ty, lhs, rhs, .. } if binary(op).is_some() && kind(ty) > 1 => {
            node.op = 7;
            node.sub = binary(op).unwrap();
            node.kind = kind(ty);
            operands(&[*lhs, *rhs]);
        },
        Cmp { op, ty, lhs, rhs, .. } if comparison(op).is_some() && kind(ty) != 0 => {
            node.op = 10;
            node.sub = comparison(op).unwrap();
            node.kind = kind(ty);
            operands(&[*lhs, *rhs]);
        },
        ICmp { pred, ty, lhs, rhs, .. } if kind(ty) != 0 => {
            node.op = 11;
            node.sub = predicate(pred);
            node.kind = kind(ty);
            operands(&[*lhs, *rhs]);
        },
        Select { ty, cond, on_true, on_false, .. } if kind(ty) != 0 => {
            node.op = 13;
            node.kind = kind(ty);
            operands(&[*cond, *on_true, *on_false]);
        },
        Phi { ty, incomings, .. } if kind(ty) != 0 => {
            node.op = 15;
            node.kind = kind(ty);
            references.extend(incomings.iter().map(|(id, _)| u64::from(id.0)));
        },
        _ => {},
    }
    // Unsupported literal categories stay indexed, with no arithmetic fallback.
    if node.kind == 0 {
        node.op = 63;
        references.truncate(node.first as usize);
    }
}

fn decode(value: Value) -> Result<(Type, ConstValue), String> {
    let bits = u128::from(value.low) | (u128::from(value.high) << 64);
    let ty = match value.kind {
        1 => Type::Bool,
        2 => Type::I1,
        3 => Type::U1,
        4 => Type::I8,
        5 => Type::I16,
        6 => Type::I32,
        7 => Type::I64,
        8 => Type::I128,
        9 => Type::U8,
        10 => Type::U16,
        11 => Type::U32,
        12 => Type::U64,
        13 => Type::U128,
        _ => return Err("constant folder result type transport".into()),
    };
    let constant = match value.kind {
        1 if bits <= 1 => ConstValue::Bool(bits != 0),
        2 | 4..=8 => ConstValue::I(bits as i128),
        3 | 9..=13 => ConstValue::U(bits),
        _ => return Err("constant folder result literal transport".into()),
    };
    if !crate::constant::valid_constant(&ty, &constant) {
        return Err("constant folder result literal transport".into());
    }
    Ok((ty, constant))
}

fn reference_count(instruction: &Instruction) -> usize {
    use Instruction::*;
    match instruction {
        Mov { ty, .. } | Neg { ty, .. } | Not { ty, .. } if kind(ty) != 0 => 1,
        Bin { op, ty, .. } if binary(op).is_some() && kind(ty) > 1 => 2,
        Cmp { op, ty, .. } if comparison(op).is_some() && kind(ty) != 0 => 2,
        ICmp { ty, .. } if kind(ty) != 0 => 2,
        Select { ty, .. } if kind(ty) != 0 => 3,
        Phi { ty, incomings, .. } if kind(ty) != 0 => incomings.len(),
        _ => 0,
    }
}

pub(super) fn fold(module: &mut Module, limits: IrLimits) -> Result<PassChange, String> {
    crate::verify_module_with_limits(module, limits).map_err(|e| format!("{e:?}"))?;
    let count = module
        .functions
        .iter()
        .try_fold(0usize, |n, f| {
            f.blocks.iter().try_fold(n.checked_add(f.params.len())?, |n, b| {
                n.checked_add(b.instructions.iter().filter(|i| result_id(i).is_some()).count())
            })
        })
        .ok_or("constant folder model limit exceeded")?;
    let capacity = count
        .checked_mul(2)
        .and_then(|n| n.max(2).checked_next_power_of_two())
        .ok_or("constant folder model limit exceeded")?;
    let reference_count = module
        .functions
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.instructions)
        .try_fold(0usize, |count, instruction| count.checked_add(reference_count(instruction)))
        .ok_or("constant folder reference limit exceeded")?;
    if count > limits.max_nodes
        || capacity > limits.max_analysis_work
        || reference_count > limits.max_nodes
    {
        return Err("constant folder model limit exceeded".into());
    }
    // Admit every owned transport and worst-case staged replacement before
    // reserving any model buffer, even when callers raise the IR node limit.
    let workspace = count
        .checked_mul(80 + 40 + 16 + 8 + 32 + std::mem::size_of::<Instruction>() + 24)
        .and_then(|n| n.checked_add(capacity.checked_mul(24)?))
        .and_then(|n| n.checked_add(reference_count.checked_mul(24)?))
        .ok_or("constant folder workspace limit exceeded")?;
    if workspace > 128 * 1024 * 1024 {
        return Err("constant folder workspace limit exceeded".into());
    }
    let mut nodes = Vec::new();
    let mut paths = Vec::new();
    let mut references = Vec::new();
    nodes.try_reserve_exact(count).map_err(|_| "constant folder allocation failed")?;
    paths.try_reserve_exact(count).map_err(|_| "constant folder allocation failed")?;
    references
        .try_reserve_exact(reference_count)
        .map_err(|_| "constant folder allocation failed")?;
    for (fi, function) in module.functions.iter().enumerate() {
        for parameter in &function.params {
            nodes.push(Node {
                op: 31,
                scope: function.id.0.into(),
                id: parameter.id.0.into(),
                kind: kind(&parameter.ty),
                first: references.len() as u64,
                ..Node::default()
            });
            paths.push(None);
        }
        for (bi, block) in function.blocks.iter().enumerate() {
            for (ii, instruction) in block.instructions.iter().enumerate() {
                let Some(id) = result_id(instruction) else { continue };
                let mut node = Node {
                    op: 63,
                    scope: function.id.0.into(),
                    id: id.0.into(),
                    first: references.len() as u64,
                    ..Node::default()
                };
                marshal(instruction, &mut node, &mut references);
                node.count = references.len() as u64 - node.first;
                nodes.push(node);
                paths.push(Some((fi, bi, ii)));
            }
        }
    }
    if references.len() != reference_count {
        return Err("constant folder reference transport".into());
    }
    let mut values = allocate::<Value>(count)?;
    let mut slots = allocate::<Slot>(capacity)?;
    let mut users = allocate::<User>(references.len())?;
    let mut pending = allocate::<Pending>(count)?;
    let mut queue = allocate::<u64>(count)?;
    let input = Input {
        nodes: nodes.as_ptr(),
        count: count as u64,
        references: references.as_ptr(),
        reference_count: references.len() as u64,
        values: values.as_mut_ptr(),
        slots: slots.as_mut_ptr(),
        capacity: capacity as u64,
        users: users.as_mut_ptr(),
        pending: pending.as_mut_ptr(),
        queue: queue.as_mut_ptr(),
        max_work: limits.max_analysis_work as u64,
    };
    let mut output = Output::default();
    let status = unsafe { whale_ir_fold_integer(&input, &mut output) };
    if status == 1 && output.status == 1 {
        return Err("constant folder analysis work limit exceeded".into());
    }
    if status != 0
        || output.status != 0
        || output.changed > 1
        || output.folded > count as u64
        || output.work > limits.max_analysis_work as u64
    {
        return Err("constant folder transport".into());
    }
    let mut replacements = Vec::new();
    replacements
        .try_reserve_exact(output.folded as usize)
        .map_err(|_| "constant folder allocation failed")?;
    for ((value, node), path) in values.into_iter().zip(nodes).zip(paths) {
        if value.known > 1 || value.replace > value.known {
            return Err("constant folder result transport".into());
        }
        if value.replace == 0 {
            continue;
        }
        let path = path.ok_or("constant folder replacement transport")?;
        if !matches!(node.op, 6..=11 | 13 | 15) {
            return Err("constant folder replacement transport".into());
        }
        let expected = if matches!(node.op, 10 | 11) { 1 } else { node.kind };
        if value.kind != expected {
            return Err("constant folder result type transport".into());
        }
        let (ty, value) = decode(value)?;
        replacements.push((path, Instruction::Const { dst: ValueId(node.id as u32), ty, value }));
    }
    if replacements.len() as u64 != output.folded
        || output.changed != u64::from(!replacements.is_empty())
    {
        return Err("constant folder report transport".into());
    }
    for ((fi, bi, ii), instruction) in replacements {
        module.functions[fi].blocks[bi].instructions[ii] = instruction;
    }
    Ok(PassChange {
        changed: output.changed != 0,
        invalidates: if output.changed != 0 {
            super::super::Invalidates::ALL
        } else {
            Default::default()
        },
    })
}
