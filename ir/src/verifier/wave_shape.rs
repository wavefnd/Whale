// SPDX-License-Identifier: MPL-2.0
//! Flat, caller-owned bridge for Wave-owned structural IR verification.

use super::{instr_result, AnalysisBudget, IndexedFunction, VerifyError};

use crate::{BlockId, Terminator, Type, ValueId};

#[repr(C)]
struct Block {
    id: u64,
    terminated: u64,
    first_edge: u64,
    edge_count: u64,
}

#[repr(C)]
struct Value {
    id: u64,
    void_parameter: u64,
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    index: u64,
    detail: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_verify_shape(
        blocks: *const Block,
        block_count: u64,
        entry: u64,
        values: *const Value,
        value_count: u64,
        edges: *const u64,
        edge_count: u64,
        max_work: u64,
        block_table: *mut u64,
        block_table_capacity: u64,
        value_table: *mut u64,
        value_table_capacity: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_shape_contains(
        table: *const u64,
        capacity: u64,
        id: u64,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;
}

pub(super) struct Definitions {
    table: Vec<u64>,
}

impl Definitions {
    pub(super) fn contains(
        &self,
        value: ValueId,
        budget: &AnalysisBudget,
    ) -> Result<bool, VerifyError> {
        let remaining = budget.limit.saturating_sub(budget.work.get()) as u64;
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_shape_contains(
                self.table.as_ptr(),
                self.table.len() as u64,
                u64::from(value.0),
                remaining,
                &mut result,
            )
        };
        if status != 0 || result.status != 0 || result.detail > 1 || result.work > remaining {
            return Err(resource_error(budget));
        }
        budget.work.set(budget.work.get() + result.work as usize);
        Ok(result.detail != 0)
    }
}

fn table_capacity(count: usize) -> Option<usize> {
    count.max(1).checked_mul(2)?.checked_next_power_of_two()
}

pub(super) fn verify(
    f: &IndexedFunction<'_>,
    budget: &AnalysisBudget,
) -> Result<Definitions, VerifyError> {
    let mut blocks = Vec::new();
    let mut values = Vec::new();
    let mut edges = Vec::new();
    for param in &f.params {
        values.push(Value {
            id: u64::from(param.id.0),
            void_parameter: u64::from(param.ty == Type::Void),
        });
    }
    for block in &f.blocks {
        for ins in &block.instructions {
            if let Some((dst, _)) = instr_result(ins) {
                values.push(Value { id: u64::from(dst.0), void_parameter: 0 });
            }
        }
    }
    for block in &f.blocks {
        let first = edges.len();
        match &block.terminator {
            Some(Terminator::Br { target }) => edges.push(u64::from(target.0)),
            Some(Terminator::CBr { then_bb, else_bb, .. }) => {
                edges.push(u64::from(then_bb.0));
                edges.push(u64::from(else_bb.0));
            },
            Some(Terminator::Switch { default_bb, cases, .. }) => {
                edges.push(u64::from(default_bb.0));
                edges.extend(cases.iter().map(|(_, target)| u64::from(target.0)));
            },
            Some(Terminator::Ret { .. } | Terminator::Trap { .. }) | None => {},
        }
        blocks.push(Block {
            id: u64::from(block.id.0),
            terminated: u64::from(block.terminator.is_some()),
            first_edge: first as u64,
            edge_count: (edges.len() - first) as u64,
        });
    }
    let block_capacity = table_capacity(blocks.len()).ok_or_else(|| resource_error(budget))?;
    let value_capacity = table_capacity(values.len()).ok_or_else(|| resource_error(budget))?;
    let mut block_table = vec![0_u64; block_capacity];
    let mut value_table = vec![0_u64; value_capacity];
    let mut result = ResultRecord::default();
    let remaining = budget.limit.saturating_sub(budget.work.get()) as u64;
    let status = unsafe {
        whale_ir_verify_shape(
            blocks.as_ptr(),
            blocks.len() as u64,
            u64::from(f.entry.0),
            values.as_ptr(),
            values.len() as u64,
            edges.as_ptr(),
            edges.len() as u64,
            remaining,
            block_table.as_mut_ptr(),
            block_capacity as u64,
            value_table.as_mut_ptr(),
            value_capacity as u64,
            &mut result,
        )
    };
    if status != result.status as i32 {
        return Err(resource_error(budget));
    }
    let index = result.index as usize;
    match status {
        0 if result.work <= remaining => {
            budget.work.set(budget.work.get() + result.work as usize);
            Ok(Definitions { table: value_table })
        },
        1 => Err(VerifyError::DuplicateBlock {
            func: f.name.clone(),
            block: BlockId(result.detail as u32),
        }),
        2 => Err(VerifyError::InvalidEntryBlock { func: f.name.clone(), entry: f.entry }),
        3 => Err(VerifyError::DuplicateValue {
            func: f.name.clone(),
            value: ValueId(result.detail as u32),
        }),
        4 if index < f.blocks.len() => Err(VerifyError::UnterminatedBlock {
            func: f.name.clone(),
            block: f.blocks[index].name.clone(),
        }),
        5 if index < f.blocks.len() => Err(VerifyError::InvalidBranchTarget {
            func: f.name.clone(),
            block: f.blocks[index].id,
            target: BlockId(result.detail as u32),
        }),
        8 if index < f.params.len() => Err(VerifyError::VoidParameter {
            func: f.name.clone(),
            param: f.params[index].name.clone(),
        }),
        _ => Err(resource_error(budget)),
    }
}

fn resource_error(budget: &AnalysisBudget) -> VerifyError {
    VerifyError::ResourceLimit(crate::LimitError {
        resource: crate::Resource::AnalysisWork,
        limit: budget.limit,
    })
}
