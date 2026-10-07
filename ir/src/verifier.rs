// SPDX-License-Identifier: MPL-2.0

use std::collections::{HashMap, HashSet};

use crate::{BlockId, ConstValue, Instruction, Module, Terminator, Type, ValueId};

mod calls;
mod cfg;
mod constants;
mod operands;

#[derive(Debug)]
pub enum VerifyError {
    InvalidCast {
        func: String,
        value: ValueId,
        op: crate::CastOp,
        source: Type,
        destination: Type,
    },
    Call {
        func: String,
        reason: crate::CallError,
    },
    Target(crate::TargetError),
    InvalidConstExpression {
        scope: String,
        declaration: crate::ConstRef,
        reason: crate::ConstEvalError,
    },
    NonDominatingValue {
        func: String,
        value: ValueId,
        block: BlockId,
    },
    InvalidPhi {
        func: String,
        block: BlockId,
        value: ValueId,
        reason: &'static str,
    },
    InvalidGep {
        func: String,
        value: ValueId,
        index: Option<usize>,
        reason: &'static str,
    },
    InvalidInstructionType {
        func: String,
        operation: &'static str,
        ty: Type,
    },
    OperandTypeMismatch {
        func: String,
        value: ValueId,
        expected: Type,
        got: Type,
    },
    InvalidMemoryAlignment {
        func: String,
        align: u32,
    },
    InvalidSwitchCase {
        func: String,
        block: BlockId,
        index: usize,
    },
    DuplicateSwitchCase {
        func: String,
        block: BlockId,
        index: usize,
    },
    DuplicateFunction {
        name: String,
    },
    DuplicateGlobal {
        name: String,
    },
    InvalidConstant {
        func: String,
        value: ValueId,
        ty: Type,
        payload: ConstValue,
    },
    InvalidGlobalInitializer {
        name: String,
        ty: Type,
        payload: ConstValue,
    },
    InvalidGlobalAlignment {
        name: String,
        align: u32,
    },
    ConditionTypeMismatch {
        func: String,
        value: ValueId,
        got: Type,
    },
    DuplicateBlock {
        func: String,
        block: BlockId,
    },
    DuplicateValue {
        func: String,
        value: ValueId,
    },
    DuplicateValueType {
        func: String,
        value: ValueId,
    },
    UnexpectedValueType {
        func: String,
        value: ValueId,
    },
    ValueTypeMismatch {
        func: String,
        value: ValueId,
        expected: Type,
        got: Type,
    },
    InvalidEntryBlock {
        func: String,
        entry: BlockId,
    },
    EntryHasPredecessor {
        func: String,
        entry: BlockId,
        predecessor: BlockId,
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
    let target = crate::Target::lookup(&m.target).map_err(VerifyError::Target)?;
    target
        .validate_layout(m.datalayout)
        .map_err(VerifyError::Target)?;
    let mut globals = HashSet::new();
    for g in &m.globals {
        if !globals.insert(&g.name) {
            return Err(VerifyError::DuplicateGlobal {
                name: g.name.clone(),
            });
        }
        if !crate::constant::valid_constant(&g.ty, &g.init) {
            return Err(VerifyError::InvalidGlobalInitializer {
                name: g.name.clone(),
                ty: g.ty.clone(),
                payload: g.init.clone(),
            });
        }
        if !g.align.is_power_of_two() {
            return Err(VerifyError::InvalidGlobalAlignment {
                name: g.name.clone(),
                align: g.align,
            });
        }
    }
    let constant_globals = constants::verify_globals(m)?;
    let mut functions = HashSet::new();
    for f in &m.functions {
        if !functions.insert(&f.name) {
            return Err(VerifyError::DuplicateFunction {
                name: f.name.clone(),
            });
        }
    }
    calls::verify_declarations(m)?;
    for f in &m.functions {
        let mut blocks = HashSet::new();
        for block in &f.blocks {
            if !blocks.insert(block.id) {
                return Err(VerifyError::DuplicateBlock {
                    func: f.name.clone(),
                    block: block.id,
                });
            }
        }
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
            if !defined.insert(p.id) {
                return Err(VerifyError::DuplicateValue {
                    func: f.name.clone(),
                    value: p.id,
                });
            }
        }
        // Collect identities before inspecting uses. Physical block storage
        // order does not describe execution order or loop backedges.
        for block in &f.blocks {
            for ins in &block.instructions {
                if let Some((dst, _)) = instr_result(ins) {
                    if !defined.insert(dst) {
                        return Err(VerifyError::DuplicateValue {
                            func: f.name.clone(),
                            value: dst,
                        });
                    }
                }
            }
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
                match ins {
                    Instruction::Const { dst, ty, value } => {
                        if !crate::constant::valid_constant(ty, value) {
                            return Err(VerifyError::InvalidConstant {
                                func: f.name.clone(),
                                value: *dst,
                                ty: ty.clone(),
                                payload: value.clone(),
                            });
                        }
                    }
                    Instruction::Select { cond, .. } | Instruction::TrapIf { cond, .. } => {
                        verify_condition(f, *cond)?;
                    }
                    _ => {}
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
                    verify_condition(f, *cond)?;
                }
                Terminator::Switch {
                    value, ty, cases, ..
                } => {
                    if !defined.contains(value) {
                        return Err(VerifyError::UseOfUndefinedValue {
                            func: f.name.clone(),
                            value: *value,
                        });
                    }
                    operands::verify_type_category(
                        f,
                        "switch",
                        ty,
                        operands::is_integer(ty) || *ty == Type::Bool,
                    )?;
                    operands::verify_operand(f, *value, ty)?;
                    let mut keys = HashSet::new();
                    for (index, (key, _)) in cases.iter().enumerate() {
                        if !crate::constant::valid_constant(ty, key) {
                            return Err(VerifyError::InvalidSwitchCase {
                                func: f.name.clone(),
                                block: b.id,
                                index,
                            });
                        }
                        let bits = match key {
                            ConstValue::I(value) => *value as u128,
                            ConstValue::U(value) => *value,
                            ConstValue::Bool(value) => u128::from(*value),
                            _ => unreachable!("validated integer key"),
                        };
                        if !keys.insert(bits) {
                            return Err(VerifyError::DuplicateSwitchCase {
                                func: f.name.clone(),
                                block: b.id,
                                index,
                            });
                        }
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
        verify_value_types(f)?;
        constants::verify_locals(f, &constant_globals)?;
        cfg::verify(f)?;
        // Operand checks consume the type table only after its definition/type
        // correspondence has been validated, so metadata corruption is not
        // misreported as a consumer's operand mismatch.
        for block in &f.blocks {
            for instruction in &block.instructions {
                calls::verify_instruction(m, f, instruction)?;
                operands::verify_instruction(f, instruction)?;
            }
        }
    }
    Ok(())
}

fn verify_condition(f: &crate::Function, value: ValueId) -> Result<(), VerifyError> {
    let ty = f
        .value_type(value)
        .ok_or_else(|| VerifyError::MissingValueType {
            func: f.name.clone(),
            value,
        })?;
    if ty != &Type::Bool {
        return Err(VerifyError::ConditionTypeMismatch {
            func: f.name.clone(),
            value,
            got: ty.clone(),
        });
    }
    Ok(())
}

fn verify_value_types(f: &crate::Function) -> Result<(), VerifyError> {
    let mut types = HashMap::new();
    for (value, ty) in &f.value_types {
        if types.insert(*value, ty).is_some() {
            return Err(VerifyError::DuplicateValueType {
                func: f.name.clone(),
                value: *value,
            });
        }
    }
    let definitions = f.params.iter().map(|p| (p.id, p.ty.clone())).chain(
        f.blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .filter_map(instr_result),
    );
    for (value, expected) in definitions {
        let got = types
            .remove(&value)
            .ok_or_else(|| VerifyError::MissingValueType {
                func: f.name.clone(),
                value,
            })?;
        if got != &expected {
            return Err(VerifyError::ValueTypeMismatch {
                func: f.name.clone(),
                value,
                expected,
                got: got.clone(),
            });
        }
    }
    // Iterate the source table rather than the hash map for deterministic diagnostics.
    for (value, _) in &f.value_types {
        if types.contains_key(value) {
            return Err(VerifyError::UnexpectedValueType {
                func: f.name.clone(),
                value: *value,
            });
        }
    }
    Ok(())
}

fn instr_result(ins: &Instruction) -> Option<(ValueId, Type)> {
    use Instruction::*;
    match ins {
        NullFunction { dst, signature } | FunctionAddr { dst, signature, .. } => {
            Some((*dst, Type::FnPtr(Box::new(signature.clone()))))
        }
        ConstDecl {
            dst, expression, ..
        } => Some((*dst, expression.ty.clone())),
        Const { dst, ty, .. }
        | Undef { dst, ty }
        | Mov { dst, ty, .. }
        | Bin { dst, ty, .. }
        | Not { dst, ty, .. }
        | Select { dst, ty, .. }
        | Phi { dst, ty, .. }
        | Load { dst, ty, .. } => Some((*dst, ty.clone())),
        Cmp { dst, .. } | ICmp { dst, .. } | FCmp { dst, .. } => Some((*dst, Type::Bool)),
        Cast { dst, dst_ty, .. } | Extract { dst, dst_ty, .. } | Gep { dst, dst_ty, .. } => {
            Some((*dst, dst_ty.clone()))
        }
        Checked { dst, ty, .. } => Some((*dst, Type::Tuple(vec![ty.clone(), Type::Bool]))),
        Alloca { dst, ty, .. } => Some((*dst, Type::ptr_to(ty.clone()))),

        Store { .. } | Memcpy { .. } | Memset { .. } | Call { dst: None, .. } | TrapIf { .. } => {
            None
        }

        Call {
            dst: Some(v),
            ret_ty,
            ..
        } => Some((*v, ret_ty.clone())),
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
        ConstDecl { expression, .. } => expression
            .references()
            .into_iter()
            .filter_map(|r| match r {
                crate::ConstRef::Local(value) => Some(value),
                crate::ConstRef::Global(_) => None,
            })
            .collect(),
        Const { .. } | Undef { .. } | NullFunction { .. } | FunctionAddr { .. } => vec![],

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

        Call { callee, args, .. } => {
            let mut uses = Vec::new();
            if let crate::Callee::Indirect(value) = callee {
                uses.push(*value);
            }
            uses.extend(args);
            uses
        }

        TrapIf { cond, .. } => vec![*cond],
    }
}
