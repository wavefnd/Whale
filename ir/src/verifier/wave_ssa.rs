// SPDX-License-Identifier: MPL-2.0
//! Borrowed instruction/CFG transport for the Wave SSA verifier.

use std::collections::HashMap;

use super::{wave_semantics, AnalysisBudget, IndexedFunction, VerifyError};

use crate::{Instruction, Terminator, Type, ValueId};

const NONE: u64 = u64::MAX;

#[repr(C)]
struct Block {
    id: u64,
    first: u64,
    count: u64,
    pred_first: u64,
    pred_count: u64,
    reachable: u64,
    start: u64,
    end: u64,
    terminator_use: u64,
}

#[repr(C)]
struct InstructionRecord {
    phi: u64,
    dst: u64,
    ty: u64,
    first: u64,
    count: u64,
}

#[repr(C)]
struct Usage {
    value: u64,
    ty: u64,
    pred: u64,
    def_block: u64,
    def_position: u64,
    defined: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Predecessor {
    block: u64,
    seen: u64,
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    instruction: u64,
    usage: u64,
    detail: u64,
    work: u64,
    queries: u64,
}

unsafe extern "C" {
    fn whale_ir_verify_ssa(
        types: *const wave_semantics::Node,
        type_count: u64,
        edges: *const u64,
        edge_count: u64,
        blocks: *const Block,
        block_count: u64,
        entry: u64,
        instructions: *const InstructionRecord,
        instruction_count: u64,
        uses: *const Usage,
        use_count: u64,
        predecessors: *const u64,
        predecessor_count: u64,
        table: *mut Predecessor,
        capacity: u64,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;
}

fn resource_error(budget: &AnalysisBudget) -> VerifyError {
    VerifyError::ResourceLimit(crate::LimitError {
        resource: crate::Resource::AnalysisWork,
        limit: budget.limit,
    })
}

pub(super) fn verify(
    f: &IndexedFunction<'_>,
    cfg: &super::cfg::Cfg,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let mut definitions: HashMap<ValueId, Option<(usize, usize)>> =
        f.params.iter().map(|param| (param.id, None)).collect();
    for (block, body) in f.blocks.iter().enumerate() {
        for (position, instruction) in body.instructions.iter().enumerate() {
            if let Some((value, _)) = super::instr_result(instruction) {
                definitions.insert(value, Some((block, position)));
            }
        }
    }
    let mut input = wave_semantics::Input::default();
    let void = input.ty(&Type::Void);
    let mut blocks = Vec::new();
    let mut instructions = Vec::new();
    let mut uses = Vec::new();
    let mut predecessors = Vec::new();
    let mut source = Vec::new();
    let make_use = |value: ValueId, ty: u64, pred: u64| {
        let (defined, def_block, def_position) = match definitions.get(&value) {
            Some(Some((block, position))) => (1, *block as u64, *position as u64),
            Some(None) => (1, NONE, 0),
            None => (0, NONE, 0),
        };
        Usage { value: u64::from(value.0), ty, pred, defined, def_block, def_position }
    };
    for (block_index, block) in f.blocks.iter().enumerate() {
        let first = instructions.len();
        for instruction in &block.instructions {
            let first_use = uses.len();
            let (phi, dst, ty) = match instruction {
                Instruction::Phi { dst, ty, incomings } => {
                    let root = input.ty(ty);
                    for (value, predecessor) in incomings {
                        let ty = f.value_type(*value).map(|ty| input.ty(ty)).unwrap_or(NONE);
                        let pred =
                            cfg.indices.get(predecessor).map(|index| *index as u64).unwrap_or(NONE);
                        uses.push(make_use(*value, ty, pred));
                    }
                    (1, u64::from(dst.0), root)
                },
                _ => {
                    uses.extend(
                        super::instr_uses(instruction)
                            .into_iter()
                            .map(|value| make_use(value, NONE, NONE)),
                    );
                    (0, 0, void)
                },
            };
            instructions.push(InstructionRecord {
                phi,
                dst,
                ty,
                first: first_use as u64,
                count: (uses.len() - first_use) as u64,
            });
            source.push((block_index, instruction));
        }
        let terminator_value = match block.terminator.as_ref().unwrap() {
            Terminator::CBr { cond, .. } => Some(*cond),
            Terminator::Switch { value, .. } => Some(*value),
            Terminator::Ret { value, .. } => *value,
            Terminator::Br { .. } | Terminator::Trap { .. } => None,
        };
        let terminator_use = terminator_value
            .map(|value| {
                let index = uses.len() as u64;
                uses.push(make_use(value, NONE, NONE));
                index
            })
            .unwrap_or(NONE);
        let pred_first = predecessors.len();
        predecessors.extend(cfg.predecessors[block_index].iter().map(|index| *index as u64));
        let (reachable, start, end) = match cfg.intervals[block_index] {
            Some((start, end)) => (1, start as u64, end as u64),
            None => (0, 0, 0),
        };
        blocks.push(Block {
            id: u64::from(block.id.0),
            first: first as u64,
            count: (instructions.len() - first) as u64,
            pred_first: pred_first as u64,
            pred_count: (predecessors.len() - pred_first) as u64,
            reachable,
            start,
            end,
            terminator_use,
        });
    }
    let max_pred = cfg.predecessors.iter().map(Vec::len).max().unwrap_or(0);
    let capacity = max_pred
        .max(1)
        .checked_mul(2)
        .and_then(usize::checked_next_power_of_two)
        .ok_or_else(|| resource_error(budget))?;
    let mut table = vec![Predecessor::default(); capacity];
    let remaining = budget.limit.saturating_sub(budget.work.get()) as u64;
    let mut result = ResultRecord::default();
    let status = unsafe {
        whale_ir_verify_ssa(
            input.nodes.as_ptr(),
            input.nodes.len() as u64,
            input.edges.as_ptr(),
            input.edges.len() as u64,
            blocks.as_ptr(),
            blocks.len() as u64,
            cfg.indices[&f.entry] as u64,
            instructions.as_ptr(),
            instructions.len() as u64,
            uses.as_ptr(),
            uses.len() as u64,
            predecessors.as_ptr(),
            predecessors.len() as u64,
            table.as_mut_ptr(),
            capacity as u64,
            remaining,
            &mut result,
        )
    };
    if status != result.status as i32 || result.work > remaining || result.queries > result.work {
        return Err(resource_error(budget));
    }
    budget.work.set(budget.work.get() + result.work as usize);
    budget.queries.set(budget.queries.get() + result.queries as usize);
    let usage = || uses.get(result.usage as usize).ok_or_else(|| resource_error(budget));
    let instruction =
        || source.get(result.instruction as usize).ok_or_else(|| resource_error(budget));
    Err(match status {
        0 => return Ok(()),
        1 => {
            let (block, instruction) = instruction()?;
            let Instruction::Phi { dst, .. } = instruction else {
                return Err(resource_error(budget));
            };
            let reason = match result.detail {
                0 => "phi must precede non-phi instructions",
                1 => "function entry has no incoming SSA value",
                2 => "phi requires incoming values",
                3 => "phi names an unknown predecessor",
                4 => "phi input is not a predecessor",
                5 => "duplicate phi predecessor",
                6 => "phi is missing a predecessor",
                _ => return Err(resource_error(budget)),
            };
            VerifyError::InvalidPhi {
                func: f.name.clone(),
                block: f.blocks[*block].id,
                value: *dst,
                reason,
            }
        },
        2 => VerifyError::UseOfUndefinedValue {
            func: f.name.clone(),
            value: ValueId(usage()?.value as u32),
        },
        3 => VerifyError::NonDominatingValue {
            func: f.name.clone(),
            value: ValueId(usage()?.value as u32),
            block: f.blocks.get(result.detail as usize).ok_or_else(|| resource_error(budget))?.id,
        },
        4 => {
            let (_, instruction) = instruction()?;
            let Instruction::Phi { ty, .. } = instruction else {
                return Err(resource_error(budget));
            };
            VerifyError::InvalidInstructionType {
                func: f.name.clone(),
                operation: "phi",
                ty: ty.clone(),
            }
        },
        5 => {
            let (_, instruction) = instruction()?;
            let Instruction::Phi { ty, .. } = instruction else {
                return Err(resource_error(budget));
            };
            VerifyError::OperandTypeMismatch {
                func: f.name.clone(),
                value: ValueId(usage()?.value as u32),
                expected: ty.clone(),
                got: input.decode_type(usage()?.ty).ok_or_else(|| resource_error(budget))?,
            }
        },
        6 => VerifyError::MissingValueType {
            func: f.name.clone(),
            value: ValueId(usage()?.value as u32),
        },
        _ => resource_error(budget),
    })
}
