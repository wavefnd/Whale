// SPDX-License-Identifier: MPL-2.0

use crate::{
    BasicBlock, BinOp, BlockId, Callee, CheckedOp, CmpOp, ConstValue, DataLayout, Function, Global,
    ICmpPred, Instruction, Module, Param, Terminator, Type, ValueId,
};

#[cfg(whale_wave_ir)]
mod body;

#[cfg(whale_wave_ir)]
mod namespace;

#[cfg(whale_wave_ir)]
mod wave;

/// Construction-order errors returned by the `try_*` builder operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuilderError {
    UnknownBlock(BlockId),
    UnknownParameter { index: usize, count: usize },
    UnknownValue(ValueId),
    UnknownGlobal(crate::GlobalId),
    InvalidPointerOperands,
    TerminatedBlock(BlockId),
    PhiAfterInstruction(BlockId),
    ResourceLimit(crate::LimitError),
    IdentityOverflow,
    InvalidType,
}

impl std::fmt::Display for BuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResourceLimit(error) => error.fmt(f),
            Self::IdentityOverflow => f.write_str("builder identity space exhausted"),
            Self::InvalidType => f.write_str("invalid typed builder model"),
            Self::UnknownBlock(id) => write!(f, "unknown builder block {id:?}"),
            Self::UnknownParameter { index, count } => {
                write!(f, "parameter {index} is outside 0..{count}")
            },
            Self::UnknownValue(id) => write!(f, "unknown builder value {id:?}"),
            Self::UnknownGlobal(id) => write!(f, "unknown addressable global {id:?}"),
            Self::InvalidPointerOperands => f.write_str("matching data-pointer operands required"),
            Self::TerminatedBlock(id) => write!(f, "builder block {id:?} is already terminated"),
            Self::PhiAfterInstruction(id) => {
                write!(f, "phi must precede other instructions in {id:?}")
            },
        }
    }
}

impl std::error::Error for BuilderError {}

pub struct ModuleBuilder {
    module: Module,
    next_value: u32,
    next_block: u32,
    next_global: u32,
    #[cfg(not(whale_wave_ir))]
    limits: crate::IrLimits,
    #[cfg(whale_wave_ir)]
    wave: wave::Builder,
    #[cfg(feature = "socket")]
    source_work: usize,
    #[cfg(feature = "socket")]
    lower_storage_budget: crate::limits::Budget,
    source_shifts: std::collections::HashMap<crate::FunctionId, (usize, usize)>,
    source_mapped_functions: std::collections::HashSet<crate::FunctionId>,
}

#[cfg(feature = "socket")]
impl FunctionBuilder<'_> {
    pub(crate) fn charge_lower_type(&mut self, ty: &Type) -> Result<(), crate::LowerError> {
        // Bound repeated aggregate-type cloning and emitted storage annotations.
        // This is shared across all functions in one lowering invocation.
        for _ in 0..16 {
            self.mb
                .lower_storage_budget
                .ty(ty)
                .map_err(|_| crate::LowerError::ResourceLimit(crate::AstLimitError::Expansion))?;
        }
        Ok(())
    }
}

impl<'a> FunctionBuilder<'a> {
    pub fn try_global_addr(&mut self, global: crate::GlobalId) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        let ty = &self
            .mb
            .module
            .global_storage
            .iter()
            .find(|g| g.id == global)
            .ok_or(BuilderError::UnknownGlobal(global))?
            .ty;
        self.mb.preflight_type(ty)?;
        let pointer = Type::ptr_to(ty.clone());
        self.preflight_value(&pointer, 0, 0)?;
        let ty = ty.clone();
        let dst = self.define_value(&pointer);
        self.push_instruction(Instruction::GlobalAddr { dst, global, ty });
        Ok(dst)
    }

    pub fn try_lifetime_end(&mut self, ptr: ValueId) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.preflight_instruction(1, 0)?;
        self.ensure_values(&[ptr])?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.pointer(self.first_value, ptr, None)?;
        #[cfg(not(whale_wave_ir))]
        if !matches!(self.func.value_type(ptr), Some(Type::Ptr(_))) {
            return Err(BuilderError::InvalidPointerOperands);
        }
        self.push_instruction(Instruction::LifetimeEnd { ptr });
        Ok(())
    }

    pub fn try_ptrdiff(&mut self, lhs: ValueId, rhs: ValueId) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.preflight_value(&Type::I128, 2, 0)?;
        self.ensure_values(&[lhs, rhs])?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.pointer(self.first_value, lhs, Some(rhs))?;
        #[cfg(not(whale_wave_ir))]
        let ty = self.func.value_type(lhs).unwrap();
        #[cfg(not(whale_wave_ir))]
        if !matches!(ty, Type::Ptr(_)) || self.func.value_type(rhs) != Some(ty) {
            return Err(BuilderError::InvalidPointerOperands);
        }
        let dst = self.define_value(&Type::I128);
        self.push_instruction(Instruction::PtrDiff { dst, lhs, rhs });
        Ok(dst)
    }

    pub fn current_block_id(&self) -> BlockId {
        self.insert_block
    }

    pub fn try_not(&mut self, ty: Type, src: ValueId) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.ensure_values(&[src])?;
        self.preflight_value(&ty, 1, 0)?;
        Ok(self.not(ty, src))
    }

    pub fn neg(&mut self, ty: Type, src: ValueId) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Neg { dst, ty, src });
        dst
    }

    pub fn not(&mut self, ty: Type, src: ValueId) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Not { dst, ty, src });
        dst
    }

    /// Append a phi at a block's instruction prefix. Inputs are checked for
    /// existence; the verifier checks predecessor edges, dominance and types.
    pub fn try_phi(
        &mut self,
        ty: Type,
        incomings: Vec<(ValueId, BlockId)>,
    ) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        for (value, block) in &incomings {
            self.ensure_values(&[*value])?;
            self.ensure_block(*block)?;
        }
        #[cfg(whale_wave_ir)]
        self.mb.wave.order(
            self.first_block,
            self.first_value,
            self.insert_block,
            15,
            &[],
            &[],
            true,
        )?;
        #[cfg(not(whale_wave_ir))]
        if self._cur_block().instructions.iter().any(|i| !matches!(i, Instruction::Phi { .. })) {
            return Err(BuilderError::PhiAfterInstruction(self.insert_block));
        }
        self.mb.preflight_type(&ty)?;
        self.preflight_value(&ty, incomings.len(), incomings.len())?;
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Phi { dst, ty, incomings });
        Ok(dst)
    }

    pub fn entry_block(&self) -> BlockId {
        self.func.entry
    }

    /// Legacy diagnostic construction; verification rejects this instruction.
    pub fn undef(&mut self, ty: Type) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Undef { dst, ty });
        dst
    }

    #[cfg(not(whale_wave_ir))]
    fn _cur_block(&self) -> &BasicBlock {
        self.func.blocks.iter().find(|b| b.id == self.insert_block).expect("insert block not found")
    }

    pub fn is_current_block_terminated(&self) -> bool {
        self.func
            .blocks
            .iter()
            .find(|b| b.id == self.insert_block)
            .map(|b| b.terminator.is_some())
            .unwrap_or(false)
    }

    pub fn alloca_in_entry(&mut self, ty: Type, align: u32) -> ValueId {
        self.mb.preflight_type(&ty).expect("invalid builder type");
        let dst = self.define_value(&Type::Ptr(Box::new(ty.clone())));
        let entry = self.func.entry;
        let instruction = Instruction::Alloca { dst, ty, align };
        #[cfg(whale_wave_ir)]
        let pos = {
            self.mb
                .wave
                .order(self.first_block, self.first_value, entry, 36, &[], &[], true)
                .expect("invalid builder entry instruction");
            self.mb
                .wave
                .body
                .instruction(entry, &instruction, true, false)
                .expect("invalid Wave entry allocation body")
        };
        let entry_block =
            self.func.blocks.iter_mut().find(|b| b.id == entry).expect("entry block not found");
        #[cfg(not(whale_wave_ir))]
        let pos = entry_block
            .instructions
            .iter()
            .position(|ins| !matches!(ins, Instruction::Alloca { .. }))
            .unwrap_or(entry_block.instructions.len());

        #[cfg(not(whale_wave_ir))]
        entry_block.instructions.insert(pos, instruction);
        #[cfg(whale_wave_ir)]
        entry_block.instructions.push(instruction);
        #[cfg(whale_wave_ir)]
        self.mb
            .wave
            .mark(self.first_block, self.first_value, entry, 36, &[], &[])
            .expect("invalid builder entry instruction");

        if self.mb.source_mapped_functions.contains(&self.func.id) {
            self.mb
                .source_shifts
                .entry(self.func.id)
                .and_modify(|(_, count)| *count += 1)
                .or_insert((pos, 1));
        }

        dst
    }
}

impl ModuleBuilder {
    pub fn new(target: impl Into<String>, datalayout: DataLayout) -> Self {
        Self::with_limits(target, datalayout, crate::IrLimits::default())
            .expect("valid builder limits")
    }

    /// Construct with explicit limits for typed construction.
    pub fn with_limits(
        target: impl Into<String>,
        datalayout: DataLayout,
        limits: crate::IrLimits,
    ) -> Result<Self, BuilderError> {
        limits.validate().map_err(BuilderError::ResourceLimit)?;
        Ok(Self {
            module: Module::new(target, datalayout),
            #[cfg(not(whale_wave_ir))]
            limits,
            #[cfg(whale_wave_ir)]
            wave: wave::Builder::new(limits),
            next_value: 0,
            next_block: 0,
            next_global: 0,
            #[cfg(feature = "socket")]
            source_work: crate::IrLimits::default().max_analysis_work,
            #[cfg(feature = "socket")]
            lower_storage_budget: crate::limits::Budget::new(crate::IrLimits::default())
                .expect("valid defaults"),
            source_shifts: std::collections::HashMap::new(),
            source_mapped_functions: std::collections::HashSet::new(),
        })
    }

    pub fn add_global(
        &mut self,
        name: impl Into<String>,
        ty: Type,
        init: ConstValue,
        align: u32,
    ) -> crate::GlobalId {
        self.add_global_const(name, crate::ConstExpr::literal(ty, init.clone()), init, align)
    }

    pub fn add_global_const(
        &mut self,
        name: impl Into<String>,
        expression: crate::ConstExpr,
        init: ConstValue,
        align: u32,
    ) -> crate::GlobalId {
        let name = name.into();
        #[cfg(whale_wave_ir)]
        let id = {
            let id = crate::GlobalId(
                u32::try_from(self.wave.state.next_global)
                    .expect("builder global identity overflow"),
            );
            if let Err(error) = self.wave.body.global(id, &name, &expression, &init, align, true) {
                body::discard_expression(expression);
                panic!("invalid builder global payload: {error}");
            }
            let admitted = self
                .wave
                .global_constant(&name, &expression.ty)
                .expect("invalid builder global model");
            assert_eq!(id, admitted, "inconsistent Wave global identity");
            self.wave
                .body
                .global(id, &name, &expression, &init, align, false)
                .expect("invalid builder global payload");
            id
        };
        #[cfg(not(whale_wave_ir))]
        let id = crate::GlobalId(self.next_global);
        self.next_global = id.0.checked_add(1).expect("builder global identity overflow");
        self.module.globals.push(Global {
            id,
            name,
            ty: expression.ty.clone(),
            init,
            init_expr: expression,
            align,
        });
        id
    }

    /// Add initialized or explicitly uninitialized addressable scalar storage.
    /// The shared global namespace and explicit external names are checked atomically.
    pub fn try_add_global_storage(
        &mut self,
        mut storage: crate::GlobalStorage,
    ) -> Result<crate::GlobalId, crate::GlobalStorageError> {
        #[cfg(whale_wave_ir)]
        let id = match self.wave.storage(&storage) {
            Ok(id) => id,
            Err(error) => {
                crate::limits::discard_type(storage.ty);
                return Err(error);
            },
        };
        #[cfg(not(whale_wave_ir))]
        let id = {
            if let Err(error) = storage.validate_with_limits(crate::IrLimits::default()) {
                crate::limits::discard_type(storage.ty);
                return Err(error);
            }
            if self.module.globals.iter().any(|g| g.name == storage.name)
                || self.module.global_storage.iter().any(|g| g.name == storage.name)
            {
                return Err(crate::GlobalStorageError::DuplicateName);
            }
            if storage.link_name.as_ref().is_some_and(|link| {
                self.module.declarations.iter().any(|d| d.link_name.as_ref() == Some(link))
                    || self.module.global_storage.iter().any(|g| g.link_name.as_ref() == Some(link))
            }) {
                return Err(crate::GlobalStorageError::LinkNameCollision);
            }
            let id = crate::GlobalId(self.next_global);
            self.next_global = self
                .next_global
                .checked_add(1)
                .ok_or(crate::GlobalStorageError::DuplicateIdentity)?;
            id
        };
        self.next_global =
            id.0.checked_add(1).ok_or(crate::GlobalStorageError::DuplicateIdentity)?;
        storage.id = id;
        self.module.global_storage.push(storage);
        Ok(id)
    }

    pub fn declare_function(
        &mut self,
        name: impl Into<String>,
        signature: crate::FunctionSignature,
        linkage: crate::Linkage,
        link_name: Option<String>,
    ) -> Result<crate::FunctionId, crate::CallError> {
        self.declare_function_with_limits(
            name,
            signature,
            linkage,
            link_name,
            crate::IrLimits::default(),
        )
    }

    pub fn declare_function_with_limits(
        &mut self,
        name: impl Into<String>,
        signature: crate::FunctionSignature,
        linkage: crate::Linkage,
        link_name: Option<String>,
        limits: crate::IrLimits,
    ) -> Result<crate::FunctionId, crate::CallError> {
        let name = name.into();
        let mut candidate = crate::FunctionDecl {
            id: crate::FunctionId(self.module.declarations.len() as u32),
            name,
            signature,
            linkage,
            link_name,
        };
        #[cfg(whale_wave_ir)]
        let id = match self.wave.declaration(&candidate, limits, false) {
            Ok(id) => id,
            Err(error) => {
                crate::limits::discard_signature(candidate.signature);
                return Err(error);
            },
        };
        #[cfg(whale_wave_ir)]
        if (id.0 as usize) < self.module.declarations.len() {
            return Ok(id);
        }
        #[cfg(not(whale_wave_ir))]
        let id = {
            if let Err(error) = crate::function::validate_decl_with_limits(&candidate, limits) {
                crate::limits::discard_signature(candidate.signature);
                return Err(error);
            }
            if let Some(existing) =
                self.module.declarations.iter().find(|d| d.name == candidate.name)
            {
                let mut same = candidate.clone();
                same.id = existing.id;
                return if &same == existing {
                    Ok(existing.id)
                } else {
                    Err(crate::CallError::ConflictingDeclaration(candidate.name))
                };
            }
            if let Some(link) = &candidate.link_name {
                if self.module.declarations.iter().any(|d| d.link_name.as_ref() == Some(link))
                    || self.module.global_storage.iter().any(|g| g.link_name.as_ref() == Some(link))
                {
                    return Err(crate::CallError::LinkNameCollision(link.clone()));
                }
            }
            candidate.id
        };
        candidate.id = id;
        self.module.declarations.push(candidate);
        Ok(id)
    }

    pub fn function_id(&self, name: &str) -> Result<crate::FunctionId, crate::CallError> {
        #[cfg(whale_wave_ir)]
        {
            self.wave.lookup_function(name)
        }
        #[cfg(not(whale_wave_ir))]
        {
            self.module
                .declarations
                .iter()
                .find(|d| d.name == name)
                .map(|d| d.id)
                .ok_or_else(|| crate::CallError::UnknownFunctionName(name.into()))
        }
    }

    pub fn begin_declared_function(
        &mut self,
        id: crate::FunctionId,
        names: Vec<String>,
    ) -> Result<FunctionBuilder<'_>, crate::CallError> {
        #[cfg(whale_wave_ir)]
        self.wave.definition(id, names.len(), false)?;
        let decl = self
            .module
            .declarations
            .iter()
            .find(|d| d.id == id)
            .ok_or(crate::CallError::UnknownFunction(id))?;
        #[cfg(not(whale_wave_ir))]
        crate::function::validate_signature(&decl.signature)?;
        #[cfg(whale_wave_ir)]
        self.wave.preflight_function(id)?;
        let decl = decl.clone();
        #[cfg(not(whale_wave_ir))]
        if self.module.functions.iter().any(|f| f.id == id) {
            return Err(crate::CallError::DuplicateDefinition(id));
        }
        #[cfg(not(whale_wave_ir))]
        if names.len() != decl.signature.params.len() {
            return Err(crate::CallError::DefinitionMismatch(id));
        }
        let params = names.into_iter().zip(decl.signature.params).collect();
        Ok(self.build_function(id, decl.name, params, decl.signature.ret))
    }

    pub fn begin_function(
        &mut self,
        name: impl Into<String>,
        params: Vec<(String, Type)>,
        ret_ty: Type,
    ) -> FunctionBuilder<'_> {
        let name = name.into();
        self.preflight_type(&ret_ty).expect("invalid builder return type");
        for (_, ty) in &params {
            self.preflight_type(ty).expect("invalid builder parameter type");
        }
        // This legacy convenience API is intentionally infallible. The verifier
        // diagnoses duplicate names and invalid signatures, as before.
        let id = crate::FunctionId(self.module.declarations.len() as u32);
        #[allow(unused_mut)]
        let mut declaration = crate::FunctionDecl {
            id,
            name: name.clone(),
            signature: crate::FunctionSignature::whale(
                params.iter().map(|(_, t)| t.clone()).collect(),
                ret_ty.clone(),
            ),
            linkage: crate::Linkage::Internal,
            link_name: None,
        };
        #[cfg(whale_wave_ir)]
        {
            declaration.id = self
                .wave
                .declaration(&declaration, self.wave.limits, true)
                .expect("invalid builder function model");
        }
        let id = declaration.id;
        self.module.declarations.push(declaration);
        self.build_function(id, name, params, ret_ty)
    }

    fn build_function(
        &mut self,
        id: crate::FunctionId,
        name: String,
        params: Vec<(String, Type)>,
        ret_ty: Type,
    ) -> FunctionBuilder<'_> {
        #[cfg(whale_wave_ir)]
        let first_value = self.wave.next_value();
        #[cfg(whale_wave_ir)]
        let first_block = self.wave.next_block();
        #[cfg(not(whale_wave_ir))]
        self.preflight_type(&ret_ty).expect("invalid builder return type");
        #[cfg(not(whale_wave_ir))]
        for (_, ty) in &params {
            self.preflight_type(ty).expect("invalid builder parameter type");
        }
        let mut p = Vec::new();
        let mut value_types = Vec::new();

        for (nm, ty) in params {
            let id = self.fresh_value(&ty);
            value_types.push((id, ty.clone()));
            p.push(Param { name: nm, id, ty });
        }

        let entry = self.fresh_block();
        let entry_block = BasicBlock::new(entry, "entry");

        let func =
            Function { id, name, params: p, ret_ty, blocks: vec![entry_block], entry, value_types };

        FunctionBuilder {
            mb: self,
            func,
            insert_block: entry,
            #[cfg(whale_wave_ir)]
            first_value,
            #[cfg(whale_wave_ir)]
            first_block,
        }
    }

    fn preflight_type(&self, ty: &Type) -> Result<(), BuilderError> {
        #[cfg(whale_wave_ir)]
        {
            self.wave.preflight(ty)
        }
        #[cfg(not(whale_wave_ir))]
        {
            crate::limits::Budget::new(self.limits)
                .and_then(|mut budget| budget.ty(ty))
                .map_err(BuilderError::ResourceLimit)
        }
    }

    fn fresh_value(&mut self, ty: &Type) -> ValueId {
        #[cfg(not(whale_wave_ir))]
        self.preflight_type(ty).expect("invalid builder value type");
        #[cfg(whale_wave_ir)]
        let id = self.wave.value(ty).expect("invalid builder value allocation").0;
        #[cfg(not(whale_wave_ir))]
        let id = self.next_value;
        self.next_value = id.checked_add(1).expect("builder value identity overflow");
        ValueId(id)
    }

    fn fresh_block(&mut self) -> BlockId {
        #[cfg(whale_wave_ir)]
        let id = self.wave.block().expect("invalid builder block allocation").0;
        #[cfg(not(whale_wave_ir))]
        let id = self.next_block;
        self.next_block = id.checked_add(1).expect("builder block identity overflow");
        BlockId(id)
    }

    pub fn finish(self) -> Module {
        #[cfg(whale_wave_ir)]
        {
            let mut module = self.module;
            self.wave.body.restore_globals(&mut module.globals).expect("invalid Wave global model");
            module
        }
        #[cfg(not(whale_wave_ir))]
        {
            self.module
        }
    }
}

/// Legacy convenience operations panic on invalid construction order. Use the
/// matching `try_*` operations for externally supplied IDs and recoverable errors.
pub struct FunctionBuilder<'a> {
    mb: &'a mut ModuleBuilder,
    func: Function,
    insert_block: BlockId,
    #[cfg(whale_wave_ir)]
    first_value: u32,
    #[cfg(whale_wave_ir)]
    first_block: u32,
}

#[cfg(feature = "socket")]
pub(crate) struct SourceCheckpoint(Vec<(BlockId, usize, usize, bool)>);

#[cfg(feature = "socket")]
impl FunctionBuilder<'_> {
    fn charge_source_work(&mut self, amount: usize) -> Result<(), crate::LowerError> {
        self.mb.charge_source_work(amount)
    }

    pub(crate) fn retain_const_sources(
        &mut self,
        expression: &crate::lower_ast::frontend::Expr,
    ) -> Result<(), crate::LowerError> {
        self.mb.retain_const_sources(expression)
    }

    /// Flush the final hoisting adjustment while resource failures can still be
    /// returned to the frontend. Public finish then has no pending metadata scan.
    pub(crate) fn finish_source_mapping(&mut self) -> Result<(), crate::LowerError> {
        if self.mb.source_shifts.contains_key(&self.func.id) {
            self.charge_source_work(self.mb.module.sources.mappings.len())?;
            self.flush_source_shifts();
        }
        Ok(())
    }

    pub(crate) fn source_checkpoint(&mut self) -> Result<SourceCheckpoint, crate::LowerError> {
        self.charge_source_work(self.func.blocks.len())?;
        let work = self.func.blocks.iter().map(|b| b.instructions.len()).sum::<usize>();
        self.charge_source_work(work + self.mb.module.sources.mappings.len())?;
        self.flush_source_shifts();
        Ok(SourceCheckpoint(
            self.func
                .blocks
                .iter()
                .map(|b| {
                    #[cfg(whale_wave_ir)]
                    let allocas = self.mb.wave.body.allocation_prefix(b.id);
                    #[cfg(not(whale_wave_ir))]
                    let allocas = b
                        .instructions
                        .iter()
                        .take_while(|i| matches!(i, Instruction::Alloca { .. }))
                        .count();
                    (b.id, b.instructions.len(), allocas, b.terminator.is_some())
                })
                .collect(),
        ))
    }

    pub(crate) fn annotate_source(
        &mut self,
        annotation: &crate::SourceAnnotation,
        before: SourceCheckpoint,
    ) -> Result<(), crate::LowerError> {
        self.charge_source_work(self.func.blocks.len() + self.mb.module.sources.files.len())?;
        let work = self.func.blocks.iter().map(|b| b.instructions.len()).sum::<usize>()
            + self.mb.module.sources.mappings.len()
            + self.mb.module.sources.originals.len()
            + self.mb.module.sources.files.iter().map(|f| f.name.len()).sum::<usize>()
            + annotation.source.as_ref().map_or(0, |s| s.file.len());
        self.charge_source_work(work)?;
        self.flush_source_shifts();
        let sources = &mut self.mb.module.sources;
        sources.add_frontend_original(annotation);
        let existing: std::collections::HashSet<_> =
            sources.mappings.iter().map(|m| m.target).collect();
        let before: std::collections::HashMap<_, _> = before
            .0
            .into_iter()
            .map(|(id, len, allocas, terminated)| (id, (len, allocas, terminated)))
            .collect();
        for b in &self.func.blocks {
            let (old_len, old_allocas, old_term) =
                before.get(&b.id).copied().unwrap_or((0, 0, false));
            #[cfg(whale_wave_ir)]
            let allocas = self.mb.wave.body.allocation_prefix(b.id);
            #[cfg(not(whale_wave_ir))]
            let allocas = b
                .instructions
                .iter()
                .take_while(|i| matches!(i, Instruction::Alloca { .. }))
                .count();
            let new_allocas = allocas.saturating_sub(old_allocas);
            for index in 0..b.instructions.len() {
                let new = if b.id == self.func.entry {
                    (old_allocas..allocas).contains(&index) || index >= old_len + new_allocas
                } else {
                    index >= old_len
                };
                let target =
                    crate::SourceTarget::Instruction { function: self.func.id, block: b.id, index };
                if new && !existing.contains(&target) {
                    sources.mappings.push(crate::SourceMapping { target, original: annotation.id });
                }
            }
            let target = crate::SourceTarget::Terminator { function: self.func.id, block: b.id };
            if b.terminator.is_some() && !old_term && !existing.contains(&target) {
                sources.mappings.push(crate::SourceMapping { target, original: annotation.id });
            }
        }
        self.mb.source_mapped_functions.insert(self.func.id);
        Ok(())
    }
}

impl FunctionBuilder<'_> {
    fn flush_source_shifts(&mut self) {
        if let Some((position, count)) = self.mb.source_shifts.remove(&self.func.id) {
            for mapping in &mut self.mb.module.sources.mappings {
                if let crate::SourceTarget::Instruction { function, block, index } =
                    &mut mapping.target
                {
                    if *function == self.func.id && *block == self.func.entry && *index >= position
                    {
                        *index += count;
                    }
                }
            }
        }
    }
}

#[cfg(feature = "socket")]
impl ModuleBuilder {
    pub(crate) fn set_source_work_limit(&mut self, work: usize) {
        self.source_work = work;
    }

    fn charge_source_work(&mut self, amount: usize) -> Result<(), crate::LowerError> {
        self.source_work = self
            .source_work
            .checked_sub(amount)
            .ok_or(crate::LowerError::ResourceLimit(crate::AstLimitError::SourceWork))?;
        Ok(())
    }

    fn retain_source(
        &mut self,
        annotation: &crate::SourceAnnotation,
    ) -> Result<(), crate::LowerError> {
        let work = 1
            + self.module.sources.originals.len()
            + self.module.sources.files.len()
            + self.module.sources.files.iter().map(|f| f.name.len()).sum::<usize>()
            + annotation.source.as_ref().map_or(0, |s| s.file.len());
        self.charge_source_work(work)?;
        self.module.sources.add_frontend_original(annotation);
        Ok(())
    }

    pub(crate) fn retain_const_sources(
        &mut self,
        expression: &crate::lower_ast::frontend::Expr,
    ) -> Result<(), crate::LowerError> {
        use crate::lower_ast::frontend::Expr;
        let mut pending = vec![expression];
        while let Some(expression) = pending.pop() {
            self.charge_source_work(1)?;
            match expression {
                Expr::Located { annotation, expression } => {
                    self.retain_source(annotation)?;
                    pending.push(expression);
                },
                Expr::Binary { left, right, .. } | Expr::Cmp { left, right, .. } => {
                    pending.push(right);
                    pending.push(left);
                },
                Expr::Neg(value) | Expr::BitNot(value) | Expr::Cast { value, .. } => {
                    pending.push(value)
                },
                _ => {},
            }
        }
        Ok(())
    }

    pub(crate) fn annotate_global_source(
        &mut self,
        id: crate::GlobalId,
        annotation: &crate::SourceAnnotation,
    ) -> Result<(), crate::LowerError> {
        self.retain_source(annotation)?;
        self.module.sources.mappings.push(crate::SourceMapping {
            target: crate::SourceTarget::Global(id),
            original: annotation.id,
        });
        Ok(())
    }
}

impl<'a> FunctionBuilder<'a> {
    fn bound_type(&self, ty: Type) -> Result<Type, BuilderError> {
        match self.mb.preflight_type(&ty) {
            Ok(()) => {
                #[cfg(whale_wave_ir)]
                self.mb.wave.body.type_room(&ty, 0)?;
                Ok(ty)
            },
            Err(error) => {
                crate::limits::discard_type(ty);
                Err(error)
            },
        }
    }

    fn preflight_value(
        &self,
        ty: &Type,
        operands: usize,
        targets: usize,
    ) -> Result<(), BuilderError> {
        #[cfg(whale_wave_ir)]
        {
            self.mb.wave.preflight_value(ty, operands, targets)
        }
        #[cfg(not(whale_wave_ir))]
        {
            let _ = (operands, targets);
            self.mb.preflight_type(ty)
        }
    }

    fn ensure_block(&self, bb: BlockId) -> Result<(), BuilderError> {
        #[cfg(whale_wave_ir)]
        {
            self.mb.wave.order(self.first_block, self.first_value, bb, 34, &[], &[], true)
        }
        #[cfg(not(whale_wave_ir))]
        {
            if self.func.blocks.iter().any(|b| b.id == bb) {
                Ok(())
            } else {
                Err(BuilderError::UnknownBlock(bb))
            }
        }
    }

    fn ensure_insertable(&self) -> Result<(), BuilderError> {
        #[cfg(whale_wave_ir)]
        {
            self.mb.wave.order(
                self.first_block,
                self.first_value,
                self.insert_block,
                35,
                &[],
                &[],
                true,
            )
        }
        #[cfg(not(whale_wave_ir))]
        {
            let block = self
                .func
                .blocks
                .iter()
                .find(|b| b.id == self.insert_block)
                .ok_or(BuilderError::UnknownBlock(self.insert_block))?;
            if block.terminator.is_some() {
                Err(BuilderError::TerminatedBlock(block.id))
            } else {
                Ok(())
            }
        }
    }

    fn ensure_values(&self, values: &[ValueId]) -> Result<(), BuilderError> {
        #[cfg(whale_wave_ir)]
        {
            let ids: Vec<u64> = values.iter().map(|value| value.0.into()).collect();
            self.mb.wave.order(
                self.first_block,
                self.first_value,
                self.insert_block,
                34,
                &ids,
                &[],
                true,
            )
        }
        #[cfg(not(whale_wave_ir))]
        {
            for value in values {
                if self.func.value_type(*value).is_none() {
                    return Err(BuilderError::UnknownValue(*value));
                }
            }
            Ok(())
        }
    }

    pub fn try_param_value(&self, index: usize) -> Result<ValueId, BuilderError> {
        self.func
            .params
            .get(index)
            .map(|p| p.id)
            .ok_or(BuilderError::UnknownParameter { index, count: self.func.params.len() })
    }

    pub fn try_set_insert_point(&mut self, bb: BlockId) -> Result<(), BuilderError> {
        self.ensure_block(bb)?;
        self.insert_block = bb;
        Ok(())
    }

    pub fn try_undef(&mut self, ty: Type) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.preflight_value(&ty, 0, 0)?;
        Ok(self.undef(ty))
    }

    pub fn try_const_decl(
        &mut self,
        name: impl Into<String>,
        expression: crate::ConstExpr,
        value: ConstValue,
    ) -> Result<ValueId, BuilderError> {
        #[cfg(whale_wave_ir)]
        {
            let candidate =
                Instruction::ConstDecl { dst: ValueId(0), name: name.into(), expression, value };
            let check = (|| {
                self.ensure_insertable()?;
                let Instruction::ConstDecl { expression, .. } = &candidate else { unreachable!() };
                self.mb.preflight_type(&expression.ty)?;
                self.preflight_value(&expression.ty, 0, 0)?;
                self.mb.wave.body.instruction(self.insert_block, &candidate, false, true)?;
                Ok(())
            })();
            let Instruction::ConstDecl { name, expression, value, .. } = candidate else {
                unreachable!()
            };
            if let Err(error) = check {
                body::discard_expression(expression);
                return Err(error);
            }
            Ok(self.const_decl(name, expression, value))
        }
        #[cfg(not(whale_wave_ir))]
        {
            self.ensure_insertable()?;
            self.mb.preflight_type(&expression.ty)?;
            self.preflight_value(&expression.ty, 0, 0)?;
            Ok(self.const_decl(name, expression, value))
        }
    }

    pub fn try_const_bool(&mut self, v: bool) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.preflight_value(&Type::Bool, 0, 0)?;
        Ok(self.const_bool(v))
    }

    pub fn try_const_i32(&mut self, v: i32) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.preflight_value(&Type::I32, 0, 0)?;
        Ok(self.const_i32(v))
    }

    pub fn try_add(
        &mut self,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        self.preflight_value(&ty, 2, 0)?;
        Ok(self.add(ty, lhs, rhs))
    }

    pub fn try_bin(
        &mut self,
        op: BinOp,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        self.preflight_value(&ty, 2, 0)?;
        Ok(self.bin(op, ty, lhs, rhs))
    }

    pub fn try_icmp(
        &mut self,
        pred: ICmpPred,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        self.mb.preflight_type(&ty)?;
        self.preflight_value(&Type::Bool, 2, 0)?;
        Ok(self.icmp(pred, ty, lhs, rhs))
    }

    pub fn try_cmp(
        &mut self,
        op: CmpOp,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        self.mb.preflight_type(&ty)?;
        self.preflight_value(&Type::Bool, 2, 0)?;
        Ok(self.cmp(op, ty, lhs, rhs))
    }

    pub fn try_checked(
        &mut self,
        op: CheckedOp,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        self.mb.preflight_type(&ty)?;
        self.preflight_value(&Type::Tuple(vec![ty.clone(), Type::Bool]), 2, 0)?;
        Ok(self.checked(op, ty, lhs, rhs))
    }

    pub fn try_extract(
        &mut self,
        tuple_ty: Type,
        dst_ty: Type,
        tuple: ValueId,
        index: u32,
    ) -> Result<ValueId, BuilderError> {
        let dst_ty = self.bound_type(dst_ty)?;
        crate::limits::discard_type(tuple_ty);
        let tuple_ty = Type::Void;
        self.ensure_insertable()?;
        self.ensure_values(&[tuple])?;
        self.preflight_value(&dst_ty, 1, 0)?;
        Ok(self.extract(tuple_ty, dst_ty, tuple, index))
    }

    pub fn try_trap_if(
        &mut self,
        cond: ValueId,
        reason: impl Into<String>,
    ) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.preflight_instruction(1, 0)?;
        self.ensure_values(&[cond])?;
        #[cfg(whale_wave_ir)]
        {
            let candidate = Instruction::TrapIf { cond, reason: reason.into() };
            self.mb.wave.body.instruction(self.insert_block, &candidate, false, true)?;
            let Instruction::TrapIf { reason, .. } = candidate else { unreachable!() };
            self.trap_if(cond, reason);
            Ok(())
        }
        #[cfg(not(whale_wave_ir))]
        {
            self.trap_if(cond, reason);
            Ok(())
        }
    }

    pub fn try_br(&mut self, target: BlockId) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.preflight_instruction(0, 1)?;
        self.ensure_block(target)?;
        self.br(target);
        Ok(())
    }

    pub fn try_cbr(
        &mut self,
        cond: ValueId,
        then_bb: BlockId,
        else_bb: BlockId,
    ) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.preflight_instruction(1, 2)?;
        self.ensure_values(&[cond])?;
        self.ensure_block(then_bb)?;
        self.ensure_block(else_bb)?;
        self.cbr(cond, then_bb, else_bb);
        Ok(())
    }

    pub fn try_trap(&mut self, reason: impl Into<String>) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.preflight_instruction(0, 0)?;
        #[cfg(whale_wave_ir)]
        let reason = {
            let candidate = Terminator::Trap { reason: reason.into() };
            self.mb.wave.body.terminator(self.insert_block, &candidate, true)?;
            let Terminator::Trap { reason } = candidate else { unreachable!() };
            reason
        };
        self.trap(reason);
        Ok(())
    }

    pub fn try_const_int(&mut self, ty: Type, v: i128) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.preflight_value(&ty, 0, 0)?;
        Ok(self.const_int(ty, v))
    }

    pub fn try_const_uint(&mut self, ty: Type, v: u128) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.preflight_value(&ty, 0, 0)?;
        Ok(self.const_uint(ty, v))
    }

    pub fn try_const_float(&mut self, ty: Type, v: f64) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.preflight_value(&ty, 0, 0)?;
        Ok(self.const_float(ty, v))
    }

    pub fn try_const_float_bits(
        &mut self,
        ty: Type,
        v: crate::FloatBits,
    ) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.preflight_value(&ty, 0, 0)?;
        Ok(self.const_float_bits(ty, v))
    }

    pub fn try_alloca(&mut self, ty: Type, align: u32) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.mb.preflight_type(&ty)?;
        self.preflight_value(&Type::Ptr(Box::new(ty.clone())), 0, 0)?;
        Ok(self.alloca(ty, align))
    }

    pub fn try_alloca_in_entry(&mut self, ty: Type, align: u32) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.ensure_block(self.func.entry)?;
        if self.func.blocks.iter().find(|b| b.id == self.func.entry).unwrap().terminator.is_some() {
            return Err(BuilderError::TerminatedBlock(self.func.entry));
        }
        self.mb.preflight_type(&ty)?;
        self.preflight_value(&Type::Ptr(Box::new(ty.clone())), 0, 0)?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.body.instruction(
            self.func.entry,
            &Instruction::Alloca { dst: ValueId(0), ty: ty.clone(), align },
            true,
            true,
        )?;
        Ok(self.alloca_in_entry(ty, align))
    }

    pub fn try_load(
        &mut self,
        ty: Type,
        ptr: ValueId,
        align: u32,
    ) -> Result<ValueId, BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        self.ensure_values(&[ptr])?;
        self.preflight_value(&ty, 1, 0)?;
        Ok(self.load(ty, ptr, align))
    }

    pub fn try_uninit(&mut self, ty: Type, ptr: ValueId, align: u32) -> Result<(), BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.preflight_instruction(1, 0)?;
        self.ensure_values(&[ptr])?;
        self.mb.preflight_type(&ty)?;
        self.uninit(ty, ptr, align);
        Ok(())
    }

    pub fn try_store(
        &mut self,
        ty: Type,
        value: ValueId,
        ptr: ValueId,
        align: u32,
    ) -> Result<(), BuilderError> {
        let ty = self.bound_type(ty)?;
        self.ensure_insertable()?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.preflight_instruction(2, 0)?;
        self.ensure_values(&[value, ptr])?;
        self.mb.preflight_type(&ty)?;
        self.store(ty, value, ptr, align);
        Ok(())
    }

    pub fn try_ret(&mut self, value: Option<ValueId>) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.preflight_instruction(usize::from(value.is_some()), 0)?;
        if let Some(value) = value {
            self.ensure_values(&[value])?;
        }
        #[cfg(whale_wave_ir)]
        self.mb.wave.body.terminator(
            self.insert_block,
            &Terminator::Ret { ty: self.func.ret_ty.clone(), value },
            true,
        )?;
        self.ret(value);
        Ok(())
    }

    pub fn param_value(&self, index: usize) -> ValueId {
        self.try_param_value(index).expect("invalid builder parameter")
    }

    pub fn const_decl(
        &mut self,
        name: impl Into<String>,
        expression: crate::ConstExpr,
        value: ConstValue,
    ) -> ValueId {
        let dst = self.define_value(&expression.ty);
        self.push_instruction(Instruction::ConstDecl { dst, name: name.into(), expression, value });
        dst
    }

    pub fn create_block(&mut self, name: &str) -> BlockId {
        let id = self.mb.fresh_block();
        self.func.blocks.push(BasicBlock::new(id, name));
        id
    }

    pub fn set_insert_point(&mut self, bb: BlockId) {
        self.try_set_insert_point(bb).expect("invalid builder block");
    }

    fn cur_block_mut(&mut self) -> &mut BasicBlock {
        self.ensure_insertable().expect("invalid builder insertion");
        self.func
            .blocks
            .iter_mut()
            .find(|b| b.id == self.insert_block)
            .expect("insert block not found")
    }

    fn push_instruction(&mut self, instruction: Instruction) {
        #[cfg(whale_wave_ir)]
        self.mb
            .wave
            .instruction(self.first_block, self.first_value, self.insert_block, &instruction, false)
            .expect("invalid builder instruction order");
        self.cur_block_mut().instructions.push(instruction);
    }

    fn mark_terminated(&mut self) {
        #[cfg(whale_wave_ir)]
        {
            let term = self
                .func
                .blocks
                .iter()
                .find(|block| block.id == self.insert_block)
                .and_then(|block| block.terminator.as_ref())
                .expect("builder terminator");
            self.mb
                .wave
                .terminator(self.first_block, self.first_value, self.insert_block, term, false)
                .expect("invalid builder terminator order");
        }
    }

    fn define_value(&mut self, ty: &Type) -> ValueId {
        self.ensure_insertable().expect("invalid builder insertion");
        let id = self.mb.fresh_value(ty);
        self.func.value_types.push((id, ty.clone()));
        id
    }

    pub fn const_bool(&mut self, v: bool) -> ValueId {
        let dst = self.define_value(&Type::Bool);
        self.push_instruction(Instruction::Const {
            dst,
            ty: Type::Bool,
            value: ConstValue::Bool(v),
        });
        dst
    }

    pub fn const_i32(&mut self, v: i32) -> ValueId {
        let dst = self.define_value(&Type::I32);
        self.push_instruction(Instruction::Const {
            dst,
            ty: Type::I32,
            value: ConstValue::I(v as i128),
        });
        dst
    }

    pub fn add(&mut self, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Bin { dst, op: BinOp::Add, ty, lhs, rhs });
        dst
    }

    pub fn bin(&mut self, op: BinOp, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Bin { dst, op, ty, lhs, rhs });
        dst
    }

    pub fn icmp(&mut self, pred: ICmpPred, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(&Type::Bool);
        self.push_instruction(Instruction::ICmp { dst, pred, ty, lhs, rhs });
        dst
    }

    pub fn cmp(&mut self, op: CmpOp, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(&Type::Bool);
        self.push_instruction(Instruction::Cmp { dst, op, ty, lhs, rhs });
        dst
    }

    pub fn cast(&mut self, op: crate::CastOp, src_ty: Type, dst_ty: Type, src: ValueId) -> ValueId {
        let dst = self.define_value(&dst_ty);
        self.push_instruction(Instruction::Cast { dst, op, dst_ty, src_ty, src });
        dst
    }

    pub fn checked(&mut self, op: CheckedOp, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        self.mb.preflight_type(&ty).expect("invalid builder type");
        let dst_ty = Type::Tuple(vec![ty.clone(), Type::Bool]);
        let dst = self.define_value(&dst_ty);
        self.push_instruction(Instruction::Checked { dst, op, ty, lhs, rhs });
        dst
    }

    pub fn extract(&mut self, tuple_ty: Type, dst_ty: Type, tuple: ValueId, index: u32) -> ValueId {
        let dst = self.define_value(&dst_ty);
        self.push_instruction(Instruction::Extract { dst, dst_ty, tuple, index });
        crate::limits::discard_type(tuple_ty);
        dst
    }

    pub fn trap_if(&mut self, cond: ValueId, reason: impl Into<String>) {
        self.push_instruction(Instruction::TrapIf { cond, reason: reason.into() });
    }

    pub fn br(&mut self, target: BlockId) {
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::Br { target });
        self.mark_terminated();
    }

    pub fn cbr(&mut self, cond: ValueId, then_bb: BlockId, else_bb: BlockId) {
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::CBr { cond, then_bb, else_bb });
        self.mark_terminated();
    }

    pub fn ret(&mut self, value: Option<ValueId>) {
        let ty = self.func.ret_ty.clone();
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::Ret { ty, value });
        self.mark_terminated();
    }

    pub fn trap(&mut self, reason: impl Into<String>) {
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::Trap { reason: reason.into() });
        self.mark_terminated();
    }

    pub fn function_id(&self, name: &str) -> Result<crate::FunctionId, crate::CallError> {
        self.mb.function_id(name)
    }

    pub fn null_function(
        &mut self,
        signature: crate::FunctionSignature,
    ) -> Result<ValueId, crate::CallError> {
        #[cfg(whale_wave_ir)]
        let validity = self.mb.wave.signature(&signature);
        #[cfg(not(whale_wave_ir))]
        let validity = crate::function::validate_signature(&signature);
        if let Err(error) = validity {
            crate::limits::discard_signature(signature);
            return Err(error);
        }
        self.ensure_insertable().map_err(crate::CallError::Builder)?;
        let pointer = Type::FnPtr(Box::new(signature.clone()));
        self.preflight_value(&pointer, 0, 0).map_err(crate::CallError::Builder)?;
        let dst = self.define_value(&pointer);
        self.push_instruction(Instruction::NullFunction { dst, signature });
        Ok(dst)
    }

    pub fn function_addr(
        &mut self,
        function: crate::FunctionId,
    ) -> Result<ValueId, crate::CallError> {
        let signature = self.callee_signature(&Callee::Direct(function))?;
        #[cfg(not(whale_wave_ir))]
        crate::function::validate_signature(&signature)?;
        self.ensure_insertable().map_err(crate::CallError::Builder)?;
        let pointer = Type::FnPtr(Box::new(signature.clone()));
        self.preflight_value(&pointer, 0, 0).map_err(crate::CallError::Builder)?;
        let dst = self.define_value(&pointer);
        self.push_instruction(Instruction::FunctionAddr { dst, function, signature });
        Ok(dst)
    }

    pub fn callee_signature(
        &self,
        callee: &Callee,
    ) -> Result<crate::FunctionSignature, crate::CallError> {
        #[cfg(whale_wave_ir)]
        self.mb.wave.check_call(self.first_value, callee, None, None, &self.func)?;
        let signature = match callee {
            Callee::Direct(id) => {
                &self
                    .mb
                    .module
                    .declarations
                    .iter()
                    .find(|d| d.id == *id)
                    .ok_or(crate::CallError::UnknownFunction(*id))?
                    .signature
            },
            Callee::Indirect(value) => match self.func.value_type(*value) {
                Some(Type::FnPtr(sig)) => sig,
                Some(_) => return Err(crate::CallError::NotFunctionPointer(*value)),
                None => return Err(crate::CallError::UnknownValue(*value)),
            },
        };
        #[cfg(not(whale_wave_ir))]
        crate::function::validate_signature(signature)?;
        Ok(signature.clone())
    }

    pub fn call(
        &mut self,
        callee: Callee,
        args: Vec<ValueId>,
    ) -> Result<Option<ValueId>, crate::CallError> {
        self.ensure_insertable().map_err(crate::CallError::Builder)?;
        let signature = self.callee_signature(&callee)?;
        #[cfg(not(whale_wave_ir))]
        crate::function::validate_signature(&signature)?;
        #[cfg(whale_wave_ir)]
        self.mb.wave.check_call(
            self.first_value,
            &callee,
            Some(&args),
            Some(&signature),
            &self.func,
        )?;
        #[cfg(not(whale_wave_ir))]
        if args.len() != signature.params.len() {
            return Err(crate::CallError::ArgumentCount {
                expected: signature.params.len(),
                got: args.len(),
            });
        }
        #[cfg(not(whale_wave_ir))]
        for (index, (arg, expected)) in args.iter().zip(&signature.params).enumerate() {
            let got = self.func.value_type(*arg).ok_or(crate::CallError::UnknownValue(*arg))?;
            if got != expected {
                return Err(crate::CallError::ArgumentType {
                    index,
                    expected: expected.clone(),
                    got: got.clone(),
                });
            }
        }
        let ret_ty = signature.ret;

        #[cfg(whale_wave_ir)]
        {
            let operands =
                args.len().saturating_add(usize::from(matches!(callee, Callee::Indirect(_))));
            if ret_ty != Type::Void {
                self.preflight_value(&ret_ty, operands, 0).map_err(crate::CallError::Builder)?;
            } else {
                self.mb
                    .wave
                    .preflight_instruction(operands, 0)
                    .map_err(crate::CallError::Builder)?;
            }
        }
        let dst = if ret_ty == Type::Void { None } else { Some(self.define_value(&ret_ty)) };
        self.push_instruction(Instruction::Call {
            convention: signature.convention,
            dst,
            ret_ty,
            callee,
            args,
        });
        Ok(dst)
    }

    pub fn finish(mut self) {
        #[cfg(whale_wave_ir)]
        self.mb.wave.body.restore(&mut self.func).expect("invalid Wave builder body snapshot");
        #[cfg(whale_wave_ir)]
        self.mb
            .wave
            .definition(self.func.id, self.func.params.len(), true)
            .expect("invalid builder definition model");
        self.flush_source_shifts();
        self.mb.module.functions.push(self.func);
    }

    pub fn const_int(&mut self, ty: Type, v: i128) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Const { dst, ty, value: ConstValue::I(v) });
        dst
    }

    pub fn const_uint(&mut self, ty: Type, v: u128) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Const { dst, ty, value: ConstValue::U(v) });
        dst
    }

    pub fn const_float(&mut self, ty: Type, v: f64) -> ValueId {
        let width = match ty {
            Type::F16 => 16,
            Type::F32 => 32,
            _ => 64,
        };
        self.const_float_bits(
            ty,
            crate::FloatBits::from_f64(width, v).expect("supported float width"),
        )
    }

    pub fn const_float_bits(&mut self, ty: Type, v: crate::FloatBits) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Const { dst, ty, value: ConstValue::F(v) });
        dst
    }

    pub fn alloca(&mut self, ty: Type, align: u32) -> ValueId {
        self.mb.preflight_type(&ty).expect("invalid builder type");
        let dst = self.define_value(&Type::Ptr(Box::new(ty.clone())));
        self.push_instruction(Instruction::Alloca { dst, ty, align });
        dst
    }

    pub fn gep(&mut self, base_ptr: ValueId, indices: Vec<ValueId>, dst_ty: Type) -> ValueId {
        let dst = self.define_value(&dst_ty);
        self.push_instruction(Instruction::Gep { dst, dst_ty, base_ptr, indices });
        dst
    }

    pub fn memcpy(&mut self, dst: ValueId, src: ValueId, n: ValueId, align: u32) {
        self.push_instruction(Instruction::Memcpy { dst, src, n, align });
    }

    pub fn volatile_load(&mut self, ty: Type, ptr: ValueId, align: u32) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::VolatileLoad { dst, ty, ptr, align });
        dst
    }

    pub fn volatile_store(&mut self, ty: Type, value: ValueId, ptr: ValueId, align: u32) {
        self.push_instruction(Instruction::VolatileStore { ty, value, ptr, align });
    }

    pub fn load(&mut self, ty: Type, ptr: ValueId, align: u32) -> ValueId {
        let dst = self.define_value(&ty);
        self.push_instruction(Instruction::Load { dst, ty, ptr, align });
        dst
    }

    /// Revoke initialization at this program point without zeroing storage.
    pub fn uninit(&mut self, ty: Type, ptr: ValueId, align: u32) {
        self.push_instruction(Instruction::Uninit { ty, ptr, align });
    }

    pub fn store(&mut self, ty: Type, value: ValueId, ptr: ValueId, align: u32) {
        self.push_instruction(Instruction::Store { ty, value, ptr, align });
    }
}

#[cfg(all(test, whale_wave_ir))]
mod wave_model_tests {
    use super::*;

    #[test]
    fn finish_reads_complete_global_payloads_from_the_wave_model() {
        let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        let first = builder.add_global("base", Type::I32, ConstValue::I(40), 4);
        builder
            .try_add_global_storage(crate::GlobalStorage::internal(
                "slot",
                Type::I32,
                crate::GlobalStorageInit::Uninitialized,
                false,
                4,
            ))
            .unwrap();
        let second = builder.add_global_const(
            "λ total",
            crate::ConstExpr {
                ty: Type::I32,
                kind: crate::ConstExprKind::Binary {
                    op: crate::ConstBinaryOp::Add,
                    left: Box::new(crate::ConstExpr {
                        ty: Type::I32,
                        kind: crate::ConstExprKind::Reference(crate::ConstRef::Global(first)),
                    }),
                    right: Box::new(crate::ConstExpr::literal(Type::I32, ConstValue::I(2))),
                },
            },
            ConstValue::I(42),
            16,
        );
        assert_eq!(second, crate::GlobalId(2));
        let expected = builder.module.globals.clone();
        builder.module.globals.clear();
        let module = builder.finish();
        assert_eq!(module.globals, expected);
        crate::verify_module(&module).unwrap();
    }

    #[test]
    fn many_hoists_append_the_host_mirror_and_restore_canonical_body_order() {
        let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        let mut function = builder.begin_function("many", vec![], Type::Void);
        for index in 0..10000 {
            function.const_i32(index);
        }
        for _ in 0..10000 {
            function.try_alloca_in_entry(Type::I32, 4).unwrap();
        }
        // The compatibility mirror only appends. Its physical order must not
        // impose a second quadratic insertion path beside Wave's indexed model.
        assert!(matches!(function.func.blocks[0].instructions[0], Instruction::Const { .. }));
        function.ret(None);
        function.finish();
        let module = builder.finish();
        assert!(module.functions[0].blocks[0].instructions[..10000]
            .iter()
            .all(|i| matches!(i, Instruction::Alloca { .. })));
        assert!(module.functions[0].blocks[0].instructions[10000..]
            .iter()
            .all(|i| matches!(i, Instruction::Const { .. })));
        crate::verify_module(&module).unwrap();
    }
}
