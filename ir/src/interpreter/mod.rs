// SPDX-License-Identifier: MPL-2.0
//! A bounded oracle for verified scalar integer/control-flow IR.
//! Tracked stack memory is supported; floating-point computation and calls remain unsupported.

mod integer;
mod memory;
pub use memory::{MemoryLimits, MemoryResource, MemoryTrap};

use crate::*;
use std::{collections::HashMap, fmt};

/// Limits verification as well as executed instructions and terminators.
#[derive(Clone, Copy, Debug)]
pub struct InterpreterOptions {
    pub max_steps: u64,
    pub ir_limits: IrLimits,
    pub memory_limits: MemoryLimits,
}
impl Default for InterpreterOptions {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
            ir_limits: IrLimits::default(),
            memory_limits: MemoryLimits::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct InterpreterResult {
    pub value: Option<ConstValue>,
    pub steps: u64,
}
/// An instruction index is zero-based; the terminator follows the instructions.
/// These are IR identities, not a fabricated source-file location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionSite {
    pub function: FunctionId,
    pub block: BlockId,
    pub instruction: usize,
    pub value: Option<ValueId>,
}
impl fmt::Display for ExecutionSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "@f{} {} instruction {}",
            self.function.0, self.block, self.instruction
        )?;
        if let Some(value) = self.value {
            write!(f, " ({value})")?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrapReason {
    DivisionByZero,
    RemainderByZero,
    Explicit(String),
    Memory(MemoryTrap),
}
impl fmt::Display for TrapReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DivisionByZero => f.write_str("division by zero"),
            Self::RemainderByZero => f.write_str("remainder by zero"),
            Self::Explicit(reason) => write!(f, "{reason:?}"),
            Self::Memory(reason) => reason.fmt(f),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpreterTrap {
    pub site: ExecutionSite,
    pub reason: TrapReason,
    pub steps: u64,
}
#[derive(Debug)]
pub enum InterpreterError {
    Verification(Box<VerifyError>),
    UnknownFunction(FunctionId),
    UnsupportedSignature(FunctionId),
    ArgumentCount {
        expected: usize,
        got: usize,
    },
    ArgumentType {
        index: usize,
        expected: Type,
    },
    UnsupportedInstruction {
        site: ExecutionSite,
        operation: &'static str,
    },
    StepLimit {
        site: ExecutionSite,
        limit: u64,
    },
    MemoryLimit {
        site: ExecutionSite,
        resource: MemoryResource,
        limit: u64,
    },
    Trap(InterpreterTrap),
}
impl fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(e) => write!(f, "IR verification failed: {e:?}"),
            Self::UnknownFunction(id) => write!(f, "no body for function @f{}", id.0),
            Self::UnsupportedSignature(id) => write!(f, "unsupported interpreter signature for @f{} (requires Whale convention, scalar integer/bool parameters and scalar or void return)", id.0),
            Self::ArgumentCount { expected, got } => write!(f, "expected {expected} arguments, got {got}"),
            Self::ArgumentType { index, expected } => write!(f, "argument {index} must be a {expected} literal in range"),
            Self::UnsupportedInstruction { site, operation } => write!(f, "unsupported interpreter operation {operation} at {site}"),
            Self::StepLimit { site, limit } => write!(f, "interpreter step limit {limit} reached at {site}"),
            Self::MemoryLimit { site, resource, limit } => write!(f, "interpreter memory {resource:?} limit {limit} reached at {site}"),
            Self::Trap(trap) => write!(f, "trap at {}: {} (after {} steps)", trap.site, trap.reason, trap.steps),
        }
    }
}
impl std::error::Error for InterpreterError {}

#[derive(Clone)]
enum Value {
    Scalar(ConstValue),
    Checked(ConstValue, bool),
    Pointer(memory::Pointer),
}
impl Value {
    fn scalar(&self) -> &ConstValue {
        match self {
            Self::Scalar(v) => v,
            _ => unreachable!("verified scalar operand"),
        }
    }
    fn pointer(&self) -> memory::Pointer {
        match self {
            Self::Pointer(p) => *p,
            _ => unreachable!("verified pointer operand"),
        }
    }
    fn boolean(&self) -> bool {
        match self.scalar() {
            ConstValue::Bool(v) => *v,
            _ => unreachable!("verified bool operand"),
        }
    }
}
fn scalar_type(ty: &Type) -> bool {
    *ty == Type::Bool || integer::shape(ty).is_some()
}
fn value_type(ty: &Type) -> bool {
    scalar_type(ty)
        || matches!(ty, Type::Ptr(_))
        || matches!(ty, Type::Tuple(ts) if ts.len() == 2 && integer::shape(&ts[0]).is_some() && ts[1] == Type::Bool)
}
fn destination(inst: &Instruction) -> Option<ValueId> {
    use Instruction::*;
    match inst {
        ConstDecl { dst, .. }
        | Const { dst, .. }
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
        | Gep { dst, .. }
        | NullFunction { dst, .. }
        | FunctionAddr { dst, .. } => Some(*dst),
        Call { dst, .. } => *dst,
        Uninit { .. } | Store { .. } | Memcpy { .. } | Memset { .. } | TrapIf { .. } => None,
    }
}
fn supported(inst: &Instruction) -> Result<(), &'static str> {
    use Instruction::*;
    let unsupported = match inst {
        ConstDecl { value, .. } if !matches!(value, ConstValue::F(_)) => return Ok(()),
        Const { ty, .. } if scalar_type(ty) => return Ok(()),
        Mov { ty, .. } | Phi { ty, .. } | Select { ty, .. } if value_type(ty) => return Ok(()),
        Bin { ty, .. } | Not { ty, .. } | Checked { ty, .. } if integer::shape(ty).is_some() => {
            return Ok(())
        }
        Cmp { ty, .. } | ICmp { ty, .. } if scalar_type(ty) || matches!(ty, Type::Ptr(_)) => {
            return Ok(())
        }
        Cast {
            op, src_ty, dst_ty, ..
        } if matches!(
            op,
            CastOp::ZExt | CastOp::SExt | CastOp::Trunc | CastOp::Bitcast
        ) && scalar_type(src_ty)
            && integer::shape(dst_ty).is_some() =>
        {
            return Ok(())
        }
        Extract { dst_ty, .. } if scalar_type(dst_ty) => return Ok(()),
        Alloca { ty, .. } | Uninit { ty, .. } if allocation_type(ty) => return Ok(()),
        Load { ty, .. } | Store { ty, .. } if value_type(ty) => return Ok(()),
        Gep { .. } => return Ok(()),
        Cast {
            op: CastOp::Bitcast,
            src_ty: Type::Ptr(_),
            dst_ty: Type::Ptr(_),
            ..
        }
        | Cast {
            op: CastOp::IntToPtr | CastOp::PtrToInt,
            ..
        } => return Ok(()),
        TrapIf { .. } | Memcpy { .. } | Memset { .. } => return Ok(()),
        Undef { .. } => "undef",
        ConstDecl { .. } | Const { .. } => "non-integer constant",
        Mov { .. } | Phi { .. } | Select { .. } | Extract { .. } => "non-scalar value",
        Bin { .. } | Not { .. } | Checked { .. } | Cmp { .. } | ICmp { .. } | FCmp { .. } => {
            "non-integer arithmetic/comparison"
        }
        Cast { .. } => "unsupported cast",
        Alloca { .. } => "alloca",
        Load { .. } => "load",
        Store { .. } => "store",
        Uninit { .. } => "uninit",
        NullFunction { .. } => "null_function",
        FunctionAddr { .. } => "function_addr",
        Call { .. } => "call",
    };
    Err(unsupported)
}
fn allocation_type(root: &Type) -> bool {
    let mut pending = vec![root];
    while let Some(ty) = pending.pop() {
        if scalar_type(ty) || matches!(ty, Type::Ptr(_)) {
            continue;
        }
        match ty {
            Type::Array(ty, _) => pending.push(ty),
            Type::Struct(fields) | Type::Tuple(fields) => pending.extend(fields),
            _ => return false,
        }
    }
    true
}
fn memory_failure(fault: memory::Fault, site: ExecutionSite, steps: u64) -> InterpreterError {
    match fault {
        memory::Fault::Trap(reason) => InterpreterError::Trap(InterpreterTrap {
            site,
            reason: TrapReason::Memory(reason),
            steps,
        }),
        memory::Fault::Limit { resource, limit } => InterpreterError::MemoryLimit {
            site,
            resource,
            limit,
        },
    }
}
fn gep_plan(fun: &Function, base: ValueId, indices: &[ValueId]) -> Vec<memory::GepStep> {
    let Type::Ptr(pointee) = fun.value_type(base).unwrap() else {
        unreachable!("verified GEP base")
    };
    let mut selected: &Type = pointee;
    let mut plan = Vec::new();
    for (position, index) in indices.iter().enumerate() {
        let l =
            |ty| crate::layout_of_with_limit(ty, Target::X86_64WhaleLinux, MAX_IR_NESTING).unwrap();
        if position == 0 {
            plan.push(memory::GepStep::Stride(l(selected).size));
            continue;
        }
        match selected {
            Type::Array(element, _) => {
                selected = element;
                plan.push(memory::GepStep::Stride(l(selected).size));
            }
            Type::Struct(fields) | Type::Tuple(fields) => {
                let ordinal = fun
                    .blocks
                    .iter()
                    .flat_map(|b| &b.instructions)
                    .find_map(|ins| match ins {
                        Instruction::Const { dst, value, .. }
                        | Instruction::ConstDecl { dst, value, .. }
                            if dst == index =>
                        {
                            Some(match value {
                                ConstValue::I(v) => *v as usize,
                                ConstValue::U(v) => *v as usize,
                                _ => unreachable!("verified field ordinal"),
                            })
                        }
                        _ => None,
                    })
                    .unwrap();
                plan.push(memory::GepStep::Field(l(selected).field_offsets[ordinal]));
                selected = &fields[ordinal];
            }
            _ => unreachable!("verified GEP path"),
        }
    }
    plan
}
fn cmp_pred(op: CmpOp) -> ICmpPred {
    use CmpOp::*;
    match op {
        Eq => ICmpPred::Eq,
        Ne => ICmpPred::Ne,
        SLt => ICmpPred::Slt,
        SLe => ICmpPred::Sle,
        SGt => ICmpPred::Sgt,
        SGe => ICmpPred::Sge,
        ULt => ICmpPred::Ult,
        ULe => ICmpPred::Ule,
        UGt => ICmpPred::Ugt,
        UGe => ICmpPred::Uge,
        _ => unreachable!("integer subset checked"),
    }
}

/// Verify the whole module, then run one explicitly identified function.
/// All blocks of that function must belong to the supported subset, including
/// unreachable blocks. Other functions are verified but need not be executable.
pub fn interpret(
    module: &Module,
    function: FunctionId,
    arguments: &[ConstValue],
) -> Result<InterpreterResult, InterpreterError> {
    interpret_with_options(module, function, arguments, InterpreterOptions::default())
}
pub fn interpret_with_options(
    module: &Module,
    function: FunctionId,
    arguments: &[ConstValue],
    options: InterpreterOptions,
) -> Result<InterpreterResult, InterpreterError> {
    verify_module_with_limits(module, options.ir_limits)
        .map_err(|e| InterpreterError::Verification(Box::new(e)))?;
    let fun = module
        .functions
        .iter()
        .find(|f| f.id == function)
        .ok_or(InterpreterError::UnknownFunction(function))?;
    let decl = module
        .declarations
        .iter()
        .find(|d| d.id == function)
        .expect("verified declaration");
    if decl.signature.convention != CallingConvention::Whale
        || !fun.params.iter().all(|p| scalar_type(&p.ty))
        || !(fun.ret_ty == Type::Void || scalar_type(&fun.ret_ty))
    {
        return Err(InterpreterError::UnsupportedSignature(function));
    }
    if arguments.len() != fun.params.len() {
        return Err(InterpreterError::ArgumentCount {
            expected: fun.params.len(),
            got: arguments.len(),
        });
    }
    for (index, (param, argument)) in fun.params.iter().zip(arguments).enumerate() {
        if !crate::constant::valid_constant(&param.ty, argument) {
            return Err(InterpreterError::ArgumentType {
                index,
                expected: param.ty.clone(),
            });
        }
    }
    let site = |block: BlockId, instruction: usize, value: Option<ValueId>| ExecutionSite {
        function,
        block,
        instruction,
        value,
    };
    let mut sizes = HashMap::new();
    let mut geps = HashMap::new();
    for block in &fun.blocks {
        for (index, inst) in block.instructions.iter().enumerate() {
            let at = site(block.id, index, destination(inst));
            supported(inst).map_err(|operation| InterpreterError::UnsupportedInstruction {
                site: at,
                operation,
            })?;
            match inst {
                Instruction::Alloca { ty, .. } | Instruction::Uninit { ty, .. } => {
                    let l =
                        layout_of_with_limit(ty, Target::X86_64WhaleLinux, MAX_IR_NESTING).unwrap();
                    let alignment = if matches!(ty, Type::Array(..)) && l.size >= 16 {
                        l.align.max(16)
                    } else {
                        l.align
                    };
                    sizes.insert((block.id, index), (l.size, alignment));
                }
                Instruction::Gep {
                    base_ptr, indices, ..
                } => {
                    let plan = gep_plan(fun, *base_ptr, indices);
                    if plan
                        .iter()
                        .any(|step| matches!(step, memory::GepStep::Stride(0)))
                    {
                        return Err(InterpreterError::UnsupportedInstruction {
                            site: at,
                            operation: "zero-sized pointer arithmetic",
                        });
                    }
                    geps.insert((block.id, index), plan);
                }
                _ => {}
            }
        }
    }
    let mut memory = memory::Memory::new(options.memory_limits);
    // Maps avoid allocations proportional to potentially sparse u32 identities.
    let blocks: HashMap<_, _> = fun.blocks.iter().map(|b| (b.id, b)).collect();
    let mut values: HashMap<_, _> = fun
        .params
        .iter()
        .zip(arguments)
        .map(|(p, a)| (p.id, Value::Scalar(a.clone())))
        .collect();
    let mut current = fun.entry;
    let mut predecessor = None;
    let mut steps = 0;
    let charge = |steps: &mut u64, site| -> Result<(), InterpreterError> {
        if *steps >= options.max_steps {
            return Err(InterpreterError::StepLimit {
                site,
                limit: options.max_steps,
            });
        }
        *steps += 1;
        Ok(())
    };
    loop {
        let block = blocks[&current];
        // All incoming operands are read from the predecessor's environment.
        // Commit simultaneously, so loop-carried phis can swap each other's values.
        let mut phis = Vec::new();
        for (index, inst) in block.instructions.iter().enumerate() {
            if let Instruction::Phi { dst, incomings, .. } = inst {
                charge(&mut steps, site(current, index, Some(*dst)))?;
                let input = incomings
                    .iter()
                    .find(|(_, b)| Some(*b) == predecessor)
                    .expect("verified phi predecessor")
                    .0;
                phis.push((*dst, values[&input].clone()));
            } else {
                break;
            }
        }
        let phi_count = phis.len();
        values.extend(phis);
        for (index, inst) in block.instructions.iter().enumerate().skip(phi_count) {
            let at = site(current, index, destination(inst));
            charge(&mut steps, at)?;
            let get = |id: &ValueId| &values[id];
            let scalar = |id: &ValueId| get(id).scalar();
            let result = match inst {
                Instruction::Const { value, .. } | Instruction::ConstDecl { value, .. } => {
                    Value::Scalar(value.clone())
                }
                Instruction::Mov { src, .. } => get(src).clone(),
                Instruction::Bin {
                    op, ty, lhs, rhs, ..
                } => Value::Scalar(integer::bin(op, ty, scalar(lhs), scalar(rhs)).map_err(
                    |reason| {
                        InterpreterError::Trap(InterpreterTrap {
                            site: at,
                            reason,
                            steps,
                        })
                    },
                )?),
                Instruction::Not { ty, src, .. } => Value::Scalar(integer::pack(
                    ty,
                    !integer::raw(scalar(src), integer::shape(ty).unwrap().0),
                )),
                Instruction::Cmp {
                    op, ty, lhs, rhs, ..
                } => {
                    let result = if matches!(ty, Type::Ptr(_)) {
                        let equal = memory
                            .equal(get(lhs).pointer(), get(rhs).pointer())
                            .map_err(|e| memory_failure(e, at, steps))?;
                        if *op == CmpOp::Eq {
                            equal
                        } else {
                            !equal
                        }
                    } else {
                        integer::compare(&cmp_pred(*op), ty, scalar(lhs), scalar(rhs))
                    };
                    Value::Scalar(ConstValue::Bool(result))
                }
                Instruction::ICmp {
                    pred, ty, lhs, rhs, ..
                } => {
                    let result = if matches!(ty, Type::Ptr(_)) {
                        let equal = memory
                            .equal(get(lhs).pointer(), get(rhs).pointer())
                            .map_err(|e| memory_failure(e, at, steps))?;
                        if *pred == ICmpPred::Eq {
                            equal
                        } else {
                            !equal
                        }
                    } else {
                        integer::compare(pred, ty, scalar(lhs), scalar(rhs))
                    };
                    Value::Scalar(ConstValue::Bool(result))
                }
                Instruction::Select {
                    cond,
                    on_true,
                    on_false,
                    ..
                } => get(if get(cond).boolean() {
                    on_true
                } else {
                    on_false
                })
                .clone(),
                Instruction::Cast {
                    op,
                    src_ty,
                    dst_ty,
                    src,
                    ..
                } => match op {
                    CastOp::IntToPtr => {
                        Value::Pointer(memory::Pointer::raw(integer::raw(scalar(src), 64) as u64))
                    }
                    CastOp::PtrToInt => Value::Scalar(integer::pack(
                        dst_ty,
                        u128::from(get(src).pointer().address),
                    )),
                    CastOp::Bitcast if matches!(src_ty, Type::Ptr(_)) => get(src).clone(),
                    _ => Value::Scalar(integer::cast(op, src_ty, dst_ty, scalar(src))),
                },
                Instruction::Alloca { align, .. } => {
                    let (size, natural) = sizes[&(current, index)];
                    Value::Pointer(
                        memory
                            .allocate(size, natural, *align)
                            .map_err(|e| memory_failure(e, at, steps))?,
                    )
                }
                Instruction::Load { ty, ptr, align, .. } => memory
                    .load(get(ptr).pointer(), ty, *align)
                    .map_err(|e| memory_failure(e, at, steps))?,
                Instruction::Store {
                    ty,
                    value,
                    ptr,
                    align,
                } => {
                    memory
                        .store(get(ptr).pointer(), ty, get(value), *align)
                        .map_err(|e| memory_failure(e, at, steps))?;
                    continue;
                }
                Instruction::Uninit { ptr, align, .. } => {
                    memory
                        .uninit(get(ptr).pointer(), sizes[&(current, index)].0, *align)
                        .map_err(|e| memory_failure(e, at, steps))?;
                    continue;
                }
                Instruction::Memcpy { dst, src, n, align } => {
                    memory
                        .copy(
                            get(dst).pointer(),
                            get(src).pointer(),
                            integer::raw(scalar(n), 64) as u64,
                            *align,
                        )
                        .map_err(|e| memory_failure(e, at, steps))?;
                    continue;
                }
                Instruction::Memset { dst, val, n, align } => {
                    memory
                        .set(
                            get(dst).pointer(),
                            integer::raw(scalar(val), 8) as u8,
                            integer::raw(scalar(n), 64) as u64,
                            *align,
                        )
                        .map_err(|e| memory_failure(e, at, steps))?;
                    continue;
                }
                Instruction::Gep {
                    base_ptr, indices, ..
                } => {
                    let inputs: Vec<_> = indices.iter().map(|id| scalar(id).clone()).collect();
                    Value::Pointer(
                        memory
                            .gep(get(base_ptr).pointer(), &geps[&(current, index)], &inputs)
                            .map_err(|e| memory_failure(e, at, steps))?,
                    )
                }
                Instruction::Checked {
                    op, ty, lhs, rhs, ..
                } => {
                    let (v, overflow) = integer::checked(op, ty, scalar(lhs), scalar(rhs));
                    Value::Checked(v, overflow)
                }
                Instruction::Extract { tuple, index, .. } => match get(tuple) {
                    Value::Checked(v, overflow) => Value::Scalar(if *index == 0 {
                        v.clone()
                    } else {
                        ConstValue::Bool(*overflow)
                    }),
                    _ => unreachable!("verified checked pair"),
                },
                Instruction::TrapIf { cond, reason } => {
                    if get(cond).boolean() {
                        return Err(InterpreterError::Trap(InterpreterTrap {
                            site: at,
                            reason: TrapReason::Explicit(reason.clone()),
                            steps,
                        }));
                    }
                    continue;
                }
                _ => unreachable!("supported subset checked before execution"),
            };
            values.insert(
                destination(inst).expect("value-producing instruction"),
                result,
            );
        }
        let at = site(current, block.instructions.len(), None);
        charge(&mut steps, at)?;
        let next = match block.terminator.as_ref().expect("verified terminator") {
            Terminator::Br { target } => *target,
            Terminator::CBr {
                cond,
                then_bb,
                else_bb,
            } => {
                if values[cond].boolean() {
                    *then_bb
                } else {
                    *else_bb
                }
            }
            Terminator::Switch {
                value,
                default_bb,
                cases,
                ..
            } => cases
                .iter()
                .find(|(case, _)| case == values[value].scalar())
                .map_or(*default_bb, |(_, target)| *target),
            Terminator::Ret { value, .. } => {
                memory.retire_all();
                return Ok(InterpreterResult {
                    value: value.map(|v| values[&v].scalar().clone()),
                    steps,
                });
            }
            Terminator::Trap { reason } => {
                return Err(InterpreterError::Trap(InterpreterTrap {
                    site: at,
                    reason: TrapReason::Explicit(reason.clone()),
                    steps,
                }))
            }
        };
        predecessor = Some(current);
        current = next;
    }
}
