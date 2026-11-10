// SPDX-License-Identifier: MPL-2.0
//! Typed transport for the resumable Wave interpreter. Rust owns buffers and
//! memory-operation requests; Wave owns reachability, execution and counters.
use super::*;

use std::collections::HashMap;

const MISSING: u64 = u64::MAX;

static VOID_TYPE: Type = Type::Void;

const BACKEND: &str = "invalid Wave interpreter transport";

const ALLOCATION: &str = "unable to allocate Wave interpreter transport";

type Result<T> = std::result::Result<T, InterpreterError>;

pub(crate) trait WireDefault {
    fn wire_default() -> Self;
}

impl WireDefault for u64 {
    fn wire_default() -> Self {
        0
    }
}

impl WireDefault for u8 {
    fn wire_default() -> Self {
        0
    }
}

impl<T> WireDefault for *mut T {
    fn wire_default() -> Self {
        std::ptr::null_mut()
    }
}

macro_rules! record {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub(crate) struct $name { $(pub(crate) $field: $ty),* }

        impl Default for $name {
            fn default() -> Self { Self { $($field: <$ty as WireDefault>::wire_default()),* } }
        }

        impl WireDefault for $name { fn wire_default() -> Self { Self::default() } }
    };
}
record!(RawType { kind: u64, child: u64, first: u64, count: u64, extra: u64 });
record!(RawPointer {
    address: u64,
    present: u64,
    arena: u64,
    allocation: u64,
    generation: u64,
    lower: u64,
    upper: u64,
    offset: u64,
    read: u64,
    write: u64
});
record!(RawGep { kind: u64, amount: u64, low: u64, high: u64, signed: u64 });
record!(Storage {
    ptr_bits: u64,
    pointer_align: u64,
    scalar_align1: u64,
    scalar_align2: u64,
    scalar_align4: u64,
    scalar_align8: u64,
    scalar_align16: u64,
    large_array_threshold: u64,
    large_array_align: u64,
    has_large_array: u64
});
record!(LayoutNode { kind: u64, child: u64, first: u64, count: u64 });
record!(Layout { size: u64, align: u64, stride: u64, allocation_align: u64, depth: u64 });
record!(RawValue {
    kind: u64,
    ty: u64,
    literal: u64,
    low: u64,
    high: u64,
    flag: u64,
    pointer: RawPointer,
    signature: u64,
    target_kind: u64,
    target: u64,
    origin: u64
});
record!(RawSlot { generation: u64, value: RawValue });
record!(RawFrame {
    function: u64,
    block: u64,
    predecessor: u64,
    instruction: u64,
    value_base: u64,
    value_count: u64,
    generation: u64,
    allocation_start: u64,
    return_slot: u64,
    call_site: u64
});
record!(RawFunction {
    id: u64,
    signature: u64,
    entry: u64,
    first_block: u64,
    block_count: u64,
    first_parameter: u64,
    parameter_count: u64,
    value_count: u64,
    first_value_type: u64,
    has_body: u64,
    linkage: u64
});
record!(RawBlock { id: u64, function: u64, first: u64, count: u64 });
record!(RawInstruction {
    op: u64,
    sub: u64,
    ty: u64,
    other: u64,
    dst: u64,
    a: u64,
    b: u64,
    c: u64,
    align: u64,
    size: u64,
    natural: u64,
    first: u64,
    count: u64,
    low: u64,
    high: u64,
    literal: u64,
    signature: u64,
    flags: u64,
    global: u64,
    plan_first: u64,
    plan_count: u64
});
record!(RawGlobal {
    id: u64,
    ty: u64,
    size: u64,
    align: u64,
    natural: u64,
    readonly: u64,
    initialized: u64,
    value: RawValue
});
record!(RawEvent {
    kind: u64,
    ty: u64,
    size: u64,
    natural: u64,
    align: u64,
    flag: u64,
    owner: u64,
    first: u64,
    count: u64,
    frame_capacity: u64,
    value_capacity: u64,
    phi_capacity: u64,
    left: RawValue,
    right: RawValue
});
record!(RawResult {
    status: u64,
    error: u64,
    site: u64,
    detail: u64,
    extra: u64,
    global: u64,
    steps: u64,
    value: RawValue,
    event: RawEvent
});
record!(RawDriver {
    types: *mut RawType,
    type_count: u64,
    type_edges: *mut u64,
    type_edge_count: u64,
    functions: *mut RawFunction,
    function_count: u64,
    blocks: *mut RawBlock,
    block_count: u64,
    instructions: *mut RawInstruction,
    instruction_count: u64,
    operands: *mut u64,
    operand_count: u64,
    value_types: *mut u64,
    value_type_count: u64,
    globals: *mut RawGlobal,
    global_count: u64,
    global_values: *mut RawValue,
    arguments: *mut RawValue,
    argument_count: u64,
    scalar_arguments: u64,
    entry_id: u64,
    storage: *mut Storage,
    layout_nodes: *mut LayoutNode,
    layouts: *mut Layout,
    layout_offsets: *mut u64,
    plans: *mut RawGep,
    plan_capacity: u64,
    plan_count: u64,
    ordinals: *mut u64,
    function_table: *mut u64,
    function_table_capacity: u64,
    prepared: *mut u8,
    pending_previous: *mut u64,
    pending_next: *mut u64,
    pending_flags: *mut u8,
    callable_flags: *mut u8,
    callable_targets: *mut u64,
    callable_count: u64,
    indirect_signatures: *mut u64,
    indirect_sites: *mut u64,
    indirect_count: u64,
    indirect_capacity: u64,
    pending_head: u64,
    frames: *mut RawFrame,
    frame_capacity: u64,
    frame_count: u64,
    slots: *mut RawSlot,
    slot_capacity: u64,
    slot_count: u64,
    phis: *mut RawValue,
    phi_capacity: u64,
    phase: u64,
    cursor: u64,
    global_phase: u64,
    entry: u64,
    pending: u64,
    pending_destination: u64,
    pending_type: u64,
    pending_flag: u64,
    pending_target: u64,
    pending_site: u64,
    pending_value: RawValue,
    resume: u64,
    resume_value: RawValue,
    memory_allocations: u64,
    frames_created: u64,
    steps: u64,
    work: u64,
    max_steps: u64,
    max_depth: u64,
    max_frames: u64,
    max_values: u64,
    max_work: u64,
    result: *mut RawResult
});

unsafe extern "C" {
    fn whale_ir_runtime_step(driver: *mut RawDriver) -> i32;

    fn whale_ir_bytecode_runtime_step(
        program: *mut std::ffi::c_void,
        driver: *mut RawDriver,
        scratch: *mut u64,
        capacity: u64,
        limits: *const crate::bytecode::BytecodeLimits,
        admission: *mut std::ffi::c_void,
    ) -> i32;
}

pub(crate) struct RuntimeAdmission {
    pub(crate) program: *mut std::ffi::c_void,
    pub(crate) scratch: *mut u64,
    pub(crate) capacity: u64,
    pub(crate) limits: crate::bytecode::BytecodeLimits,
    pub(crate) result: *mut std::ffi::c_void,
}

fn backend(site: Option<ExecutionSite>) -> InterpreterError {
    InterpreterError::Backend { site, reason: BACKEND }
}

fn reserve<T>(values: &mut Vec<T>, count: usize) -> Result<()> {
    values
        .try_reserve(count)
        .map_err(|_| InterpreterError::Backend { site: None, reason: ALLOCATION })
}

fn push<T>(values: &mut Vec<T>, value: T) -> Result<u64> {
    reserve(values, 1)?;
    let index = values.len() as u64;
    values.push(value);
    Ok(index)
}

fn initialized<T: Default>(count: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    reserve(&mut values, count)?;
    values.resize_with(count, T::default);
    Ok(values)
}

fn map_reserve<K: Eq + std::hash::Hash, V>(map: &mut HashMap<K, V>, count: usize) -> Result<()> {
    map.try_reserve(count).map_err(|_| InterpreterError::Backend { site: None, reason: ALLOCATION })
}

impl From<memory::Pointer> for RawPointer {
    fn from(value: memory::Pointer) -> Self {
        let mut raw = Self { address: value.address, ..Self::default() };
        if let Some(c) = value.capability {
            raw.present = 1;
            raw.arena = c.arena;
            raw.allocation = c.allocation as u64;
            raw.generation = c.generation;
            raw.lower = c.lower;
            raw.upper = c.upper;
            raw.offset = c.offset;
            raw.read = u64::from(c.read);
            raw.write = u64::from(c.write);
        }
        raw
    }
}

impl RawPointer {
    fn decode(self) -> Result<memory::Pointer> {
        if self.present > 1 || self.read > 1 || self.write > 1 {
            return Err(backend(None));
        }
        Ok(memory::Pointer {
            address: self.address,
            capability: if self.present == 0 {
                None
            } else {
                Some(memory::Capability {
                    arena: self.arena,
                    allocation: usize::try_from(self.allocation).map_err(|_| backend(None))?,
                    generation: self.generation,
                    lower: self.lower,
                    upper: self.upper,
                    offset: self.offset,
                    read: self.read != 0,
                    write: self.write != 0,
                })
            },
        })
    }
}

#[derive(Clone, Copy)]
enum TypeView<'a> {
    Type(&'a Type),
    Signature(&'a FunctionSignature),
}

pub(crate) struct ArgumentCheckpoint {
    types: usize,
    edges: usize,
    type_capacity: usize,
    edge_capacity: usize,
    cache_capacity: usize,
    signature_capacity: usize,
}

pub(crate) struct Program<'a> {
    module: Option<&'a Module>,
    pub(crate) sources: SourceMetadata,
    pub(crate) types: Vec<RawType>,
    type_views: Vec<TypeView<'a>>,
    type_ids: HashMap<&'a Type, u64>,
    signature_ids: HashMap<&'a FunctionSignature, u64>,
    signature_arcs: HashMap<u64, Arc<FunctionSignature>>,
    cached_types: HashMap<u64, Type>,
    pub(crate) edges: Vec<u64>,
    pub(crate) functions: Vec<RawFunction>,
    pub(crate) blocks: Vec<RawBlock>,
    pub(crate) instructions: Vec<RawInstruction>,
    pub(crate) operands: Vec<u64>,
    pub(crate) value_types: Vec<u64>,
    pub(crate) value_ids: Vec<u64>,
    pub(crate) globals: Vec<RawGlobal>,
    pub(crate) sites: Vec<ExecutionSite>,
    pub(crate) reasons: Vec<std::borrow::Cow<'a, str>>,
    declaration_ids: HashMap<FunctionId, u64>,
    global_ids: HashMap<GlobalId, u64>,
}

impl<'a> Program<'a> {
    pub(crate) fn new(module: Option<&'a Module>) -> Result<Self> {
        Ok(Self {
            module,
            sources: SourceMetadata::default(),
            types: Vec::new(),
            type_views: Vec::new(),
            type_ids: HashMap::new(),
            signature_ids: HashMap::new(),
            signature_arcs: HashMap::new(),
            cached_types: HashMap::new(),
            edges: Vec::new(),
            functions: Vec::new(),
            blocks: Vec::new(),
            instructions: Vec::new(),
            operands: Vec::new(),
            value_types: Vec::new(),
            value_ids: Vec::new(),
            globals: Vec::new(),
            sites: Vec::new(),
            reasons: Vec::new(),
            declaration_ids: HashMap::new(),
            global_ids: HashMap::new(),
        })
    }

    fn append_type(&mut self, node: RawType, view: TypeView<'a>) -> Result<u64> {
        let id = push(&mut self.types, node)?;
        push(&mut self.type_views, view)?;
        Ok(id)
    }

    fn signature(&mut self, signature: &'a FunctionSignature) -> Result<u64> {
        if let Some(id) = self.signature_ids.get(signature) {
            return Ok(*id);
        }
        let mut children = Vec::new();
        push(&mut children, self.ty(&signature.ret)?)?;
        for param in &signature.params {
            push(&mut children, self.ty(param)?)?;
        }
        let first = self.edges.len() as u64;
        reserve(&mut self.edges, children.len())?;
        self.edges.extend(children);
        let id = self.append_type(
            RawType {
                kind: 18,
                first,
                count: signature.params.len() as u64 + 1,
                extra: u64::from(signature.convention == CallingConvention::SysV64)
                    | (u64::from(signature.variadic) << 1),
                ..RawType::default()
            },
            TypeView::Signature(signature),
        )?;
        map_reserve(&mut self.signature_ids, 1)?;
        self.signature_ids.insert(signature, id);
        Ok(id)
    }

    fn ty(&mut self, ty: &'a Type) -> Result<u64> {
        if let Some(id) = self.type_ids.get(ty) {
            return Ok(*id);
        }
        if let Type::FnPtr(signature) = ty {
            let id = self.signature(signature)?;
            map_reserve(&mut self.type_ids, 1)?;
            self.type_ids.insert(ty, id);
            return Ok(id);
        }
        let mut node = RawType::default();
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
                node.child = self.ty(child)?;
                17
            },
            Type::Array(child, count) => {
                node.child = self.ty(child)?;
                node.count = *count;
                19
            },
            Type::Struct(fields) | Type::Tuple(fields) => {
                let mut children = Vec::new();
                reserve(&mut children, fields.len())?;
                for field in fields {
                    children.push(self.ty(field)?);
                }
                node.first = self.edges.len() as u64;
                node.count = fields.len() as u64;
                reserve(&mut self.edges, children.len())?;
                self.edges.extend(children);
                if matches!(ty, Type::Struct(_)) {
                    20
                } else {
                    21
                }
            },
            Type::FnPtr(_) => unreachable!(),
        };
        let id = self.append_type(node, TypeView::Type(ty))?;
        map_reserve(&mut self.type_ids, 1)?;
        self.type_ids.insert(ty, id);
        Ok(id)
    }

    pub(crate) fn type_model(&self, id: u64) -> Result<Type> {
        if let Some(ty) = self.cached_types.get(&id) {
            return Ok(ty.clone());
        }
        match self.type_views.get(usize::try_from(id).map_err(|_| backend(None))?) {
            Some(TypeView::Type(ty)) => Ok((*ty).clone()),
            Some(TypeView::Signature(signature)) => Ok(Type::FnPtr(Box::new((*signature).clone()))),
            None => Err(backend(None)),
        }
    }

    fn prepare_type(&mut self, id: u64) -> Result<()> {
        if let Some(TypeView::Signature(signature)) = self.type_views.get(id as usize) {
            if !self.cached_types.contains_key(&id) {
                map_reserve(&mut self.cached_types, 1)?;
                self.cached_types.insert(id, Type::FnPtr(Box::new((*signature).clone())));
            }
        }
        Ok(())
    }

    fn type_ref(&self, id: u64) -> Result<&Type> {
        if let Some(ty) = self.cached_types.get(&id) {
            return Ok(ty);
        }
        match self.type_views.get(id as usize) {
            Some(TypeView::Type(ty)) => Ok(ty),
            Some(TypeView::Signature(_)) => self.cached_types.get(&id).ok_or_else(|| backend(None)),
            None => Err(backend(None)),
        }
    }

    fn signature_arc(&mut self, id: u64) -> Result<Arc<FunctionSignature>> {
        if let Some(signature) = self.signature_arcs.get(&id) {
            return Ok(signature.clone());
        }
        let signature = match self.type_ref(id)? {
            Type::FnPtr(signature) => Arc::new((**signature).clone()),
            _ => return Err(backend(None)),
        };
        map_reserve(&mut self.signature_arcs, 1)?;
        self.signature_arcs.insert(id, signature.clone());
        Ok(signature)
    }

    fn value(&mut self, value: &Value, ty: u64, origin: bool) -> Result<RawValue> {
        let mut raw = RawValue { ty, origin: u64::from(origin), ..RawValue::default() };
        match value {
            Value::Scalar(value) => {
                raw.kind = 1;
                (raw.literal, raw.low, raw.high) = literal(value);
            },
            Value::Checked(value, overflow) => {
                raw.kind = 2;
                raw.flag = u64::from(*overflow);
                (raw.literal, raw.low, raw.high) = literal(value);
            },
            Value::Pointer(pointer) => {
                raw.kind = 3;
                raw.pointer = (*pointer).into();
            },
            Value::FunctionPointer(pointer) => {
                raw.kind = 4;
                raw.signature = if self.types.get(ty as usize).is_some_and(|node| node.kind == 18) {
                    ty
                } else {
                    self.signature_ids
                        .get(&*pointer.signature)
                        .copied()
                        .ok_or_else(|| backend(None))?
                };
                (raw.target_kind, raw.target) = match pointer.target {
                    FunctionTarget::Null => (0, 0),
                    FunctionTarget::Defined(id) => (1, u64::from(id.0)),
                    FunctionTarget::InvalidBits(bits) => (2, bits),
                };
            },
        }
        Ok(raw)
    }

    fn decode_value(&mut self, raw: RawValue) -> Result<Value> {
        self.prepare_type(raw.ty)?;
        let function_signature =
            if raw.kind == 4 { Some(self.signature_arc(raw.signature)?) } else { None };
        let ty = self.type_ref(raw.ty)?;
        match (raw.kind, ty) {
            (1, _) => Ok(Value::Scalar(decode_literal(raw.literal, raw.low, raw.high, ty)?)),
            (2, Type::Tuple(fields))
                if fields.len() == 2 && fields[1] == Type::Bool && raw.flag <= 1 =>
            {
                Ok(Value::Checked(
                    decode_literal(raw.literal, raw.low, raw.high, &fields[0])?,
                    raw.flag != 0,
                ))
            },
            (3, Type::Ptr(_)) => Ok(Value::Pointer(raw.pointer.decode()?)),
            (4, Type::FnPtr(expected)) if raw.origin <= 1 => {
                let signature = function_signature.ok_or_else(|| backend(None))?;
                if **expected != *signature {
                    return Err(backend(None));
                }
                let target = match raw.target_kind {
                    0 if raw.target == 0 => FunctionTarget::Null,
                    1 if raw.origin == 1 => {
                        let id = FunctionId(u32::try_from(raw.target).map_err(|_| backend(None))?);
                        let index = *self.declaration_ids.get(&id).ok_or_else(|| backend(None))?;
                        if self.type_ref(self.functions[index as usize].signature)?
                            != self.type_ref(raw.signature)?
                        {
                            return Err(backend(None));
                        }
                        FunctionTarget::Defined(id)
                    },
                    2 => FunctionTarget::InvalidBits(raw.target),
                    _ => return Err(backend(None)),
                };
                Ok(Value::FunctionPointer(FunctionPointer { signature, target }))
            },
            _ => Err(backend(None)),
        }
    }

    pub(crate) fn marshal(&mut self) -> Result<()> {
        let module = self.module.ok_or_else(|| backend(None))?;
        map_reserve(&mut self.declaration_ids, module.declarations.len())?;
        for (index, declaration) in module.declarations.iter().enumerate() {
            self.declaration_ids.insert(declaration.id, index as u64);
        }
        map_reserve(&mut self.global_ids, module.global_storage.len())?;
        for (index, global) in module.global_storage.iter().enumerate() {
            self.global_ids.insert(global.id, index as u64);
        }
        let mut bodies = HashMap::new();
        map_reserve(&mut bodies, module.functions.len())?;
        for function in &module.functions {
            bodies.insert(function.id, function);
        }
        for (index, declaration) in module.declarations.iter().enumerate() {
            let signature = self.signature(&declaration.signature)?;
            let mut raw = RawFunction {
                id: u64::from(declaration.id.0),
                signature,
                entry: MISSING,
                first_block: self.blocks.len() as u64,
                first_parameter: self.operands.len() as u64,
                first_value_type: self.value_types.len() as u64,
                linkage: u64::from(declaration.linkage == Linkage::External),
                ..RawFunction::default()
            };
            if let Some(function) = bodies.get(&declaration.id) {
                raw.has_body = 1;
                raw.block_count = function.blocks.len() as u64;
                raw.parameter_count = function.params.len() as u64;
                let mut slots = HashMap::new();
                map_reserve(&mut slots, function.value_types.len())?;
                let mut annotations = HashMap::new();
                map_reserve(&mut annotations, function.value_types.len())?;
                for (id, ty) in &function.value_types {
                    annotations.insert(*id, ty);
                }
                for param in &function.params {
                    let slot = slots.len() as u64;
                    slots.insert(param.id, slot);
                    let type_id = self.ty(&param.ty)?;
                    push(&mut self.value_types, type_id)?;
                    push(&mut self.value_ids, u64::from(param.id.0))?;
                    push(&mut self.operands, slot)?;
                }
                for block in &function.blocks {
                    for instruction in &block.instructions {
                        if let Some(dst) = destination(instruction) {
                            let slot = slots.len() as u64;
                            slots.insert(dst, slot);
                            let ty = annotations.get(&dst).ok_or_else(|| backend(None))?;
                            let type_id = self.ty(ty)?;
                            push(&mut self.value_types, type_id)?;
                            push(&mut self.value_ids, u64::from(dst.0))?;
                        }
                    }
                }
                raw.value_count = slots.len() as u64;
                let mut blocks = HashMap::new();
                map_reserve(&mut blocks, function.blocks.len())?;
                for (local, block) in function.blocks.iter().enumerate() {
                    blocks.insert(block.id, raw.first_block + local as u64);
                }
                raw.entry = *blocks.get(&function.entry).ok_or_else(|| backend(None))?;
                for block in &function.blocks {
                    let first = self.instructions.len() as u64;
                    for (ordinal, instruction) in block.instructions.iter().enumerate() {
                        let native = self.instruction(instruction, &slots, &blocks)?;
                        push(&mut self.instructions, native)?;
                        push(
                            &mut self.sites,
                            ExecutionSite {
                                function: function.id,
                                block: block.id,
                                instruction: ordinal,
                                value: destination(instruction),
                            },
                        )?;
                    }
                    let native = self.terminator(
                        block.terminator.as_ref().ok_or_else(|| backend(None))?,
                        &slots,
                        &blocks,
                    )?;
                    push(&mut self.instructions, native)?;
                    push(
                        &mut self.sites,
                        ExecutionSite {
                            function: function.id,
                            block: block.id,
                            instruction: block.instructions.len(),
                            value: None,
                        },
                    )?;
                    push(
                        &mut self.blocks,
                        RawBlock {
                            id: u64::from(block.id.0),
                            function: index as u64,
                            first,
                            count: self.instructions.len() as u64 - first,
                        },
                    )?;
                }
            }
            push(&mut self.functions, raw)?;
        }
        for global in &module.global_storage {
            let ty = self.ty(&global.ty)?;
            let mut native = RawGlobal {
                id: u64::from(global.id.0),
                ty,
                align: u64::from(global.align),
                readonly: u64::from(global.readonly),
                ..RawGlobal::default()
            };
            if let GlobalStorageInit::Value(value) = &global.init {
                native.initialized = 1;
                native.value = self.value(&Value::Scalar(value.clone()), ty, true)?;
            }
            push(&mut self.globals, native)?;
        }
        Ok(())
    }

    fn instruction(
        &mut self,
        instruction: &'a Instruction,
        slots: &HashMap<ValueId, u64>,
        blocks: &HashMap<BlockId, u64>,
    ) -> Result<RawInstruction> {
        let slot = |id: &ValueId| slots.get(id).copied().ok_or_else(|| backend(None));
        let block = |id: &BlockId| blocks.get(id).copied().ok_or_else(|| backend(None));
        let mut raw = RawInstruction {
            ty: self.ty(&VOID_TYPE)?,
            dst: MISSING,
            global: MISSING,
            ..RawInstruction::default()
        };
        if let Some(dst) = destination(instruction) {
            raw.dst = slot(&dst)?;
        }
        match instruction {
            Instruction::GlobalAddr { global, ty, .. } => {
                raw.op = 0;
                raw.ty = self.ty(ty)?;
                raw.global = *self.global_ids.get(global).ok_or_else(|| backend(None))?;
            },
            Instruction::LifetimeEnd { ptr } => {
                raw.op = 1;
                raw.sub = 1;
                raw.a = slot(ptr)?;
            },
            Instruction::PtrDiff { lhs, rhs, .. } => {
                raw.op = 2;
                raw.a = slot(lhs)?;
                raw.b = slot(rhs)?;
            },
            Instruction::ConstDecl { expression, value, .. } => {
                raw.op = 3;
                raw.ty = self.ty(&expression.ty)?;
                (raw.literal, raw.low, raw.high) = literal(value);
            },
            Instruction::Const { ty, value, .. } => {
                raw.op = 4;
                raw.ty = self.ty(ty)?;
                (raw.literal, raw.low, raw.high) = literal(value);
            },
            Instruction::Undef { ty, .. } => {
                raw.op = 5;
                raw.ty = self.ty(ty)?;
            },
            Instruction::Mov { ty, src, .. } => {
                raw.op = 6;
                raw.ty = self.ty(ty)?;
                raw.a = slot(src)?;
            },
            Instruction::Bin { op, ty, lhs, rhs, .. } => {
                raw.op = 7;
                raw.sub = bin_code(op);
                raw.ty = self.ty(ty)?;
                raw.a = slot(lhs)?;
                raw.b = slot(rhs)?;
            },
            Instruction::Neg { ty, src, .. } | Instruction::Not { ty, src, .. } => {
                raw.op = if matches!(instruction, Instruction::Neg { .. }) { 8 } else { 9 };
                raw.ty = self.ty(ty)?;
                raw.a = slot(src)?;
            },
            Instruction::Cmp { op, ty, lhs, rhs, .. } => {
                raw.op = 10;
                raw.sub = cmp_code(*op);
                raw.ty = self.ty(ty)?;
                raw.a = slot(lhs)?;
                raw.b = slot(rhs)?;
            },
            Instruction::ICmp { pred, ty, lhs, rhs, .. } => {
                raw.op = 11;
                raw.sub = icmp_code(pred);
                raw.ty = self.ty(ty)?;
                raw.a = slot(lhs)?;
                raw.b = slot(rhs)?;
            },
            Instruction::FCmp { pred, ty, lhs, rhs, .. } => {
                raw.op = 12;
                raw.sub = fcmp_code(pred);
                raw.ty = self.ty(ty)?;
                raw.a = slot(lhs)?;
                raw.b = slot(rhs)?;
            },
            Instruction::Select { ty, cond, on_true, on_false, .. } => {
                raw.op = 13;
                raw.ty = self.ty(ty)?;
                raw.a = slot(cond)?;
                raw.b = slot(on_true)?;
                raw.c = slot(on_false)?;
            },
            Instruction::Cast { op, dst_ty, src_ty, src, .. } => {
                raw.op = 14;
                raw.sub = cast_code(op);
                raw.ty = self.ty(dst_ty)?;
                raw.other = self.ty(src_ty)?;
                raw.a = slot(src)?;
            },
            Instruction::Phi { ty, incomings, .. } => {
                raw.op = 15;
                raw.ty = self.ty(ty)?;
                raw.first = self.operands.len() as u64;
                raw.count = incomings.len() as u64;
                for (value, predecessor) in incomings {
                    push(&mut self.operands, slot(value)?)?;
                    push(&mut self.operands, block(predecessor)?)?;
                }
            },
            Instruction::Extract { dst_ty, tuple, index, .. } => {
                raw.op = 16;
                raw.ty = self.ty(dst_ty)?;
                raw.a = slot(tuple)?;
                raw.sub = u64::from(*index);
            },
            Instruction::Checked { op, ty, lhs, rhs, .. } => {
                raw.op = 17;
                raw.sub = checked_code(op);
                raw.ty = self.ty(ty)?;
                raw.a = slot(lhs)?;
                raw.b = slot(rhs)?;
            },
            Instruction::Alloca { ty, align, .. } => {
                raw.op = 18;
                raw.ty = self.ty(ty)?;
                raw.align = u64::from(*align);
            },
            Instruction::VolatileLoad { ty, ptr, align, .. }
            | Instruction::Load { ty, ptr, align, .. } => {
                raw.op =
                    if matches!(instruction, Instruction::VolatileLoad { .. }) { 19 } else { 21 };
                raw.ty = self.ty(ty)?;
                raw.a = slot(ptr)?;
                raw.align = u64::from(*align);
            },
            Instruction::VolatileStore { ty, value, ptr, align }
            | Instruction::Store { ty, value, ptr, align } => {
                raw.op =
                    if matches!(instruction, Instruction::VolatileStore { .. }) { 20 } else { 22 };
                raw.ty = self.ty(ty)?;
                raw.a = slot(value)?;
                raw.b = slot(ptr)?;
                raw.align = u64::from(*align);
            },
            Instruction::Uninit { ty, ptr, align } => {
                raw.op = 23;
                raw.ty = self.ty(ty)?;
                raw.a = slot(ptr)?;
                raw.align = u64::from(*align);
            },
            Instruction::Gep { dst_ty, base_ptr, indices, .. } => {
                raw.op = 24;
                raw.ty = self.ty(dst_ty)?;
                raw.a = slot(base_ptr)?;
                raw.first = self.operands.len() as u64;
                raw.count = indices.len() as u64;
                for value in indices {
                    push(&mut self.operands, slot(value)?)?;
                }
            },
            Instruction::Memcpy { dst, src, n, align } => {
                raw.op = 25;
                raw.a = slot(dst)?;
                raw.b = slot(src)?;
                raw.c = slot(n)?;
                raw.align = u64::from(*align);
            },
            Instruction::Memset { dst, val, n, align } => {
                raw.op = 26;
                raw.a = slot(dst)?;
                raw.b = slot(val)?;
                raw.c = slot(n)?;
                raw.align = u64::from(*align);
            },
            Instruction::NullFunction { signature, .. } => {
                raw.op = 27;
                raw.signature = self.signature(signature)?;
                raw.ty = raw.signature;
            },
            Instruction::FunctionAddr { function, signature, .. } => {
                raw.op = 28;
                raw.signature = self.signature(signature)?;
                raw.ty = raw.signature;
                raw.a = *self.declaration_ids.get(function).ok_or_else(|| backend(None))?;
            },
            Instruction::Call { convention, ret_ty, callee, args, .. } => {
                raw.op = 29;
                raw.ty = self.ty(ret_ty)?;
                raw.sub = u64::from(*convention == CallingConvention::SysV64);
                match callee {
                    Callee::Direct(function) => {
                        raw.a = *self.declaration_ids.get(function).ok_or_else(|| backend(None))?;
                    },
                    Callee::Indirect(value) => {
                        raw.flags = 1;
                        raw.a = slot(value)?;
                    },
                }
                raw.first = self.operands.len() as u64;
                raw.count = args.len() as u64;
                for arg in args {
                    push(&mut self.operands, slot(arg)?)?;
                }
            },
            Instruction::TrapIf { cond, reason } => {
                raw.op = 30;
                raw.a = slot(cond)?;
                raw.global = push(&mut self.reasons, std::borrow::Cow::Borrowed(reason))?;
            },
        }
        Ok(raw)
    }

    fn terminator(
        &mut self,
        terminator: &'a Terminator,
        slots: &HashMap<ValueId, u64>,
        blocks: &HashMap<BlockId, u64>,
    ) -> Result<RawInstruction> {
        let slot = |id: &ValueId| slots.get(id).copied().ok_or_else(|| backend(None));
        let block = |id: &BlockId| blocks.get(id).copied().ok_or_else(|| backend(None));
        let mut raw = RawInstruction {
            ty: self.ty(&VOID_TYPE)?,
            dst: MISSING,
            global: MISSING,
            ..RawInstruction::default()
        };
        match terminator {
            Terminator::Br { target } => {
                raw.op = 31;
                raw.a = block(target)?;
            },
            Terminator::CBr { cond, then_bb, else_bb } => {
                raw.op = 32;
                raw.a = slot(cond)?;
                raw.b = block(then_bb)?;
                raw.c = block(else_bb)?;
            },
            Terminator::Switch { ty, value, default_bb, cases } => {
                raw.op = 33;
                raw.ty = self.ty(ty)?;
                raw.a = slot(value)?;
                raw.b = block(default_bb)?;
                raw.first = self.operands.len() as u64;
                raw.count = cases.len() as u64;
                for (value, target) in cases {
                    let (_, low, high) = literal(value);
                    push(&mut self.operands, low)?;
                    push(&mut self.operands, high)?;
                    push(&mut self.operands, block(target)?)?;
                }
            },
            Terminator::Ret { ty, value } => {
                raw.op = 34;
                raw.ty = self.ty(ty)?;
                raw.a = value.as_ref().map(slot).transpose()?.unwrap_or(MISSING);
            },
            Terminator::Trap { reason } => {
                raw.op = 35;
                raw.global = push(&mut self.reasons, std::borrow::Cow::Borrowed(reason))?;
            },
        }
        Ok(raw)
    }
}

fn literal(value: &ConstValue) -> (u64, u64, u64) {
    let (kind, bits) = match value {
        ConstValue::I(value) => (1, *value as u128),
        ConstValue::U(value) => (2, *value),
        ConstValue::Bool(value) => (3, u128::from(*value)),
        ConstValue::F(FloatBits::F16(value)) => (4, u128::from(*value)),
        ConstValue::F(FloatBits::F32(value)) => (5, u128::from(*value)),
        ConstValue::F(FloatBits::F64(value)) => (6, u128::from(*value)),
    };
    (kind, bits as u64, (bits >> 64) as u64)
}

fn decode_literal(kind: u64, low: u64, high: u64, ty: &Type) -> Result<ConstValue> {
    let raw = u128::from(low) | (u128::from(high) << 64);
    let signed_width = match ty {
        Type::I1 => Some(1),
        Type::I8 => Some(8),
        Type::I16 => Some(16),
        Type::I32 => Some(32),
        Type::I64 => Some(64),
        Type::I128 => Some(128),
        _ => None,
    };
    let unsigned_width = match ty {
        Type::U1 => Some(1),
        Type::U8 => Some(8),
        Type::U16 => Some(16),
        Type::U32 => Some(32),
        Type::U64 => Some(64),
        Type::U128 => Some(128),
        _ => None,
    };
    match kind {
        1 if signed_width.is_some_and(|width| {
            width == 128 || ((raw as i128) << (128 - width) >> (128 - width)) == raw as i128
        }) =>
        {
            Ok(ConstValue::I(raw as i128))
        },
        2 if unsigned_width.is_some_and(|width| width == 128 || raw >> width == 0) => {
            Ok(ConstValue::U(raw))
        },
        3 if *ty == Type::Bool && raw <= 1 => Ok(ConstValue::Bool(raw != 0)),
        4 if *ty == Type::F16 && high == 0 && low <= u64::from(u16::MAX) => {
            Ok(ConstValue::F(FloatBits::F16(low as u16)))
        },
        5 if *ty == Type::F32 && high == 0 && low <= u64::from(u32::MAX) => {
            Ok(ConstValue::F(FloatBits::F32(low as u32)))
        },
        6 if *ty == Type::F64 && high == 0 => Ok(ConstValue::F(FloatBits::F64(low))),
        _ => Err(backend(None)),
    }
}

fn bin_code(op: &BinOp) -> u64 {
    match op {
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

fn cast_code(op: &CastOp) -> u64 {
    match op {
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
    }
}

fn cmp_code(op: CmpOp) -> u64 {
    match op {
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

fn icmp_code(pred: &ICmpPred) -> u64 {
    match pred {
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

fn fcmp_code(pred: &FCmpPred) -> u64 {
    match pred {
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

fn checked_code(op: &CheckedOp) -> u64 {
    match op {
        CheckedOp::UAdd => 0,
        CheckedOp::USub => 1,
        CheckedOp::UMul => 2,
        CheckedOp::SAdd => 3,
        CheckedOp::SSub => 4,
        CheckedOp::SMul => 5,
    }
}

impl Program<'_> {
    pub(crate) fn callable_value(&self, function: FunctionId) -> Result<RuntimeValue<'static>> {
        let body =
            self.function_record(function).ok_or(InterpreterError::UnknownFunction(function))?;
        let Type::FnPtr(signature) = self.type_model(body.signature)? else {
            return Err(backend(None));
        };
        Ok(RuntimeValue {
            ty: Type::FnPtr(signature.clone()),
            value: Value::FunctionPointer(FunctionPointer {
                signature: Arc::new(*signature),
                target: FunctionTarget::Defined(function),
            }),
            callable_module: None,
        })
    }

    fn append_argument_type(&mut self, ty: &Type) -> Result<u64> {
        let mut node = RawType::default();
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
                node.child = self.append_argument_type(child)?;
                17
            },
            Type::Array(child, count) => {
                node.child = self.append_argument_type(child)?;
                node.count = *count;
                19
            },
            Type::FnPtr(signature) => {
                let mut children = Vec::new();
                push(&mut children, self.append_argument_type(&signature.ret)?)?;
                for param in &signature.params {
                    push(&mut children, self.append_argument_type(param)?)?;
                }
                node.first = self.edges.len() as u64;
                node.count = children.len() as u64;
                node.extra = u64::from(signature.convention == CallingConvention::SysV64)
                    | (u64::from(signature.variadic) << 1);
                reserve(&mut self.edges, children.len())?;
                self.edges.extend(children);
                18
            },
            Type::Struct(fields) | Type::Tuple(fields) => {
                let mut children = Vec::new();
                for field in fields {
                    push(&mut children, self.append_argument_type(field)?)?;
                }
                node.first = self.edges.len() as u64;
                node.count = children.len() as u64;
                reserve(&mut self.edges, children.len())?;
                self.edges.extend(children);
                if matches!(ty, Type::Struct(_)) {
                    20
                } else {
                    21
                }
            },
        };
        push(&mut self.types, node)
    }

    pub(crate) fn prepare_argument_types(
        &mut self,
        arguments: &[(&RuntimeValue<'_>, bool)],
    ) -> Result<Vec<u64>> {
        let mut ids = Vec::new();
        reserve(&mut ids, arguments.len())?;
        for (value, _) in arguments {
            ids.push(self.append_argument_type(value.ty())?);
        }
        Ok(ids)
    }

    pub(crate) fn argument_checkpoint(&self) -> ArgumentCheckpoint {
        ArgumentCheckpoint {
            types: self.types.len(),
            edges: self.edges.len(),
            type_capacity: self.types.capacity(),
            edge_capacity: self.edges.capacity(),
            cache_capacity: self.cached_types.capacity(),
            signature_capacity: self.signature_arcs.capacity(),
        }
    }

    pub(crate) fn argument_spare_bytes(&self) -> u64 {
        ((self.types.capacity() - self.types.len()) as u64) * 40
            + ((self.edges.capacity() - self.edges.len()) as u64) * 8
    }

    pub(crate) fn rollback_argument_types(&mut self, checkpoint: ArgumentCheckpoint) {
        self.types.truncate(checkpoint.types);
        self.edges.truncate(checkpoint.edges);
        self.cached_types.retain(|id, _| *id < checkpoint.types as u64);
        self.signature_arcs.retain(|id, _| *id < checkpoint.types as u64);
        self.types.shrink_to(checkpoint.type_capacity);
        self.edges.shrink_to(checkpoint.edge_capacity);
        self.cached_types.shrink_to(checkpoint.cache_capacity);
        self.signature_arcs.shrink_to(checkpoint.signature_capacity);
    }

    pub(crate) fn function_record(&self, id: FunctionId) -> Option<&RawFunction> {
        let index = self.declaration_ids.get(&id)?;
        self.functions.get(*index as usize)
    }

    pub(crate) fn typed_arguments(
        &mut self,
        arguments: &[(&RuntimeValue<'_>, bool)],
        ids: &[u64],
    ) -> Result<Vec<RawValue>> {
        if arguments.len() != ids.len() {
            return Err(backend(None));
        }
        let mut input = Vec::new();
        reserve(&mut input, arguments.len())?;
        for ((value, origin), ty) in arguments.iter().zip(ids) {
            input.push(self.value(&value.value, *ty, *origin)?);
        }
        Ok(input)
    }

    /// Materialize verified structural type records for the typed memory adapter.
    /// The loader has already bounded depth and expanded metadata size.
    pub(crate) fn materialize_types(&mut self) -> Result<()> {
        let missing = self.types.len().saturating_sub(self.cached_types.len());
        map_reserve(&mut self.cached_types, missing)?;
        if self.declaration_ids.is_empty() {
            map_reserve(&mut self.declaration_ids, self.functions.len())?;
            for (index, function) in self.functions.iter().enumerate() {
                self.declaration_ids.insert(FunctionId(function.id as u32), index as u64);
            }
        }
        for (index, raw) in self.types.iter().enumerate() {
            if self.cached_types.contains_key(&(index as u64)) {
                continue;
            }
            let child = |id: u64| self.cached_types.get(&id).cloned().ok_or_else(|| backend(None));
            let fields = || -> Result<Vec<Type>> {
                let first = usize::try_from(raw.first).map_err(|_| backend(None))?;
                let count = usize::try_from(raw.count).map_err(|_| backend(None))?;
                let end = first.checked_add(count).ok_or_else(|| backend(None))?;
                let mut fields = Vec::new();
                reserve(&mut fields, count)?;
                for id in self.edges.get(first..end).ok_or_else(|| backend(None))? {
                    fields.push(child(*id)?);
                }
                Ok(fields)
            };
            let ty = match raw.kind {
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
                17 => Type::Ptr(Box::new(child(raw.child)?)),
                18 => {
                    let mut parameters = fields()?;
                    if parameters.is_empty() {
                        return Err(backend(None));
                    }
                    let ret = parameters.remove(0);
                    Type::FnPtr(Box::new(FunctionSignature {
                        params: parameters,
                        ret,
                        convention: if raw.extra & 1 == 0 {
                            CallingConvention::Whale
                        } else {
                            CallingConvention::SysV64
                        },
                        variadic: raw.extra & 2 != 0,
                    }))
                },
                19 => Type::Array(Box::new(child(raw.child)?), raw.count),
                20 => Type::Struct(fields()?),
                21 => Type::Tuple(fields()?),
                _ => return Err(backend(None)),
            };
            self.cached_types.insert(index as u64, ty);
        }
        Ok(())
    }

    fn execution_source(
        &self,
        site: ExecutionSite,
        budget: &mut SourceDetailBudget,
    ) -> Option<OriginalLocation> {
        if let Some(module) = self.module {
            return execution_source(module, site, budget);
        }
        let block = self.blocks.iter().find(|block| {
            block.id == u64::from(site.block.0)
                && self
                    .functions
                    .get(block.function as usize)
                    .is_some_and(|f| f.id == u64::from(site.function.0))
        })?;
        let target = if site.instruction as u64 + 1 == block.count {
            SourceTarget::Terminator { function: site.function, block: site.block }
        } else {
            SourceTarget::Instruction {
                function: site.function,
                block: site.block,
                index: site.instruction,
            }
        };
        budget.location_metadata(&self.sources, target)
    }

    pub(crate) fn site(&self, index: u64, entry: FunctionId) -> Result<ExecutionSite> {
        if index != MISSING {
            return self
                .sites
                .get(usize::try_from(index).map_err(|_| backend(None))?)
                .copied()
                .ok_or_else(|| backend(None));
        }
        let function = self
            .functions
            .iter()
            .find(|body| body.id == u64::from(entry.0))
            .ok_or_else(|| backend(None))?;
        let block = self.blocks.get(function.entry as usize).ok_or_else(|| backend(None))?;
        Ok(ExecutionSite {
            function: entry,
            block: BlockId(block.id as u32),
            instruction: 0,
            value: None,
        })
    }

    fn error(
        &self,
        raw: &RawResult,
        entry: FunctionId,
        options: InterpreterOptions,
    ) -> Result<InterpreterError> {
        let count = |value| usize::try_from(value).map_err(|_| backend(None));
        let function = |index: u64| {
            self.functions
                .get(count(index)?)
                .map(|decl| FunctionId(decl.id as u32))
                .ok_or_else(|| backend(None))
        };
        let at = || self.site(raw.site, entry);
        let trap = |reason| {
            Ok(InterpreterError::Trap(InterpreterTrap { site: at()?, reason, steps: raw.steps }))
        };
        Ok(match raw.error {
            1 => InterpreterError::UnknownFunction(FunctionId(
                u32::try_from(raw.detail).map_err(|_| backend(None))?,
            )),
            2 => InterpreterError::UnsupportedSignature(function(raw.detail)?),
            3 => InterpreterError::ArgumentCount {
                expected: count(raw.detail)?,
                got: count(raw.extra)?,
            },
            4 => InterpreterError::ArgumentType {
                index: count(raw.detail)?,
                expected: self.type_model(raw.extra)?,
            },
            5 => InterpreterError::UnsupportedInstruction {
                site: at()?,
                operation: match raw.detail {
                    0 => "undef",
                    1 => "non-scalar constant",
                    2 => "non-scalar value",
                    3 => "non-integer arithmetic/comparison",
                    4 => "unsupported cast",
                    5 => "alloca",
                    6 => "load",
                    7 => "store",
                    8 => "uninit",
                    9 => "call",
                    10 => "zero-sized pointer arithmetic",
                    _ => return Err(backend(None)),
                },
            },
            6 if raw.detail == options.max_steps => {
                InterpreterError::StepLimit { site: at()?, limit: raw.detail }
            },
            7..=9 => {
                let (resource, expected) = match raw.error {
                    7 => (CallResource::Depth, options.max_call_depth as u64),
                    8 => (CallResource::Frames, options.max_frames),
                    _ => (CallResource::Values, options.max_live_values as u64),
                };
                if raw.detail != expected {
                    return Err(backend(None));
                }
                InterpreterError::CallLimit { site: at()?, resource, limit: raw.detail }
            },
            10 => return trap(TrapReason::DivisionByZero),
            11 => return trap(TrapReason::RemainderByZero),
            12 => return trap(TrapReason::InvalidConversion),
            13 => {
                let reason = self.reasons.get(count(raw.detail)?).ok_or_else(|| backend(None))?;
                return trap(TrapReason::Explicit(reason.to_string()));
            },
            14 => return trap(TrapReason::NullFunction),
            15 => return trap(TrapReason::InvalidFunctionPointer),
            16 => return trap(TrapReason::FunctionSignatureMismatch),
            17 => InterpreterError::UnsupportedGlobal(GlobalId(
                self.globals.get(count(raw.detail)?).ok_or_else(|| backend(None))?.id as u32,
            )),
            18 => {
                InterpreterError::Verification(Box::new(VerifyError::ResourceLimit(LimitError {
                    resource: Resource::AnalysisWork,
                    limit: options.ir_limits.max_analysis_work,
                })))
            },
            _ => return Err(backend(None)),
        })
    }

    fn failure(
        &self,
        cause: InterpreterError,
        entry: FunctionId,
        frames: &[RawFrame],
        trace_limit: usize,
    ) -> InterpreterError {
        let mut budget = SourceDetailBudget::new();
        let global = match &cause {
            InterpreterError::GlobalMemory { global, .. }
            | InterpreterError::GlobalMemoryLimit { global, .. }
            | InterpreterError::UnsupportedGlobal(global) => Some(*global),
            _ => None,
        };
        if let Some(global) = global {
            let metadata = self.module.map_or(&self.sources, |module| &module.sources);
            if let Some(original) = budget.location_metadata(metadata, SourceTarget::Global(global))
            {
                return InterpreterError::Diagnostic {
                    cause: Box::new(cause),
                    diagnostic: Box::new(RuntimeDiagnostic {
                        site: None,
                        global: Some(global),
                        original: Some(original),
                        calls: Vec::new(),
                        omitted_call_frames: 0,
                        omitted_source_details: budget.omitted,
                    }),
                };
            }
        }
        let Some(site) = cause.runtime_site() else {
            return cause;
        };
        let original = self.execution_source(site, &mut budget);
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
            let Some(callee) =
                self.functions.get(child.function as usize).map(|decl| FunctionId(decl.id as u32))
            else {
                return backend(Some(site));
            };
            let Ok(call_site) = self.site(child.call_site, entry) else {
                return backend(Some(site));
            };
            if child.call_site == MISSING {
                return backend(Some(site));
            }
            calls.push(RuntimeCallFrame {
                caller: call_site.function,
                callee,
                site: call_site,
                original: self.execution_source(call_site, &mut budget),
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

    fn memory_event(
        &mut self,
        event: &RawEvent,
        plans: &[RawGep],
        memory: &mut memory::Memory,
    ) -> std::result::Result<RawValue, memory::Fault> {
        fn transport<T>(_: T) -> memory::Fault {
            memory::Fault::Backend(BACKEND)
        }
        let pointer = |value: RawValue| {
            if value.kind != 3 {
                return Err(memory::Fault::Backend(BACKEND));
            }
            value.pointer.decode().map_err(transport)
        };
        let align = u32::try_from(event.align).map_err(transport)?;
        let mut value = RawValue { ty: event.ty, ..RawValue::default() };
        match event.kind {
            1 => {
                let natural = u32::try_from(event.natural).map_err(transport)?;
                let result = match event.flag {
                    0 => memory.allocate(event.size, natural, align)?,
                    1 => memory.allocate_global(event.size, natural, align)?,
                    _ => return Err(memory::Fault::Backend(BACKEND)),
                };
                value.kind = 3;
                value.pointer = result.into();
            },
            2 => {
                self.prepare_type(event.ty).map_err(transport)?;
                let signature =
                    if matches!(self.type_ref(event.ty).map_err(transport)?, Type::FnPtr(_)) {
                        Some(self.signature_arc(event.ty).map_err(transport)?)
                    } else {
                        None
                    };
                let loaded = memory.load_with_signature(
                    pointer(event.left)?,
                    self.type_ref(event.ty).map_err(transport)?,
                    align,
                    signature.as_ref(),
                )?;
                value = self.value(&loaded, event.ty, true).map_err(transport)?;
            },
            3 => {
                self.prepare_type(event.ty).map_err(transport)?;
                let stored = self.decode_value(event.right).map_err(transport)?;
                memory.store(
                    pointer(event.left)?,
                    self.type_ref(event.ty).map_err(transport)?,
                    &stored,
                    align,
                )?;
            },
            4 => memory.uninit(pointer(event.left)?, event.size, align)?,
            5 => memory.copy(pointer(event.left)?, pointer(event.right)?, event.size, align)?,
            6 => memory.set(
                pointer(event.left)?,
                u8::try_from(event.flag).map_err(transport)?,
                event.size,
                align,
            )?,
            7 => {
                let first = usize::try_from(event.first).map_err(transport)?;
                let end = first
                    .checked_add(usize::try_from(event.count).map_err(transport)?)
                    .ok_or(memory::Fault::Backend(BACKEND))?;
                let plans = plans.get(first..end).ok_or(memory::Fault::Backend(BACKEND))?;
                let mut steps = Vec::new();
                let mut indices = Vec::new();
                steps.try_reserve_exact(plans.len()).map_err(|_| memory::Fault::Limit {
                    resource: MemoryResource::HostAllocation,
                    limit: plans.len() as u64,
                })?;
                indices.try_reserve_exact(plans.len()).map_err(|_| memory::Fault::Limit {
                    resource: MemoryResource::HostAllocation,
                    limit: plans.len() as u64,
                })?;
                for plan in plans {
                    steps.push(match plan.kind {
                        0 => memory::GepStep::Stride(plan.amount),
                        1 => memory::GepStep::Field(plan.amount),
                        _ => return Err(memory::Fault::Backend(BACKEND)),
                    });
                    let bits = u128::from(plan.low) | (u128::from(plan.high) << 64);
                    indices.push(match plan.signed {
                        0 => ConstValue::U(bits),
                        1 => ConstValue::I(bits as i128),
                        _ => return Err(memory::Fault::Backend(BACKEND)),
                    });
                }
                value.kind = 3;
                value.pointer = memory.gep(pointer(event.left)?, &steps, &indices)?.into();
            },
            8 => memory.retire_stack(
                pointer(event.left)?,
                usize::try_from(event.owner).map_err(transport)?,
            )?,
            9 => {
                value.kind = 1;
                (value.literal, value.low, value.high) = literal(&ConstValue::I(
                    memory.difference(pointer(event.left)?, pointer(event.right)?)?,
                ));
            },
            10 => {
                value.kind = 1;
                (value.literal, value.low, value.high) = literal(&ConstValue::Bool(
                    memory.equal(pointer(event.left)?, pointer(event.right)?)?,
                ));
            },
            11 => memory.retire_from(usize::try_from(event.owner).map_err(transport)?)?,
            12 if event.flag <= 1 => {
                value.kind = 3;
                value.pointer = memory.seal_global(pointer(event.left)?, event.flag != 0)?.into();
            },
            _ => return Err(memory::Fault::Backend(BACKEND)),
        }
        Ok(value)
    }
}

fn storage() -> Storage {
    let target = Target::X86_64WhaleLinux.specification();
    let storage = &target.storage;
    let (large_array_threshold, large_array_align, has_large_array) = storage
        .large_array
        .map(|(threshold, align)| (threshold, u64::from(align), 1))
        .unwrap_or((0, 0, 0));
    Storage {
        ptr_bits: u64::from(storage.data.ptr_bits),
        pointer_align: u64::from(storage.pointer_align),
        scalar_align1: u64::from(storage.scalar_align[0]),
        scalar_align2: u64::from(storage.scalar_align[1]),
        scalar_align4: u64::from(storage.scalar_align[2]),
        scalar_align8: u64::from(storage.scalar_align[3]),
        scalar_align16: u64::from(storage.scalar_align[4]),
        large_array_threshold,
        large_array_align,
        has_large_array,
    }
}

fn grow<T: Default>(buffer: &mut Vec<T>, requested: u64, maximum: usize) -> Result<bool> {
    let requested = usize::try_from(requested).map_err(|_| backend(None))?;
    if requested > maximum {
        return Err(backend(None));
    }
    if requested <= buffer.len() {
        return Ok(false);
    }
    reserve(buffer, requested - buffer.len())?;
    buffer.resize_with(requested, T::default);
    Ok(true)
}

pub(crate) fn scalar_arguments(
    program: &Program<'_>,
    function: FunctionId,
    arguments: &[ConstValue],
) -> Result<Vec<RawValue>> {
    let signature = program
        .function_record(function)
        .and_then(|body| program.types.get(body.signature as usize));
    let mut input = Vec::new();
    reserve(&mut input, arguments.len())?;
    for (index, argument) in arguments.iter().enumerate() {
        let (literal, low, high) = literal(argument);
        let ty = signature
            .and_then(|signature| {
                (index as u64 + 1 < signature.count)
                    .then(|| program.edges.get(signature.first as usize + index + 1))
                    .flatten()
                    .copied()
            })
            .unwrap_or(MISSING);
        input.push(RawValue { kind: 1, ty, literal, low, high, origin: 1, ..RawValue::default() });
    }
    Ok(input)
}

pub(super) fn run<'module>(
    module: &'module Module,
    function: FunctionId,
    arguments: Arguments<'_, '_>,
    options: InterpreterOptions,
) -> Result<RuntimeInterpreterResult<'module>> {
    verify_module_with_limits(module, options.ir_limits)
        .map_err(|error| InterpreterError::Verification(Box::new(error)))?;
    let mut program = Program::new(Some(module))?;
    program.marshal()?;
    let mut input = Vec::new();
    reserve(&mut input, arguments.len())?;
    match &arguments {
        Arguments::Scalars(values) => {
            for value in *values {
                input.push(program.value(&Value::Scalar(value.clone()), MISSING, true)?);
            }
        },
        Arguments::Runtime(values) => {
            for value in *values {
                let ty = program.ty(&value.ty)?;
                input.push(program.value(
                    &value.value,
                    ty,
                    arguments.valid_origin(input.len(), module),
                )?);
            }
        },
    }
    let result = execute(
        &mut program,
        function,
        input,
        matches!(arguments, Arguments::Scalars(_)),
        options,
        None,
    )?;
    Ok(RuntimeInterpreterResult {
        value: result.value.map(|value| RuntimeValue {
            callable_module: value.function_target().map(|_| module),
            ty: value.ty,
            value: value.value,
        }),
        steps: result.steps,
    })
}

pub(crate) fn execute<'module>(
    program: &mut Program<'module>,
    function: FunctionId,
    mut input: Vec<RawValue>,
    scalar_arguments: bool,
    options: InterpreterOptions,
    admission: Option<RuntimeAdmission>,
) -> Result<RuntimeInterpreterResult<'module>> {
    let type_count = program.types.len();
    let function_count = program.functions.len();
    let instruction_count = program.instructions.len();
    let global_count = program.globals.len();
    let table_capacity = function_count
        .max(1)
        .checked_mul(2)
        .and_then(usize::checked_next_power_of_two)
        .ok_or_else(|| backend(None))?;
    let mut layout_nodes = initialized::<LayoutNode>(type_count)?;
    let mut layouts = initialized::<Layout>(type_count)?;
    let mut offsets = initialized::<u64>(program.edges.len())?;
    let mut plans = initialized::<RawGep>(program.operands.len())?;
    let mut ordinals = initialized::<u64>(program.value_types.len())?;
    let mut table = initialized::<u64>(table_capacity)?;
    let mut prepared = initialized::<u8>(function_count)?;
    let mut pending_previous = initialized::<u64>(function_count)?;
    let mut pending_next = initialized::<u64>(function_count)?;
    let mut pending_flags = initialized::<u8>(function_count)?;
    let mut callable_flags = initialized::<u8>(function_count)?;
    let mut callable_targets = initialized::<u64>(function_count)?;
    let mut indirect_signatures = initialized::<u64>(instruction_count)?;
    let mut indirect_sites = initialized::<u64>(instruction_count)?;
    let mut global_values = initialized::<RawValue>(global_count)?;
    let mut frames = Vec::<RawFrame>::new();
    let mut slots = Vec::<RawSlot>::new();
    let mut phis = Vec::<RawValue>::new();
    let mut storage = storage();
    let mut result = RawResult::default();
    let mut driver = RawDriver {
        types: program.types.as_mut_ptr(),
        type_count: type_count as u64,
        type_edges: program.edges.as_mut_ptr(),
        type_edge_count: program.edges.len() as u64,
        functions: program.functions.as_mut_ptr(),
        function_count: function_count as u64,
        blocks: program.blocks.as_mut_ptr(),
        block_count: program.blocks.len() as u64,
        instructions: program.instructions.as_mut_ptr(),
        instruction_count: instruction_count as u64,
        operands: program.operands.as_mut_ptr(),
        operand_count: program.operands.len() as u64,
        value_types: program.value_types.as_mut_ptr(),
        value_type_count: program.value_types.len() as u64,
        globals: program.globals.as_mut_ptr(),
        global_count: global_count as u64,
        global_values: global_values.as_mut_ptr(),
        arguments: input.as_mut_ptr(),
        argument_count: input.len() as u64,
        scalar_arguments: u64::from(scalar_arguments),
        entry_id: u64::from(function.0),
        storage: &mut storage,
        layout_nodes: layout_nodes.as_mut_ptr(),
        layouts: layouts.as_mut_ptr(),
        layout_offsets: offsets.as_mut_ptr(),
        plans: plans.as_mut_ptr(),
        plan_capacity: plans.len() as u64,
        ordinals: ordinals.as_mut_ptr(),
        function_table: table.as_mut_ptr(),
        function_table_capacity: table.len() as u64,
        prepared: prepared.as_mut_ptr(),
        pending_previous: pending_previous.as_mut_ptr(),
        pending_next: pending_next.as_mut_ptr(),
        pending_flags: pending_flags.as_mut_ptr(),
        callable_flags: callable_flags.as_mut_ptr(),
        callable_targets: callable_targets.as_mut_ptr(),
        indirect_signatures: indirect_signatures.as_mut_ptr(),
        indirect_sites: indirect_sites.as_mut_ptr(),
        indirect_capacity: indirect_signatures.len() as u64,
        max_steps: options.max_steps,
        max_depth: options.max_call_depth as u64,
        max_frames: options.max_frames,
        max_values: options.max_live_values as u64,
        max_work: options.ir_limits.max_analysis_work as u64,
        result: &mut result,
        ..RawDriver::default()
    };
    let mut memory = memory::Memory::new(options.memory_limits);
    let mut previous_steps = 0;
    let mut previous_work = 0;
    loop {
        driver.memory_allocations = memory.allocation_count() as u64;
        // All spans are initialized, stable for this step, and exclusively borrowed.
        let status = unsafe {
            match &admission {
                Some(admission) => whale_ir_bytecode_runtime_step(
                    admission.program,
                    &mut driver,
                    admission.scratch,
                    admission.capacity,
                    &admission.limits,
                    admission.result,
                ),
                None => whale_ir_runtime_step(&mut driver),
            }
        };
        if status != result.status as i32
            || result.status > 4
            || driver.steps < previous_steps
            || driver.steps > options.max_steps
            || driver.work < previous_work
            || driver.work > options.ir_limits.max_analysis_work as u64
            || result.steps != driver.steps
            || driver.frame_count > frames.len() as u64
            || driver.slot_count > slots.len() as u64
            || driver.plan_count > plans.len() as u64
        {
            return Err(backend(program.site(result.site, function).ok()));
        }
        previous_steps = driver.steps;
        previous_work = driver.work;
        let active_frames = &frames[..driver.frame_count as usize];
        let fail =
            |cause| program.failure(cause, function, active_frames, options.max_trace_frames);
        match result.status {
            0 => {
                let value = if result.value.kind == 0 {
                    None
                } else {
                    let ty = program.type_model(result.value.ty)?;
                    let value = program.decode_value(result.value)?;
                    let callable_module = matches!(
                        value,
                        Value::FunctionPointer(FunctionPointer {
                            target: FunctionTarget::Defined(_),
                            ..
                        })
                    )
                    .then_some(program.module)
                    .flatten();
                    Some(RuntimeValue { ty, value, callable_module })
                };
                return Ok(RuntimeInterpreterResult { value, steps: result.steps });
            },
            1 => match program.memory_event(&result.event, &plans, &mut memory) {
                Ok(value) => {
                    driver.resume_value = value;
                    driver.resume = 1;
                },
                Err(fault) => {
                    let global = if result.global == MISSING {
                        None
                    } else {
                        Some(GlobalId(
                            program
                                .globals
                                .get(result.global as usize)
                                .ok_or_else(|| backend(None))?
                                .id as u32,
                        ))
                    };
                    let site = program.site(result.site, function).ok();
                    let cause = match (global, fault) {
                        (_, memory::Fault::Backend(reason)) => {
                            InterpreterError::Backend { site, reason }
                        },
                        (Some(global), memory::Fault::Trap(reason)) => {
                            InterpreterError::GlobalMemory { global, reason }
                        },
                        (Some(global), memory::Fault::Limit { resource, limit }) => {
                            InterpreterError::GlobalMemoryLimit { global, resource, limit }
                        },
                        (None, memory::Fault::Trap(reason)) => {
                            InterpreterError::Trap(InterpreterTrap {
                                site: site.ok_or_else(|| backend(None))?,
                                reason: TrapReason::Memory(reason),
                                steps: result.steps,
                            })
                        },
                        (None, memory::Fault::Limit { resource, limit }) => {
                            InterpreterError::MemoryLimit {
                                site: site.ok_or_else(|| backend(None))?,
                                resource,
                                limit,
                            }
                        },
                    };
                    return Err(program.failure(
                        cause,
                        function,
                        active_frames,
                        options.max_trace_frames,
                    ));
                },
            },
            2 => {
                let event = &result.event;
                let resized = (|| -> Result<bool> {
                    Ok(grow(&mut frames, event.frame_capacity, options.max_call_depth)?
                        | grow(&mut slots, event.value_capacity, options.max_live_values)?
                        | grow(&mut phis, event.phi_capacity, options.max_live_values)?)
                })();
                let changed = resized.map_err(|cause| {
                    let cause =
                        if matches!(cause, InterpreterError::Backend { reason: ALLOCATION, .. }) {
                            match program.site(result.site, function) {
                                Ok(site) => InterpreterError::CallLimit {
                                    site,
                                    resource: CallResource::HostAllocation,
                                    limit: event
                                        .frame_capacity
                                        .max(event.value_capacity)
                                        .max(event.phi_capacity),
                                },
                                Err(_) => cause,
                            }
                        } else {
                            cause
                        };
                    program.failure(
                        cause,
                        function,
                        &frames[..driver.frame_count as usize],
                        options.max_trace_frames,
                    )
                })?;
                if !changed {
                    return Err(program.failure(
                        backend(program.site(result.site, function).ok()),
                        function,
                        &frames[..driver.frame_count as usize],
                        options.max_trace_frames,
                    ));
                }
                driver.frames = frames.as_mut_ptr();
                driver.frame_capacity = frames.len() as u64;
                driver.slots = slots.as_mut_ptr();
                driver.slot_capacity = slots.len() as u64;
                driver.phis = phis.as_mut_ptr();
                driver.phi_capacity = phis.len() as u64;
                driver.resume = 2;
            },
            3 => return Err(fail(program.error(&result, function, options)?)),
            4 => return Err(fail(backend(program.site(result.site, function).ok()))),
            _ => return Err(backend(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_records_match_native_field_widths() {
        use std::mem::{align_of, offset_of, size_of};
        assert_eq!(size_of::<RawType>(), 40);
        assert_eq!(size_of::<RawPointer>(), 80);
        assert_eq!(size_of::<RawValue>(), 160);
        assert_eq!(size_of::<RawSlot>(), 168);
        assert_eq!(size_of::<RawFrame>(), 80);
        assert_eq!(size_of::<RawFunction>(), 88);
        assert_eq!(size_of::<RawBlock>(), 32);
        assert_eq!(size_of::<RawInstruction>(), 168);
        assert_eq!(size_of::<RawGlobal>(), 216);
        assert_eq!(align_of::<RawDriver>(), 8);
        assert_eq!(offset_of!(RawValue, pointer), 48);
        assert_eq!(offset_of!(RawValue, origin), 152);
        assert_eq!(offset_of!(RawSlot, value), 8);
        assert_eq!(offset_of!(RawGlobal, value), 56);
    }

    #[test]
    fn pointer_transport_preserves_every_authority_field_and_rejects_flags() {
        let pointer = memory::Pointer {
            address: 0x1008,
            capability: Some(memory::Capability {
                arena: 7,
                allocation: 13,
                generation: 19,
                lower: 3,
                upper: 29,
                offset: 8,
                read: true,
                write: false,
            }),
        };
        let raw = RawPointer::from(pointer);
        assert_eq!(raw.decode().unwrap(), pointer);
        for field in 0..3 {
            let mut invalid = raw;
            match field {
                0 => invalid.present = 2,
                1 => invalid.read = 2,
                _ => invalid.write = 2,
            }
            assert!(matches!(invalid.decode(), Err(InterpreterError::Backend { .. })));
        }
        assert_eq!(
            RawPointer::from(memory::Pointer::raw(u64::MAX)).decode().unwrap(),
            memory::Pointer::raw(u64::MAX)
        );
    }

    #[test]
    fn literal_transport_retains_payloads_and_rejects_truncated_results() {
        for (ty, value) in [
            (Type::I1, ConstValue::I(-1)),
            (Type::I128, ConstValue::I(i128::MIN)),
            (Type::U128, ConstValue::U(u128::MAX)),
            (Type::F16, ConstValue::F(FloatBits::F16(0xfd55))),
            (Type::F32, ConstValue::F(FloatBits::F32(0x7f80_0123))),
            (Type::F64, ConstValue::F(FloatBits::F64(0xfff0_0123_4567_89ab))),
        ] {
            let (kind, low, high) = literal(&value);
            assert_eq!(decode_literal(kind, low, high, &ty).unwrap(), value);
        }
        for (kind, low, high, ty) in [
            (1, 128, 0, Type::I8),
            (1, 1, 0, Type::I1),
            (2, 256, 0, Type::U8),
            (2, 0, 1, Type::U64),
            (3, 2, 0, Type::Bool),
            (4, 65536, 0, Type::F16),
            (5, 1, 1, Type::F32),
            (6, 0, 1, Type::F64),
        ] {
            assert!(matches!(
                decode_literal(kind, low, high, &ty),
                Err(InterpreterError::Backend { .. })
            ));
        }
    }

    #[test]
    fn native_driver_rejects_invalid_transport_without_fabricating_a_value() {
        assert_eq!(unsafe { whale_ir_runtime_step(std::ptr::null_mut()) }, 4);
        let mut result = RawResult::default();
        let mut driver = RawDriver { result: &mut result, ..RawDriver::default() };
        driver.type_count = u64::MAX;
        assert_eq!(unsafe { whale_ir_runtime_step(&mut driver) }, 4);
        assert_eq!(result.status, 4);
        assert_eq!(result.value.kind, 0);
        driver.type_count = 1;
        assert_eq!(unsafe { whale_ir_runtime_step(&mut driver) }, 4);
        assert_eq!(result.status, 4);
        assert_eq!(result.value.kind, 0);
    }

    #[test]
    fn marshaler_retains_sparse_ids_and_uses_dense_local_slots() {
        let module = parse_module(
            r#"module {
            format_version 7 semantics_version 1 target "x86_64-whale-linux"
            datalayout { ptr=64, endian=little }
            declare @f4294967295 "sparse": whale (i64) -> i64, linkage internal
            fn @f4294967295 "sparse"(%v2147483647 "argument": i64) -> i64, entry %b4294967295 {
                %b4294967295 "entry":
                    %v4294967295: i64 = mov i64 %v2147483647
                    ret i64 %v4294967295
            }
        }"#,
        )
        .unwrap();
        verify_module(&module).unwrap();
        let mut program = Program::new(Some(&module)).unwrap();
        program.marshal().unwrap();
        assert_eq!(program.functions[0].id, u64::from(u32::MAX));
        assert_eq!(program.functions[0].value_count, 2);
        assert_eq!(program.functions[0].entry, 0);
        assert_eq!(program.blocks[0].id, u64::from(u32::MAX));
        assert_eq!(program.instructions[0].a, 0);
        assert_eq!(program.instructions[0].dst, 1);
        assert_eq!(program.instructions[1].a, 1);
        assert_eq!(program.sites[0].value, Some(ValueId(u32::MAX)));
        assert_eq!(program.sites[1].instruction, 1);
    }
}
