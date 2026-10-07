// SPDX-License-Identifier: MPL-2.0

use crate::{
    BasicBlock, BinOp, BlockId, Callee, CheckedOp, CmpOp, ConstValue, DataLayout, Function, Global,
    ICmpPred, Instruction, Module, Param, Terminator, Type, ValueId,
};

pub struct ModuleBuilder {
    module: Module,
    next_value: u32,
    next_block: u32,
    next_global: u32,
}

impl<'a> FunctionBuilder<'a> {
    pub fn entry_block(&self) -> BlockId {
        self.func.entry
    }

    pub fn undef(&mut self, ty: Type) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut()
            .instructions
            .push(Instruction::Undef { dst, ty });
        dst
    }

    fn _cur_block(&self) -> &BasicBlock {
        self.func
            .blocks
            .iter()
            .find(|b| b.id == self.insert_block)
            .expect("insert block not found")
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
        let dst = self.define_value(Type::Ptr(Box::new(ty.clone())));
        let entry = self.func.entry;

        let entry_block = self
            .func
            .blocks
            .iter_mut()
            .find(|b| b.id == entry)
            .expect("entry block not found");

        let pos = entry_block
            .instructions
            .iter()
            .position(|ins| !matches!(ins, Instruction::Alloca { .. }))
            .unwrap_or(entry_block.instructions.len());

        entry_block
            .instructions
            .insert(pos, Instruction::Alloca { dst, ty, align });

        dst
    }
}

impl ModuleBuilder {
    pub fn new(target: impl Into<String>, datalayout: DataLayout) -> Self {
        Self {
            module: Module::new(target, datalayout),
            next_value: 0,
            next_block: 0,
            next_global: 0,
        }
    }

    pub fn add_global(
        &mut self,
        name: impl Into<String>,
        ty: Type,
        init: ConstValue,
        align: u32,
    ) -> crate::GlobalId {
        self.add_global_const(
            name,
            crate::ConstExpr::literal(ty, init.clone()),
            init,
            align,
        )
    }

    pub fn add_global_const(
        &mut self,
        name: impl Into<String>,
        expression: crate::ConstExpr,
        init: ConstValue,
        align: u32,
    ) -> crate::GlobalId {
        let id = crate::GlobalId(self.next_global);
        self.next_global += 1;
        self.module.globals.push(Global {
            id,
            name: name.into(),
            ty: expression.ty.clone(),
            init,
            init_expr: expression,
            align,
        });
        id
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
        let candidate = crate::FunctionDecl {
            id: crate::FunctionId(self.module.declarations.len() as u32),
            name,
            signature,
            linkage,
            link_name,
        };
        if let Err(error) = crate::function::validate_decl_with_limits(&candidate, limits) {
            crate::limits::discard_signature(candidate.signature);
            return Err(error);
        }
        if let Some(existing) = self
            .module
            .declarations
            .iter()
            .find(|d| d.name == candidate.name)
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
            if self
                .module
                .declarations
                .iter()
                .any(|d| d.link_name.as_ref() == Some(link))
            {
                return Err(crate::CallError::LinkNameCollision(link.clone()));
            }
        }
        let id = candidate.id;
        self.module.declarations.push(candidate);
        Ok(id)
    }

    pub fn function_id(&self, name: &str) -> Result<crate::FunctionId, crate::CallError> {
        self.module
            .declarations
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.id)
            .ok_or_else(|| crate::CallError::UnknownFunctionName(name.into()))
    }

    pub fn begin_declared_function(
        &mut self,
        id: crate::FunctionId,
        names: Vec<String>,
    ) -> Result<FunctionBuilder<'_>, crate::CallError> {
        let decl = self
            .module
            .declarations
            .iter()
            .find(|d| d.id == id)
            .ok_or(crate::CallError::UnknownFunction(id))?;
        crate::function::validate_signature(&decl.signature)?;
        let decl = decl.clone();
        if self.module.functions.iter().any(|f| f.id == id) {
            return Err(crate::CallError::DuplicateDefinition(id));
        }
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
        // This legacy convenience API is intentionally infallible. The verifier
        // diagnoses duplicate names and invalid signatures, as before.
        let id = crate::FunctionId(self.module.declarations.len() as u32);
        self.module.declarations.push(crate::FunctionDecl {
            id,
            name: name.clone(),
            signature: crate::FunctionSignature::whale(
                params.iter().map(|(_, t)| t.clone()).collect(),
                ret_ty.clone(),
            ),
            linkage: crate::Linkage::Internal,
            link_name: None,
        });
        self.build_function(id, name, params, ret_ty)
    }

    fn build_function(
        &mut self,
        id: crate::FunctionId,
        name: String,
        params: Vec<(String, Type)>,
        ret_ty: Type,
    ) -> FunctionBuilder<'_> {
        let mut p = Vec::new();
        let mut value_types = Vec::new();

        for (nm, ty) in params {
            let id = self.fresh_value();
            value_types.push((id, ty.clone()));
            p.push(Param { name: nm, id, ty });
        }

        let entry = self.fresh_block();
        let entry_block = BasicBlock::new(entry, "entry");

        let func = Function {
            id,
            name,
            params: p,
            ret_ty,
            blocks: vec![entry_block],
            entry,
            value_types,
        };

        FunctionBuilder {
            mb: self,
            func,
            insert_block: entry,
        }
    }

    fn fresh_value(&mut self) -> ValueId {
        let id = self.next_value;
        self.next_value += 1;
        ValueId(id)
    }

    fn fresh_block(&mut self) -> BlockId {
        let id = self.next_block;
        self.next_block += 1;
        BlockId(id)
    }

    pub fn finish(self) -> Module {
        self.module
    }
}

pub struct FunctionBuilder<'a> {
    mb: &'a mut ModuleBuilder,
    func: Function,
    insert_block: BlockId,
}

impl<'a> FunctionBuilder<'a> {
    pub fn param_value(&self, index: usize) -> ValueId {
        self.func.params[index].id
    }

    pub fn const_decl(
        &mut self,
        name: impl Into<String>,
        expression: crate::ConstExpr,
        value: ConstValue,
    ) -> ValueId {
        let dst = self.define_value(expression.ty.clone());
        self.cur_block_mut()
            .instructions
            .push(Instruction::ConstDecl {
                dst,
                name: name.into(),
                expression,
                value,
            });
        dst
    }

    pub fn create_block(&mut self, name: &str) -> BlockId {
        let id = self.mb.fresh_block();
        self.func.blocks.push(BasicBlock::new(id, name));
        id
    }

    pub fn set_insert_point(&mut self, bb: BlockId) {
        self.insert_block = bb;
    }

    fn cur_block_mut(&mut self) -> &mut BasicBlock {
        self.func
            .blocks
            .iter_mut()
            .find(|b| b.id == self.insert_block)
            .expect("insert block not found")
    }

    fn define_value(&mut self, ty: Type) -> ValueId {
        let id = self.mb.fresh_value();
        self.func.value_types.push((id, ty));
        id
    }

    pub fn const_bool(&mut self, v: bool) -> ValueId {
        let dst = self.define_value(Type::Bool);
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty: Type::Bool,
            value: ConstValue::Bool(v),
        });
        dst
    }

    pub fn const_i32(&mut self, v: i32) -> ValueId {
        let dst = self.define_value(Type::I32);
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty: Type::I32,
            value: ConstValue::I(v as i128),
        });
        dst
    }

    pub fn add(&mut self, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Bin {
            dst,
            op: BinOp::Add,
            ty,
            lhs,
            rhs,
        });
        dst
    }

    pub fn bin(&mut self, op: BinOp, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Bin {
            dst,
            op,
            ty,
            lhs,
            rhs,
        });
        dst
    }

    pub fn icmp(&mut self, pred: ICmpPred, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(Type::Bool);
        self.cur_block_mut().instructions.push(Instruction::ICmp {
            dst,
            pred,
            ty,
            lhs,
            rhs,
        });
        dst
    }

    pub fn cmp(&mut self, op: CmpOp, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(Type::Bool);
        self.cur_block_mut().instructions.push(Instruction::Cmp {
            dst,
            op,
            ty,
            lhs,
            rhs,
        });
        dst
    }

    pub fn checked(&mut self, op: CheckedOp, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst_ty = Type::Tuple(vec![ty.clone(), Type::Bool]);
        let dst = self.define_value(dst_ty);
        self.cur_block_mut()
            .instructions
            .push(Instruction::Checked {
                dst,
                op,
                ty,
                lhs,
                rhs,
            });
        dst
    }

    pub fn extract(&mut self, tuple_ty: Type, dst_ty: Type, tuple: ValueId, index: u32) -> ValueId {
        let dst = self.define_value(dst_ty.clone());
        self.cur_block_mut()
            .instructions
            .push(Instruction::Extract {
                dst,
                dst_ty,
                tuple,
                index,
            });
        let _ = tuple_ty;
        dst
    }

    pub fn trap_if(&mut self, cond: ValueId, reason: impl Into<String>) {
        self.cur_block_mut().instructions.push(Instruction::TrapIf {
            cond,
            reason: reason.into(),
        });
    }

    pub fn br(&mut self, target: BlockId) {
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::Br { target });
    }

    pub fn cbr(&mut self, cond: ValueId, then_bb: BlockId, else_bb: BlockId) {
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::CBr {
            cond,
            then_bb,
            else_bb,
        });
    }

    pub fn ret(&mut self, value: Option<ValueId>) {
        let ty = self.func.ret_ty.clone();
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::Ret { ty, value });
    }

    pub fn trap(&mut self, reason: impl Into<String>) {
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::Trap {
            reason: reason.into(),
        });
    }

    pub fn function_id(&self, name: &str) -> Result<crate::FunctionId, crate::CallError> {
        self.mb.function_id(name)
    }

    pub fn null_function(
        &mut self,
        signature: crate::FunctionSignature,
    ) -> Result<ValueId, crate::CallError> {
        if let Err(error) = crate::function::validate_signature(&signature) {
            crate::limits::discard_signature(signature);
            return Err(error);
        }
        let dst = self.define_value(Type::FnPtr(Box::new(signature.clone())));
        self.cur_block_mut()
            .instructions
            .push(Instruction::NullFunction { dst, signature });
        Ok(dst)
    }

    pub fn function_addr(
        &mut self,
        function: crate::FunctionId,
    ) -> Result<ValueId, crate::CallError> {
        let signature = self.callee_signature(&Callee::Direct(function))?;
        crate::function::validate_signature(&signature)?;
        let dst = self.define_value(Type::FnPtr(Box::new(signature.clone())));
        self.cur_block_mut()
            .instructions
            .push(Instruction::FunctionAddr {
                dst,
                function,
                signature,
            });
        Ok(dst)
    }

    pub fn callee_signature(
        &self,
        callee: &Callee,
    ) -> Result<crate::FunctionSignature, crate::CallError> {
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
            }
            Callee::Indirect(value) => match self.func.value_type(*value) {
                Some(Type::FnPtr(sig)) => sig,
                Some(_) => return Err(crate::CallError::NotFunctionPointer(*value)),
                None => return Err(crate::CallError::UnknownValue(*value)),
            },
        };
        crate::function::validate_signature(signature)?;
        Ok(signature.clone())
    }

    pub fn call(
        &mut self,
        callee: Callee,
        args: Vec<ValueId>,
    ) -> Result<Option<ValueId>, crate::CallError> {
        let signature = self.callee_signature(&callee)?;
        crate::function::validate_signature(&signature)?;
        if args.len() != signature.params.len() {
            return Err(crate::CallError::ArgumentCount {
                expected: signature.params.len(),
                got: args.len(),
            });
        }
        for (index, (arg, expected)) in args.iter().zip(&signature.params).enumerate() {
            let got = self
                .func
                .value_type(*arg)
                .ok_or(crate::CallError::UnknownValue(*arg))?;
            if got != expected {
                return Err(crate::CallError::ArgumentType {
                    index,
                    expected: expected.clone(),
                    got: got.clone(),
                });
            }
        }
        let ret_ty = signature.ret;

        let dst = if ret_ty == Type::Void {
            None
        } else {
            Some(self.define_value(ret_ty.clone()))
        };
        self.cur_block_mut().instructions.push(Instruction::Call {
            convention: signature.convention,
            dst,
            ret_ty,
            callee,
            args,
        });
        Ok(dst)
    }

    pub fn finish(self) {
        self.mb.module.functions.push(self.func);
    }

    pub fn const_int(&mut self, ty: Type, v: i128) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty,
            value: ConstValue::I(v),
        });
        dst
    }

    pub fn const_uint(&mut self, ty: Type, v: u128) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty,
            value: ConstValue::U(v),
        });
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
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty,
            value: ConstValue::F(v),
        });
        dst
    }

    pub fn alloca(&mut self, ty: Type, align: u32) -> ValueId {
        let dst = self.define_value(Type::Ptr(Box::new(ty.clone())));
        self.cur_block_mut()
            .instructions
            .push(Instruction::Alloca { dst, ty, align });
        dst
    }

    pub fn load(&mut self, ty: Type, ptr: ValueId, align: u32) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Load {
            dst,
            ty,
            ptr,
            align,
        });
        dst
    }

    pub fn store(&mut self, ty: Type, value: ValueId, ptr: ValueId, align: u32) {
        self.cur_block_mut().instructions.push(Instruction::Store {
            ty,
            value,
            ptr,
            align,
        });
    }
}
