// SPDX-License-Identifier: MPL-2.0

use crate::{Instruction, Module, Terminator, Type, ValueId};

#[derive(Debug)]
pub enum VerifyError {
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
        // value id set
        let mut defined = std::collections::HashSet::<ValueId>::new();
        for p in &f.params {
            defined.insert(p.id);
        }

        for b in &f.blocks {
            if b.terminator.is_none() {
                return Err(VerifyError::UnterminatedBlock {
                    func: f.name.clone(),
                    block: b.name.clone(),
                });
            }

            for ins in &b.instructions {
                check_uses(ins, &defined, &f.name)?;
                if let Some(dst) = instr_def(ins) {
                    defined.insert(dst);
                }
            }

            // terminator uses
            match b.terminator.as_ref().unwrap() {
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
