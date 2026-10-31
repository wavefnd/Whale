// SPDX-License-Identifier: MPL-2.0

#[cfg(not(whale_wave_ir))]
use std::collections::HashSet;

use crate::{BlockId, ConstValue, Instruction, Module, Terminator, Type, ValueId};

mod calls;

mod cfg;

#[cfg(whale_wave_ir)]
mod wave_cfg;

#[cfg(whale_wave_ir)]
mod wave_shape;

#[cfg(whale_wave_ir)]
pub(crate) mod wave_semantics;

#[cfg(whale_wave_ir)]
mod wave_ssa;

#[cfg(whale_wave_ir)]
mod wave_index;

mod constants;

mod index;

mod operands;

use index::IndexedFunction;

use std::cell::Cell;

/// Work counters for reproducible verification scaling checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VerificationStats {
    pub value_type_lookups: usize,
    pub declaration_work: usize,
    pub dominance_queries: usize,
    pub analysis_work: usize,
}

struct AnalysisBudget {
    limit: usize,
    work: Cell<usize>,
    queries: Cell<usize>,
}

impl AnalysisBudget {
    fn charge(&self) -> Result<(), VerifyError> {
        let work = self.work.get();
        if work >= self.limit {
            return Err(VerifyError::ResourceLimit(crate::LimitError {
                resource: crate::Resource::AnalysisWork,
                limit: self.limit,
            }));
        }
        self.work.set(work + 1);
        Ok(())
    }
}

#[derive(Debug)]
pub enum VerifyError {
    GlobalStorage {
        name: String,
        global: crate::GlobalId,
        reason: crate::GlobalStorageError,
    },
    SourceMetadata(crate::SourceMetadataError),
    ResourceLimit(crate::LimitError),
    ForbiddenUndef {
        func: String,
        value: ValueId,
    },
    InvalidMemoryLayout {
        func: String,
        operation: &'static str,
        reason: crate::LayoutError,
    },
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
    verify_module_with_limits(m, crate::IrLimits::default())
}

/// Shared internal type comparison for parser annotation consistency.
#[cfg(whale_wave_ir)]
pub(crate) fn wave_types_equal(
    left: &Type,
    right: &Type,
    max_work: usize,
) -> Result<(bool, usize), crate::LimitError> {
    let budget = AnalysisBudget { limit: max_work, work: Cell::new(0), queries: Cell::new(0) };
    match wave_semantics::types_equal(left, right, &budget) {
        Ok(equal) => Ok((equal, budget.work.get())),
        Err(_) => {
            Err(crate::LimitError { resource: crate::Resource::AnalysisWork, limit: max_work })
        },
    }
}

pub fn verify_module_with_limits(m: &Module, limits: crate::IrLimits) -> Result<(), VerifyError> {
    verify_module_with_stats(m, limits).map(|_| ())
}

/// Verify immutable IR and report lookup/CFG work without changing its order.
pub fn verify_module_with_stats(
    m: &Module,
    limits: crate::IrLimits,
) -> Result<VerificationStats, VerifyError> {
    let target = crate::Target::lookup(&m.target).map_err(VerifyError::Target)?;
    target.validate_layout(m.datalayout).map_err(VerifyError::Target)?;
    verify_module_for_target_with_stats(m, &target.specification(), limits)
}

/// Verify against an explicitly supplied target contract; no global registry is used.
pub fn verify_module_for_target(
    m: &Module,
    target: &target_spec::TargetSpec,
    limits: crate::IrLimits,
) -> Result<(), VerifyError> {
    verify_module_for_target_with_stats(m, target, limits).map(|_| ())
}

pub fn verify_module_for_target_with_stats(
    m: &Module,
    target: &target_spec::TargetSpec,
    limits: crate::IrLimits,
) -> Result<VerificationStats, VerifyError> {
    crate::limits::check_module(m, limits).map_err(VerifyError::ResourceLimit)?;
    #[cfg(whale_wave_ir)]
    let source_work = crate::source_wave::validate(&m.sources, m, limits.max_analysis_work)
        .map_err(|error| match error {
            crate::source_wave::ValidationError::Invalid(error) => {
                VerifyError::SourceMetadata(error)
            },
            crate::source_wave::ValidationError::Work => {
                VerifyError::ResourceLimit(crate::LimitError {
                    resource: crate::Resource::AnalysisWork,
                    limit: limits.max_analysis_work,
                })
            },
        })?;
    #[cfg(not(whale_wave_ir))]
    m.sources.validate(m).map_err(VerifyError::SourceMetadata)?;
    target.validate().map_err(|e| VerifyError::Target(crate::TargetError::Contract(e)))?;
    if m.target != target.id || m.datalayout != target.storage.data {
        return Err(VerifyError::Target(crate::TargetError::Contract(format!(
            "module target/layout does not match {}",
            target.id
        ))));
    }
    let indexed: Vec<_> = m.functions.iter().map(IndexedFunction::new).collect();
    let analysis = AnalysisBudget {
        limit: limits.max_analysis_work,
        work: Cell::new({
            #[cfg(whale_wave_ir)]
            {
                source_work
            }
            #[cfg(not(whale_wave_ir))]
            {
                0
            }
        }),
        queries: Cell::new(0),
    };
    for f in &indexed {
        for block in &f.blocks {
            for ins in &block.instructions {
                if let Instruction::Undef { dst, .. } = ins {
                    return Err(VerifyError::ForbiddenUndef { func: f.name.clone(), value: *dst });
                }
                let storage = match ins {
                    Instruction::Alloca { ty, .. } => Some(("alloca", ty)),
                    Instruction::VolatileLoad { ty, .. } | Instruction::Load { ty, .. } => {
                        Some(("load", ty))
                    },
                    Instruction::VolatileStore { ty, .. } | Instruction::Store { ty, .. } => {
                        Some(("store", ty))
                    },
                    Instruction::Uninit { ty, .. } => Some(("uninit", ty)),
                    Instruction::Gep { base_ptr, indices, .. } if !indices.is_empty() => {
                        match f.value_type(*base_ptr) {
                            Some(Type::Ptr(ty)) => Some(("gep", &**ty)),
                            _ => None,
                        }
                    },
                    _ => None,
                };
                if let Some((operation, ty)) = storage {
                    // Category errors keep their existing diagnostics below.
                    #[cfg(whale_wave_ir)]
                    let storable = wave_semantics::storable(ty, &analysis)?;
                    #[cfg(not(whale_wave_ir))]
                    let storable = operands::is_storable(ty);
                    if storable {
                        crate::layout_for_target(ty, target, limits.max_type_depth).map_err(
                            |reason| VerifyError::InvalidMemoryLayout {
                                func: f.name.clone(),
                                operation,
                                reason,
                            },
                        )?;
                    }
                }
            }
        }
    }
    #[cfg(whale_wave_ir)]
    let mut globals = wave_index::Index::new(m.globals.len() + m.global_storage.len(), &analysis)?;
    #[cfg(not(whale_wave_ir))]
    let mut globals = HashSet::new();
    for g in &m.globals {
        #[cfg(whale_wave_ir)]
        let inserted = globals.name(&g.name, &analysis)?;
        #[cfg(not(whale_wave_ir))]
        let inserted = globals.insert(&g.name);
        if !inserted {
            return Err(VerifyError::DuplicateGlobal { name: g.name.clone() });
        }
        #[cfg(whale_wave_ir)]
        let valid = wave_semantics::literal_valid(&g.ty, &g.init, &analysis)?;
        #[cfg(not(whale_wave_ir))]
        let valid = crate::constant::valid_constant(&g.ty, &g.init);
        if !valid {
            return Err(VerifyError::InvalidGlobalInitializer {
                name: g.name.clone(),
                ty: g.ty.clone(),
                payload: g.init.clone(),
            });
        }
        #[cfg(whale_wave_ir)]
        let aligned = wave_semantics::alignment(g.align, &analysis)?;
        #[cfg(not(whale_wave_ir))]
        let aligned = g.align.is_power_of_two();
        if !aligned {
            return Err(VerifyError::InvalidGlobalAlignment {
                name: g.name.clone(),
                align: g.align,
            });
        }
    }
    #[cfg(whale_wave_ir)]
    let mut global_ids = {
        let mut ids = wave_index::Index::new(m.globals.len() + m.global_storage.len(), &analysis)?;
        for global in &m.globals {
            ids.id(global.id.0, &analysis)?;
        }
        ids
    };
    #[cfg(not(whale_wave_ir))]
    let mut global_ids: HashSet<_> = m.globals.iter().map(|g| g.id).collect();
    #[cfg(whale_wave_ir)]
    let mut links = {
        let mut links =
            wave_index::Index::new(m.declarations.len() + m.global_storage.len(), &analysis)?;
        for declaration in &m.declarations {
            if let Some(link) = &declaration.link_name {
                links.name(link, &analysis)?;
            }
        }
        links
    };
    #[cfg(not(whale_wave_ir))]
    let mut links: HashSet<_> =
        m.declarations.iter().filter_map(|d| d.link_name.as_deref()).collect();
    let mut storage_types = std::collections::HashMap::new();
    for g in &m.global_storage {
        let error =
            |reason| VerifyError::GlobalStorage { name: g.name.clone(), global: g.id, reason };
        #[cfg(whale_wave_ir)]
        wave_semantics::global_storage(g, &analysis)?;
        #[cfg(not(whale_wave_ir))]
        g.validate_with_limits(limits).map_err(error)?;
        #[cfg(whale_wave_ir)]
        let inserted = global_ids.id(g.id.0, &analysis)?;
        #[cfg(not(whale_wave_ir))]
        let inserted = global_ids.insert(g.id);
        if !inserted {
            return Err(error(crate::GlobalStorageError::DuplicateIdentity));
        }
        #[cfg(whale_wave_ir)]
        let inserted = globals.name(&g.name, &analysis)?;
        #[cfg(not(whale_wave_ir))]
        let inserted = globals.insert(&g.name);
        if !inserted {
            return Err(error(crate::GlobalStorageError::DuplicateName));
        }
        #[cfg(whale_wave_ir)]
        let link_collision = match g.link_name.as_deref() {
            Some(link) => !links.name(link, &analysis)?,
            None => false,
        };
        #[cfg(not(whale_wave_ir))]
        let link_collision = g.link_name.as_deref().is_some_and(|link| !links.insert(link));
        if link_collision {
            return Err(error(crate::GlobalStorageError::LinkNameCollision));
        }
        crate::layout_for_target(&g.ty, target, limits.max_type_depth).map_err(|reason| {
            VerifyError::InvalidMemoryLayout {
                func: g.name.clone(),
                operation: "global storage",
                reason,
            }
        })?;
        storage_types.insert(g.id, &g.ty);
    }
    let constant_globals = constants::verify_globals(m, limits)?;
    #[cfg(whale_wave_ir)]
    let mut functions = wave_index::Index::new(m.functions.len(), &analysis)?;
    #[cfg(not(whale_wave_ir))]
    let mut functions = HashSet::new();
    for f in &indexed {
        #[cfg(whale_wave_ir)]
        let inserted = functions.name(&f.name, &analysis)?;
        #[cfg(not(whale_wave_ir))]
        let inserted = functions.insert(&f.name);
        if !inserted {
            return Err(VerifyError::DuplicateFunction { name: f.name.clone() });
        }
    }
    let declarations = calls::verify_declarations(m, limits, &analysis)?;
    for f in &indexed {
        #[cfg(whale_wave_ir)]
        let defined = wave_shape::verify(f, &analysis)?;
        #[cfg(not(whale_wave_ir))]
        let (blocks, defined) = {
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
                    return Err(VerifyError::DuplicateValue { func: f.name.clone(), value: p.id });
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
            (blocks, defined)
        };

        for b in &f.blocks {
            let Some(terminator) = &b.terminator else {
                return Err(VerifyError::UnterminatedBlock {
                    func: f.name.clone(),
                    block: b.name.clone(),
                });
            };

            #[cfg(not(whale_wave_ir))]
            {
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
                    Terminator::CBr { then_bb, else_bb, .. } => {
                        check_target(*then_bb)?;
                        check_target(*else_bb)?;
                    },
                    Terminator::Switch { default_bb, cases, .. } => {
                        check_target(*default_bb)?;
                        for (_, target) in cases {
                            check_target(*target)?;
                        }
                    },
                    Terminator::Ret { .. } | Terminator::Trap { .. } => {},
                }
            }

            for ins in &b.instructions {
                check_uses(ins, &defined, &f.name, &analysis)?;
                match ins {
                    Instruction::Const { dst, ty, value } => {
                        #[cfg(whale_wave_ir)]
                        let valid = wave_semantics::literal_valid(ty, value, &analysis)?;
                        #[cfg(not(whale_wave_ir))]
                        let valid = crate::constant::valid_constant(ty, value);
                        if !valid {
                            return Err(VerifyError::InvalidConstant {
                                func: f.name.clone(),
                                value: *dst,
                                ty: ty.clone(),
                                payload: value.clone(),
                            });
                        }
                    },
                    Instruction::Select { cond, .. } | Instruction::TrapIf { cond, .. } => {
                        verify_condition(f, *cond, &analysis)?;
                    },
                    _ => {},
                }
            }

            // terminator uses
            match terminator {
                Terminator::Br { .. } => {},
                Terminator::CBr { cond, .. } => {
                    if !is_defined(&defined, *cond, &analysis)? {
                        return Err(VerifyError::UseOfUndefinedValue {
                            func: f.name.clone(),
                            value: *cond,
                        });
                    }
                    verify_condition(f, *cond, &analysis)?;
                },
                Terminator::Switch { value, ty, cases, .. } => {
                    if !is_defined(&defined, *value, &analysis)? {
                        return Err(VerifyError::UseOfUndefinedValue {
                            func: f.name.clone(),
                            value: *value,
                        });
                    }
                    #[cfg(whale_wave_ir)]
                    wave_semantics::switch(f, *value, ty, &analysis)?;
                    #[cfg(not(whale_wave_ir))]
                    operands::verify_type_category(
                        f,
                        "switch",
                        ty,
                        operands::is_integer(ty) || *ty == Type::Bool,
                    )?;
                    #[cfg(not(whale_wave_ir))]
                    operands::verify_operand(f, *value, ty)?;
                    #[cfg(whale_wave_ir)]
                    let mut keys = wave_index::Index::new(cases.len(), &analysis)?;
                    #[cfg(not(whale_wave_ir))]
                    let mut keys = HashSet::new();
                    for (index, (key, _)) in cases.iter().enumerate() {
                        #[cfg(whale_wave_ir)]
                        let valid = wave_semantics::literal_valid(ty, key, &analysis)?;
                        #[cfg(not(whale_wave_ir))]
                        let valid = crate::constant::valid_constant(ty, key);
                        if !valid {
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
                        #[cfg(whale_wave_ir)]
                        let inserted = keys.bits(bits, &analysis)?;
                        #[cfg(not(whale_wave_ir))]
                        let inserted = keys.insert(bits);
                        if !inserted {
                            return Err(VerifyError::DuplicateSwitchCase {
                                func: f.name.clone(),
                                block: b.id,
                                index,
                            });
                        }
                    }
                },
                Terminator::Trap { .. } => {},
                Terminator::Ret { ty, value } => {
                    #[cfg(whale_wave_ir)]
                    {
                        let present = match value {
                            Some(value) => is_defined(&defined, *value, &analysis)?,
                            None => true,
                        };
                        wave_semantics::return_value(f, ty, *value, present, &analysis)?;
                    }
                    #[cfg(not(whale_wave_ir))]
                    {
                        if *ty != f.ret_ty {
                            return Err(VerifyError::RetTypeMismatch {
                                func: f.name.clone(),
                                expected: f.ret_ty.clone(),
                                got: Some(ty.clone()),
                            });
                        }
                        if let Some(v) = value {
                            if !is_defined(&defined, *v, &analysis)? {
                                return Err(VerifyError::UseOfUndefinedValue {
                                    func: f.name.clone(),
                                    value: *v,
                                });
                            }
                            let value_ty = f.value_type(*v).ok_or_else(|| {
                                VerifyError::MissingValueType { func: f.name.clone(), value: *v }
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
                },
            }
        }
        #[cfg(whale_wave_ir)]
        wave_semantics::value_types(f, &analysis)?;
        #[cfg(not(whale_wave_ir))]
        verify_value_types(f)?;
        constants::verify_locals(f, &constant_globals, limits)?;
        cfg::verify(f, &analysis)?;
        // Operand checks consume the type table only after its definition/type
        // correspondence has been validated, so metadata corruption is not
        // misreported as a consumer's operand mismatch.
        for block in &f.blocks {
            for instruction in &block.instructions {
                if let Instruction::GlobalAddr { global, ty, .. } = instruction {
                    #[cfg(whale_wave_ir)]
                    let reason = match storage_types.get(global) {
                        None => Some(crate::GlobalStorageError::UnknownGlobal),
                        Some(declared) => {
                            if wave_semantics::types_equal(declared, ty, &analysis)? {
                                None
                            } else {
                                Some(crate::GlobalStorageError::AddressTypeMismatch)
                            }
                        },
                    };
                    #[cfg(not(whale_wave_ir))]
                    let reason = match storage_types.get(global) {
                        None => Some(crate::GlobalStorageError::UnknownGlobal),
                        Some(declared) if *declared != ty => {
                            Some(crate::GlobalStorageError::AddressTypeMismatch)
                        },
                        _ => None,
                    };
                    if let Some(reason) = reason {
                        return Err(VerifyError::GlobalStorage {
                            name: f.name.clone(),
                            global: *global,
                            reason,
                        });
                    }
                }
                calls::verify_instruction(&declarations, f, instruction, limits, &analysis)?;
                operands::verify_instruction(f, instruction, &analysis)?;
            }
        }
    }
    Ok(VerificationStats {
        value_type_lookups: indexed.iter().map(|f| f.queries.get()).sum(),
        declaration_work: declarations.work(),
        dominance_queries: analysis.queries.get(),
        analysis_work: analysis.work.get(),
    })
}

#[cfg(whale_wave_ir)]
fn verify_condition(
    f: &IndexedFunction<'_>,
    value: ValueId,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    wave_semantics::condition(f, value, budget)
}

#[cfg(not(whale_wave_ir))]
fn verify_condition(
    f: &IndexedFunction<'_>,
    value: ValueId,
    _budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let ty = f
        .value_type(value)
        .ok_or_else(|| VerifyError::MissingValueType { func: f.name.clone(), value })?;
    if ty != &Type::Bool {
        return Err(VerifyError::ConditionTypeMismatch {
            func: f.name.clone(),
            value,
            got: ty.clone(),
        });
    }
    Ok(())
}

#[cfg(not(whale_wave_ir))]
fn verify_value_types(f: &IndexedFunction<'_>) -> Result<(), VerifyError> {
    let mut seen = HashSet::new();
    for (value, _) in &f.value_types {
        if !seen.insert(*value) {
            return Err(VerifyError::DuplicateValueType { func: f.name.clone(), value: *value });
        }
    }
    let definitions = f
        .params
        .iter()
        .map(|p| (p.id, p.ty.clone()))
        .chain(f.blocks.iter().flat_map(|b| b.instructions.iter()).filter_map(instr_result));
    for (value, expected) in definitions {
        let got = f
            .value_type(value)
            .ok_or_else(|| VerifyError::MissingValueType { func: f.name.clone(), value })?;
        if got != &expected {
            return Err(VerifyError::ValueTypeMismatch {
                func: f.name.clone(),
                value,
                expected,
                got: got.clone(),
            });
        }
        seen.remove(&value);
    }
    for (value, _) in &f.value_types {
        if seen.contains(value) {
            return Err(VerifyError::UnexpectedValueType { func: f.name.clone(), value: *value });
        }
    }
    Ok(())
}

fn instr_result(ins: &Instruction) -> Option<(ValueId, Type)> {
    use Instruction::*;
    match ins {
        NullFunction { dst, signature } | FunctionAddr { dst, signature, .. } => {
            Some((*dst, Type::FnPtr(Box::new(signature.clone()))))
        },
        ConstDecl { dst, expression, .. } => Some((*dst, expression.ty.clone())),
        Const { dst, ty, .. }
        | Undef { dst, ty }
        | Mov { dst, ty, .. }
        | Bin { dst, ty, .. }
        | Neg { dst, ty, .. }
        | Not { dst, ty, .. }
        | Select { dst, ty, .. }
        | Phi { dst, ty, .. }
        | VolatileLoad { dst, ty, .. }
        | Load { dst, ty, .. } => Some((*dst, ty.clone())),
        Cmp { dst, .. } | ICmp { dst, .. } | FCmp { dst, .. } => Some((*dst, Type::Bool)),
        Cast { dst, dst_ty, .. } | Extract { dst, dst_ty, .. } | Gep { dst, dst_ty, .. } => {
            Some((*dst, dst_ty.clone()))
        },
        Checked { dst, ty, .. } => Some((*dst, Type::Tuple(vec![ty.clone(), Type::Bool]))),
        Alloca { dst, ty, .. } | GlobalAddr { dst, ty, .. } => {
            Some((*dst, Type::ptr_to(ty.clone())))
        },
        PtrDiff { dst, .. } => Some((*dst, Type::I128)),

        LifetimeEnd { .. }
        | Uninit { .. }
        | VolatileStore { .. }
        | Store { .. }
        | Memcpy { .. }
        | Memset { .. }
        | Call { dst: None, .. }
        | TrapIf { .. } => None,

        Call { dst: Some(v), ret_ty, .. } => Some((*v, ret_ty.clone())),
    }
}

#[cfg(whale_wave_ir)]
fn is_defined(
    defined: &wave_shape::Definitions,
    value: ValueId,
    budget: &AnalysisBudget,
) -> Result<bool, VerifyError> {
    defined.contains(value, budget)
}

#[cfg(not(whale_wave_ir))]
fn is_defined(
    defined: &std::collections::HashSet<ValueId>,
    value: ValueId,
    _budget: &AnalysisBudget,
) -> Result<bool, VerifyError> {
    Ok(defined.contains(&value))
}

#[cfg(whale_wave_ir)]
fn check_uses(
    ins: &Instruction,
    defined: &wave_shape::Definitions,
    func: &str,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    for value in instr_uses(ins) {
        if !defined.contains(value, budget)? {
            return Err(VerifyError::UseOfUndefinedValue { func: func.into(), value });
        }
    }
    Ok(())
}

#[cfg(not(whale_wave_ir))]
fn check_uses(
    ins: &Instruction,
    defined: &std::collections::HashSet<ValueId>,
    func: &str,
    _budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    for u in instr_uses(ins) {
        if !defined.contains(&u) {
            return Err(VerifyError::UseOfUndefinedValue { func: func.to_string(), value: u });
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
        GlobalAddr { .. }
        | Const { .. }
        | Undef { .. }
        | NullFunction { .. }
        | FunctionAddr { .. } => vec![],

        Mov { src, .. } => vec![*src],

        Bin { lhs, rhs, .. } | PtrDiff { lhs, rhs, .. } => vec![*lhs, *rhs],
        Neg { src, .. } | Not { src, .. } => vec![*src],

        Cmp { lhs, rhs, .. } => vec![*lhs, *rhs],
        ICmp { lhs, rhs, .. } => vec![*lhs, *rhs],
        FCmp { lhs, rhs, .. } => vec![*lhs, *rhs],

        Select { cond, on_true, on_false, .. } => vec![*cond, *on_true, *on_false],

        Cast { src, .. } => vec![*src],

        Phi { incomings, .. } => incomings.iter().map(|(v, _)| *v).collect(),

        Extract { tuple, .. } => vec![*tuple],

        Checked { lhs, rhs, .. } => vec![*lhs, *rhs],

        Alloca { .. } => vec![],
        VolatileLoad { ptr, .. } | Load { ptr, .. } | Uninit { ptr, .. } | LifetimeEnd { ptr } => {
            vec![*ptr]
        },
        VolatileStore { value, ptr, .. } | Store { value, ptr, .. } => vec![*value, *ptr],

        Gep { base_ptr, indices, .. } => {
            let mut v = vec![*base_ptr];
            v.extend(indices.iter().copied());
            v
        },

        Memcpy { dst, src, n, .. } => vec![*dst, *src, *n],
        Memset { dst, val, n, .. } => vec![*dst, *val, *n],

        Call { callee, args, .. } => {
            let mut uses = Vec::new();
            if let crate::Callee::Indirect(value) = callee {
                uses.push(*value);
            }
            uses.extend(args);
            uses
        },

        TrapIf { cond, .. } => vec![*cond],
    }
}
