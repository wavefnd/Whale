// SPDX-License-Identifier: MPL-2.0

use crate::{BasicBlock, BinOp, BlockId, Callee, CheckedOp, CmpOp, ConstValue, DataLayout, Function, Global, ICmpPred, Instruction, Module, Param, Terminator, Type, ValueId};

pub struct ModuleBuilder {
    module: Module,
    next_value: u32,
    next_block: u32,
}

impl<'a> FunctionBuilder<'a> {
    pub fn entry_block(&self) -> BlockId {
        self.func.entry
    }

    pub fn undef(&mut self, ty: Type) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Undef { dst, ty });
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
        }
    }

    pub fn add_global(&mut self, name: impl Into<String>, ty: Type, init: ConstValue, align: u32) {
        self.module.globals.push(Global {
            name: name.into(),
            ty,
            init,
            align,
        });
    }

    pub fn begin_function(
        &mut self,
        name: impl Into<String>,
        params: Vec<(String, Type)>,
        ret_ty: Type,
    ) -> FunctionBuilder<'_> {
        let name = name.into();

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
        let dst = self.define_value(Type::I1);
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty: Type::I1,
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
        let dst = self.define_value(Type::I1);
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
        let dst = self.define_value(Type::I1);
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
        let dst_ty = Type::Tuple(vec![ty.clone(), Type::I1]);
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

    pub fn call(&mut self, ret_ty: Type, callee: Callee, args: Vec<ValueId>) -> Option<ValueId> {
        let dst = if ret_ty == Type::Void {
            None
        } else {
            Some(self.define_value(ret_ty.clone()))
        };
        self.cur_block_mut().instructions.push(Instruction::Call {
            dst,
            ret_ty,
            callee,
            args,
        });
        dst
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
