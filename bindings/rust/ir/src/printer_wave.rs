// SPDX-License-Identifier: MPL-2.0
//! Allocation and borrowed-model transport for the Wave canonical printer.
//! All text grammar, numeric spelling and escaping are owned by printer.wave.

use super::{PrintError, PrintLimits};

use crate::*;

use std::io::Write;

const NONE: u64 = u64::MAX;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Node([u64; 12]);

#[repr(C)]
struct State {
    nodes: *const Node,
    node_count: u64,
    edges: *const u64,
    edge_count: u64,
    stack: *mut [u64; 10],
    stack_capacity: u64,
    stack_len: u64,
    output_limit: u64,
    total: u64,
    work: u64,
    depth_limit: u64,
    status: u64,
    initialized: u64,
}

unsafe extern "C" {
    fn whale_ir_print_step(
        state: *mut State,
        root: u64,
        output: *mut u8,
        capacity: u64,
        written: *mut u64,
    ) -> i32;
}

macro_rules! node {
    ($self:ident, $tag:expr $(, $field:expr)* $(,)?) => {{
        let fields = [$($field),*];
        $self.add($tag, &fields)
    }};
}

struct Transport {
    nodes: Vec<Node>,
    edges: Vec<u64>,
    limits: PrintLimits,
    type_depth: usize,
    expression_depth: usize,
}

impl Transport {
    fn new(limits: PrintLimits) -> Self {
        Self { nodes: Vec::new(), edges: Vec::new(), limits, type_depth: 0, expression_depth: 0 }
    }

    fn add(&mut self, tag: u64, fields: &[u64]) -> Result<u64, PrintError> {
        if self.nodes.len() >= self.limits.max_nodes {
            return Err(PrintError::Nodes);
        }
        self.nodes.try_reserve(1).map_err(|_| PrintError::Allocation)?;
        let mut n = Node::default();
        n.0[0] = tag;
        n.0[1..1 + fields.len()].copy_from_slice(fields);
        let id = self.nodes.len() as u64;
        self.nodes.push(n);
        Ok(id)
    }

    fn string(&mut self, value: &str) -> Result<u64, PrintError> {
        node!(self, 30, value.as_ptr() as u64, value.len() as u64)
    }

    fn list(&mut self, values: &[u64]) -> Result<(u64, u64), PrintError> {
        if values.len() > self.limits.max_nodes.saturating_sub(self.edges.len()) {
            return Err(PrintError::Nodes);
        }
        self.edges.try_reserve(values.len()).map_err(|_| PrintError::Allocation)?;
        let start = self.edges.len() as u64;
        self.edges.extend_from_slice(values);
        Ok((start, values.len() as u64))
    }

    fn collect<T>(
        &mut self,
        items: &[T],
        mut visit: impl FnMut(&mut Self, &T) -> Result<u64, PrintError>,
    ) -> Result<(u64, u64), PrintError> {
        if items.len() > self.limits.max_nodes.saturating_sub(self.nodes.len()) {
            return Err(PrintError::Nodes);
        }
        let mut values = Vec::new();
        values.try_reserve(items.len()).map_err(|_| PrintError::Allocation)?;
        for item in items {
            values.push(visit(self, item)?);
        }
        self.list(&values)
    }

    fn ty(&mut self, ty: &Type) -> Result<u64, PrintError> {
        if self.type_depth > self.limits.max_depth {
            return Err(PrintError::Depth);
        }
        self.type_depth += 1;
        let result = match ty {
            Type::Void => self.add(1, &[]),
            Type::Bool => self.add(2, &[]),
            Type::I1 => self.add(3, &[]),
            Type::U1 => self.add(4, &[]),
            Type::I8 => self.add(5, &[]),
            Type::I16 => self.add(6, &[]),
            Type::I32 => self.add(7, &[]),
            Type::I64 => self.add(8, &[]),
            Type::I128 => self.add(9, &[]),
            Type::U8 => self.add(10, &[]),
            Type::U16 => self.add(11, &[]),
            Type::U32 => self.add(12, &[]),
            Type::U64 => self.add(13, &[]),
            Type::U128 => self.add(14, &[]),
            Type::F16 => self.add(15, &[]),
            Type::F32 => self.add(16, &[]),
            Type::F64 => self.add(17, &[]),
            Type::Ptr(inner) => node!(self, 18, self.ty(inner)?),
            Type::FnPtr(signature) => node!(self, 19, self.signature(signature)?),
            Type::Array(inner, count) => node!(self, 20, self.ty(inner)?, *count),
            Type::Struct(fields) | Type::Tuple(fields) => {
                let (first, count) = self.collect(fields, Self::ty)?;
                node!(self, if matches!(ty, Type::Struct(_)) { 21 } else { 22 }, first, count)
            },
        };
        self.type_depth -= 1;
        result
    }

    fn signature(&mut self, signature: &FunctionSignature) -> Result<u64, PrintError> {
        let (first, count) = self.collect(&signature.params, Self::ty)?;
        node!(
            self,
            23,
            convention(signature.convention),
            first,
            count,
            self.ty(&signature.ret)?,
            u64::from(signature.variadic)
        )
    }

    fn constant(&mut self, value: &ConstValue) -> Result<u64, PrintError> {
        match value {
            ConstValue::Bool(value) => node!(self, 31, u64::from(*value)),
            ConstValue::I(value) => node!(self, 32, *value as u64, (*value >> 64) as u64),
            ConstValue::U(value) => node!(self, 33, *value as u64, (*value >> 64) as u64),
            ConstValue::F(value) => node!(self, 34, value.bits(), u64::from(value.width())),
        }
    }

    fn expression(&mut self, expression: &ConstExpr) -> Result<u64, PrintError> {
        if self.expression_depth > self.limits.max_depth {
            return Err(PrintError::Depth);
        }
        self.expression_depth += 1;
        let result = match &expression.kind {
            ConstExprKind::Literal(value) => {
                node!(self, 40, self.ty(&expression.ty)?, self.constant(value)?)
            },
            ConstExprKind::Reference(reference) => {
                let reference = match reference {
                    ConstRef::Global(id) => node!(self, 37, u64::from(id.0))?,
                    ConstRef::Local(id) => node!(self, 35, u64::from(id.0))?,
                };
                node!(self, 41, self.ty(&expression.ty)?, reference)
            },
            ConstExprKind::Neg(value) => node!(self, 42, self.expression(value)?),
            ConstExprKind::Not(value) => node!(self, 43, self.expression(value)?),
            ConstExprKind::Binary { op, left, right } => node!(
                self,
                44,
                binary_constant(op),
                self.expression(left)?,
                self.expression(right)?
            ),
            ConstExprKind::Compare { op, left, right } => node!(
                self,
                45,
                compare_constant(op),
                self.expression(left)?,
                self.expression(right)?
            ),
        };
        self.expression_depth -= 1;
        result
    }

    fn group(&mut self, first: u64, count: u64, gap: bool) -> Result<u64, PrintError> {
        node!(self, 51, first, count, u64::from(gap))
    }

    fn module(&mut self, module: &Module) -> Result<u64, PrintError> {
        let target = self.string(&module.target)?;
        let (first, count) = self.collect(&module.sources.files, |this, file| {
            node!(this, 52, u64::from(file.id), this.string(&file.name)?)
        })?;
        let files = self.group(first, count, false)?;
        let (first, count) = self.collect(&module.sources.originals, |this, original| {
            let span = if let Some(span) = &original.span {
                node!(
                    this,
                    54,
                    u64::from(span.file),
                    u64::from(span.line),
                    u64::from(span.column),
                    u64::from(span.end_line),
                    u64::from(span.end_column)
                )?
            } else {
                NONE
            };
            node!(this, 53, original.id, span)
        })?;
        let originals = self.group(first, count, false)?;
        let (first, count) =
            self.collect(&module.sources.mappings, |this, mapping| match mapping.target {
                SourceTarget::Instruction { function, block, index } => node!(
                    this,
                    55,
                    u64::from(function.0),
                    u64::from(block.0),
                    index as u64,
                    mapping.original
                ),
                SourceTarget::Terminator { function, block } => {
                    node!(this, 56, u64::from(function.0), u64::from(block.0), mapping.original)
                },
                SourceTarget::Global(id) => node!(this, 57, u64::from(id.0), mapping.original),
            })?;
        let mappings = self.group(first, count, false)?;
        let (first, count) = self.collect(&module.globals, |this, global| {
            node!(
                this,
                58,
                u64::from(global.id.0),
                this.string(&global.name)?,
                this.ty(&global.ty)?,
                this.constant(&global.init)?,
                u64::from(global.align),
                this.expression(&global.init_expr)?
            )
        })?;
        let globals = self.group(first, count, true)?;
        let (first, count) = self.collect(&module.global_storage, |this, global| {
            let init = match &global.init {
                GlobalStorageInit::Value(value) => this.constant(value)?,
                GlobalStorageInit::Uninitialized => NONE,
            };
            let link_name = match &global.link_name {
                Some(name) => this.string(name)?,
                None => NONE,
            };
            node!(
                this,
                59,
                u64::from(global.id.0),
                this.string(&global.name)?,
                this.ty(&global.ty)?,
                init,
                u64::from(global.align),
                u64::from(global.readonly),
                linkage(global.linkage),
                link_name
            )
        })?;
        let storage = self.group(first, count, true)?;
        let (first, count) = self.collect(&module.declarations, |this, declaration| {
            let link_name = match &declaration.link_name {
                Some(name) => this.string(name)?,
                None => NONE,
            };
            node!(
                this,
                60,
                u64::from(declaration.id.0),
                this.string(&declaration.name)?,
                this.signature(&declaration.signature)?,
                linkage(declaration.linkage),
                link_name
            )
        })?;
        let declarations = self.group(first, count, true)?;
        let (first, count) = self.collect(&module.functions, Self::function)?;
        let functions = self.group(first, count, false)?;
        let (first, count) = self.list(&[globals, storage, declarations, functions])?;
        let entities = self.group(first, count, false)?;
        node!(
            self,
            50,
            target,
            u64::from(module.datalayout.ptr_bits),
            if module.datalayout.endian == Endian::Little { 0 } else { 1 },
            files,
            originals,
            mappings,
            entities,
            u64::from(crate::IR_FORMAT_VERSION),
            u64::from(crate::SEMANTICS_VERSION)
        )
    }

    fn function(&mut self, function: &Function) -> Result<u64, PrintError> {
        let (params, param_count) = self.collect(&function.params, |this, param| {
            node!(this, 62, u64::from(param.id.0), this.string(&param.name)?, this.ty(&param.ty)?)
        })?;
        let (blocks, block_count) = self.collect(&function.blocks, |this, block| {
            let (instructions, instruction_count) =
                this.collect(&block.instructions, Self::instruction)?;
            let term = match &block.terminator {
                Some(term) => this.terminator(term)?,
                None => NONE,
            };
            node!(
                this,
                63,
                u64::from(block.id.0),
                this.string(&block.name)?,
                instructions,
                instruction_count,
                term
            )
        })?;
        node!(
            self,
            61,
            u64::from(function.id.0),
            self.string(&function.name)?,
            params,
            param_count,
            self.ty(&function.ret_ty)?,
            u64::from(function.entry.0),
            blocks,
            block_count
        )
    }

    fn instruction(&mut self, instruction: &Instruction) -> Result<u64, PrintError> {
        use Instruction::*;
        let v = |value: &ValueId| u64::from(value.0);
        match instruction {
            GlobalAddr { dst, global, ty } => {
                node!(self, 100, v(dst), u64::from(global.0), self.ty(ty)?)
            },
            LifetimeEnd { ptr } => node!(self, 101, v(ptr)),
            PtrDiff { dst, lhs, rhs } => node!(self, 102, v(dst), v(lhs), v(rhs)),
            ConstDecl { dst, name, expression, value } => node!(
                self,
                103,
                v(dst),
                self.string(name)?,
                self.expression(expression)?,
                self.constant(value)?,
                self.ty(&expression.ty)?
            ),
            Const { dst, ty, value } => {
                node!(self, 104, v(dst), self.ty(ty)?, self.constant(value)?)
            },
            Undef { dst, ty } => node!(self, 105, v(dst), self.ty(ty)?),
            Mov { dst, ty, src } => node!(self, 106, v(dst), self.ty(ty)?, v(src)),
            Bin { dst, op, ty, lhs, rhs } => {
                node!(self, 107, v(dst), binop(op), self.ty(ty)?, v(lhs), v(rhs))
            },
            Neg { dst, ty, src } => node!(self, 108, v(dst), self.ty(ty)?, v(src)),
            Not { dst, ty, src } => node!(self, 109, v(dst), self.ty(ty)?, v(src)),
            Cmp { dst, op, ty, lhs, rhs } => {
                node!(self, 110, v(dst), cmpop(op), self.ty(ty)?, v(lhs), v(rhs))
            },
            ICmp { dst, pred, ty, lhs, rhs } => {
                node!(self, 111, v(dst), icmp(pred), self.ty(ty)?, v(lhs), v(rhs))
            },
            FCmp { dst, pred, ty, lhs, rhs } => {
                node!(self, 112, v(dst), fcmp(pred), self.ty(ty)?, v(lhs), v(rhs))
            },
            Select { dst, ty, cond, on_true, on_false } => {
                node!(self, 113, v(dst), self.ty(ty)?, v(cond), v(on_true), v(on_false))
            },
            Cast { dst, op, dst_ty, src_ty, src } => {
                node!(self, 114, v(dst), cast(op), self.ty(dst_ty)?, self.ty(src_ty)?, v(src))
            },
            Phi { dst, ty, incomings } => {
                let (first, count) = self.collect(incomings, |this, (value, block)| {
                    node!(this, 140, v(value), u64::from(block.0))
                })?;
                node!(self, 115, v(dst), self.ty(ty)?, first, count)
            },
            Extract { dst, dst_ty, tuple, index } => {
                node!(self, 116, v(dst), self.ty(dst_ty)?, v(tuple), u64::from(*index))
            },
            Checked { dst, op, ty, lhs, rhs } => {
                node!(self, 117, v(dst), checked(op), self.ty(ty)?, v(lhs), v(rhs))
            },
            Alloca { dst, ty, align } => node!(self, 118, v(dst), self.ty(ty)?, u64::from(*align)),
            VolatileLoad { dst, ty, ptr, align } => {
                node!(self, 119, v(dst), self.ty(ty)?, v(ptr), u64::from(*align))
            },
            VolatileStore { ty, value, ptr, align } => {
                node!(self, 120, self.ty(ty)?, v(value), v(ptr), u64::from(*align))
            },
            Load { dst, ty, ptr, align } => {
                node!(self, 121, v(dst), self.ty(ty)?, v(ptr), u64::from(*align))
            },
            Uninit { ty, ptr, align } => node!(self, 122, self.ty(ty)?, v(ptr), u64::from(*align)),
            Store { ty, value, ptr, align } => {
                node!(self, 123, self.ty(ty)?, v(value), v(ptr), u64::from(*align))
            },
            Gep { dst, dst_ty, base_ptr, indices } => {
                let (first, count) =
                    self.collect(indices, |this, index| node!(this, 35, v(index)))?;
                node!(self, 124, v(dst), self.ty(dst_ty)?, v(base_ptr), first, count)
            },
            Memcpy { dst, src, n, align } => {
                node!(self, 125, v(dst), v(src), v(n), u64::from(*align))
            },
            Memset { dst, val, n, align } => {
                node!(self, 126, v(dst), v(val), v(n), u64::from(*align))
            },
            NullFunction { dst, signature } => node!(self, 127, v(dst), self.signature(signature)?),
            FunctionAddr { dst, function, signature } => {
                node!(self, 128, v(dst), u64::from(function.0), self.signature(signature)?)
            },
            Call { convention: cc, dst, ret_ty, callee, args } => {
                let callee = match callee {
                    Callee::Direct(id) => node!(self, 141, u64::from(id.0))?,
                    Callee::Indirect(value) => node!(self, 142, v(value))?,
                };
                let (first, count) = self.collect(args, |this, arg| node!(this, 35, v(arg)))?;
                node!(
                    self,
                    129,
                    convention(*cc),
                    dst.map(|id| u64::from(id.0)).unwrap_or(NONE),
                    self.ty(ret_ty)?,
                    callee,
                    first,
                    count
                )
            },
            TrapIf { cond, reason } => node!(self, 130, v(cond), self.string(reason)?),
        }
    }

    fn terminator(&mut self, term: &Terminator) -> Result<u64, PrintError> {
        match term {
            Terminator::Br { target } => node!(self, 150, u64::from(target.0)),
            Terminator::CBr { cond, then_bb, else_bb } => {
                node!(self, 151, u64::from(cond.0), u64::from(then_bb.0), u64::from(else_bb.0))
            },
            Terminator::Switch { ty, value, default_bb, cases } => {
                let (first, count) = self.collect(cases, |this, (constant, block)| {
                    node!(this, 155, this.constant(constant)?, u64::from(block.0))
                })?;
                node!(
                    self,
                    152,
                    self.ty(ty)?,
                    u64::from(value.0),
                    u64::from(default_bb.0),
                    first,
                    count
                )
            },
            Terminator::Ret { ty, value } => {
                node!(
                    self,
                    153,
                    self.ty(ty)?,
                    value.map(|value| u64::from(value.0)).unwrap_or(NONE)
                )
            },
            Terminator::Trap { reason } => node!(self, 154, self.string(reason)?),
        }
    }

    fn write(&self, root: u64, writer: &mut impl Write) -> Result<(), PrintError> {
        let mut stack = Vec::new();
        let capacity =
            self.limits.max_stack.min(self.nodes.len().saturating_mul(32).saturating_add(128));
        stack.try_reserve_exact(capacity).map_err(|_| PrintError::Allocation)?;
        stack.resize(capacity, [0u64; 10]);
        let mut state = State {
            nodes: self.nodes.as_ptr(),
            node_count: self.nodes.len() as u64,
            edges: self.edges.as_ptr(),
            edge_count: self.edges.len() as u64,
            stack: stack.as_mut_ptr(),
            stack_capacity: stack.len() as u64,
            stack_len: 0,
            output_limit: self.limits.max_output_bytes as u64,
            total: 0,
            work: self.limits.max_work as u64,
            depth_limit: 1024,
            status: 0,
            initialized: 0,
        };
        let mut buffer = [0u8; 4096];
        loop {
            let mut written = 0;
            // SAFETY: all model spans are borrowed from the live module; the
            // mutable task and output spans are exclusively owned by this call.
            let status = unsafe {
                whale_ir_print_step(
                    &mut state,
                    root,
                    buffer.as_mut_ptr(),
                    buffer.len() as u64,
                    &mut written,
                )
            };
            if written > buffer.len() as u64 || state.total > self.limits.max_output_bytes as u64 {
                return Err(PrintError::Backend);
            }
            if status != 0 && status != 6 {
                return Err(match status {
                    2 => PrintError::Output,
                    3 => PrintError::Stack,
                    4 => PrintError::Work,
                    5 => PrintError::Depth,
                    _ => PrintError::Backend,
                });
            }
            writer.write_all(&buffer[..written as usize]).map_err(PrintError::Io)?;
            if status == 0 {
                return Ok(());
            }
        }
    }
}

pub(super) fn write_module(
    module: &Module,
    writer: &mut impl Write,
    limits: PrintLimits,
) -> Result<(), PrintError> {
    limits.validate()?;
    let mut model = Transport::new(limits);
    let root = model.module(module)?;
    model.write(root, writer)
}

pub(super) fn write_expression(
    expression: &ConstExpr,
    writer: &mut impl Write,
    limits: PrintLimits,
) -> Result<(), PrintError> {
    limits.validate()?;
    let mut model = Transport::new(limits);
    let root = model.expression(expression)?;
    model.write(root, writer)
}

fn convention(value: CallingConvention) -> u64 {
    match value {
        CallingConvention::Whale => 0,
        CallingConvention::SysV64 => 1,
    }
}

fn linkage(value: Linkage) -> u64 {
    match value {
        Linkage::Internal => 0,
        Linkage::External => 1,
    }
}

fn binary_constant(value: &ConstBinaryOp) -> u64 {
    match value {
        ConstBinaryOp::Add => 0,
        ConstBinaryOp::Sub => 1,
        ConstBinaryOp::Mul => 2,
        ConstBinaryOp::BitAnd => 3,
        ConstBinaryOp::BitOr => 4,
        ConstBinaryOp::BitXor => 5,
        ConstBinaryOp::Div => 6,
        ConstBinaryOp::Rem => 7,
        ConstBinaryOp::ShiftLeft => 8,
        ConstBinaryOp::ShiftRight => 9,
        ConstBinaryOp::LogicalAnd => 10,
        ConstBinaryOp::LogicalOr => 11,
    }
}

fn compare_constant(value: &ConstCompareOp) -> u64 {
    match value {
        ConstCompareOp::Eq => 0,
        ConstCompareOp::Ne => 1,
        ConstCompareOp::Lt => 2,
        ConstCompareOp::Le => 3,
        ConstCompareOp::Gt => 4,
        ConstCompareOp::Ge => 5,
    }
}

fn binop(value: &BinOp) -> u64 {
    match value {
        BinOp::Add => 0,
        BinOp::Sub => 1,
        BinOp::Mul => 2,
        BinOp::UDiv => 3,
        BinOp::SDiv => 4,
        BinOp::URem => 5,
        BinOp::SRem => 6,
        BinOp::FAdd => 7,
        BinOp::FSub => 8,
        BinOp::FMul => 9,
        BinOp::FDiv => 10,
        BinOp::FRem => 11,
        BinOp::And => 12,
        BinOp::Or => 13,
        BinOp::Xor => 14,
        BinOp::Shl => 15,
        BinOp::LShr => 16,
        BinOp::AShr => 17,
    }
}

fn cmpop(value: &CmpOp) -> u64 {
    match value {
        CmpOp::Eq => 0,
        CmpOp::Ne => 1,
        CmpOp::SLt => 2,
        CmpOp::SLe => 3,
        CmpOp::SGt => 4,
        CmpOp::SGe => 5,
        CmpOp::ULt => 6,
        CmpOp::ULe => 7,
        CmpOp::UGt => 8,
        CmpOp::UGe => 9,
        CmpOp::FEq => 10,
        CmpOp::FNe => 11,
        CmpOp::FLt => 12,
        CmpOp::FLe => 13,
        CmpOp::FGt => 14,
        CmpOp::FGe => 15,
    }
}

fn icmp(value: &ICmpPred) -> u64 {
    match value {
        ICmpPred::Eq => 0,
        ICmpPred::Ne => 1,
        ICmpPred::Ult => 2,
        ICmpPred::Ule => 3,
        ICmpPred::Ugt => 4,
        ICmpPred::Uge => 5,
        ICmpPred::Slt => 6,
        ICmpPred::Sle => 7,
        ICmpPred::Sgt => 8,
        ICmpPred::Sge => 9,
    }
}

fn fcmp(value: &FCmpPred) -> u64 {
    match value {
        FCmpPred::Oeq => 0,
        FCmpPred::One => 1,
        FCmpPred::Olt => 2,
        FCmpPred::Ole => 3,
        FCmpPred::Ogt => 4,
        FCmpPred::Oge => 5,
        FCmpPred::Ord => 6,
        FCmpPred::Uno => 7,
        FCmpPred::Ueq => 8,
        FCmpPred::Une => 9,
        FCmpPred::Ult => 10,
        FCmpPred::Ule => 11,
        FCmpPred::Ugt => 12,
        FCmpPred::Uge => 13,
    }
}

fn cast(value: &CastOp) -> u64 {
    match value {
        CastOp::CheckedInt => 0,
        CastOp::ZExt => 1,
        CastOp::SExt => 2,
        CastOp::Trunc => 3,
        CastOp::FExt => 4,
        CastOp::FTrunc => 5,
        CastOp::IToF_S => 6,
        CastOp::IToF_U => 7,
        CastOp::FToI_S => 8,
        CastOp::FToI_U => 9,
        CastOp::Bitcast => 10,
        CastOp::PtrToInt => 11,
        CastOp::IntToPtr => 12,
    }
}

fn checked(value: &CheckedOp) -> u64 {
    match value {
        CheckedOp::UAdd => 0,
        CheckedOp::USub => 1,
        CheckedOp::UMul => 2,
        CheckedOp::SAdd => 3,
        CheckedOp::SSub => 4,
        CheckedOp::SMul => 5,
    }
}
