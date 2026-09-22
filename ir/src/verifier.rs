// SPDX-License-Identifier: MPL-2.0

use std::collections::HashSet;

use crate::{BlockId, Instruction, Module, Terminator, Type, ValueId};

#[derive(Debug)]
pub enum VerifyError {
    InvalidEntryBlock {
        func: String,
        entry: BlockId,
    },
    InvalidBranchTarget {
        func: String,
        block: BlockId,
        target: BlockId,
    },
    MissingValueType {
        func: String,
        value: ValueId,
    },
    VoidParameter {
        func: String,
        param: String,
    },
    UnterminatedBlock {
        func: String,
        block: String,
    },
    RetTypeMismatch {
        func: String,
        expected: Type,
        got: Option<Type>,
    },
    UseOfUndefinedValue {
        func: String,
        value: ValueId,
    },
}

pub fn verify_module(m: &Module) -> Result<(), VerifyError> {
    for f in &m.functions {
        let blocks: HashSet<BlockId> = f.blocks.iter().map(|b| b.id).collect();
        if !blocks.contains(&f.entry) {
            return Err(VerifyError::InvalidEntryBlock {
                func: f.name.clone(),
                entry: f.entry,
            });
        }
        // value id set
        let mut defined = std::collections::HashSet::<ValueId>::new();
        for p in &f.params {
            if p.ty == Type::Void {
                return Err(VerifyError::VoidParameter {
                    func: f.name.clone(),
                    param: p.name.clone(),
                });
            }
            defined.insert(p.id);
        }

        for b in &f.blocks {
            let Some(terminator) = &b.terminator else {
                return Err(VerifyError::UnterminatedBlock {
                    func: f.name.clone(),
                    block: b.name.clone(),
                });
            };

            let check_target = |target: BlockId| {
                if blocks.contains(&target) {
                    Ok(())
                } else {
                    Err(VerifyError::InvalidBranchTarget {
                        func: f.name.clone(),
                        block: b.id,
                        target,
                    })
                }
            };
            match terminator {
                Terminator::Br { target } => check_target(*target)?,
                Terminator::CBr {
                    then_bb, else_bb, ..
                } => {
                    check_target(*then_bb)?;
                    check_target(*else_bb)?;
                }
                Terminator::Switch {
                    default_bb, cases, ..
                } => {
                    check_target(*default_bb)?;
                    for (_, target) in cases {
                        check_target(*target)?;
                    }
                }
                Terminator::Ret { .. } | Terminator::Trap { .. } => {}
            }

            for ins in &b.instructions {
                check_uses(ins, &defined, &f.name)?;
                if let Some(dst) = instr_def(ins) {
                    defined.insert(dst);
                }
            }

            // terminator uses
            match terminator {
                Terminator::Br { .. } => {}
                Terminator::CBr { cond, .. } => {
                    if !defined.contains(cond) {
                        return Err(VerifyError::UseOfUndefinedValue {
                            func: f.name.clone(),
                            value: *cond,
                        });
                    }
                }
                Terminator::Switch { value, .. } => {
                    if !defined.contains(value) {
                        return Err(VerifyError::UseOfUndefinedValue {
                            func: f.name.clone(),
                            value: *value,
                        });
                    }
                }
                Terminator::Trap { .. } => {}
                Terminator::Ret { ty, value } => {
                    if *ty != f.ret_ty {
                        return Err(VerifyError::RetTypeMismatch {
                            func: f.name.clone(),
                            expected: f.ret_ty.clone(),
                            got: Some(ty.clone()),
                        });
                    }
                    if let Some(v) = value {
                        if !defined.contains(v) {
                            return Err(VerifyError::UseOfUndefinedValue {
                                func: f.name.clone(),
                                value: *v,
                            });
                        }
                        let value_ty =
                            f.value_type(*v)
                                .ok_or_else(|| VerifyError::MissingValueType {
                                    func: f.name.clone(),
                                    value: *v,
                                })?;
                        if f.ret_ty == Type::Void || value_ty != &f.ret_ty {
                            return Err(VerifyError::RetTypeMismatch {
                                func: f.name.clone(),
                                expected: f.ret_ty.clone(),
                                got: Some(value_ty.clone()),
                            });
                        }
                    } else if f.ret_ty != Type::Void {
                        return Err(VerifyError::RetTypeMismatch {
                            func: f.name.clone(),
                            expected: f.ret_ty.clone(),
                            got: None,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn instr_def(ins: &Instruction) -> Option<ValueId> {
    use Instruction::*;
    match ins {
        Const { dst, .. }
        | Undef { dst, .. }
        | Mov { dst, .. }
        | Bin { dst, .. }
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
        | Load { dst, .. }
        | Gep { dst, .. } => Some(*dst),

        Store { .. } | Memcpy { .. } | Memset { .. } | Call { dst: None, .. } | TrapIf { .. } => {
            None
        }

        Call { dst: Some(v), .. } => Some(*v),
    }
}

fn check_uses(
    ins: &Instruction,
    defined: &std::collections::HashSet<ValueId>,
    func: &str,
) -> Result<(), VerifyError> {
    for u in instr_uses(ins) {
        if !defined.contains(&u) {
            return Err(VerifyError::UseOfUndefinedValue {
                func: func.to_string(),
                value: u,
            });
        }
    }
    Ok(())
}

fn instr_uses(ins: &Instruction) -> Vec<ValueId> {
    use Instruction::*;
    match ins {
        Const { .. } | Undef { .. } => vec![],

        Mov { src, .. } => vec![*src],

        Bin { lhs, rhs, .. } => vec![*lhs, *rhs],
        Not { src, .. } => vec![*src],

        Cmp { lhs, rhs, .. } => vec![*lhs, *rhs],
        ICmp { lhs, rhs, .. } => vec![*lhs, *rhs],
        FCmp { lhs, rhs, .. } => vec![*lhs, *rhs],

        Select {
            cond,
            on_true,
            on_false,
            ..
        } => vec![*cond, *on_true, *on_false],

        Cast { src, .. } => vec![*src],

        Phi { incomings, .. } => incomings.iter().map(|(v, _)| *v).collect(),

        Extract { tuple, .. } => vec![*tuple],

        Checked { lhs, rhs, .. } => vec![*lhs, *rhs],

        Alloca { .. } => vec![],
        Load { ptr, .. } => vec![*ptr],
        Store { value, ptr, .. } => vec![*value, *ptr],

        Gep {
            base_ptr, indices, ..
        } => {
            let mut v = vec![*base_ptr];
            v.extend(indices.iter().copied());
            v
        }

        Memcpy { dst, src, n, .. } => vec![*dst, *src, *n],
        Memset { dst, val, n, .. } => vec![*dst, *val, *n],

        Call { args, .. } => args.clone(),

        TrapIf { cond, .. } => vec![*cond],
    }
}
