// SPDX-License-Identifier: MPL-2.0
//! Borrowed typed expression packets and resumable reference-value transport.
use crate::verifier::wave_semantics::{Input as Types, Node as TypeNode};

use crate::{
    ConstEvalError, ConstExpr, ConstExprKind, ConstRef, ConstValue, FloatBits, IrLimits,
    LimitError, Resource, Type,
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Node {
    pub(crate) kind: u64,
    pub(crate) op: u64,
    pub(crate) ty: u64,
    pub(crate) left: u64,
    pub(crate) right: u64,
    pub(crate) reference: u64,
    pub(crate) literal: u64,
    pub(crate) low: u64,
    pub(crate) high: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Resolved {
    ty: u64,
    literal: u64,
    low: u64,
    high: u64,
    present: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Value {
    literal: u64,
    low: u64,
    high: u64,
    present: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Frame {
    node: u64,
    phase: u64,
    depth: u64,
}

#[repr(C)]
#[derive(Default)]
struct State {
    initialized: u64,
    phase: u64,
    length: u64,
    checked_types: u64,
    work: u64,
    root: u64,
}

#[repr(C)]
struct Context {
    nodes: *const Node,
    node_count: u64,
    types: *const TypeNode,
    type_count: u64,
    edges: *const u64,
    edge_count: u64,
    resolved: *mut Resolved,
    resolved_count: u64,
    values: *mut Value,
    value_count: u64,
    frames: *mut Frame,
    capacity: u64,
    state: *mut State,
    root: u64,
    max_depth: u64,
    max_work: u64,
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    index: u64,
    detail: u64,
    literal: u64,
    low: u64,
    high: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_const_literal(kind: u64, literal: u64, low: u64, high: u64) -> i32;

    fn whale_ir_const_step(context: *mut Context, result: *mut ResultRecord) -> i32;
}

pub(crate) struct Wire<'a> {
    pub(crate) types: Types,
    pub(crate) nodes: Vec<Node>,
    pub(crate) sources: Vec<&'a ConstExpr>,
    limits: IrLimits,
}

impl<'a> Wire<'a> {
    pub(crate) fn new(limits: IrLimits) -> Self {
        Self { types: Types::default(), nodes: Vec::new(), sources: Vec::new(), limits }
    }

    pub(crate) fn expression(&mut self, expr: &'a ConstExpr) -> Result<u64, ConstEvalError> {
        let mut node = Node { ty: self.types.ty(&expr.ty), ..Node::default() };
        match &expr.kind {
            ConstExprKind::Literal(value) => {
                let (tag, low, high) = payload(value);
                node.literal = tag;
                node.low = low;
                node.high = high;
            },
            ConstExprKind::Reference(reference) => match reference {
                ConstRef::Global(id) => {
                    node.kind = 1;
                    node.reference = id.0.into();
                },
                ConstRef::Local(id) => {
                    node.kind = 2;
                    node.reference = id.0.into();
                },
            },
            ConstExprKind::Not(child) => {
                node.kind = 3;
                node.left = self.expression(child)?;
            },
            ConstExprKind::Neg(child) => {
                node.kind = 4;
                node.left = self.expression(child)?;
            },
            ConstExprKind::Binary { op, left, right } => {
                node.kind = 5;
                node.op = match op {
                    crate::ConstBinaryOp::Add => 0,
                    crate::ConstBinaryOp::Sub => 1,
                    crate::ConstBinaryOp::Mul => 2,
                    crate::ConstBinaryOp::BitAnd => 3,
                    crate::ConstBinaryOp::BitOr => 4,
                    crate::ConstBinaryOp::BitXor => 5,
                    crate::ConstBinaryOp::Div => 6,
                    crate::ConstBinaryOp::Rem => 7,
                    crate::ConstBinaryOp::ShiftLeft => 8,
                    crate::ConstBinaryOp::ShiftRight => 9,
                    crate::ConstBinaryOp::LogicalAnd => 10,
                    crate::ConstBinaryOp::LogicalOr => 11,
                };
                node.left = self.expression(left)?;
                node.right = self.expression(right)?;
            },
            ConstExprKind::Compare { op, left, right } => {
                node.kind = 6;
                node.op = match op {
                    crate::ConstCompareOp::Eq => 0,
                    crate::ConstCompareOp::Ne => 1,
                    crate::ConstCompareOp::Lt => 2,
                    crate::ConstCompareOp::Le => 3,
                    crate::ConstCompareOp::Gt => 4,
                    crate::ConstCompareOp::Ge => 5,
                };
                node.left = self.expression(left)?;
                node.right = self.expression(right)?;
            },
        }
        self.nodes
            .try_reserve(1)
            .map_err(|_| self.resource(Resource::Nodes, self.limits.max_nodes))?;
        self.sources
            .try_reserve(1)
            .map_err(|_| self.resource(Resource::Nodes, self.limits.max_nodes))?;
        let id = self.nodes.len() as u64;
        self.nodes.push(node);
        self.sources.push(expr);
        Ok(id)
    }

    fn resource(&self, resource: Resource, limit: usize) -> ConstEvalError {
        ConstEvalError::ResourceLimit(LimitError { resource, limit })
    }
}

pub(crate) fn payload(value: &ConstValue) -> (u64, u64, u64) {
    let (tag, bits) = match value {
        ConstValue::I(v) => (1, *v as u128),
        ConstValue::U(v) => (2, *v),
        ConstValue::Bool(v) => (3, u128::from(*v)),
        ConstValue::F(FloatBits::F16(v)) => (4, u128::from(*v)),
        ConstValue::F(FloatBits::F32(v)) => (5, u128::from(*v)),
        ConstValue::F(FloatBits::F64(v)) => (6, u128::from(*v)),
    };
    (tag, bits as u64, (bits >> 64) as u64)
}

fn decode(tag: u64, low: u64, high: u64, ty: &Type) -> Option<ConstValue> {
    let bits = u128::from(low) | (u128::from(high) << 64);
    let value = match tag {
        1 => ConstValue::I(bits as i128),
        2 => ConstValue::U(bits),
        3 if low <= 1 && high == 0 => ConstValue::Bool(low != 0),
        4 if low <= u16::MAX.into() && high == 0 => ConstValue::F(FloatBits::F16(low as u16)),
        5 if low <= u32::MAX.into() && high == 0 => ConstValue::F(FloatBits::F32(low as u32)),
        6 if high == 0 => ConstValue::F(FloatBits::F64(low)),
        _ => return None,
    };
    crate::constant::valid_constant(ty, &value).then_some(value)
}

fn allocate<T: Clone + Default>(count: usize, limits: IrLimits) -> Result<Vec<T>, ConstEvalError> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| {
        ConstEvalError::ResourceLimit(LimitError {
            resource: Resource::Nodes,
            limit: limits.max_nodes,
        })
    })?;
    values.resize(count, T::default());
    Ok(values)
}

fn backend() -> ConstEvalError {
    ConstEvalError::Backend("constant expression transport")
}

pub(crate) fn evaluate<'e>(
    expr: &'e ConstExpr,
    resolve: &impl Fn(ConstRef) -> Option<(Type, ConstValue)>,
    limits: IrLimits,
) -> Result<ConstValue, (ConstEvalError, &'e ConstExpr)> {
    let mut budget =
        crate::limits::Budget::new(limits).map_err(|e| (ConstEvalError::ResourceLimit(e), expr))?;
    budget.expression(expr).map_err(|e| (ConstEvalError::ResourceLimit(e), expr))?;
    let mut wire = Wire::new(limits);
    let root = wire.expression(expr).map_err(|e| (e, expr))?;
    let count = wire.nodes.len();
    let mut resolved = allocate::<Resolved>(count, limits).map_err(|e| (e, expr))?;
    let mut values = allocate::<Value>(count, limits).map_err(|e| (e, expr))?;
    let capacity = limits.max_const_depth.saturating_mul(2).saturating_add(3);
    let mut frames = allocate::<Frame>(capacity, limits).map_err(|e| (e, expr))?;
    let mut state = State::default();
    let mut previous_work = 0;
    loop {
        let mut context = Context {
            nodes: wire.nodes.as_ptr(),
            node_count: count as u64,
            types: wire.types.nodes.as_ptr(),
            type_count: wire.types.nodes.len() as u64,
            edges: wire.types.edges.as_ptr(),
            edge_count: wire.types.edges.len() as u64,
            resolved: resolved.as_mut_ptr(),
            resolved_count: count as u64,
            values: values.as_mut_ptr(),
            value_count: count as u64,
            frames: frames.as_mut_ptr(),
            capacity: capacity as u64,
            state: &mut state,
            root,
            max_depth: limits.max_const_depth as u64,
            max_work: limits.max_analysis_work as u64,
        };
        let mut result = ResultRecord::default();
        let status = unsafe { whale_ir_const_step(&mut context, &mut result) };
        if i32::try_from(result.status).ok() != Some(status)
            || result.work > limits.max_analysis_work as u64
            || result.work < previous_work
            || state.work != result.work
            || state.length > capacity as u64
        {
            return Err((backend(), expr));
        }
        previous_work = result.work;
        let index = usize::try_from(result.index).map_err(|_| (backend(), expr))?;
        let failed = *wire.sources.get(index).ok_or_else(|| (backend(), expr))?;
        let resource = |resource, limit| {
            (ConstEvalError::ResourceLimit(LimitError { resource, limit }), failed)
        };
        match status {
            0 => {
                if result.index != root || state.phase != 2 || state.length != 0 {
                    return Err((backend(), expr));
                }
                return decode(result.literal, result.low, result.high, &expr.ty)
                    .ok_or_else(|| (backend(), expr));
            },
            10 => {
                let node = wire.nodes.get(index).ok_or_else(|| (backend(), expr))?;
                if !matches!(node.kind, 1 | 2)
                    || node.reference != result.detail
                    || resolved[index].present != 0
                {
                    return Err((backend(), expr));
                }
                let reference = match failed.kind {
                    ConstExprKind::Reference(reference) => reference,
                    _ => return Err((backend(), expr)),
                };
                match resolve(reference) {
                    None => resolved[index].present = 2,
                    Some((ty, value)) => {
                        if let Err(e) = budget.ty(&ty) {
                            crate::limits::discard_type(ty);
                            return Err((ConstEvalError::ResourceLimit(e), failed));
                        }
                        let type_id = wire.types.ty(&ty);
                        let (literal, low, high) = payload(&value);
                        resolved[index] = Resolved { ty: type_id, literal, low, high, present: 1 };
                    },
                }
            },
            1 => return Err((ConstEvalError::InvalidLiteral, failed)),
            2 => return Err((ConstEvalError::TypeMismatch, failed)),
            3 => return Err((ConstEvalError::UnsupportedOperation, failed)),
            4 => return Err((ConstEvalError::DivisionByZero, failed)),
            5 => return Err((ConstEvalError::RemainderByZero, failed)),
            6 => {
                return Err((
                    match failed.kind {
                        ConstExprKind::Reference(reference) => {
                            ConstEvalError::UnknownReference(reference)
                        },
                        _ => backend(),
                    },
                    failed,
                ))
            },
            7 => {
                return Err(if result.detail == 0 {
                    resource(Resource::AnalysisWork, limits.max_analysis_work)
                } else if result.detail == 1 {
                    resource(Resource::ConstantDepth, limits.max_const_depth)
                } else {
                    (backend(), expr)
                })
            },
            _ => return Err((backend(), expr)),
        }
    }
}

pub(crate) fn valid_literal(ty: &Type, value: &ConstValue) -> bool {
    let kind = match ty {
        Type::Void => 0,
        Type::Bool => 1,
        Type::I1 => 2,
        Type::U1 => 3,
        Type::I8 => 4,
        Type::I16 => 5,
        Type::I32 => 6,
        Type::I64 => 7,
        Type::I128 => 8,
        Type::U8 => 9,
        Type::U16 => 10,
        Type::U32 => 11,
        Type::U64 => 12,
        Type::U128 => 13,
        Type::F16 => 14,
        Type::F32 => 15,
        Type::F64 => 16,
        Type::Ptr(_) => 17,
        Type::FnPtr(_) => 18,
        Type::Array(..) => 19,
        Type::Struct(_) => 20,
        Type::Tuple(_) => 21,
    };
    let (tag, low, high) = payload(value);
    match unsafe { whale_ir_const_literal(kind, tag, low, high) } {
        0 => false,
        1 => true,
        _ => panic!("Wave literal validation transport"),
    }
}
