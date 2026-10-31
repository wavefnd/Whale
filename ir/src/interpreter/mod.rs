// SPDX-License-Identifier: MPL-2.0
//! A bounded oracle for verified scalar/control-flow IR.
//! Tracked stack memory and bounded direct/indirect Whale calls are supported.

pub(crate) mod integer;

mod scalar;

mod memory;

#[cfg(whale_wave_ir)]
mod runtime_wave;

pub use memory::{MemoryLimits, MemoryResource, MemoryTrap};

use crate::*;

#[cfg(not(whale_wave_ir))]
use std::collections::HashMap;

use std::{fmt, sync::Arc};

#[cfg(not(whale_wave_ir))]
use std::collections::HashSet;

/// Limits verification as well as executed instructions and terminators.
#[derive(Clone, Copy, Debug)]
pub struct InterpreterOptions {
    pub max_steps: u64,
    /// Maximum simultaneously active invocation frames, including the entry.
    pub max_call_depth: usize,
    /// Maximum invocation frames created over the entire run.
    pub max_frames: u64,
    /// Reserved SSA slots across all simultaneously active frames.
    pub max_live_values: usize,
    /// Retained caller edges in failures; hard-clamped to 256. Zero retains only an omitted count.
    pub max_trace_frames: usize,
    pub ir_limits: IrLimits,
    pub memory_limits: MemoryLimits,
}

impl Default for InterpreterOptions {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
            max_call_depth: 256,
            max_frames: 100_000,
            max_live_values: 1_000_000,
            max_trace_frames: 64,
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
        write!(f, "@f{} {} instruction {}", self.function.0, self.block, self.instruction)?;
        if let Some(value) = self.value {
            write!(f, " ({value})")?;
        }
        Ok(())
    }
}

pub const MAX_RUNTIME_TRACE_FRAMES: usize = 256;

pub const MAX_RUNTIME_DIAGNOSTIC_SOURCE_BYTES: usize = 1024 * 1024;

/// One active caller edge, in outermost-to-innermost order within the retained tail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCallFrame {
    pub caller: FunctionId,
    pub callee: FunctionId,
    pub site: ExecutionSite,
    pub original: Option<OriginalLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDiagnostic {
    pub site: Option<ExecutionSite>,
    pub global: Option<GlobalId>,
    pub original: Option<OriginalLocation>,
    pub calls: Vec<RuntimeCallFrame>,
    /// Outer caller edges omitted by the configured/hard trace bound.
    pub omitted_call_frames: usize,
    /// Source filename/span details omitted by the independent diagnostic byte budget.
    pub omitted_source_details: usize,
}

impl fmt::Display for RuntimeDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(original) = &self.original {
            write!(f, "\n  at {original}")?;
        }
        if self.omitted_source_details > 0 {
            write!(
                f,
                "\n  ... {} source details omitted by diagnostic byte budget",
                self.omitted_source_details
            )?;
        }
        if self.omitted_call_frames > 0 {
            write!(f, "\n  ... {} outer caller frames omitted", self.omitted_call_frames)?;
        }
        for call in &self.calls {
            write!(f, "\n  called from {} -> @f{}", call.site, call.callee.0)?;
            if let Some(original) = &call.original {
                write!(f, " ({original})")?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrapReason {
    DivisionByZero,
    RemainderByZero,
    InvalidConversion,
    Explicit(String),
    Memory(MemoryTrap),
    NullFunction,
    InvalidFunctionPointer,
    FunctionSignatureMismatch,
}

impl fmt::Display for TrapReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DivisionByZero => f.write_str("division by zero"),
            Self::InvalidConversion => f.write_str("numeric conversion is out of range"),
            Self::RemainderByZero => f.write_str("remainder by zero"),
            Self::Explicit(reason) => write!(f, "{reason:?}"),
            Self::Memory(reason) => reason.fmt(f),
            Self::NullFunction => f.write_str("null function pointer call"),
            Self::InvalidFunctionPointer => {
                f.write_str("function pointer has no callable authority")
            },
            Self::FunctionSignatureMismatch => {
                f.write_str("function pointer signature does not match its target")
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpreterTrap {
    pub site: ExecutionSite,
    pub reason: TrapReason,
    pub steps: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallResource {
    Depth,
    Frames,
    Values,
    HostAllocation,
}

#[derive(Debug)]
pub enum InterpreterError {
    /// Added only for relevant original metadata or a nested interpreted caller chain.
    Diagnostic {
        cause: Box<InterpreterError>,
        diagnostic: Box<RuntimeDiagnostic>,
    },
    Verification(Box<VerifyError>),
    Backend {
        site: Option<ExecutionSite>,
        reason: &'static str,
    },
    UnknownFunction(FunctionId),
    UnsupportedGlobal(crate::GlobalId),
    GlobalMemory {
        global: crate::GlobalId,
        reason: MemoryTrap,
    },
    GlobalMemoryLimit {
        global: crate::GlobalId,
        resource: MemoryResource,
        limit: u64,
    },
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
    CallLimit {
        site: ExecutionSite,
        resource: CallResource,
        limit: u64,
    },
    Trap(InterpreterTrap),
}

impl fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diagnostic { cause, diagnostic } => { cause.root_cause().fmt(f)?; diagnostic.fmt(f) },
            Self::Verification(e) => write!(f, "IR verification failed: {e:?}"),
            Self::Backend { site, reason } => { write!(f, "IR Wave backend error: {reason}")?; if let Some(site) = site { write!(f, " at {site}")?; } Ok(()) },
            Self::UnsupportedGlobal(id) => write!(f, "unsupported interpreter global storage @g{}", id.0),
            Self::GlobalMemory { global, reason } => write!(f, "global storage @g{}: {reason}", global.0),
            Self::GlobalMemoryLimit { global, resource, limit } => write!(f, "global storage @g{} memory {resource:?} limit {limit}", global.0),
            Self::UnknownFunction(id) => write!(f, "no body for function @f{}", id.0),
            Self::UnsupportedSignature(id) => write!(f, "unsupported interpreter signature for @f{} (requires Whale convention and supported integer/bool/float/pointer parameters and scalar/pointer or void return)", id.0),
            Self::ArgumentCount { expected, got } => write!(f, "expected {expected} arguments, got {got}"),
            Self::ArgumentType { index, expected } => write!(f, "argument {index} must be a valid {expected} runtime value"),
            Self::UnsupportedInstruction { site, operation } => write!(f, "unsupported interpreter operation {operation} at {site}"),
            Self::StepLimit { site, limit } => write!(f, "interpreter step limit {limit} reached at {site}"),
            Self::MemoryLimit { site, resource, limit } => write!(f, "interpreter memory {resource:?} limit {limit} reached at {site}"),
            Self::CallLimit { site, resource, limit } => write!(f, "interpreter call {resource:?} limit {limit} reached at {site}"),
            Self::Trap(trap) => write!(f, "trap at {}: {} (after {} steps)", trap.site, trap.reason, trap.steps),
        }
    }
}

impl InterpreterError {
    pub fn root_cause(&self) -> &Self {
        let mut error = self;
        while let Self::Diagnostic { cause, .. } = error {
            error = cause;
        }
        error
    }

    pub fn into_root_cause(self) -> Self {
        let mut error = self;
        while let Self::Diagnostic { cause, .. } = error {
            error = *cause;
        }
        error
    }

    pub fn diagnostic(&self) -> Option<&RuntimeDiagnostic> {
        match self {
            Self::Diagnostic { diagnostic, .. } => Some(diagnostic),
            _ => None,
        }
    }

    fn runtime_site(&self) -> Option<ExecutionSite> {
        match self.root_cause() {
            Self::Trap(trap) => Some(trap.site),
            Self::Backend { site, .. } => *site,
            Self::StepLimit { site, .. }
            | Self::MemoryLimit { site, .. }
            | Self::CallLimit { site, .. } => Some(*site),
            _ => None,
        }
    }
}

impl std::error::Error for InterpreterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Diagnostic { cause, .. } => Some(cause.as_ref()),
            _ => None,
        }
    }
}

/// A checked library value. Pointer/callable metadata is private and cannot be fabricated.
/// Data addresses created by `raw_pointer` never grant allocation authority.
#[derive(Clone)]
pub struct RuntimeValue<'module> {
    ty: Type,
    value: Value,
    callable_module: Option<&'module Module>,
}

impl fmt::Debug for RuntimeValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeValue")
            .field("ty", &self.ty)
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}

impl<'module> RuntimeValue<'module> {
    pub fn scalar(ty: Type, value: ConstValue) -> Result<Self, CallError> {
        if !scalar_type(&ty) || !crate::constant::valid_constant(&ty, &value) {
            crate::limits::discard_type(ty);
            return Err(CallError::InvalidSignature("invalid scalar runtime value".into()));
        }
        Ok(Self { ty, value: Value::Scalar(value), callable_module: None })
    }

    pub fn raw_pointer(ty: Type, address: u64) -> Result<Self, CallError> {
        if let Err(error) =
            crate::limits::Budget::new(IrLimits::default()).and_then(|mut b| b.ty(&ty))
        {
            crate::limits::discard_type(ty);
            return Err(CallError::ResourceLimit(error));
        }
        if !matches!(ty, Type::Ptr(_)) {
            crate::limits::discard_type(ty);
            return Err(CallError::InvalidSignature("data pointer type required".into()));
        }
        let mut signature = FunctionSignature::whale(vec![ty], Type::Void);
        if let Err(error) = validate_signature_with_limits(&signature, IrLimits::default()) {
            crate::limits::discard_signature(signature);
            return Err(error);
        }
        let ty = signature.params.pop().unwrap();
        Ok(Self { ty, value: Value::Pointer(memory::Pointer::raw(address)), callable_module: None })
    }

    pub fn function(module: &'module Module, target: FunctionId) -> Result<Self, CallError> {
        let declaration = module
            .declarations
            .iter()
            .find(|d| d.id == target)
            .ok_or(CallError::UnknownFunction(target))?;
        validate_signature_with_limits(&declaration.signature, IrLimits::default())?;
        let signature = declaration.signature.clone();
        Ok(Self {
            ty: Type::FnPtr(Box::new(signature.clone())),
            value: Value::FunctionPointer(FunctionPointer {
                signature: Arc::new(signature),
                target: FunctionTarget::Defined(target),
            }),
            callable_module: Some(module),
        })
    }

    pub fn null_function(signature: FunctionSignature) -> Result<Self, CallError> {
        if let Err(error) = validate_signature_with_limits(&signature, IrLimits::default()) {
            crate::limits::discard_signature(signature);
            return Err(error);
        }
        Ok(Self {
            ty: Type::FnPtr(Box::new(signature.clone())),
            value: Value::FunctionPointer(FunctionPointer {
                signature: Arc::new(signature),
                target: FunctionTarget::Null,
            }),
            callable_module: None,
        })
    }

    pub fn ty(&self) -> &Type {
        &self.ty
    }

    pub fn scalar_value(&self) -> Option<&ConstValue> {
        match &self.value {
            Value::Scalar(value) => Some(value),
            _ => None,
        }
    }

    /// Synthetic address only; converting it back to a pointer cannot recover authority.
    pub fn pointer_address(&self) -> Option<u64> {
        match self.value {
            Value::Pointer(pointer) => Some(pointer.address),
            _ => None,
        }
    }

    pub fn function_target(&self) -> Option<FunctionId> {
        match &self.value {
            Value::FunctionPointer(FunctionPointer {
                target: FunctionTarget::Defined(id), ..
            }) => Some(*id),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeInterpreterResult<'module> {
    pub value: Option<RuntimeValue<'module>>,
    pub steps: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FunctionTarget {
    Null,
    Defined(FunctionId),
    InvalidBits(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FunctionPointer {
    signature: Arc<FunctionSignature>,
    target: FunctionTarget,
}

impl FunctionPointer {
    fn bits(&self) -> u64 {
        match self.target {
            FunctionTarget::Null => 0,
            FunctionTarget::Defined(id) => u64::from(id.0) + 1,
            FunctionTarget::InvalidBits(bits) => bits,
        }
    }
}

#[derive(Clone, Debug)]
enum Value {
    Scalar(ConstValue),
    Checked(ConstValue, bool),
    Pointer(memory::Pointer),
    FunctionPointer(FunctionPointer),
}

impl Value {
    fn scalar(&self) -> &ConstValue {
        match self {
            Self::Scalar(v) => v,
            _ => unreachable!("verified scalar operand"),
        }
    }

    #[cfg(any(test, not(whale_wave_ir)))]
    fn pointer(&self) -> memory::Pointer {
        match self {
            Self::Pointer(p) => *p,
            _ => unreachable!("verified pointer operand"),
        }
    }

    #[cfg(not(whale_wave_ir))]
    fn boolean(&self) -> bool {
        match self.scalar() {
            ConstValue::Bool(v) => *v,
            _ => unreachable!("verified bool operand"),
        }
    }
}

fn scalar_type(ty: &Type) -> bool {
    *ty == Type::Bool || integer::shape(ty).is_some() || scalar::float_type(ty)
}

#[cfg(not(whale_wave_ir))]
fn value_type(ty: &Type) -> bool {
    scalar_type(ty)
        || matches!(ty, Type::Ptr(_) | Type::FnPtr(_))
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
        | Neg { dst, .. }
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
        | VolatileLoad { dst, .. }
        | Load { dst, .. }
        | Gep { dst, .. }
        | NullFunction { dst, .. }
        | FunctionAddr { dst, .. }
        | GlobalAddr { dst, .. }
        | PtrDiff { dst, .. } => Some(*dst),
        Call { dst, .. } => *dst,
        LifetimeEnd { .. }
        | Uninit { .. }
        | VolatileStore { .. }
        | Store { .. }
        | Memcpy { .. }
        | Memset { .. }
        | TrapIf { .. } => None,
    }
}

#[cfg(not(whale_wave_ir))]
fn supported(inst: &Instruction) -> Result<(), &'static str> {
    use Instruction::*;
    let unsupported = match inst {
        Neg { ty, .. } if integer::shape(ty).is_some() || scalar::float_type(ty) => return Ok(()),
        Bin { ty, .. } if scalar::float_type(ty) => return Ok(()),
        ConstDecl { .. } => return Ok(()),
        Const { ty, .. } if scalar_type(ty) => return Ok(()),
        Mov { ty, .. } | Phi { ty, .. } | Select { ty, .. } if value_type(ty) => return Ok(()),
        Bin { ty, .. } | Not { ty, .. } | Checked { ty, .. } if integer::shape(ty).is_some() => {
            return Ok(())
        },
        Cmp { ty, .. } | ICmp { ty, .. }
            if *ty == Type::Bool || integer::shape(ty).is_some() || matches!(ty, Type::Ptr(_)) =>
        {
            return Ok(())
        },
        Cmp { ty, .. } | FCmp { ty, .. } if scalar::float_type(ty) => return Ok(()),
        Cast { op, src_ty, dst_ty, .. }
            if matches!(op, CastOp::ZExt | CastOp::SExt | CastOp::Trunc | CastOp::CheckedInt)
                && (*src_ty == Type::Bool || integer::shape(src_ty).is_some())
                && integer::shape(dst_ty).is_some() =>
        {
            return Ok(())
        },
        Cast {
            op:
                CastOp::FExt
                | CastOp::FTrunc
                | CastOp::IToF_S
                | CastOp::IToF_U
                | CastOp::FToI_S
                | CastOp::FToI_U,
            ..
        } => return Ok(()),
        Cast { op: CastOp::Bitcast, src_ty, dst_ty, .. }
            if (integer::shape(src_ty).is_some() || scalar::float_type(src_ty))
                && (integer::shape(dst_ty).is_some() || scalar::float_type(dst_ty)) =>
        {
            return Ok(())
        },
        Extract { dst_ty, .. } if scalar_type(dst_ty) => return Ok(()),
        Alloca { ty, .. } | Uninit { ty, .. } if allocation_type(ty) => return Ok(()),
        VolatileLoad { ty, .. } | Load { ty, .. } | VolatileStore { ty, .. } | Store { ty, .. }
            if value_type(ty) =>
        {
            return Ok(())
        },
        Gep { .. } => return Ok(()),
        Cast { op: CastOp::Bitcast, src_ty: Type::Ptr(_), dst_ty: Type::Ptr(_), .. }
        | Cast { op: CastOp::IntToPtr | CastOp::PtrToInt, .. } => return Ok(()),
        GlobalAddr { .. }
        | LifetimeEnd { .. }
        | PtrDiff { .. }
        | TrapIf { .. }
        | Memcpy { .. }
        | Memset { .. } => return Ok(()),
        Undef { .. } => "undef",
        Const { .. } => "non-scalar constant",
        Mov { .. } | Phi { .. } | Select { .. } | Extract { .. } => "non-scalar value",
        Neg { .. }
        | Bin { .. }
        | Not { .. }
        | Checked { .. }
        | Cmp { .. }
        | ICmp { .. }
        | FCmp { .. } => "non-integer arithmetic/comparison",
        Cast { .. } => "unsupported cast",
        Alloca { .. } => "alloca",
        VolatileLoad { .. } | Load { .. } => "load",
        VolatileStore { .. } | Store { .. } => "store",
        Uninit { .. } => "uninit",
        NullFunction { .. } | FunctionAddr { .. } | Call { .. } => return Ok(()),
    };
    Err(unsupported)
}

#[cfg(not(whale_wave_ir))]
fn allocation_type(root: &Type) -> bool {
    let mut pending = vec![root];
    while let Some(ty) = pending.pop() {
        if scalar_type(ty) || matches!(ty, Type::Ptr(_) | Type::FnPtr(_)) {
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

#[cfg(not(whale_wave_ir))]
fn integer_failure(
    error: integer::IntegerError,
    site: ExecutionSite,
    steps: u64,
) -> InterpreterError {
    match error {
        integer::IntegerError::Trap(reason) => {
            InterpreterError::Trap(InterpreterTrap { site, reason, steps })
        },
        integer::IntegerError::Backend(reason) => {
            InterpreterError::Backend { site: Some(site), reason }
        },
    }
}

#[cfg(not(whale_wave_ir))]
fn memory_failure(fault: memory::Fault, site: ExecutionSite, steps: u64) -> InterpreterError {
    match fault {
        memory::Fault::Backend(reason) => InterpreterError::Backend { site: Some(site), reason },
        memory::Fault::Trap(reason) => InterpreterError::Trap(InterpreterTrap {
            site,
            reason: TrapReason::Memory(reason),
            steps,
        }),
        memory::Fault::Limit { resource, limit } => {
            InterpreterError::MemoryLimit { site, resource, limit }
        },
    }
}

#[cfg(not(whale_wave_ir))]
fn gep_plan(
    value_types: &HashMap<ValueId, &Type>,
    ordinals: &HashMap<ValueId, usize>,
    base: ValueId,
    indices: &[ValueId],
) -> Vec<memory::GepStep> {
    let Type::Ptr(pointee) = value_types[&base] else { unreachable!("verified GEP base") };
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
            },
            Type::Struct(fields) | Type::Tuple(fields) => {
                let ordinal = ordinals[index];
                plan.push(memory::GepStep::Field(l(selected).field_offsets[ordinal]));
                selected = &fields[ordinal];
            },
            _ => unreachable!("verified GEP path"),
        }
    }
    plan
}

#[cfg(not(whale_wave_ir))]
fn pointer_compare(
    memory: &memory::Memory,
    pred: &ICmpPred,
    lhs: memory::Pointer,
    rhs: memory::Pointer,
) -> Result<bool, memory::Fault> {
    use ICmpPred::*;
    if matches!(pred, Eq | Ne) {
        let equal = memory.equal(lhs, rhs)?;
        return Ok(if *pred == Eq { equal } else { !equal });
    }
    let ordering = memory.compare_order(lhs, rhs)?;
    Ok(match pred {
        Ult | Slt => ordering.is_lt(),
        Ule | Sle => ordering.is_le(),
        Ugt | Sgt => ordering.is_gt(),
        Uge | Sge => ordering.is_ge(),
        _ => unreachable!(),
    })
}

#[cfg(not(whale_wave_ir))]
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

/// Verify the module and run a scalar entry function. Internal calls may carry tracked pointers.
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
    let result = run(module, function, Arguments::Scalars(arguments), options)?;
    Ok(InterpreterResult {
        value: result.value.map(|v| v.value.scalar().clone()),
        steps: result.steps,
    })
}

/// Run a checked library entry with integer/Bool, data-pointer or typed callable values.
/// Returned local pointers are expired. Each run owns a separate arena; passing an
/// expired/foreign tracked value to another run cannot resurrect its allocation.
pub fn interpret_values<'module>(
    module: &'module Module,
    function: FunctionId,
    arguments: &[RuntimeValue<'_>],
) -> Result<RuntimeInterpreterResult<'module>, InterpreterError> {
    interpret_values_with_options(module, function, arguments, InterpreterOptions::default())
}

pub fn interpret_values_with_options<'module>(
    module: &'module Module,
    function: FunctionId,
    arguments: &[RuntimeValue<'_>],
    options: InterpreterOptions,
) -> Result<RuntimeInterpreterResult<'module>, InterpreterError> {
    run(module, function, Arguments::Runtime(arguments), options)
}

enum Arguments<'a, 'module> {
    Scalars(&'a [ConstValue]),
    Runtime(&'a [RuntimeValue<'module>]),
}

impl Arguments<'_, '_> {
    fn len(&self) -> usize {
        match self {
            Self::Scalars(values) => values.len(),
            Self::Runtime(values) => values.len(),
        }
    }

    #[cfg(not(whale_wave_ir))]
    fn value(&self, index: usize) -> Value {
        match self {
            Self::Scalars(values) => Value::Scalar(values[index].clone()),
            Self::Runtime(values) => values[index].value.clone(),
        }
    }

    #[cfg(not(whale_wave_ir))]
    fn valid_type(&self, index: usize, expected: &Type) -> bool {
        match self {
            Self::Scalars(_) => true,
            Self::Runtime(values) => &values[index].ty == expected,
        }
    }

    fn valid_origin(&self, index: usize, module: &Module) -> bool {
        match self {
            Self::Scalars(_) => true,
            Self::Runtime(values) => match &values[index].value {
                Value::FunctionPointer(FunctionPointer {
                    target: FunctionTarget::Defined(_),
                    ..
                }) => {
                    values[index].callable_module.is_some_and(|origin| std::ptr::eq(origin, module))
                },
                _ => true,
            },
        }
    }
}

#[cfg(not(whale_wave_ir))]
struct Prepared<'a> {
    fun: &'a Function,
    value_capacity: usize,
    blocks: HashMap<BlockId, &'a BasicBlock>,
    sizes: HashMap<(BlockId, usize), (u64, u32)>,
    geps: HashMap<(BlockId, usize), Vec<memory::GepStep>>,
    signatures: HashMap<(BlockId, usize), Arc<FunctionSignature>>,
}

struct Frame {
    function: FunctionId,
    #[cfg(not(whale_wave_ir))]
    values: HashMap<ValueId, Value>,
    #[cfg(not(whale_wave_ir))]
    current: BlockId,
    #[cfg(not(whale_wave_ir))]
    predecessor: Option<BlockId>,
    #[cfg(not(whale_wave_ir))]
    instruction: usize,
    #[cfg(not(whale_wave_ir))]
    allocation_start: usize,
    #[cfg(not(whale_wave_ir))]
    return_destination: Option<ValueId>,
    call_site: Option<ExecutionSite>,
}

#[cfg(not(whale_wave_ir))]
fn callable_signature(signature: &FunctionSignature) -> bool {
    signature.convention == CallingConvention::Whale
        && !signature.variadic
        && signature.params.iter().all(call_value_type)
        && (signature.ret == Type::Void || call_value_type(&signature.ret))
}

#[cfg(not(whale_wave_ir))]
fn call_value_type(ty: &Type) -> bool {
    scalar_type(ty) || matches!(ty, Type::Ptr(_) | Type::FnPtr(_))
}

#[cfg(not(whale_wave_ir))]
fn intern_signature<'a>(
    signatures: &mut HashMap<&'a FunctionSignature, Arc<FunctionSignature>>,
    signature: &'a FunctionSignature,
) -> Arc<FunctionSignature> {
    signatures.entry(signature).or_insert_with(|| Arc::new(signature.clone())).clone()
}

#[cfg(not(whale_wave_ir))]
fn frame_values(
    fun: &Function,
    arguments: impl IntoIterator<Item = Value>,
    capacity: usize,
    site: ExecutionSite,
) -> Result<HashMap<ValueId, Value>, InterpreterError> {
    let mut values = HashMap::new();
    values.try_reserve(capacity).map_err(|_| InterpreterError::CallLimit {
        site,
        resource: CallResource::HostAllocation,
        limit: capacity as u64,
    })?;
    values.extend(fun.params.iter().zip(arguments).map(|(p, a)| (p.id, a)));
    Ok(values)
}

struct SourceDetailBudget {
    remaining: usize,
    omitted: usize,
}

impl SourceDetailBudget {
    fn new() -> Self {
        Self { remaining: MAX_RUNTIME_DIAGNOSTIC_SOURCE_BYTES, omitted: 0 }
    }

    fn location(&mut self, module: &Module, target: SourceTarget) -> Option<OriginalLocation> {
        let original = module.sources.original(target)?;
        let source = original.span.as_ref().and_then(|span| {
            let file = module.sources.files.iter().find(|f| f.id == span.file)?;
            if file.name.len() > self.remaining {
                self.omitted += 1;
                return None;
            }
            let mut name = String::new();
            if name.try_reserve_exact(file.name.len()).is_err() {
                self.omitted += 1;
                return None;
            }
            name.push_str(&file.name);
            self.remaining -= file.name.len();
            Some((name, span.clone()))
        });
        Some(OriginalLocation { original: original.id, source })
    }
}

fn run<'module>(
    module: &'module Module,
    function: FunctionId,
    arguments: Arguments<'_, '_>,
    options: InterpreterOptions,
) -> Result<RuntimeInterpreterResult<'module>, InterpreterError> {
    run_inner(module, function, arguments, options).map_err(|cause| {
        if cause.diagnostic().is_some() {
            return cause;
        }
        let global = match &cause {
            InterpreterError::GlobalMemory { global, .. }
            | InterpreterError::GlobalMemoryLimit { global, .. } => Some(*global),
            InterpreterError::UnsupportedGlobal(global) => Some(*global),
            _ => None,
        };
        let mut budget = SourceDetailBudget::new();
        if let Some((global, original)) = global.and_then(|global| {
            budget.location(module, SourceTarget::Global(global)).map(|original| (global, original))
        }) {
            InterpreterError::Diagnostic {
                cause: Box::new(cause),
                diagnostic: Box::new(RuntimeDiagnostic {
                    site: None,
                    global: Some(global),
                    original: Some(original),
                    calls: Vec::new(),
                    omitted_call_frames: 0,
                    omitted_source_details: budget.omitted,
                }),
            }
        } else {
            runtime_failure(module, cause, &[], options.max_trace_frames)
        }
    })
}

fn execution_source(
    module: &Module,
    site: ExecutionSite,
    budget: &mut SourceDetailBudget,
) -> Option<OriginalLocation> {
    let function = module.functions.iter().find(|f| f.id == site.function)?;
    let block = function.blocks.iter().find(|b| b.id == site.block)?;
    let target = if site.instruction == block.instructions.len() {
        SourceTarget::Terminator { function: site.function, block: site.block }
    } else {
        SourceTarget::Instruction {
            function: site.function,
            block: site.block,
            index: site.instruction,
        }
    };
    budget.location(module, target)
}

fn runtime_failure(
    module: &Module,
    cause: InterpreterError,
    frames: &[Frame],
    trace_limit: usize,
) -> InterpreterError {
    let Some(site) = cause.runtime_site() else {
        return cause;
    };
    let mut budget = SourceDetailBudget::new();
    let original = execution_source(module, site, &mut budget);
    let caller_count = frames.len().saturating_sub(1);
    if original.is_none() && caller_count == 0 {
        return cause;
    }
    let mut retained = caller_count.min(trace_limit.min(MAX_RUNTIME_TRACE_FRAMES));
    let mut calls = Vec::new();
    if calls.try_reserve_exact(retained).is_err() {
        retained = 0;
    }
    for child in frames.iter().skip(1).rev().take(retained) {
        let call_site = child.call_site.expect("nonentry frame records its caller");
        calls.push(RuntimeCallFrame {
            caller: call_site.function,
            callee: child.function,
            site: call_site,
            original: execution_source(module, call_site, &mut budget),
        });
    }
    calls.reverse();
    InterpreterError::Diagnostic {
        cause: Box::new(cause),
        diagnostic: Box::new(RuntimeDiagnostic {
            site: Some(site),
            global: None,
            original,
            calls,
            omitted_call_frames: caller_count - retained,
            omitted_source_details: budget.omitted,
        }),
    }
}

#[cfg(whale_wave_ir)]
fn run_inner<'module>(
    module: &'module Module,
    function: FunctionId,
    arguments: Arguments<'_, '_>,
    options: InterpreterOptions,
) -> Result<RuntimeInterpreterResult<'module>, InterpreterError> {
    runtime_wave::run(module, function, arguments, options)
}

#[cfg(not(whale_wave_ir))]
fn run_inner<'module>(
    module: &'module Module,
    function: FunctionId,
    arguments: Arguments<'_, '_>,
    options: InterpreterOptions,
) -> Result<RuntimeInterpreterResult<'module>, InterpreterError> {
    verify_module_with_limits(module, options.ir_limits)
        .map_err(|e| InterpreterError::Verification(Box::new(e)))?;
    let functions: HashMap<_, _> = module.functions.iter().map(|f| (f.id, f)).collect();
    let declarations: HashMap<_, _> = module.declarations.iter().map(|d| (d.id, d)).collect();
    // Shared immutable metadata is interned once before execution, independently of
    // the original O0 values/instructions and their execution count.
    let mut signatures = HashMap::new();
    let function_signatures: HashMap<_, _> = module
        .declarations
        .iter()
        .map(|decl| (decl.id, intern_signature(&mut signatures, &decl.signature)))
        .collect();
    let entry_fun = *functions.get(&function).ok_or(InterpreterError::UnknownFunction(function))?;
    if matches!(arguments, Arguments::Scalars(_))
        && (!entry_fun.params.iter().all(|p| scalar_type(&p.ty))
            || !(entry_fun.ret_ty == Type::Void || scalar_type(&entry_fun.ret_ty)))
    {
        return Err(InterpreterError::UnsupportedSignature(function));
    }
    if arguments.len() != entry_fun.params.len() {
        return Err(InterpreterError::ArgumentCount {
            expected: entry_fun.params.len(),
            got: arguments.len(),
        });
    }
    let mut input_values = Vec::new();
    input_values.try_reserve_exact(arguments.len()).map_err(|_| InterpreterError::CallLimit {
        site: ExecutionSite { function, block: entry_fun.entry, instruction: 0, value: None },
        resource: CallResource::HostAllocation,
        limit: arguments.len() as u64,
    })?;
    for (index, param) in entry_fun.params.iter().enumerate() {
        let valid_type = arguments.valid_type(index, &param.ty);
        let mut argument = arguments.value(index);
        let valid_value = match &argument {
            Value::Scalar(value) => {
                scalar_type(&param.ty) && crate::constant::valid_constant(&param.ty, value)
            },
            Value::Pointer(_) => matches!(param.ty, Type::Ptr(_)),
            Value::FunctionPointer(pointer) => {
                matches!(&param.ty, Type::FnPtr(signature) if **signature == *pointer.signature)
                    && match pointer.target {
                        FunctionTarget::Defined(id) => {
                            declarations.get(&id).is_some_and(|d| d.signature == *pointer.signature)
                        },
                        FunctionTarget::Null | FunctionTarget::InvalidBits(_) => true,
                    }
            },
            Value::Checked(..) => false,
        };
        if !valid_type || !valid_value || !arguments.valid_origin(index, module) {
            return Err(InterpreterError::ArgumentType { index, expected: param.ty.clone() });
        }
        if let Value::FunctionPointer(pointer) = &mut argument {
            let Type::FnPtr(signature) = &param.ty else {
                unreachable!("checked function parameter")
            };
            pointer.signature = intern_signature(&mut signatures, signature);
        }
        input_values.push(argument);
    }
    let arguments = &input_values;
    let mut pending = vec![function];
    let mut callable_targets = std::collections::BTreeSet::new();
    let mut callable_by_signature: HashMap<&FunctionSignature, Vec<FunctionId>> = HashMap::new();
    for argument in arguments {
        if let Value::FunctionPointer(FunctionPointer {
            target: FunctionTarget::Defined(id), ..
        }) = argument
        {
            if callable_targets.insert(id.0) {
                callable_by_signature.entry(&declarations[id].signature).or_default().push(*id);
            }
        }
    }
    let mut indirect_signatures: HashMap<&FunctionSignature, ExecutionSite> = HashMap::new();
    let mut visited = HashSet::new();
    let mut prepared = HashMap::new();
    while let Some(function) = pending.pop() {
        if !visited.insert(function) {
            continue;
        }
        let fun = functions[&function];
        if !callable_signature(&declarations[&function].signature) {
            return Err(InterpreterError::UnsupportedSignature(function));
        }
        let site = |block: BlockId, instruction: usize, value: Option<ValueId>| ExecutionSite {
            function,
            block,
            instruction,
            value,
        };
        let value_types: HashMap<_, _> = fun.value_types.iter().map(|(id, ty)| (*id, ty)).collect();
        let ordinals: HashMap<_, _> = fun
            .blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .filter_map(|i| match i {
                Instruction::Const { dst, value, .. }
                | Instruction::ConstDecl { dst, value, .. } => match value {
                    ConstValue::I(v) => usize::try_from(*v).ok().map(|v| (*dst, v)),
                    ConstValue::U(v) => usize::try_from(*v).ok().map(|v| (*dst, v)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let mut sizes = HashMap::new();
        let mut geps = HashMap::new();
        let mut instruction_signatures = HashMap::new();
        for block in &fun.blocks {
            for (index, inst) in block.instructions.iter().enumerate() {
                let at = site(block.id, index, destination(inst));
                supported(inst).map_err(|operation| InterpreterError::UnsupportedInstruction {
                    site: at,
                    operation,
                })?;
                match inst {
                    Instruction::NullFunction { signature, .. } => {
                        instruction_signatures.insert(
                            (block.id, index),
                            intern_signature(&mut signatures, signature),
                        );
                    },
                    Instruction::VolatileLoad { ty: Type::FnPtr(signature), .. }
                    | Instruction::Load { ty: Type::FnPtr(signature), .. } => {
                        instruction_signatures.insert(
                            (block.id, index),
                            intern_signature(&mut signatures, signature),
                        );
                    },
                    Instruction::Call { callee, convention, .. } => {
                        if *convention != CallingConvention::Whale {
                            return Err(InterpreterError::UnsupportedInstruction {
                                site: at,
                                operation: "call",
                            });
                        }
                        match callee {
                            Callee::Direct(id) => {
                                let declaration = declarations[id];
                                if declaration.linkage != Linkage::Internal
                                    || !functions.contains_key(id)
                                {
                                    return Err(InterpreterError::UnsupportedInstruction {
                                        site: at,
                                        operation: "call",
                                    });
                                }
                                pending.push(*id);
                            },
                            Callee::Indirect(value) => {
                                let Type::FnPtr(signature) = value_types[value] else {
                                    unreachable!("verified indirect callee")
                                };
                                if let std::collections::hash_map::Entry::Vacant(entry) =
                                    indirect_signatures.entry(signature)
                                {
                                    entry.insert(at);
                                    for target in callable_by_signature
                                        .get(&**signature)
                                        .into_iter()
                                        .flatten()
                                    {
                                        let declaration = declarations[target];
                                        if declaration.linkage != Linkage::Internal
                                            || !functions.contains_key(target)
                                        {
                                            return Err(InterpreterError::UnsupportedInstruction {
                                                site: at,
                                                operation: "call",
                                            });
                                        }
                                        pending.push(*target);
                                    }
                                }
                            },
                        }
                    },
                    Instruction::FunctionAddr { function: target, signature, .. } => {
                        instruction_signatures.insert(
                            (block.id, index),
                            intern_signature(&mut signatures, signature),
                        );
                        if callable_targets.insert(target.0) {
                            callable_by_signature.entry(signature).or_default().push(*target);
                            if let Some(at) = indirect_signatures.get(signature) {
                                if declarations[target].linkage != Linkage::Internal
                                    || !functions.contains_key(target)
                                {
                                    return Err(InterpreterError::UnsupportedInstruction {
                                        site: *at,
                                        operation: "call",
                                    });
                                }
                                pending.push(*target);
                            }
                        }
                    },
                    Instruction::Alloca { ty, .. } | Instruction::Uninit { ty, .. } => {
                        let l = layout_of_with_limit(ty, Target::X86_64WhaleLinux, MAX_IR_NESTING)
                            .unwrap();
                        let alignment = if matches!(ty, Type::Array(..)) && l.size >= 16 {
                            l.align.max(16)
                        } else {
                            l.align
                        };
                        sizes.insert((block.id, index), (l.size, alignment));
                    },
                    Instruction::Gep { base_ptr, indices, .. } => {
                        let plan = gep_plan(&value_types, &ordinals, *base_ptr, indices);
                        if plan.iter().any(|step| matches!(step, memory::GepStep::Stride(0))) {
                            return Err(InterpreterError::UnsupportedInstruction {
                                site: at,
                                operation: "zero-sized pointer arithmetic",
                            });
                        }
                        geps.insert((block.id, index), plan);
                    },
                    _ => {},
                }
            }
        }
        prepared.insert(
            function,
            Prepared {
                fun,
                value_capacity: fun.params.len()
                    + fun
                        .blocks
                        .iter()
                        .flat_map(|b| &b.instructions)
                        .filter(|i| destination(i).is_some())
                        .count(),
                blocks: fun.blocks.iter().map(|b| (b.id, b)).collect(),
                sizes,
                geps,
                signatures: instruction_signatures,
            },
        );
    }
    // Validate all global storage before allocating or executing any instruction.
    for global in &module.global_storage {
        if !call_value_type(&global.ty) {
            return Err(InterpreterError::UnsupportedGlobal(global.id));
        }
    }
    let mut memory = memory::Memory::new(options.memory_limits);
    let mut global_pointers = HashMap::new();
    for global in &module.global_storage {
        let failure = |fault| match fault {
            memory::Fault::Backend(reason) => InterpreterError::Backend { site: None, reason },
            memory::Fault::Trap(reason) => {
                InterpreterError::GlobalMemory { global: global.id, reason }
            },
            memory::Fault::Limit { resource, limit } => {
                InterpreterError::GlobalMemoryLimit { global: global.id, resource, limit }
            },
        };
        let layout = layout_of_with_limit(
            &global.ty,
            Target::X86_64WhaleLinux,
            options.ir_limits.max_type_depth,
        )
        .expect("verified scalar layout");
        let pointer =
            memory.allocate_global(layout.size, layout.align, global.align).map_err(failure)?;
        if let crate::GlobalStorageInit::Value(value) = &global.init {
            memory
                .store(pointer, &global.ty, &Value::Scalar(value.clone()), global.align)
                .map_err(failure)?;
        }
        let pointer = memory.seal_global(pointer, global.readonly).map_err(failure)?;
        global_pointers.insert(global.id, pointer);
    }
    let entry_allocation_start = memory.allocation_count();
    let entry_site =
        ExecutionSite { function, block: entry_fun.entry, instruction: 0, value: None };
    if options.max_call_depth == 0 || options.max_frames == 0 {
        return Err(InterpreterError::CallLimit {
            site: entry_site,
            resource: if options.max_call_depth == 0 {
                CallResource::Depth
            } else {
                CallResource::Frames
            },
            limit: 0,
        });
    }
    let mut live_values = prepared[&function].value_capacity;
    if live_values > options.max_live_values {
        return Err(InterpreterError::CallLimit {
            site: entry_site,
            resource: CallResource::Values,
            limit: options.max_live_values as u64,
        });
    }
    let entry_values = frame_values(entry_fun, arguments.iter().cloned(), live_values, entry_site)?;
    let mut frames = vec![Frame {
        function,
        values: entry_values,
        current: entry_fun.entry,
        predecessor: None,
        instruction: 0,
        allocation_start: entry_allocation_start,
        return_destination: None,
        call_site: None,
    }];
    let mut frames_created = 1u64;
    let mut steps = 0;
    let charge = |steps: &mut u64, site| -> Result<(), InterpreterError> {
        if *steps >= options.max_steps {
            return Err(InterpreterError::StepLimit { site, limit: options.max_steps });
        }
        *steps += 1;
        Ok(())
    };
    let outcome = (|| {
        loop {
            let depth = frames.len();
            let frame = frames.last_mut().unwrap();
            let function = frame.function;
            let fun = prepared[&function].fun;
            let sizes = &prepared[&function].sizes;
            let geps = &prepared[&function].geps;
            let current = frame.current;
            let predecessor = frame.predecessor;
            let block = prepared[&function].blocks[&current];
            let site = |block: BlockId, instruction: usize, value: Option<ValueId>| ExecutionSite {
                function,
                block,
                instruction,
                value,
            };
            let values = &mut frame.values;
            // All incoming operands are read from the predecessor's environment.
            // Commit simultaneously, so loop-carried phis can swap each other's values.
            if frame.instruction == 0 {
                let mut phis = Vec::new();
                let phi_count = block
                    .instructions
                    .iter()
                    .take_while(|i| matches!(i, Instruction::Phi { .. }))
                    .count();
                phis.try_reserve_exact(phi_count).map_err(|_| InterpreterError::CallLimit {
                    site: site(current, 0, None),
                    resource: CallResource::HostAllocation,
                    limit: phi_count as u64,
                })?;
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
                frame.instruction = phis.len();
                values.extend(phis);
            }
            if frame.instruction < block.instructions.len() {
                let index = frame.instruction;
                let inst = &block.instructions[index];
                frame.instruction += 1;
                let at = site(current, index, destination(inst));
                charge(&mut steps, at)?;
                let get = |id: &ValueId| &values[id];
                let scalar = |id: &ValueId| get(id).scalar();
                let result = match inst {
                    Instruction::GlobalAddr { global, .. } => {
                        Value::Pointer(global_pointers[global])
                    },
                    Instruction::LifetimeEnd { ptr } => {
                        memory
                            .retire_stack(get(ptr).pointer(), frame.allocation_start)
                            .map_err(|e| memory_failure(e, at, steps))?;
                        continue;
                    },
                    Instruction::PtrDiff { lhs, rhs, .. } => Value::Scalar(ConstValue::I(
                        memory
                            .difference(get(lhs).pointer(), get(rhs).pointer())
                            .map_err(|e| memory_failure(e, at, steps))?,
                    )),
                    Instruction::NullFunction { .. } => Value::FunctionPointer(FunctionPointer {
                        signature: prepared[&function].signatures[&(current, index)].clone(),
                        target: FunctionTarget::Null,
                    }),
                    Instruction::FunctionAddr { function: target, .. } => {
                        Value::FunctionPointer(FunctionPointer {
                            signature: prepared[&function].signatures[&(current, index)].clone(),
                            target: FunctionTarget::Defined(*target),
                        })
                    },
                    Instruction::Call { callee, args, dst, .. } => {
                        let target = match callee {
                            Callee::Direct(id) => *id,
                            Callee::Indirect(id) => {
                                let Value::FunctionPointer(pointer) = get(id) else {
                                    unreachable!("verified function pointer operand")
                                };
                                let reason = match pointer.target {
                                    FunctionTarget::Null => Some(TrapReason::NullFunction),
                                    FunctionTarget::InvalidBits(_) => {
                                        Some(TrapReason::InvalidFunctionPointer)
                                    },
                                    FunctionTarget::Defined(id)
                                        if function_signatures.get(&id).is_none_or(
                                            |signature| !Arc::ptr_eq(signature, &pointer.signature),
                                        ) =>
                                    {
                                        Some(TrapReason::FunctionSignatureMismatch)
                                    },
                                    FunctionTarget::Defined(_) => None,
                                };
                                if let Some(reason) = reason {
                                    return Err(InterpreterError::Trap(InterpreterTrap {
                                        site: at,
                                        reason,
                                        steps,
                                    }));
                                }
                                let FunctionTarget::Defined(id) = pointer.target else {
                                    unreachable!()
                                };
                                id
                            },
                        };
                        if depth >= options.max_call_depth {
                            return Err(InterpreterError::CallLimit {
                                site: at,
                                resource: CallResource::Depth,
                                limit: options.max_call_depth as u64,
                            });
                        }
                        if frames_created >= options.max_frames {
                            return Err(InterpreterError::CallLimit {
                                site: at,
                                resource: CallResource::Frames,
                                limit: options.max_frames,
                            });
                        }
                        let child = &prepared[&target];
                        let next_live_values = live_values
                            .checked_add(child.value_capacity)
                            .filter(|n| *n <= options.max_live_values)
                            .ok_or(InterpreterError::CallLimit {
                                site: at,
                                resource: CallResource::Values,
                                limit: options.max_live_values as u64,
                            })?;
                        let child_values = frame_values(
                            child.fun,
                            args.iter().map(|a| get(a).clone()),
                            child.value_capacity,
                            at,
                        )?;
                        let destination = *dst;
                        frames.try_reserve(1).map_err(|_| InterpreterError::CallLimit {
                            site: at,
                            resource: CallResource::HostAllocation,
                            limit: depth as u64,
                        })?;
                        frames.push(Frame {
                            function: target,
                            values: child_values,
                            current: child.fun.entry,
                            predecessor: None,
                            instruction: 0,
                            allocation_start: memory.allocation_count(),
                            return_destination: destination,
                            call_site: Some(at),
                        });
                        frames_created += 1;
                        live_values = next_live_values;
                        continue;
                    },
                    Instruction::Const { value, .. } | Instruction::ConstDecl { value, .. } => {
                        Value::Scalar(value.clone())
                    },
                    Instruction::Mov { src, .. } => get(src).clone(),
                    Instruction::Bin { op, lhs, rhs, .. }
                        if matches!(scalar(lhs), ConstValue::F(_)) =>
                    {
                        let (ConstValue::F(a), ConstValue::F(b)) = (scalar(lhs), scalar(rhs))
                        else {
                            unreachable!()
                        };
                        Value::Scalar(ConstValue::F(
                            crate::float::arithmetic::arithmetic(op, *a, *b).unwrap(),
                        ))
                    },
                    Instruction::Neg { ty, src, .. } => Value::Scalar(match scalar(src) {
                        ConstValue::F(value) => {
                            ConstValue::F(crate::float::arithmetic::negate(*value))
                        },
                        value => integer::try_unary(true, ty, value)
                            .map_err(|e| integer_failure(e, at, steps))?,
                    }),
                    Instruction::Bin { op, ty, lhs, rhs, .. } => Value::Scalar(
                        integer::try_bin(op, ty, scalar(lhs), scalar(rhs))
                            .map_err(|e| integer_failure(e, at, steps))?,
                    ),
                    Instruction::Not { ty, src, .. } => Value::Scalar(
                        integer::try_unary(false, ty, scalar(src))
                            .map_err(|e| integer_failure(e, at, steps))?,
                    ),
                    Instruction::Cmp { op, ty, lhs, rhs, .. } => {
                        let result = if scalar::float_type(ty) {
                            scalar::compare(
                                &op.float_predicate().unwrap(),
                                scalar(lhs),
                                scalar(rhs),
                            )
                        } else if matches!(ty, Type::Ptr(_)) {
                            pointer_compare(
                                &memory,
                                &cmp_pred(*op),
                                get(lhs).pointer(),
                                get(rhs).pointer(),
                            )
                            .map_err(|e| memory_failure(e, at, steps))?
                        } else {
                            integer::try_compare(&cmp_pred(*op), ty, scalar(lhs), scalar(rhs))
                                .map_err(|e| integer_failure(e, at, steps))?
                        };
                        Value::Scalar(ConstValue::Bool(result))
                    },
                    Instruction::ICmp { pred, ty, lhs, rhs, .. } => {
                        let result = if matches!(ty, Type::Ptr(_)) {
                            pointer_compare(&memory, pred, get(lhs).pointer(), get(rhs).pointer())
                                .map_err(|e| memory_failure(e, at, steps))?
                        } else {
                            integer::try_compare(pred, ty, scalar(lhs), scalar(rhs))
                                .map_err(|e| integer_failure(e, at, steps))?
                        };
                        Value::Scalar(ConstValue::Bool(result))
                    },
                    Instruction::FCmp { pred, lhs, rhs, .. } => Value::Scalar(ConstValue::Bool(
                        scalar::compare(pred, scalar(lhs), scalar(rhs)),
                    )),
                    Instruction::Select { cond, on_true, on_false, .. } => {
                        get(if get(cond).boolean() { on_true } else { on_false }).clone()
                    },
                    Instruction::Cast { op, src_ty, dst_ty, src, .. } => match op {
                        CastOp::FExt
                        | CastOp::FTrunc
                        | CastOp::IToF_S
                        | CastOp::IToF_U
                        | CastOp::FToI_S
                        | CastOp::FToI_U => Value::Scalar(
                            crate::float::arithmetic::convert(op, src_ty, dst_ty, scalar(src))
                                .ok_or({
                                    InterpreterError::Trap(InterpreterTrap {
                                        site: at,
                                        reason: TrapReason::InvalidConversion,
                                        steps,
                                    })
                                })?,
                        ),
                        CastOp::CheckedInt => Value::Scalar(
                            integer::try_checked_cast(src_ty, dst_ty, scalar(src))
                                .map_err(|e| integer_failure(e, at, steps))?,
                        ),
                        CastOp::IntToPtr => {
                            Value::Pointer(memory::Pointer::raw(
                                integer::raw(scalar(src), 64) as u64
                            ))
                        },
                        CastOp::PtrToInt => Value::Scalar(integer::pack(
                            dst_ty,
                            u128::from(get(src).pointer().address),
                        )),
                        CastOp::Bitcast if matches!(src_ty, Type::Ptr(_)) => get(src).clone(),
                        CastOp::Bitcast => {
                            Value::Scalar(scalar::pack(dst_ty, scalar::raw(src_ty, scalar(src))))
                        },
                        _ => Value::Scalar(
                            integer::try_cast(op, src_ty, dst_ty, scalar(src))
                                .map_err(|e| integer_failure(e, at, steps))?,
                        ),
                    },
                    Instruction::Alloca { align, .. } => {
                        let (size, natural) = sizes[&(current, index)];
                        Value::Pointer(
                            memory
                                .allocate(size, natural, *align)
                                .map_err(|e| memory_failure(e, at, steps))?,
                        )
                    },
                    Instruction::VolatileLoad { ty, ptr, align, .. }
                    | Instruction::Load { ty, ptr, align, .. } => memory
                        .load_with_signature(
                            get(ptr).pointer(),
                            ty,
                            *align,
                            prepared[&function].signatures.get(&(current, index)),
                        )
                        .map_err(|e| memory_failure(e, at, steps))?,
                    Instruction::VolatileStore { ty, value, ptr, align }
                    | Instruction::Store { ty, value, ptr, align } => {
                        memory
                            .store(get(ptr).pointer(), ty, get(value), *align)
                            .map_err(|e| memory_failure(e, at, steps))?;
                        continue;
                    },
                    Instruction::Uninit { ptr, align, .. } => {
                        memory
                            .uninit(get(ptr).pointer(), sizes[&(current, index)].0, *align)
                            .map_err(|e| memory_failure(e, at, steps))?;
                        continue;
                    },
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
                    },
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
                    },
                    Instruction::Gep { base_ptr, indices, .. } => {
                        let inputs: Vec<_> = indices.iter().map(|id| scalar(id).clone()).collect();
                        Value::Pointer(
                            memory
                                .gep(get(base_ptr).pointer(), &geps[&(current, index)], &inputs)
                                .map_err(|e| memory_failure(e, at, steps))?,
                        )
                    },
                    Instruction::Checked { op, ty, lhs, rhs, .. } => {
                        let (v, overflow) = integer::try_checked(op, ty, scalar(lhs), scalar(rhs))
                            .map_err(|e| integer_failure(e, at, steps))?;
                        Value::Checked(v, overflow)
                    },
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
                    },
                    _ => unreachable!("supported subset checked before execution"),
                };
                values.insert(destination(inst).expect("value-producing instruction"), result);
                continue;
            }
            let at = site(current, block.instructions.len(), None);
            charge(&mut steps, at)?;
            let next = match block.terminator.as_ref().expect("verified terminator") {
                Terminator::Br { target } => *target,
                Terminator::CBr { cond, then_bb, else_bb } => {
                    if values[cond].boolean() {
                        *then_bb
                    } else {
                        *else_bb
                    }
                },
                Terminator::Switch { value, default_bb, cases, .. } => cases
                    .iter()
                    .find(|(case, _)| case == values[value].scalar())
                    .map_or(*default_bb, |(_, target)| *target),
                Terminator::Ret { value, .. } => {
                    let value = value.map(|v| values[&v].clone());
                    memory
                        .retire_from(frame.allocation_start)
                        .map_err(|e| memory_failure(e, at, steps))?;
                    let destination = frame.return_destination;
                    frames.pop();
                    live_values -= prepared[&function].value_capacity;
                    if let Some(parent) = frames.last_mut() {
                        if let Some(destination) = destination {
                            parent
                                .values
                                .insert(destination, value.expect("verified nonvoid call result"));
                        }
                        continue;
                    }
                    return Ok(RuntimeInterpreterResult {
                        value: value.map(|value| RuntimeValue {
                            ty: fun.ret_ty.clone(),
                            value,
                            callable_module: Some(module),
                        }),
                        steps,
                    });
                },
                Terminator::Trap { reason } => {
                    return Err(InterpreterError::Trap(InterpreterTrap {
                        site: at,
                        reason: TrapReason::Explicit(reason.clone()),
                        steps,
                    }))
                },
            };
            frame.predecessor = Some(current);
            frame.current = next;
            frame.instruction = 0;
        }
    })();
    outcome.map_err(|cause| runtime_failure(module, cause, &frames, options.max_trace_frames))
}
