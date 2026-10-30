//! Read-only SSA checks. Reachability never removes or simplifies O0 IR.
use std::collections::{HashMap, HashSet};

use crate::{BlockId, Terminator};

#[cfg(not(whale_wave_ir))]
use crate::{Instruction, ValueId};

use super::VerifyError;

#[cfg(not(whale_wave_ir))]
use super::{instr_result, instr_uses, operands};

pub(super) struct Cfg {
    pub(super) indices: HashMap<BlockId, usize>,
    pub(super) predecessors: Vec<Vec<usize>>,
    // None means structurally unreachable from the function entry.
    #[cfg(not(whale_wave_ir))]
    idom: Vec<Option<usize>>,
    pub(super) intervals: Vec<Option<(usize, usize)>>,
    #[cfg(not(whale_wave_ir))]
    predecessor_sets: Vec<HashSet<usize>>,
}

impl Cfg {
    // Block identity, termination and branch destinations have already passed
    // structural validation. Do not inspect constant branch conditions here.
    fn new(
        f: &super::IndexedFunction<'_>,
        budget: &super::AnalysisBudget,
    ) -> Result<Self, VerifyError> {
        let indices: HashMap<_, _> = f.blocks.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
        let mut successors = vec![Vec::new(); f.blocks.len()];
        let mut predecessors = vec![Vec::new(); f.blocks.len()];
        for (index, block) in f.blocks.iter().enumerate() {
            budget.charge()?;
            let targets = match block.terminator.as_ref().unwrap() {
                Terminator::Br { target } => vec![*target],
                Terminator::CBr { then_bb, else_bb, .. } => vec![*then_bb, *else_bb],
                Terminator::Switch { default_bb, cases, .. } => std::iter::once(*default_bb)
                    .chain(cases.iter().map(|(_, target)| *target))
                    .collect(),
                Terminator::Ret { .. } | Terminator::Trap { .. } => vec![],
            };
            let mut seen = HashSet::new();
            for target in targets {
                budget.charge()?;
                let target = indices[&target];
                if seen.insert(target) {
                    successors[index].push(target);
                    predecessors[target].push(index);
                }
            }
        }

        let entry = indices[&f.entry];
        #[cfg(whale_wave_ir)]
        let order = match super::wave_cfg::rpo(&successors, entry, budget) {
            Ok(order) => order,
            Err(super::wave_cfg::WalkError::EntryPredecessor(predecessor)) => {
                return Err(VerifyError::EntryHasPredecessor {
                    func: f.name.clone(),
                    entry: f.entry,
                    predecessor: f.blocks[predecessor].id,
                });
            },
            Err(super::wave_cfg::WalkError::ResourceLimit) => {
                return Err(super::wave_cfg::resource_error(budget));
            },
            Err(super::wave_cfg::WalkError::InvalidGraph) => {
                return Err(super::wave_cfg::resource_error(budget));
            },
        };
        #[cfg(not(whale_wave_ir))]
        let order = {
            // Iterative DFS avoids consuming the host call stack for deep CFGs.
            // Entry represents a fresh invocation, never a branch destination.
            // Check all edges, including those from unreachable blocks, without
            // rewriting the original control flow.
            if let Some(&predecessor) = predecessors[entry].first() {
                return Err(VerifyError::EntryHasPredecessor {
                    func: f.name.clone(),
                    entry: f.entry,
                    predecessor: f.blocks[predecessor].id,
                });
            }
            let mut seen = vec![false; f.blocks.len()];
            let mut stack = vec![(entry, false)];
            let mut order = Vec::new();
            while let Some((block, exiting)) = stack.pop() {
                budget.charge()?;
                if exiting {
                    order.push(block);
                    continue;
                }
                if std::mem::replace(&mut seen[block], true) {
                    continue;
                }
                stack.push((block, true));
                stack.extend(successors[block].iter().rev().map(|&next| (next, false)));
            }
            order.reverse();
            order
        };
        #[cfg(not(whale_wave_ir))]
        let idom = {
            let mut rank = vec![0; f.blocks.len()];
            for (i, &block) in order.iter().enumerate() {
                rank[block] = i;
            }
            let mut idom = vec![None; f.blocks.len()];
            idom[entry] = Some(entry);
            loop {
                let mut changed = false;
                for &block in order.iter().skip(1) {
                    budget.charge()?;
                    let mut next: Option<usize> = None;
                    for &predecessor in &predecessors[block] {
                        budget.charge()?;
                        if idom[predecessor].is_none() {
                            continue;
                        }
                        next = Some(match next {
                            None => predecessor,
                            Some(mut a) => {
                                let mut b = predecessor;
                                while a != b {
                                    budget.charge()?;
                                    if rank[a] > rank[b] {
                                        a = idom[a].unwrap();
                                    } else {
                                        b = idom[b].unwrap();
                                    }
                                }
                                a
                            },
                        });
                    }
                    if next != idom[block] {
                        idom[block] = next;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            idom
        };
        #[cfg(whale_wave_ir)]
        let idom = super::wave_cfg::idom(&predecessors, &order, entry, budget)
            .map_err(|_| super::wave_cfg::resource_error(budget))?;
        // Euler intervals in the dominator tree turn each later query into
        // two comparisons. Build iteratively to preserve deep-CFG safety.
        let mut children = vec![Vec::new(); f.blocks.len()];
        for (block, parent) in idom.iter().enumerate() {
            budget.charge()?;
            if let Some(parent) = *parent {
                if parent != block {
                    children[parent].push(block);
                }
            }
        }
        let mut intervals: Vec<Option<(usize, usize)>> = vec![None; f.blocks.len()];
        let mut stack = vec![(entry, false)];
        let mut clock = 0;
        while let Some((block, exiting)) = stack.pop() {
            budget.charge()?;
            if exiting {
                intervals[block].as_mut().unwrap().1 = clock;
                clock += 1;
            } else {
                intervals[block] = Some((clock, clock));
                clock += 1;
                stack.push((block, true));
                stack.extend(children[block].iter().rev().map(|&child| (child, false)));
            }
        }
        #[cfg(not(whale_wave_ir))]
        let predecessor_sets = predecessors.iter().map(|ps| ps.iter().copied().collect()).collect();
        Ok(Self {
            indices,
            predecessors,
            #[cfg(not(whale_wave_ir))]
            idom,
            intervals,
            #[cfg(not(whale_wave_ir))]
            predecessor_sets,
        })
    }

    #[cfg(not(whale_wave_ir))]
    fn dominates(
        &self,
        definition: usize,
        usage: usize,
        budget: &super::AnalysisBudget,
    ) -> Result<bool, VerifyError> {
        budget.charge()?;
        budget.queries.set(budget.queries.get() + 1);
        Ok(match (self.intervals[definition], self.intervals[usage]) {
            (Some((start, end)), Some((use_start, use_end))) => {
                start <= use_start && use_end <= end
            },
            _ => false,
        })
    }
}

pub(super) fn verify(
    f: &super::IndexedFunction<'_>,
    budget: &super::AnalysisBudget,
) -> Result<(), VerifyError> {
    let cfg = Cfg::new(f, budget)?;
    #[cfg(whale_wave_ir)]
    return super::wave_ssa::verify(f, &cfg, budget);
    #[cfg(not(whale_wave_ir))]
    {
        let mut definitions: HashMap<ValueId, Option<(usize, usize)>> =
            f.params.iter().map(|p| (p.id, None)).collect();
        for (block, b) in f.blocks.iter().enumerate() {
            for (position, ins) in b.instructions.iter().enumerate() {
                if let Some((value, _)) = instr_result(ins) {
                    definitions.insert(value, Some((block, position)));
                }
            }
        }
        let available = |value: ValueId, block: usize, position: usize| {
            let definition = definitions
                .get(&value)
                .ok_or_else(|| VerifyError::UseOfUndefinedValue { func: f.name.clone(), value })?;
            // Unreachable code remains in the module and still undergoes identity,
            // operand/type and CFG checks. No executable entry path exists on which
            // to require a dominating definition for its uses.
            if cfg.idom[block].is_none() {
                return Ok(());
            }
            if let Some((def_block, def_position)) = *definition {
                let valid = if def_block == block {
                    def_position < position
                } else {
                    cfg.dominates(def_block, block, budget)?
                };
                if !valid {
                    return Err(VerifyError::NonDominatingValue {
                        func: f.name.clone(),
                        value,
                        block: f.blocks[block].id,
                    });
                }
            }
            Ok(())
        };
        for (block, b) in f.blocks.iter().enumerate() {
            let mut past_phi = false;
            for (position, ins) in b.instructions.iter().enumerate() {
                budget.charge()?;
                if let Instruction::Phi { dst, ty, incomings } = ins {
                    let invalid = |reason| VerifyError::InvalidPhi {
                        func: f.name.clone(),
                        block: b.id,
                        value: *dst,
                        reason,
                    };
                    if past_phi {
                        return Err(invalid("phi must precede non-phi instructions"));
                    }
                    if b.id == f.entry {
                        return Err(invalid("function entry has no incoming SSA value"));
                    }
                    if incomings.is_empty() {
                        return Err(invalid("phi requires incoming values"));
                    }
                    operands::verify_type_category(f, "phi", ty, operands::is_storable(ty))?;
                    let mut seen = HashSet::new();
                    for (value, predecessor) in incomings {
                        budget.charge()?;
                        let Some(&pred) = cfg.indices.get(predecessor) else {
                            return Err(invalid("phi names an unknown predecessor"));
                        };
                        if !cfg.predecessor_sets[block].contains(&pred) {
                            return Err(invalid("phi input is not a predecessor"));
                        }
                        if !seen.insert(pred) {
                            return Err(invalid("duplicate phi predecessor"));
                        }
                        operands::verify_operand(f, *value, ty)?;
                        // A phi operand is used on its incoming edge, after that
                        // predecessor's instructions, including loop backedges.
                        available(*value, pred, f.blocks[pred].instructions.len())?;
                    }
                    if seen.len() != cfg.predecessors[block].len() {
                        return Err(invalid("phi is missing a predecessor"));
                    }
                } else {
                    past_phi = true;
                    for value in instr_uses(ins) {
                        available(value, block, position)?;
                    }
                }
            }
            let uses = match b.terminator.as_ref().unwrap() {
                Terminator::CBr { cond, .. } => Some(*cond),
                Terminator::Switch { value, .. } => Some(*value),
                Terminator::Ret { value, .. } => *value,
                Terminator::Br { .. } | Terminator::Trap { .. } => None,
            };
            if let Some(value) = uses {
                available(value, block, b.instructions.len())?;
            }
        }
        Ok(())
    }
}
