// SPDX-License-Identifier: MPL-2.0
//! Borrowed bridge for Wave-owned CFG reverse-postorder traversal.

use super::{AnalysisBudget, VerifyError};

#[repr(C)]
#[derive(Clone, Copy)]
struct Block {
    first: u64,
    count: u64,
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    index: u64,
    count: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_cfg_rpo(
        blocks: *const Block,
        block_count: u64,
        edges: *const u64,
        edge_count: u64,
        entry: u64,
        max_work: u64,
        scratch_seen: *mut u8,
        seen_capacity: u64,
        scratch_stack_block: *mut u64,
        scratch_stack_next: *mut u64,
        stack_capacity: u64,
        scratch_order: *mut u64,
        scratch_capacity: u64,
        output_order: *mut u64,
        output_capacity: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_cfg_idom(
        blocks: *const Block,
        block_count: u64,
        predecessors: *const u64,
        predecessor_count: u64,
        order: *const u64,
        order_count: u64,
        entry: u64,
        max_work: u64,
        scratch_rank: *mut u64,
        rank_capacity: u64,
        scratch_idom: *mut u64,
        idom_capacity: u64,
        output_idom: *mut u64,
        output_capacity: u64,
        result: *mut ResultRecord,
    ) -> i32;
}

pub(super) enum WalkError {
    EntryPredecessor(usize),
    ResourceLimit,
    InvalidGraph,
}

pub(super) fn rpo(
    successors: &[Vec<usize>],
    entry: usize,
    budget: &AnalysisBudget,
) -> Result<Vec<usize>, WalkError> {
    let count = successors.len();
    let edge_count: usize = successors.iter().map(Vec::len).sum();
    let mut blocks = Vec::new();
    let mut edges = Vec::new();
    blocks.try_reserve_exact(count).map_err(|_| WalkError::ResourceLimit)?;
    edges.try_reserve_exact(edge_count).map_err(|_| WalkError::ResourceLimit)?;
    for targets in successors {
        blocks.push(Block { first: edges.len() as u64, count: targets.len() as u64 });
        edges.extend(targets.iter().map(|index| *index as u64));
    }
    let mut seen = vec![0_u8; count];
    let mut stack_block = vec![0_u64; count];
    let mut stack_next = vec![0_u64; count];
    let mut scratch_order = vec![0_u64; count];
    let mut output = vec![0_u64; count];
    let mut result = ResultRecord::default();
    let remaining = budget.limit.saturating_sub(budget.work.get()) as u64;
    let status = unsafe {
        whale_ir_cfg_rpo(
            blocks.as_ptr(),
            count as u64,
            edges.as_ptr(),
            edge_count as u64,
            entry as u64,
            remaining,
            seen.as_mut_ptr(),
            count as u64,
            stack_block.as_mut_ptr(),
            stack_next.as_mut_ptr(),
            count as u64,
            scratch_order.as_mut_ptr(),
            count as u64,
            output.as_mut_ptr(),
            count as u64,
            &mut result,
        )
    };
    if status != result.status as i32 {
        return Err(WalkError::InvalidGraph);
    }
    match status {
        0 if result.count <= count as u64 && result.work <= remaining => {
            budget.work.set(budget.work.get() + result.work as usize);
            Ok(output[..result.count as usize].iter().map(|index| *index as usize).collect())
        },
        1 if result.index < count as u64 => Err(WalkError::EntryPredecessor(result.index as usize)),
        3 => Err(WalkError::ResourceLimit),
        _ => Err(WalkError::InvalidGraph),
    }
}

pub(super) fn resource_error(budget: &AnalysisBudget) -> VerifyError {
    VerifyError::ResourceLimit(crate::LimitError {
        resource: crate::Resource::AnalysisWork,
        limit: budget.limit,
    })
}

pub(super) fn idom(
    predecessors: &[Vec<usize>],
    order: &[usize],
    entry: usize,
    budget: &AnalysisBudget,
) -> Result<Vec<Option<usize>>, WalkError> {
    let count = predecessors.len();
    let edge_count: usize = predecessors.iter().map(Vec::len).sum();
    let mut blocks = Vec::new();
    let mut edges = Vec::new();
    blocks.try_reserve_exact(count).map_err(|_| WalkError::ResourceLimit)?;
    edges.try_reserve_exact(edge_count).map_err(|_| WalkError::ResourceLimit)?;
    for sources in predecessors {
        blocks.push(Block { first: edges.len() as u64, count: sources.len() as u64 });
        edges.extend(sources.iter().map(|index| *index as u64));
    }
    let order: Vec<u64> = order.iter().map(|index| *index as u64).collect();
    let mut rank = vec![0_u64; count];
    let mut scratch_idom = vec![0_u64; count];
    let mut output = vec![0_u64; count];
    let mut result = ResultRecord::default();
    let remaining = budget.limit.saturating_sub(budget.work.get()) as u64;
    let status = unsafe {
        whale_ir_cfg_idom(
            blocks.as_ptr(),
            count as u64,
            edges.as_ptr(),
            edge_count as u64,
            order.as_ptr(),
            order.len() as u64,
            entry as u64,
            remaining,
            rank.as_mut_ptr(),
            count as u64,
            scratch_idom.as_mut_ptr(),
            count as u64,
            output.as_mut_ptr(),
            count as u64,
            &mut result,
        )
    };
    if status != result.status as i32 {
        return Err(WalkError::InvalidGraph);
    }
    match status {
        0 if result.count == count as u64 && result.work <= remaining => {
            budget.work.set(budget.work.get() + result.work as usize);
            Ok(output
                .into_iter()
                .map(|index| (index != u64::MAX).then_some(index as usize))
                .collect())
        },
        3 => Err(WalkError::ResourceLimit),
        _ => Err(WalkError::InvalidGraph),
    }
}
