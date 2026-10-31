// SPDX-License-Identifier: MPL-2.0
//! Caller-owned storage for the authoritative Wave construction model.
use super::BuilderError;

use crate::{BlockId, IrLimits, LimitError, Resource, Type, ValueId};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Node {
    pub(super) kind: u64,
    pub(super) depth: u64,
    pub(super) children: u64,
    pub(super) auxiliary: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Value {
    id: u64,
    first_type: u64,
    type_count: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Block {
    id: u64,
    terminated: u64,
    non_phi: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct State {
    pub(super) next_value: u64,
    pub(super) next_block: u64,
    pub(super) type_count: u64,
    pub(super) instruction_count: u64,
    pub(super) namespace_nodes: u64,
    pub(super) namespace_bytes: u64,
    pub(super) next_function: u64,
    pub(super) next_global: u64,
    pub(super) work: u64,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct ResultRecord {
    pub(super) status: u64,
    pub(super) detail: u64,
    pub(super) value: u64,
    pub(super) work: u64,
}

unsafe extern "C" {
    fn whale_ir_builder_type(
        nodes: *const Node,
        count: u64,
        depth: u64,
        nodes_limit: u64,
        work_limit: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_builder_allocate(
        state: *mut State,
        kind: u64,
        types: *const Node,
        count: u64,
        depth_limit: u64,
        node_limit: u64,
        work_limit: u64,
        mode: u64,
        output_types: *mut Node,
        type_capacity: u64,
        output_values: *mut Value,
        value_capacity: u64,
        output_blocks: *mut Block,
        block_capacity: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_builder_order(
        state: *mut State,
        node_limit: u64,
        blocks: *mut Block,
        count: u64,
        first_block: u64,
        first_value: u64,
        value_end: u64,
        block: u64,
        op: u64,
        operands: *const u64,
        operand_count: u64,
        targets: *const u64,
        target_count: u64,
        check_only: u64,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;
}

pub(super) struct Builder {
    pub(super) body: super::body::Store,
    pub(super) state: State,
    pub(super) types: Vec<Node>,
    pub(super) values: Vec<Value>,
    blocks: Vec<Block>,
    pub(super) limits: IrLimits,
    pub(super) namespaces: Vec<super::namespace::Record>,
    pub(super) namespace_payloads: Vec<super::namespace::Payload>,
}

// Raw namespace pointers reference only immutable allocations retained by this
// owner. Mutating Wave calls require &mut Builder; shared queries are read-only.
unsafe impl Send for Builder {}

unsafe impl Sync for Builder {}

impl Builder {
    pub(super) fn new(limits: IrLimits) -> Self {
        Self {
            body: super::body::Store::new(limits),
            state: State::default(),
            types: Vec::new(),
            values: Vec::new(),
            blocks: Vec::new(),
            limits,
            namespaces: Vec::new(),
            namespace_payloads: Vec::new(),
        }
    }

    pub(super) fn error(&self, status: i32, detail: u64) -> BuilderError {
        match status {
            2 => self.limit(Resource::TypeDepth, self.limits.max_type_depth),
            3 => self.limit(Resource::Nodes, self.limits.max_nodes),
            4 => self.limit(Resource::AnalysisWork, self.limits.max_analysis_work),
            5 => BuilderError::IdentityOverflow,
            6 => BuilderError::UnknownBlock(BlockId(detail as u32)),
            7 => BuilderError::TerminatedBlock(BlockId(detail as u32)),
            8 => BuilderError::UnknownValue(ValueId(detail as u32)),
            9 => BuilderError::PhiAfterInstruction(BlockId(detail as u32)),
            _ => BuilderError::InvalidType,
        }
    }

    fn limit(&self, resource: Resource, limit: usize) -> BuilderError {
        BuilderError::ResourceLimit(LimitError { resource, limit })
    }

    pub(super) fn used_nodes(&self) -> usize {
        self.types
            .len()
            .saturating_add(self.values.len())
            .saturating_add(self.blocks.len())
            .saturating_add(self.state.instruction_count as usize)
            .saturating_add(self.state.namespace_nodes as usize)
    }

    pub(super) fn flatten(&self, ty: &Type) -> Result<Vec<Node>, BuilderError> {
        // This bounded borrowed transport walk precedes every recursive Rust
        // clone. Wave checks the complete exact inventory and commits budgets.
        let mut limits = self.limits;
        limits.max_nodes = limits.max_nodes.saturating_sub(self.used_nodes());
        crate::limits::Budget::new(limits)
            .and_then(|mut b| b.ty(ty))
            .map_err(BuilderError::ResourceLimit)?;
        let mut nodes = Vec::new();
        let mut pending = vec![(ty, 0)];
        while let Some((ty, depth)) = pending.pop() {
            let (kind, children, auxiliary) = match ty {
                Type::Void => (0, 0, 0),
                Type::Bool => (1, 0, 0),
                Type::I1 => (2, 0, 0),
                Type::U1 => (3, 0, 0),
                Type::I8 => (4, 0, 0),
                Type::I16 => (5, 0, 0),
                Type::I32 => (6, 0, 0),
                Type::I64 => (7, 0, 0),
                Type::I128 => (8, 0, 0),
                Type::U8 => (9, 0, 0),
                Type::U16 => (10, 0, 0),
                Type::U32 => (11, 0, 0),
                Type::U64 => (12, 0, 0),
                Type::U128 => (13, 0, 0),
                Type::F16 => (14, 0, 0),
                Type::F32 => (15, 0, 0),
                Type::F64 => (16, 0, 0),
                Type::Ptr(inner) => {
                    pending.push((inner, depth + 1));
                    (17, 1, 0)
                },
                Type::FnPtr(sig) => {
                    pending.extend(sig.params.iter().rev().map(|ty| (ty, depth + 1)));
                    pending.push((&sig.ret, depth + 1));
                    (
                        18,
                        sig.params.len() + 1,
                        u64::from(sig.variadic) * 2
                            + u64::from(sig.convention == crate::CallingConvention::SysV64),
                    )
                },
                Type::Array(inner, count) => {
                    pending.push((inner, depth + 1));
                    (19, 1, *count)
                },
                Type::Struct(fields) | Type::Tuple(fields) => {
                    pending.extend(fields.iter().rev().map(|ty| (ty, depth + 1)));
                    (if matches!(ty, Type::Struct(_)) { 20 } else { 21 }, fields.len(), 0)
                },
            };
            nodes.push(Node { kind, depth, children: children as u64, auxiliary });
        }
        Ok(nodes)
    }

    pub(super) fn preflight(&self, ty: &Type) -> Result<(), BuilderError> {
        self.preflight_value(ty, 0, 0)
    }

    pub(super) fn preflight_value(
        &self,
        ty: &Type,
        operands: usize,
        targets: usize,
    ) -> Result<(), BuilderError> {
        self.body.type_room(ty, operands.saturating_add(targets))?;
        let nodes = self.flatten(ty)?;
        if nodes.len().saturating_add(2).saturating_add(operands).saturating_add(targets)
            > self.limits.max_nodes.saturating_sub(self.used_nodes())
        {
            return Err(self.limit(Resource::Nodes, self.limits.max_nodes));
        }
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_builder_type(
                nodes.as_ptr(),
                nodes.len() as u64,
                self.limits.max_type_depth as u64,
                self.limits.max_nodes.saturating_sub(self.used_nodes()) as u64,
                self.limits.max_analysis_work.saturating_sub(self.state.work as usize) as u64,
                &mut result,
            )
        };
        if status != 0 || result.status != 0 {
            return Err(self.error(status, result.detail));
        }
        // Allocation/copy and identity publication have their own reserved work.
        if nodes
            .len()
            .saturating_mul(2)
            .saturating_add(2)
            .saturating_add(operands)
            .saturating_add(targets)
            > self.limits.max_analysis_work.saturating_sub(self.state.work as usize)
        {
            return Err(self.limit(Resource::AnalysisWork, self.limits.max_analysis_work));
        }
        Ok(())
    }

    pub(super) fn value(&mut self, ty: &Type) -> Result<ValueId, BuilderError> {
        let nodes = self.flatten(ty)?;
        let result = self.allocate(0, &nodes)?;
        Ok(ValueId(result as u32))
    }

    pub(super) fn block(&mut self) -> Result<BlockId, BuilderError> {
        let id = BlockId(self.allocate(1, &[])? as u32);
        self.body.block(id);
        Ok(id)
    }

    fn allocate(&mut self, kind: u64, nodes: &[Node]) -> Result<u64, BuilderError> {
        let before = self.state;
        if before.type_count != self.types.len() as u64
            || before.next_value != self.values.len() as u64
            || before.next_block != self.blocks.len() as u64
        {
            return Err(BuilderError::InvalidType);
        }
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_builder_allocate(
                &mut self.state,
                kind,
                nodes.as_ptr(),
                nodes.len() as u64,
                self.limits.max_type_depth as u64,
                self.limits.max_nodes as u64,
                self.limits.max_analysis_work as u64,
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                &mut result,
            )
        };
        if status != result.status as i32 || self.state != before {
            self.state = before;
            return Err(BuilderError::InvalidType);
        }
        if status != 0 {
            return Err(self.error(status, result.detail));
        }
        let value = if kind == 0 { before.next_value } else { before.next_block };
        let work = result.work;
        if result.value != value
            || work > (self.limits.max_analysis_work as u64).saturating_sub(before.work)
        {
            return Err(BuilderError::InvalidType);
        }
        let mut expected = before;
        if kind == 0 {
            expected.type_count = expected
                .type_count
                .checked_add(nodes.len() as u64)
                .ok_or(BuilderError::InvalidType)?;
            expected.next_value =
                expected.next_value.checked_add(1).ok_or(BuilderError::IdentityOverflow)?;
            self.types
                .try_reserve(nodes.len())
                .map_err(|_| self.limit(Resource::Nodes, self.limits.max_nodes))?;
            self.values
                .try_reserve(1)
                .map_err(|_| self.limit(Resource::Nodes, self.limits.max_nodes))?;
            self.types.resize(expected.type_count as usize, Node::default());
            self.values.resize(expected.next_value as usize, Value::default());
        } else {
            expected.next_block =
                expected.next_block.checked_add(1).ok_or(BuilderError::IdentityOverflow)?;
            self.blocks
                .try_reserve(1)
                .map_err(|_| self.limit(Resource::Nodes, self.limits.max_nodes))?;
            self.blocks.resize(expected.next_block as usize, Block::default());
        }
        expected.work = expected.work.checked_add(work).ok_or(BuilderError::InvalidType)?;
        let status = unsafe {
            whale_ir_builder_allocate(
                &mut self.state,
                kind,
                nodes.as_ptr(),
                nodes.len() as u64,
                self.limits.max_type_depth as u64,
                self.limits.max_nodes as u64,
                self.limits.max_analysis_work as u64,
                1,
                self.types.as_mut_ptr(),
                self.types.len() as u64,
                self.values.as_mut_ptr(),
                self.values.len() as u64,
                self.blocks.as_mut_ptr(),
                self.blocks.len() as u64,
                &mut result,
            )
        };
        if status != 0
            || result.status != 0
            || self.state != expected
            || result.value != value
            || result.work != work
        {
            self.types.truncate(before.type_count as usize);
            self.values.truncate(before.next_value as usize);
            self.blocks.truncate(before.next_block as usize);
            self.state = before;
            return Err(BuilderError::InvalidType);
        }
        Ok(value)
    }

    pub(super) fn next_value(&self) -> u32 {
        self.state.next_value as u32
    }

    pub(super) fn next_block(&self) -> u32 {
        self.state.next_block as u32
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn order(
        &self,
        first_block: u32,
        first_value: u32,
        block: BlockId,
        op: u64,
        operands: &[u64],
        targets: &[u64],
        check_only: bool,
    ) -> Result<(), BuilderError> {
        assert!(check_only);
        let mut result = ResultRecord::default();
        // The checked query mode never writes state or block storage.
        let status = unsafe {
            whale_ir_builder_order(
                (&self.state as *const State).cast_mut(),
                self.limits.max_nodes as u64,
                self.blocks.as_ptr().cast_mut(),
                self.blocks.len() as u64,
                first_block.into(),
                first_value.into(),
                self.state.next_value,
                block.0.into(),
                op,
                operands.as_ptr(),
                operands.len() as u64,
                targets.as_ptr(),
                targets.len() as u64,
                1,
                self.limits.max_analysis_work as u64,
                &mut result,
            )
        };
        if status != 0 || result.status != 0 {
            return Err(self.error(status, result.detail));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn mark(
        &mut self,
        first_block: u32,
        first_value: u32,
        block: BlockId,
        op: u64,
        operands: &[u64],
        targets: &[u64],
    ) -> Result<(), BuilderError> {
        let mut result = ResultRecord::default();
        let value_end = self.state.next_value;
        let status = unsafe {
            whale_ir_builder_order(
                &mut self.state,
                self.limits.max_nodes as u64,
                self.blocks.as_mut_ptr(),
                self.blocks.len() as u64,
                first_block.into(),
                first_value.into(),
                value_end,
                block.0.into(),
                op,
                operands.as_ptr(),
                operands.len() as u64,
                targets.as_ptr(),
                targets.len() as u64,
                0,
                self.limits.max_analysis_work as u64,
                &mut result,
            )
        };
        if status != 0 || result.status != 0 {
            return Err(self.error(status, result.detail));
        }
        Ok(())
    }
}

impl Builder {
    pub(super) fn preflight_instruction(
        &self,
        operands: usize,
        targets: usize,
    ) -> Result<(), BuilderError> {
        let cost = operands.saturating_add(targets).saturating_add(1);
        self.body.room(cost.saturating_add(64), 0)?;
        if cost > self.limits.max_nodes.saturating_sub(self.used_nodes()) {
            return Err(self.limit(Resource::Nodes, self.limits.max_nodes));
        }
        if cost > self.limits.max_analysis_work.saturating_sub(self.state.work as usize) {
            return Err(self.limit(Resource::AnalysisWork, self.limits.max_analysis_work));
        }
        Ok(())
    }

    pub(super) fn instruction(
        &mut self,
        first_block: u32,
        first_value: u32,
        block: BlockId,
        instruction: &crate::Instruction,
        check_only: bool,
    ) -> Result<(), BuilderError> {
        use crate::{Callee, Instruction as I};
        // Bound variable-length borrowed transports before allocating any list.
        match instruction {
            I::Phi { incomings, .. } => {
                self.preflight_instruction(incomings.len(), incomings.len())?
            },
            I::Gep { indices, .. } => {
                self.preflight_instruction(indices.len().saturating_add(1), 0)?
            },
            I::Call { args, callee, .. } => self.preflight_instruction(
                args.len().saturating_add(usize::from(matches!(callee, Callee::Indirect(_)))),
                0,
            )?,
            _ => {},
        }
        let mut targets = Vec::new();
        let (op, values): (u64, Vec<ValueId>) = match instruction {
            I::GlobalAddr { .. } => (0, vec![]),
            I::LifetimeEnd { ptr } => (1, vec![*ptr]),
            I::PtrDiff { lhs, rhs, .. } => (2, vec![*lhs, *rhs]),
            I::ConstDecl { .. } => (3, vec![]),
            I::Const { .. } => (4, vec![]),
            I::Undef { .. } => (5, vec![]),
            I::Mov { src, .. } => (6, vec![*src]),
            I::Bin { lhs, rhs, .. } => (7, vec![*lhs, *rhs]),
            I::Neg { src, .. } => (8, vec![*src]),
            I::Not { src, .. } => (9, vec![*src]),
            I::Cmp { lhs, rhs, .. } => (10, vec![*lhs, *rhs]),
            I::ICmp { lhs, rhs, .. } => (11, vec![*lhs, *rhs]),
            I::FCmp { lhs, rhs, .. } => (12, vec![*lhs, *rhs]),
            I::Select { cond, on_true, on_false, .. } => (13, vec![*cond, *on_true, *on_false]),
            I::Cast { src, .. } => (14, vec![*src]),
            I::Phi { incomings, .. } => {
                targets.extend(incomings.iter().map(|(_, block)| u64::from(block.0)));
                (15, incomings.iter().map(|(value, _)| *value).collect())
            },
            I::Extract { tuple, .. } => (16, vec![*tuple]),
            I::Checked { lhs, rhs, .. } => (17, vec![*lhs, *rhs]),
            I::Alloca { .. } => (18, vec![]),
            I::VolatileLoad { ptr, .. } => (19, vec![*ptr]),
            I::VolatileStore { value, ptr, .. } => (20, vec![*value, *ptr]),
            I::Load { ptr, .. } => (21, vec![*ptr]),
            I::Uninit { ptr, .. } => (22, vec![*ptr]),
            I::Store { value, ptr, .. } => (23, vec![*value, *ptr]),
            I::Gep { base_ptr, indices, .. } => {
                let mut values = vec![*base_ptr];
                values.extend(indices);
                (24, values)
            },
            I::Memcpy { dst, src, n, .. } => (25, vec![*dst, *src, *n]),
            I::Memset { dst, val, n, .. } => (26, vec![*dst, *val, *n]),
            I::NullFunction { .. } => (27, vec![]),
            I::FunctionAddr { .. } => (28, vec![]),
            I::Call { callee, args, .. } => {
                let mut values = Vec::new();
                if let Callee::Indirect(value) = callee {
                    values.push(*value);
                }
                values.extend(args);
                (29, values)
            },
            I::TrapIf { cond, .. } => (30, vec![*cond]),
        };
        let values: Vec<u64> = values.into_iter().map(|id| id.0.into()).collect();
        if check_only {
            self.body.instruction(block, instruction, false, true)?;
            self.order(first_block, first_value, block, op, &values, &targets, true)
        } else {
            self.order(first_block, first_value, block, op, &values, &targets, true)?;
            self.body.instruction(block, instruction, false, false)?;
            self.mark(first_block, first_value, block, op, &values, &targets)
        }
    }

    pub(super) fn terminator(
        &mut self,
        first_block: u32,
        first_value: u32,
        block: BlockId,
        term: &crate::Terminator,
        check_only: bool,
    ) -> Result<(), BuilderError> {
        use crate::Terminator as T;
        if let T::Switch { cases, .. } = term {
            self.preflight_instruction(1, cases.len().saturating_add(1))?;
        }
        let (op, values, targets): (u64, Vec<u64>, Vec<u64>) = match term {
            T::Br { target } => (31, vec![], vec![target.0.into()]),
            T::CBr { cond, then_bb, else_bb } => {
                (32, vec![cond.0.into()], vec![then_bb.0.into(), else_bb.0.into()])
            },
            T::Switch { value, default_bb, cases, .. } => {
                let mut targets = vec![default_bb.0.into()];
                targets.extend(cases.iter().map(|(_, block)| u64::from(block.0)));
                (32, vec![value.0.into()], targets)
            },
            T::Ret { value, .. } => (33, value.iter().map(|id| u64::from(id.0)).collect(), vec![]),
            T::Trap { .. } => (33, vec![], vec![]),
        };
        if check_only {
            self.body.terminator(block, term, true)?;
            self.order(first_block, first_value, block, op, &values, &targets, true)
        } else {
            self.order(first_block, first_value, block, op, &values, &targets, true)?;
            self.body.terminator(block, term, false)?;
            self.mark(first_block, first_value, block, op, &values, &targets)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(builder: &Builder) -> (State, Vec<Node>, Vec<Value>, Vec<Block>) {
        (builder.state, builder.types.clone(), builder.values.clone(), builder.blocks.clone())
    }

    #[test]
    fn exact_inventory_preserves_signed_and_aggregate_identity() {
        let mut builder = Builder::new(IrLimits::default());
        let signature = crate::FunctionSignature {
            ret: Type::I32,
            params: vec![Type::U32, Type::ptr_to(Type::I32)],
            convention: crate::CallingConvention::SysV64,
            variadic: true,
        };
        let types = [
            Type::Void,
            Type::Bool,
            Type::I1,
            Type::U1,
            Type::I8,
            Type::I16,
            Type::I32,
            Type::I64,
            Type::I128,
            Type::U8,
            Type::U16,
            Type::U32,
            Type::U64,
            Type::U128,
            Type::F16,
            Type::F32,
            Type::F64,
            Type::ptr_to(Type::I32),
            Type::FnPtr(Box::new(signature)),
            Type::Array(Box::new(Type::I8), 23),
            Type::Struct(vec![Type::I32, Type::U32]),
            Type::Tuple(vec![Type::Bool, Type::I64]),
        ];
        for (index, ty) in types.iter().enumerate() {
            let id = builder.value(ty).unwrap();
            assert_eq!(id.0 as usize, index);
        }
        let kinds: Vec<_> = builder
            .values
            .iter()
            .map(|value| builder.types[value.first_type as usize].kind)
            .collect();
        assert_eq!(kinds, (0..=21).collect::<Vec<_>>());
        let function = builder.values[18];
        let node = builder.types[function.first_type as usize];
        assert_eq!((node.kind, node.children, node.auxiliary), (18, 3, 3));
        assert_eq!(builder.types[function.first_type as usize + 1].kind, 6);
    }

    #[test]
    fn budgets_are_transactional_and_include_copied_nodes_and_operations() {
        let mut builder =
            Builder::new(IrLimits { max_nodes: 4, max_analysis_work: 5, ..IrLimits::default() });
        let block = builder.block().unwrap();
        builder.preflight_value(&Type::I32, 0, 0).unwrap();
        let value = builder.value(&Type::I32).unwrap();
        builder.mark(0, 0, block, 4, &[], &[]).unwrap();
        assert_eq!(value, ValueId(0));
        assert_eq!(builder.used_nodes(), 4);
        assert_eq!(builder.state.work, 5);
        let before = snapshot(&builder);
        assert!(matches!(builder.value(&Type::I32), Err(BuilderError::ResourceLimit(_))));
        assert!(matches!(builder.block(), Err(BuilderError::ResourceLimit(_))));
        assert!(matches!(
            builder.mark(0, 0, block, 4, &[], &[]),
            Err(BuilderError::ResourceLimit(_))
        ));
        builder.order(0, 0, block, 35, &[], &[], true).unwrap();
        assert_eq!(snapshot(&builder), before);
    }

    #[test]
    fn rejected_types_and_ordering_never_consume_an_identity_or_mutate_records() {
        let mut builder = Builder::new(IrLimits { max_type_depth: 2, ..IrLimits::default() });
        let block = builder.block().unwrap();
        let value = builder.value(&Type::I32).unwrap();
        let before = snapshot(&builder);
        let deep = Type::ptr_to(Type::ptr_to(Type::ptr_to(Type::I32)));
        assert!(matches!(builder.value(&deep), Err(BuilderError::ResourceLimit(_))));
        assert_eq!(
            builder.order(0, 0, block, 7, &[value.0.into(), 99], &[], true),
            Err(BuilderError::UnknownValue(ValueId(99)))
        );
        assert_eq!(
            builder.order(0, 0, block, 31, &[], &[99], true),
            Err(BuilderError::UnknownBlock(BlockId(99)))
        );
        assert_eq!(snapshot(&builder), before);
        builder.mark(0, 0, block, 15, &[value.0.into()], &[block.0.into()]).unwrap();
        builder.mark(0, 0, block, 4, &[], &[]).unwrap();
        let before = snapshot(&builder);
        assert_eq!(
            builder.mark(0, 0, block, 15, &[value.0.into()], &[block.0.into()]),
            Err(BuilderError::PhiAfterInstruction(block))
        );
        assert_eq!(snapshot(&builder), before);
        builder.mark(0, 0, block, 33, &[], &[]).unwrap();
        let before = snapshot(&builder);
        assert_eq!(
            builder.order(0, 0, block, 35, &[], &[], true),
            Err(BuilderError::TerminatedBlock(block))
        );
        assert_eq!(snapshot(&builder), before);
    }

    #[test]
    fn identities_from_previous_functions_are_rejected() {
        let mut builder = Builder::new(IrLimits::default());
        let old_block = builder.block().unwrap();
        let old_value = builder.value(&Type::I32).unwrap();
        let first_block = builder.next_block();
        let first_value = builder.next_value();
        let block = builder.block().unwrap();
        let value = builder.value(&Type::U32).unwrap();
        assert_eq!(
            builder.order(first_block, first_value, old_block, 34, &[], &[], true),
            Err(BuilderError::UnknownBlock(old_block))
        );
        assert_eq!(
            builder.order(
                first_block,
                first_value,
                block,
                7,
                &[value.0.into(), old_value.0.into()],
                &[],
                true
            ),
            Err(BuilderError::UnknownValue(old_value))
        );
    }
}
