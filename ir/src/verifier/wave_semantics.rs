// SPDX-License-Identifier: MPL-2.0
//! Transport and diagnostic conversion for Wave semantic verification.

use super::{AnalysisBudget, IndexedFunction, VerifyError};

use crate::{
    BinOp, CallError, CastOp, CmpOp, ConstValue, FloatBits, ICmpPred, Instruction, Type, ValueId,
};

const MISSING: u64 = u64::MAX;

#[repr(C)]
pub(crate) struct Node {
    kind: u64,
    child: u64,
    first: u64,
    count: u64,
    extra: u64,
}

#[repr(C)]
#[derive(Default)]
struct Operand {
    value: u64,
    ty: u64,
    literal: u64,
    low: u64,
    high: u64,
}

#[repr(C)]
#[derive(Default)]
struct Operation {
    op: u64,
    sub: u64,
    ty: u64,
    other: u64,
    dst: u64,
    align: u64,
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    index: u64,
    expected: u64,
    actual: u64,
    detail: u64,
    work: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Binding {
    id: u64,
    ty: u64,
    index: u64,
    present: u64,
}

unsafe extern "C" {
    fn whale_ir_verify_semantics(
        types: *const Node,
        type_count: u64,
        edges: *const u64,
        edge_count: u64,
        instruction: *const Operation,
        operands: *const Operand,
        operand_count: u64,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_verify_value_types(
        types: *const Node,
        type_count: u64,
        edges: *const u64,
        edge_count: u64,
        annotations: *const Binding,
        annotation_count: u64,
        definitions: *const Binding,
        definition_count: u64,
        table: *mut Binding,
        capacity: u64,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_verify_link(
        external: u64,
        present: u64,
        bytes: *const u8,
        count: u64,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;
}

#[derive(Default)]
pub(crate) struct Input {
    pub(crate) nodes: Vec<Node>,
    pub(crate) edges: Vec<u64>,
    operands: Vec<Operand>,
}

impl Input {
    // This only serializes syntax. Wave decides all type equality/category rules.
    pub(crate) fn ty(&mut self, ty: &Type) -> u64 {
        let mut node = Node { kind: 0, child: 0, first: 0, count: 0, extra: 0 };
        node.kind = match ty {
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
            Type::Ptr(child) => {
                node.child = self.ty(child);
                17
            },
            Type::FnPtr(signature) => {
                let mut children = vec![self.ty(&signature.ret)];
                children.extend(signature.params.iter().map(|ty| self.ty(ty)));
                node.first = self.edges.len() as u64;
                node.count = children.len() as u64;
                node.extra =
                    convention(signature.convention) | (u64::from(signature.variadic) << 1);
                self.edges.extend(children);
                18
            },
            Type::Array(child, count) => {
                node.child = self.ty(child);
                node.count = *count;
                19
            },
            Type::Struct(fields) | Type::Tuple(fields) => {
                let children: Vec<_> = fields.iter().map(|ty| self.ty(ty)).collect();
                node.first = self.edges.len() as u64;
                node.count = children.len() as u64;
                self.edges.extend(children);
                if matches!(ty, Type::Struct(_)) {
                    20
                } else {
                    21
                }
            },
        };
        let id = self.nodes.len() as u64;
        self.nodes.push(node);
        id
    }

    // Build a diagnostic type only on failure. Keeping a clone at every graph
    // node would multiply the memory of deeply nested types by their depth.
    pub(super) fn decode_type(&self, id: u64) -> Option<Type> {
        let node = self.nodes.get(usize::try_from(id).ok()?)?;
        Some(match node.kind {
            0 => Type::Void,
            1 => Type::Bool,
            2 => Type::I1,
            3 => Type::U1,
            4 => Type::I8,
            5 => Type::I16,
            6 => Type::I32,
            7 => Type::I64,
            8 => Type::I128,
            9 => Type::U8,
            10 => Type::U16,
            11 => Type::U32,
            12 => Type::U64,
            13 => Type::U128,
            14 => Type::F16,
            15 => Type::F32,
            16 => Type::F64,
            17 => Type::ptr_to(self.decode_type(node.child)?),
            19 => Type::Array(Box::new(self.decode_type(node.child)?), node.count),
            18 | 20 | 21 => {
                let first = usize::try_from(node.first).ok()?;
                let end = first.checked_add(usize::try_from(node.count).ok()?)?;
                let mut fields = self
                    .edges
                    .get(first..end)?
                    .iter()
                    .map(|id| self.decode_type(*id))
                    .collect::<Option<Vec<_>>>()?;
                match node.kind {
                    20 => Type::Struct(fields),
                    21 => Type::Tuple(fields),
                    _ => {
                        if fields.is_empty() {
                            return None;
                        }
                        let ret = fields.remove(0);
                        Type::FnPtr(Box::new(crate::FunctionSignature {
                            params: fields,
                            ret,
                            convention: if node.extra & 1 == 0 {
                                crate::CallingConvention::Whale
                            } else {
                                crate::CallingConvention::SysV64
                            },
                            variadic: node.extra & 2 != 0,
                        }))
                    },
                }
            },
            _ => return None,
        })
    }

    fn operand(&mut self, f: &IndexedFunction<'_>, value: ValueId) {
        let ty = f.value_type(value).map(|ty| self.ty(ty)).unwrap_or(MISSING);
        let mut operand = Operand { value: u64::from(value.0), ty, ..Operand::default() };
        if let Some(value) = f.constant(value) {
            Self::literal(&mut operand, value);
        }
        self.operands.push(operand);
    }

    fn literal(operand: &mut Operand, value: &ConstValue) {
        let (kind, bits) = match value {
            ConstValue::I(value) => (1, *value as u128),
            ConstValue::U(value) => (2, *value),
            ConstValue::Bool(value) => (3, u128::from(*value)),
            ConstValue::F(FloatBits::F16(value)) => (4, u128::from(*value)),
            ConstValue::F(FloatBits::F32(value)) => (5, u128::from(*value)),
            ConstValue::F(FloatBits::F64(value)) => (6, u128::from(*value)),
        };
        operand.literal = kind;
        operand.low = bits as u64;
        operand.high = (bits >> 64) as u64;
    }

    fn run(
        &self,
        operation: &Operation,
        budget: &AnalysisBudget,
    ) -> Result<ResultRecord, VerifyError> {
        let mut result = ResultRecord::default();
        let remaining = budget.limit.saturating_sub(budget.work.get()) as u64;
        let status = unsafe {
            whale_ir_verify_semantics(
                self.nodes.as_ptr(),
                self.nodes.len() as u64,
                self.edges.as_ptr(),
                self.edges.len() as u64,
                operation,
                self.operands.as_ptr(),
                self.operands.len() as u64,
                remaining,
                &mut result,
            )
        };
        if status != result.status as i32 || result.work > remaining || status == 7 || status == 8 {
            return Err(resource_error(budget));
        }
        budget.work.set(budget.work.get() + result.work as usize);
        Ok(result)
    }

    fn diagnostic(
        &self,
        f: &IndexedFunction<'_>,
        name: &'static str,
        ins: Option<&Instruction>,
        result: ResultRecord,
        budget: &AnalysisBudget,
    ) -> Result<(), VerifyError> {
        let operand = || self.operands.get(result.index as usize);
        let ty = |id: u64| self.decode_type(id);
        let error = match result.status {
            0 => return Ok(()),
            1 => VerifyError::InvalidInstructionType {
                func: f.name.clone(),
                operation: name,
                ty: ty(result.expected).ok_or_else(|| resource_error(budget))?,
            },
            2 => VerifyError::MissingValueType {
                func: f.name.clone(),
                value: ValueId(operand().ok_or_else(|| resource_error(budget))?.value as u32),
            },
            3 => {
                let mut expected = ty(result.expected).ok_or_else(|| resource_error(budget))?;
                if result.detail != 0 {
                    expected = Type::ptr_to(expected);
                }
                let value = if result.detail == 2 {
                    match ins {
                        Some(Instruction::Gep { dst, .. }) => *dst,
                        _ => return Err(resource_error(budget)),
                    }
                } else {
                    ValueId(operand().ok_or_else(|| resource_error(budget))?.value as u32)
                };
                VerifyError::OperandTypeMismatch {
                    func: f.name.clone(),
                    value,
                    expected,
                    got: ty(result.actual).ok_or_else(|| resource_error(budget))?,
                }
            },
            4 => {
                let align = match ins {
                    Some(
                        Instruction::Alloca { align, .. }
                        | Instruction::Uninit { align, .. }
                        | Instruction::Load { align, .. }
                        | Instruction::Store { align, .. }
                        | Instruction::VolatileLoad { align, .. }
                        | Instruction::VolatileStore { align, .. }
                        | Instruction::Memcpy { align, .. }
                        | Instruction::Memset { align, .. },
                    ) => *align,
                    _ => return Err(resource_error(budget)),
                };
                VerifyError::InvalidMemoryAlignment { func: f.name.clone(), align }
            },
            5 => match ins {
                Some(Instruction::Cast { dst, op, src_ty, dst_ty, .. }) => {
                    VerifyError::InvalidCast {
                        func: f.name.clone(),
                        value: *dst,
                        op: op.clone(),
                        source: src_ty.clone(),
                        destination: dst_ty.clone(),
                    }
                },
                _ => return Err(resource_error(budget)),
            },
            6 => {
                let dst = match ins {
                    Some(Instruction::Gep { dst, .. }) => *dst,
                    _ => return Err(resource_error(budget)),
                };
                let reason = match result.detail {
                    0 => "base must be a pointer",
                    1 => "element addressing requires a concrete stored type",
                    2 => "index must be an integer",
                    3 => "field index must be a nonnegative integer constant",
                    4 => "field index is outside the aggregate type",
                    5 => "further indices require an array, struct, or tuple; GEP does not load pointers",
                    _ => return Err(resource_error(budget)),
                };
                VerifyError::InvalidGep {
                    func: f.name.clone(),
                    value: dst,
                    index: (result.index != 0).then_some(result.index.saturating_sub(1) as usize),
                    reason,
                }
            },
            10 => VerifyError::ConditionTypeMismatch {
                func: f.name.clone(),
                value: ValueId(operand().ok_or_else(|| resource_error(budget))?.value as u32),
                got: ty(result.actual).ok_or_else(|| resource_error(budget))?,
            },
            _ => return Err(resource_error(budget)),
        };
        Err(error)
    }
}

fn resource_error(budget: &AnalysisBudget) -> VerifyError {
    VerifyError::ResourceLimit(crate::LimitError {
        resource: crate::Resource::AnalysisWork,
        limit: budget.limit,
    })
}

pub(super) fn value_types(
    f: &IndexedFunction<'_>,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let mut input = Input::default();
    let annotations: Vec<_> = f
        .value_types
        .iter()
        .map(|(value, ty)| Binding {
            id: u64::from(value.0),
            ty: input.ty(ty),
            ..Binding::default()
        })
        .collect();
    let definitions: Vec<_> = f
        .params
        .iter()
        .map(|param| (param.id, param.ty.clone()))
        .chain(
            f.blocks.iter().flat_map(|block| &block.instructions).filter_map(super::instr_result),
        )
        .map(|(value, ty)| Binding {
            id: u64::from(value.0),
            ty: input.ty(&ty),
            ..Binding::default()
        })
        .collect();
    let capacity = annotations
        .len()
        .max(1)
        .checked_mul(2)
        .and_then(usize::checked_next_power_of_two)
        .ok_or_else(|| resource_error(budget))?;
    let mut table = vec![Binding::default(); capacity];
    let mut result = ResultRecord::default();
    let remaining = budget.limit.saturating_sub(budget.work.get()) as u64;
    let status = unsafe {
        whale_ir_verify_value_types(
            input.nodes.as_ptr(),
            input.nodes.len() as u64,
            input.edges.as_ptr(),
            input.edges.len() as u64,
            annotations.as_ptr(),
            annotations.len() as u64,
            definitions.as_ptr(),
            definitions.len() as u64,
            table.as_mut_ptr(),
            capacity as u64,
            remaining,
            &mut result,
        )
    };
    if status != result.status as i32 || result.work > remaining {
        return Err(resource_error(budget));
    }
    budget.work.set(budget.work.get() + result.work as usize);
    let value = ValueId(result.detail as u32);
    Err(match status {
        0 => return Ok(()),
        1 => VerifyError::DuplicateValueType { func: f.name.clone(), value },
        2 => VerifyError::MissingValueType { func: f.name.clone(), value },
        3 => VerifyError::ValueTypeMismatch {
            func: f.name.clone(),
            value,
            expected: input.decode_type(result.expected).ok_or_else(|| resource_error(budget))?,
            got: input.decode_type(result.actual).ok_or_else(|| resource_error(budget))?,
        },
        4 => VerifyError::UnexpectedValueType { func: f.name.clone(), value },
        _ => resource_error(budget),
    })
}

fn convention(convention: crate::CallingConvention) -> u64 {
    match convention {
        crate::CallingConvention::Whale => 0,
        crate::CallingConvention::SysV64 => 1,
    }
}

pub(super) fn valid_link(
    linkage: crate::Linkage,
    name: Option<&str>,
    budget: &AnalysisBudget,
) -> Result<bool, VerifyError> {
    let bytes = name.unwrap_or("").as_bytes();
    let mut result = ResultRecord::default();
    let remaining = budget.limit.saturating_sub(budget.work.get()) as u64;
    let status = unsafe {
        whale_ir_verify_link(
            u64::from(matches!(linkage, crate::Linkage::External)),
            u64::from(name.is_some()),
            bytes.as_ptr(),
            bytes.len() as u64,
            remaining,
            &mut result,
        )
    };
    if status != result.status as i32 || result.work > remaining {
        return Err(resource_error(budget));
    }
    budget.work.set(budget.work.get() + result.work as usize);
    match status {
        0 => Ok(true),
        20 => Ok(false),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn alignment(align: u32, budget: &AnalysisBudget) -> Result<bool, VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(&Type::Void);
    let operation =
        Operation { op: 32, ty, other: ty, align: u64::from(align), ..Operation::default() };
    let result = input.run(&operation, budget)?;
    match result.status {
        0 => Ok(true),
        4 => Ok(false),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn storable(ty: &Type, budget: &AnalysisBudget) -> Result<bool, VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(ty);
    let operation = Operation { op: 33, ty, other: ty, ..Operation::default() };
    let result = input.run(&operation, budget)?;
    match result.status {
        0 => Ok(true),
        1 => Ok(false),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn types_equal(
    a: &Type,
    b: &Type,
    budget: &AnalysisBudget,
) -> Result<bool, VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(a);
    let other = input.ty(b);
    let operation = Operation { op: 27, ty, other, ..Operation::default() };
    let result = input.run(&operation, budget)?;
    match result.status {
        0 => Ok(true),
        12 => Ok(false),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn switch(
    f: &IndexedFunction<'_>,
    value: ValueId,
    ty: &Type,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(ty);
    input.operand(f, value);
    let operation = Operation { op: 34, ty, other: ty, ..Operation::default() };
    let result = input.run(&operation, budget)?;
    input.diagnostic(f, "switch", None, result, budget)
}

pub(super) fn return_value(
    f: &IndexedFunction<'_>,
    declared: &Type,
    value: Option<ValueId>,
    defined: bool,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(&f.ret_ty);
    let other = input.ty(declared);
    if let Some(value) = value {
        input.operand(f, value);
    }
    let operation = Operation {
        op: 35,
        ty,
        other,
        dst: u64::from(value.is_some()),
        sub: u64::from(defined),
        ..Operation::default()
    };
    let result = input.run(&operation, budget)?;
    match result.status {
        0 => Ok(()),
        2 => input.diagnostic(f, "return", None, result, budget),
        27 => Err(VerifyError::UseOfUndefinedValue {
            func: f.name.clone(),
            value: value.ok_or_else(|| resource_error(budget))?,
        }),
        25 | 26 => Err(VerifyError::RetTypeMismatch {
            func: f.name.clone(),
            expected: f.ret_ty.clone(),
            got: if result.status == 26 {
                None
            } else {
                Some(input.decode_type(result.actual).ok_or_else(|| resource_error(budget))?)
            },
        }),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn global_storage(
    global: &crate::GlobalStorage,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(&global.ty);
    let mut operand = Operand::default();
    let initialized = match &global.init {
        crate::GlobalStorageInit::Value(value) => {
            Input::literal(&mut operand, value);
            true
        },
        crate::GlobalStorageInit::Uninitialized => false,
    };
    input.operands.push(operand);
    let operation = Operation {
        op: 31,
        ty,
        other: ty,
        sub: u64::from(global.readonly),
        dst: u64::from(initialized),
        align: u64::from(global.align),
    };
    let result = input.run(&operation, budget)?;
    let reason = match result.status {
        0 => {
            if valid_link(global.linkage, global.link_name.as_deref(), budget)? {
                return Ok(());
            }
            crate::GlobalStorageError::InvalidLinkName
        },
        21 => crate::GlobalStorageError::InvalidType,
        22 => crate::GlobalStorageError::InvalidAlignment,
        23 => crate::GlobalStorageError::InvalidInitializer,
        24 => crate::GlobalStorageError::ReadOnlyUninitialized,
        _ => return Err(resource_error(budget)),
    };
    Err(VerifyError::GlobalStorage { name: global.name.clone(), global: global.id, reason })
}

fn signature_error(status: u64, detail: u64) -> CallError {
    match (status, detail) {
        (11, 1) => CallError::InvalidSignature(
            "void parameters, malformed types and variadic calls are unsupported".into(),
        ),
        (11, 2) => {
            CallError::InvalidSignature("SysV64 aggregate signatures are unsupported".into())
        },
        _ => CallError::InvalidSignature("invalid Wave verification response".into()),
    }
}

pub(super) fn signature(
    signature: &crate::FunctionSignature,
    limits: crate::IrLimits,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    crate::limits::Budget::new(limits)
        .and_then(|mut budget| budget.signature(signature))
        .map_err(VerifyError::ResourceLimit)?;
    let mut input = Input::default();
    let ty = input.ty(&Type::FnPtr(Box::new(signature.clone())));
    let operation = Operation { op: 25, ty, other: ty, ..Operation::default() };
    let result = input.run(&operation, budget)?;
    match result.status {
        0 => Ok(()),
        11 => Err(VerifyError::Call {
            func: String::new(),
            reason: signature_error(result.status, result.detail),
        }),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn pointer_signatures(ty: &Type, budget: &AnalysisBudget) -> Result<(), VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(ty);
    let operation = Operation { op: 29, ty, other: ty, ..Operation::default() };
    let result = input.run(&operation, budget)?;
    match result.status {
        0 => Ok(()),
        11 => Err(VerifyError::Call {
            func: String::new(),
            reason: signature_error(result.status, result.detail),
        }),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn signature_equal(
    expected: &crate::FunctionSignature,
    actual: &crate::FunctionSignature,
    budget: &AnalysisBudget,
) -> Result<bool, VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(&Type::FnPtr(Box::new(expected.clone())));
    let other = input.ty(&Type::FnPtr(Box::new(actual.clone())));
    let operation = Operation { op: 27, ty, other, ..Operation::default() };
    let result = input.run(&operation, budget)?;
    match result.status {
        0 => Ok(true),
        12 => Ok(false),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn call(
    f: &IndexedFunction<'_>,
    signature: &Type,
    ins: &Instruction,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let Instruction::Call { dst, ret_ty, convention: call_convention, args, .. } = ins else {
        return Err(resource_error(budget));
    };
    let mut input = Input::default();
    let ty = input.ty(signature);
    let other = input.ty(ret_ty);
    for arg in args {
        input.operand(f, *arg);
    }
    let operation = Operation {
        op: 28,
        ty,
        other,
        sub: convention(*call_convention),
        dst: u64::from(dst.is_some()),
        ..Operation::default()
    };
    let result = input.run(&operation, budget)?;
    let error = match result.status {
        0 => return Ok(()),
        2 => CallError::UnknownValue(
            *args.get(result.index as usize).ok_or_else(|| resource_error(budget))?,
        ),
        11 => signature_error(result.status, result.detail),
        13 => CallError::ConventionMismatch,
        14 => CallError::ReturnType {
            expected: input.decode_type(result.expected).ok_or_else(|| resource_error(budget))?,
            got: input.decode_type(result.actual).ok_or_else(|| resource_error(budget))?,
        },
        15 => CallError::ResultPresence,
        16 => CallError::ArgumentCount {
            expected: result.expected as usize,
            got: result.actual as usize,
        },
        17 => CallError::ArgumentType {
            index: result.index as usize,
            expected: input.decode_type(result.expected).ok_or_else(|| resource_error(budget))?,
            got: input.decode_type(result.actual).ok_or_else(|| resource_error(budget))?,
        },
        19 => match ins {
            Instruction::Call { callee: crate::Callee::Indirect(value), .. } => {
                CallError::NotFunctionPointer(*value)
            },
            _ => return Err(resource_error(budget)),
        },
        _ => return Err(resource_error(budget)),
    };
    Err(VerifyError::Call { func: f.name.clone(), reason: error })
}

pub(super) fn function_pointer_cast(
    f: &IndexedFunction<'_>,
    src: ValueId,
    source: &Type,
    destination: &Type,
    op: &CastOp,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(source);
    let other = input.ty(destination);
    input.operand(f, src);
    let operation = Operation {
        op: 30,
        ty,
        other,
        sub: if matches!(op, CastOp::Bitcast) { 9 } else { 0 },
        ..Operation::default()
    };
    let result = input.run(&operation, budget)?;
    match result.status {
        0 => Ok(()),
        18 => Err(VerifyError::Call {
            func: f.name.clone(),
            reason: CallError::ForbiddenFunctionPointerCast,
        }),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn condition(
    f: &IndexedFunction<'_>,
    value: ValueId,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(&Type::Void);
    input.operand(f, value);
    let operation = Operation { op: 22, ty, other: ty, ..Operation::default() };
    let result = input.run(&operation, budget)?;
    input.diagnostic(f, "condition", None, result, budget)
}

pub(super) fn literal_valid(
    ty: &Type,
    value: &ConstValue,
    budget: &AnalysisBudget,
) -> Result<bool, VerifyError> {
    let mut input = Input::default();
    let ty = input.ty(ty);
    let mut operand = Operand::default();
    Input::literal(&mut operand, value);
    input.operands.push(operand);
    let operation = Operation { op: 24, ty, other: ty, ..Operation::default() };
    let result = input.run(&operation, budget)?;
    match result.status {
        0 => Ok(true),
        9 => Ok(false),
        _ => Err(resource_error(budget)),
    }
}

pub(super) fn verify_instruction(
    f: &IndexedFunction<'_>,
    ins: &Instruction,
    budget: &AnalysisBudget,
) -> Result<(), VerifyError> {
    let mut input = Input::default();
    let mut operation = Operation::default();
    let (op, name, ty, other, operands) = match ins {
        Instruction::Neg { ty, src, .. } => (1, "neg", ty.clone(), Type::Void, vec![*src]),
        Instruction::Cast { op, src_ty, src, dst_ty, .. } => {
            operation.sub = match op {
                CastOp::ZExt => 0,
                CastOp::SExt => 1,
                CastOp::Trunc => 2,
                CastOp::FExt => 3,
                CastOp::FTrunc => 4,
                CastOp::IToF_S => 5,
                CastOp::IToF_U => 6,
                CastOp::FToI_S => 7,
                CastOp::FToI_U => 8,
                CastOp::Bitcast => 9,
                CastOp::PtrToInt => 10,
                CastOp::IntToPtr => 11,
                CastOp::CheckedInt => 12,
            };
            (2, "cast", src_ty.clone(), dst_ty.clone(), vec![*src])
        },
        Instruction::Checked { op, ty, lhs, rhs, .. } => {
            operation.sub = match op {
                crate::CheckedOp::SAdd | crate::CheckedOp::SSub | crate::CheckedOp::SMul => 0,
                crate::CheckedOp::UAdd | crate::CheckedOp::USub | crate::CheckedOp::UMul => 1,
            };
            (3, "checked arithmetic", ty.clone(), Type::Void, vec![*lhs, *rhs])
        },
        Instruction::Bin { op, ty, lhs, rhs, .. } => {
            operation.sub = match op {
                BinOp::SDiv | BinOp::SRem => 1,
                BinOp::UDiv | BinOp::URem => 2,
                BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv | BinOp::FRem => 3,
                _ => 0,
            };
            (4, "binary operation", ty.clone(), Type::Void, vec![*lhs, *rhs])
        },
        Instruction::LifetimeEnd { ptr } => (5, "lifetime_end", Type::Void, Type::Void, vec![*ptr]),
        Instruction::PtrDiff { lhs, rhs, .. } => {
            (6, "ptrdiff", Type::Void, Type::Void, vec![*lhs, *rhs])
        },
        Instruction::Not { ty, src, .. } => (7, "not", ty.clone(), Type::Void, vec![*src]),
        Instruction::Mov { ty, src, .. } => (8, "mov", ty.clone(), Type::Void, vec![*src]),
        Instruction::Cmp { op, ty, lhs, rhs, .. } => {
            operation.sub = match op {
                CmpOp::Eq | CmpOp::Ne => 0,
                CmpOp::SLt | CmpOp::SLe | CmpOp::SGt | CmpOp::SGe => 1,
                CmpOp::ULt | CmpOp::ULe | CmpOp::UGt | CmpOp::UGe => 2,
                _ => 3,
            };
            (9, "comparison", ty.clone(), Type::Void, vec![*lhs, *rhs])
        },
        Instruction::ICmp { pred, ty, lhs, rhs, .. } => {
            operation.sub = match pred {
                ICmpPred::Eq | ICmpPred::Ne => 0,
                ICmpPred::Slt | ICmpPred::Sle | ICmpPred::Sgt | ICmpPred::Sge => 1,
                _ => 2,
            };
            (9, "integer comparison", ty.clone(), Type::Void, vec![*lhs, *rhs])
        },
        Instruction::FCmp { ty, lhs, rhs, .. } => {
            (10, "floating comparison", ty.clone(), Type::Void, vec![*lhs, *rhs])
        },
        Instruction::Select { ty, cond, on_true, on_false, .. } => {
            // Keep condition diagnostics ahead of the selected type category.
            condition(f, *cond, budget)?;
            (11, "select", ty.clone(), Type::Void, vec![*cond, *on_true, *on_false])
        },
        Instruction::Extract { dst_ty, tuple, index, .. } => {
            operation.sub = u64::from(*index);
            (12, "extract", dst_ty.clone(), Type::Void, vec![*tuple])
        },
        Instruction::Uninit { ty, ptr, align } => {
            operation.align = u64::from(*align);
            (13, "uninit", ty.clone(), Type::Void, vec![*ptr])
        },
        Instruction::Alloca { ty, align, .. } => {
            operation.align = u64::from(*align);
            (14, "alloca", ty.clone(), Type::Void, vec![])
        },
        Instruction::VolatileLoad { ty, ptr, align, .. } => {
            operation.align = u64::from(*align);
            (15, "volatile load", ty.clone(), Type::Void, vec![*ptr])
        },
        Instruction::VolatileStore { ty, ptr, value, align } => {
            operation.align = u64::from(*align);
            (16, "volatile store", ty.clone(), Type::Void, vec![*ptr, *value])
        },
        Instruction::Load { ty, ptr, align, .. } => {
            operation.align = u64::from(*align);
            (17, "load", ty.clone(), Type::Void, vec![*ptr])
        },
        Instruction::Store { ty, ptr, value, align } => {
            operation.align = u64::from(*align);
            (18, "store", ty.clone(), Type::Void, vec![*ptr, *value])
        },
        Instruction::Gep { dst, dst_ty, base_ptr, indices } => {
            operation.dst = u64::from(dst.0);
            let mut operands = vec![*base_ptr];
            operands.extend(indices);
            (19, "gep", dst_ty.clone(), Type::Void, operands)
        },
        Instruction::Memcpy { dst, src, n, align } => {
            operation.align = u64::from(*align);
            (20, "memcpy", Type::U8, Type::U64, vec![*dst, *src, *n])
        },
        Instruction::Memset { dst, val, n, align } => {
            operation.align = u64::from(*align);
            (21, "memset", Type::U8, Type::U64, vec![*dst, *val, *n])
        },
        _ => return Ok(()),
    };
    operation.op = op;
    operation.ty = input.ty(&ty);
    operation.other = input.ty(&other);
    for operand in operands {
        input.operand(f, operand);
    }
    let result = input.run(&operation, budget)?;
    input.diagnostic(f, name, Some(ins), result, budget)
}
