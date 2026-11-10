// SPDX-License-Identifier: MPL-2.0
//! Allocation and typed transport for Wave-owned tracked memory.
use super::*;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct RawPointer {
    address: u64,
    present: u64,
    arena: u64,
    allocation: u64,
    generation: u64,
    lower: u64,
    upper: u64,
    offset: u64,
    read: u64,
    write: u64,
}

impl From<Pointer> for RawPointer {
    fn from(p: Pointer) -> Self {
        match p.capability {
            None => Self { address: p.address, ..Self::default() },
            Some(c) => Self {
                address: p.address,
                present: 1,
                arena: c.arena,
                allocation: c.allocation as u64,
                generation: c.generation,
                lower: c.lower,
                upper: c.upper,
                offset: c.offset,
                read: u64::from(c.read),
                write: u64::from(c.write),
            },
        }
    }
}

impl RawPointer {
    fn decode(self) -> Result<Pointer> {
        if self.present > 1 || self.read > 1 || self.write > 1 {
            return Err(backend());
        }
        Ok(Pointer {
            address: self.address,
            capability: if self.present == 0 {
                None
            } else {
                Some(Capability {
                    arena: self.arena,
                    allocation: usize::try_from(self.allocation).map_err(|_| backend())?,
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

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Authority {
    kind: u64,
    pointer: RawPointer,
    signature: u64,
    target_kind: u64,
    target: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Fragment {
    occupied: u64,
    offset: u64,
    ordinal: u64,
    authority: Authority,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawAllocation {
    kind: u64,
    address: u64,
    generation: u64,
    alive: u64,
    read: u64,
    write: u64,
    length: u64,
    data: *mut u8,
    initialized: *mut u8,
    fragments: *mut Fragment,
    fragment_capacity: u64,
    fragment_count: u64,
}

impl Default for RawAllocation {
    fn default() -> Self {
        Self {
            kind: 0,
            address: 0,
            generation: 0,
            alive: 0,
            read: 0,
            write: 0,
            length: 0,
            data: std::ptr::null_mut(),
            initialized: std::ptr::null_mut(),
            fragments: std::ptr::null_mut(),
            fragment_capacity: 0,
            fragment_count: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct State {
    arena: u64,
    allocations: u64,
    bytes: u64,
    fragments: u64,
    work: u64,
    next_address: u64,
}

#[repr(C)]
struct Limits {
    bytes: u64,
    allocations: u64,
    fragments: u64,
    work: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Part {
    offset: u64,
    kind: u64,
    low: u64,
    high: u64,
    authority: Authority,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Gep {
    kind: u64,
    amount: u64,
    low: u64,
    high: u64,
    signed: u64,
}

#[repr(C)]
struct Request {
    op: u64,
    mode: u64,
    size: u64,
    natural: u64,
    align: u64,
    flag: u64,
    owner: u64,
    pointer: RawPointer,
    source: RawPointer,
    parts: *const Part,
    part_count: u64,
    geps: *const Gep,
    gep_count: u64,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            op: 0,
            mode: 0,
            size: 0,
            natural: 0,
            align: 0,
            flag: 0,
            owner: 0,
            pointer: RawPointer::default(),
            source: RawPointer::default(),
            parts: std::ptr::null(),
            part_count: 0,
            geps: std::ptr::null(),
            gep_count: 0,
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct RawResult {
    status: u64,
    detail: u64,
    extra: u64,
    work: u64,
    allocation: u64,
    start: u64,
    end: u64,
    fragments: u64,
    fragment_capacity: u64,
    low: u64,
    high: u64,
    pointer: RawPointer,
}

unsafe extern "C" {
    fn whale_ir_memory(
        state: *mut State,
        limits: *const Limits,
        allocations: *mut RawAllocation,
        allocation_capacity: u64,
        request: *const Request,
        fragments: *mut Fragment,
        fragment_capacity: u64,
        output_parts: *mut Part,
        output_part_capacity: u64,
        result: *mut RawResult,
    ) -> i32;
}

pub(super) struct Allocation {
    pub(super) bytes: Vec<u8>,
    initialized: Vec<u8>,
    fragments: Vec<Fragment>,
}

pub(crate) struct Memory {
    state: State,
    raw: Vec<RawAllocation>,
    pub(super) allocations: Vec<Allocation>,
    pub(super) limits: MemoryLimits,
    pub(super) work: u64,
    signatures: Vec<Arc<crate::FunctionSignature>>,
}

fn backend() -> Fault {
    Fault::Backend("tracked memory transport")
}

fn host(limit: u64) -> Fault {
    Fault::Limit { resource: MemoryResource::HostAllocation, limit }
}

fn zeroed<T: Default + Clone>(count: usize) -> Result<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(count).map_err(|_| host(count as u64))?;
    v.resize(count, T::default());
    Ok(v)
}

fn kind(ty: &Type) -> Result<u64> {
    Ok(match ty {
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
        _ => return Err(backend()),
    })
}

fn raw_scalar(value: &ConstValue) -> u128 {
    match value {
        ConstValue::Bool(v) => u128::from(*v),
        ConstValue::I(v) => *v as u128,
        ConstValue::U(v) => *v,
        ConstValue::F(v) => u128::from(v.bits()),
    }
}

fn scalar_from(part: &Part) -> Result<ConstValue> {
    let bits = u128::from(part.low) | (u128::from(part.high) << 64);
    Ok(match part.kind {
        1 if part.low <= 1 && part.high == 0 => ConstValue::Bool(part.low != 0),
        2 | 4..=8 => ConstValue::I(bits as i128),
        3 | 9..=13 => ConstValue::U(bits),
        14 if part.high == 0 && part.low <= u16::MAX.into() => {
            ConstValue::F(crate::FloatBits::F16(part.low as u16))
        },
        15 if part.high == 0 && part.low <= u32::MAX.into() => {
            ConstValue::F(crate::FloatBits::F32(part.low as u32))
        },
        16 if part.high == 0 => ConstValue::F(crate::FloatBits::F64(part.low)),
        _ => return Err(backend()),
    })
}

impl Memory {
    #[cfg(test)]
    pub(super) fn test_snapshot(&self, id: usize) -> (Vec<u8>, Vec<u8>, String) {
        let a = &self.allocations[id];
        (a.bytes.clone(), a.initialized.clone(), format!("{:?}", a.fragments))
    }

    #[cfg(test)]
    pub(super) fn test_alive(&self, id: usize) -> bool {
        self.raw[id].alive == 1
    }

    pub fn new(limits: MemoryLimits) -> Self {
        Self {
            state: State {
                arena: NEXT_ARENA.fetch_add(1, Ordering::Relaxed),
                next_address: 0x1000,
                ..State::default()
            },
            raw: Vec::new(),
            allocations: Vec::new(),
            limits,
            work: 0,
            signatures: Vec::new(),
        }
    }

    fn fault(result: &RawResult) -> Fault {
        let trap = match result.status {
            1 => MemoryTrap::NullPointer,
            2 => MemoryTrap::MissingProvenance,
            3 => MemoryTrap::ExpiredAllocation,
            4 => MemoryTrap::OutOfBounds,
            5 => MemoryTrap::AddressOverflow,
            6 => MemoryTrap::PermissionDenied,
            7 => MemoryTrap::Misaligned { address: result.detail, alignment: result.extra as u32 },
            8 => MemoryTrap::Uninitialized { offset: result.detail },
            9 => MemoryTrap::InvalidBool { byte: result.detail as u8 },
            11 => MemoryTrap::OverlappingCopy,
            12 => MemoryTrap::DifferentAllocation,
            13 => MemoryTrap::InvalidLifetimeEnd,
            101..=105 => {
                return Fault::Limit {
                    resource: match result.status {
                        101 => MemoryResource::Bytes,
                        102 => MemoryResource::Allocations,
                        103 => MemoryResource::PointerFragments,
                        104 => MemoryResource::Work,
                        _ => MemoryResource::HostAllocation,
                    },
                    limit: result.detail,
                }
            },
            _ => return backend(),
        };
        Fault::Trap(trap)
    }

    fn run(
        &mut self,
        request: &Request,
        fragments: &mut [Fragment],
        parts: &mut [Part],
    ) -> Result<RawResult> {
        let limits = Limits {
            bytes: self.limits.max_bytes,
            allocations: self.limits.max_allocations as u64,
            fragments: self.limits.max_pointer_fragments as u64,
            work: self.limits.max_work,
        };
        let before = self.state;
        let mut result = RawResult::default();
        let status = unsafe {
            whale_ir_memory(
                &mut self.state,
                &limits,
                self.raw.as_mut_ptr(),
                self.raw.len() as u64,
                request,
                fragments.as_mut_ptr(),
                fragments.len() as u64,
                parts.as_mut_ptr(),
                parts.len() as u64,
                &mut result,
            )
        };
        if i32::try_from(result.status).ok() != Some(status)
            || result.work > self.limits.max_work.saturating_sub(before.work)
            || self.state.allocations > self.raw.len() as u64
            || self.state.allocations > self.limits.max_allocations as u64
            || self.state.bytes > self.limits.max_bytes
            || self.state.fragments > self.limits.max_pointer_fragments as u64
            || self.state.work > self.limits.max_work
            || self.state.arena != before.arena
            || self.state.work < before.work
            || self.state.bytes < before.bytes
            || self.state.allocations < before.allocations
        {
            return Err(backend());
        }
        self.work = self.state.work;
        if status != 0 {
            return Err(Self::fault(&result));
        }
        Ok(result)
    }

    fn simple(&mut self, mut q: Request) -> Result<RawResult> {
        q.mode = 1;
        self.run(&q, &mut [], &mut [])
    }

    pub fn allocate(&mut self, size: u64, natural: u32, align: u32) -> Result<Pointer> {
        self.allocate_kind(size, natural, align, false)
    }

    pub fn allocate_global(&mut self, size: u64, natural: u32, align: u32) -> Result<Pointer> {
        self.allocate_kind(size, natural, align, true)
    }

    fn allocate_kind(
        &mut self,
        size: u64,
        natural: u32,
        align: u32,
        global: bool,
    ) -> Result<Pointer> {
        let mut q = Request {
            op: 0,
            size,
            natural: natural.into(),
            align: align.into(),
            flag: u64::from(global),
            ..Request::default()
        };
        let probe = self.run(&q, &mut [], &mut [])?;
        if probe.allocation != self.state.allocations {
            return Err(backend());
        }
        let length = usize::try_from(size).map_err(|_| host(size))?;
        let mut data = zeroed::<u8>(length)?;
        let mut initialized = zeroed::<u8>(length)?;
        self.raw.try_reserve(1).map_err(|_| host(size))?;
        self.allocations.try_reserve(1).map_err(|_| host(size))?;
        let old_count = self.raw.len();
        self.raw.push(RawAllocation {
            data: data.as_mut_ptr(),
            initialized: initialized.as_mut_ptr(),
            ..RawAllocation::default()
        });
        q.mode = 1;
        let committed = match self.run(&q, &mut [], &mut []) {
            Ok(r) => r,
            Err(e) => {
                self.raw.truncate(old_count);
                return Err(e);
            },
        };
        if self.state.allocations != old_count as u64 + 1
            || committed.allocation != old_count as u64
            || committed.pointer.address != probe.pointer.address
        {
            return Err(backend());
        }
        self.allocations.push(Allocation { bytes: data, initialized, fragments: Vec::new() });
        committed.pointer.decode()
    }

    pub fn seal_global(&mut self, pointer: Pointer, readonly: bool) -> Result<Pointer> {
        self.simple(Request {
            op: 1,
            pointer: pointer.into(),
            flag: u64::from(readonly),
            ..Request::default()
        })?
        .pointer
        .decode()
    }

    pub fn retire_stack(&mut self, pointer: Pointer, owner_start: usize) -> Result<()> {
        let r = self.simple(Request {
            op: 2,
            pointer: pointer.into(),
            owner: owner_start as u64,
            ..Request::default()
        })?;
        let _ = r;
        self.sync_retired(owner_start);
        Ok(())
    }

    fn sync_retired(&mut self, start: usize) {
        for (raw, buffer) in self.raw.iter().zip(&mut self.allocations).skip(start) {
            if raw.alive == 0 {
                buffer.bytes.clear();
                buffer.initialized.clear();
                buffer.fragments.clear();
            }
        }
    }

    pub fn retire_from(&mut self, start: usize) -> Result<()> {
        self.simple(Request { op: 11, owner: start as u64, ..Request::default() })?;
        self.sync_retired(start);
        Ok(())
    }

    #[cfg(test)]
    pub fn retire_all(&mut self) {
        self.retire_from(0).unwrap();
    }

    pub fn allocation_count(&self) -> usize {
        self.raw.len()
    }

    pub fn difference(&self, lhs: Pointer, rhs: Pointer) -> Result<i128> {
        let r = self.readonly(Request {
            op: 10,
            pointer: lhs.into(),
            source: rhs.into(),
            ..Request::default()
        })?;
        Ok((u128::from(r.low) | u128::from(r.high) << 64) as i128)
    }

    pub fn equal(&self, lhs: Pointer, rhs: Pointer) -> Result<bool> {
        let r = self.readonly(Request {
            op: 9,
            pointer: lhs.into(),
            source: rhs.into(),
            ..Request::default()
        })?;
        if r.low > 1 {
            return Err(backend());
        }
        Ok(r.low != 0)
    }

    fn readonly(&self, q: Request) -> Result<RawResult> {
        let mut state = self.state;
        let limits = Limits {
            bytes: self.limits.max_bytes,
            allocations: self.limits.max_allocations as u64,
            fragments: self.limits.max_pointer_fragments as u64,
            work: self.limits.max_work,
        };
        let mut result = RawResult::default();
        let status = unsafe {
            whale_ir_memory(
                &mut state,
                &limits,
                self.raw.as_ptr().cast_mut(),
                self.raw.len() as u64,
                &q,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                &mut result,
            )
        };
        if i32::try_from(result.status).ok() != Some(status) || state.work != self.state.work {
            return Err(backend());
        }
        if status != 0 {
            return Err(Self::fault(&result));
        }
        Ok(result)
    }

    pub fn gep(
        &self,
        pointer: Pointer,
        plan: &[GepStep],
        indices: &[ConstValue],
    ) -> Result<Pointer> {
        if plan.len() != indices.len() {
            return Err(backend());
        }
        let mut geps = Vec::new();
        geps.try_reserve_exact(plan.len()).map_err(|_| host(plan.len() as u64))?;
        for (step, value) in plan.iter().zip(indices) {
            let (raw, signed) = match value {
                ConstValue::I(v) => (*v as u128, 1),
                ConstValue::U(v) => (*v, 0),
                _ => return Err(backend()),
            };
            let (kind, amount) = match step {
                GepStep::Stride(n) => (0, *n),
                GepStep::Field(n) => (1, *n),
            };
            geps.push(Gep { kind, amount, low: raw as u64, high: (raw >> 64) as u64, signed });
        }
        self.readonly(Request {
            op: 8,
            pointer: pointer.into(),
            geps: geps.as_ptr(),
            gep_count: geps.len() as u64,
            ..Request::default()
        })?
        .pointer
        .decode()
    }

    fn mutate(&mut self, mut q: Request) -> Result<()> {
        let probe = self.run(&q, &mut [], &mut [])?;
        let count = usize::try_from(probe.fragment_capacity).map_err(|_| backend())?;
        if count > self.limits.max_pointer_fragments.saturating_mul(4) {
            return Err(backend());
        }
        let mut fragments = zeroed::<Fragment>(count)?;
        q.mode = 1;
        let result = self.run(&q, &mut fragments, &mut [])?;
        if result.allocation == u64::MAX {
            if probe.allocation != u64::MAX || result.fragment_capacity != 0 {
                return Err(backend());
            }
            return Ok(());
        }
        let id = usize::try_from(result.allocation).map_err(|_| backend())?;
        if id >= self.allocations.len()
            || result.fragment_capacity != fragments.len() as u64
            || result.fragment_capacity != probe.fragment_capacity
            || result.fragments != probe.fragments
        {
            return Err(backend());
        }
        self.allocations[id].fragments = fragments;
        Ok(())
    }

    pub fn uninit(&mut self, pointer: Pointer, size: u64, align: u32) -> Result<()> {
        self.mutate(Request {
            op: 5,
            pointer: pointer.into(),
            size,
            align: align.into(),
            ..Request::default()
        })
    }

    pub fn copy(&mut self, dst: Pointer, src: Pointer, size: u64, align: u32) -> Result<()> {
        self.mutate(Request {
            op: 6,
            pointer: dst.into(),
            source: src.into(),
            size,
            align: align.into(),
            ..Request::default()
        })
    }

    pub fn set(&mut self, dst: Pointer, octet: u8, size: u64, align: u32) -> Result<()> {
        self.mutate(Request {
            op: 7,
            pointer: dst.into(),
            flag: octet.into(),
            size,
            align: align.into(),
            ..Request::default()
        })
    }

    fn signature(&mut self, signature: &Arc<crate::FunctionSignature>) -> Result<u64> {
        // Canonical IDs describe structural types, never native addresses.
        crate::limits::Budget::new(crate::IrLimits::default())
            .and_then(|mut b| b.signature(signature))
            .map_err(|_| backend())?;
        for (id, candidate) in self.signatures.iter().enumerate() {
            let a = Type::FnPtr(Box::new((**signature).clone()));
            let b = Type::FnPtr(Box::new((**candidate).clone()));
            if crate::verifier::wave_types_equal(
                &a,
                &b,
                crate::IrLimits::default().max_analysis_work,
            )
            .map_err(|_| backend())?
            .0
            {
                return Ok(id as u64 + 1);
            }
        }
        self.signatures.try_reserve(1).map_err(|_| host(self.signatures.len() as u64))?;
        self.signatures.push(signature.clone());
        Ok(self.signatures.len() as u64)
    }

    fn part(
        &mut self,
        ty: &Type,
        value: Option<&Value>,
        offset: u64,
        signature: Option<&Arc<crate::FunctionSignature>>,
    ) -> Result<Part> {
        let mut part = Part { offset, kind: kind(ty)?, ..Part::default() };
        if let Some(Value::Pointer(p)) = value {
            part.low = p.address;
            part.authority = Authority { kind: 1, pointer: (*p).into(), ..Authority::default() };
        } else if let Some(Value::FunctionPointer(p)) = value {
            let sid = self.signature(&p.signature)?;
            let (target_kind, target) = match p.target {
                FunctionTarget::Null => (0, 0),
                FunctionTarget::Defined(id) => (1, u64::from(id.0)),
                FunctionTarget::InvalidBits(bits) => (2, bits),
            };
            part.low = p.bits();
            part.authority =
                Authority { kind: 2, signature: sid, target_kind, target, ..Authority::default() };
        } else if let Some(Value::Scalar(v)) = value {
            let bits = raw_scalar(v);
            part.low = bits as u64;
            part.high = (bits >> 64) as u64;
        } else if value.is_some() {
            return Err(backend());
        }
        if let Type::FnPtr(sig) = ty {
            let sig = signature.cloned().unwrap_or_else(|| Arc::new((**sig).clone()));
            part.authority.signature = self.signature(&sig)?;
        }
        Ok(part)
    }

    pub fn store(&mut self, pointer: Pointer, ty: &Type, value: &Value, align: u32) -> Result<()> {
        let mut parts = Vec::new();
        if let (Type::Tuple(fields), Value::Checked(v, flag)) = (ty, value) {
            let offsets = layout(ty).field_offsets;
            parts.push(self.part(&fields[0], Some(&Value::Scalar(v.clone())), offsets[0], None)?);
            parts.push(self.part(
                &fields[1],
                Some(&Value::Scalar(ConstValue::Bool(*flag))),
                offsets[1],
                None,
            )?);
        } else {
            parts.push(self.part(ty, Some(value), 0, None)?);
        }
        self.mutate(Request {
            op: 4,
            pointer: pointer.into(),
            size: layout(ty).size,
            align: align.into(),
            parts: parts.as_ptr(),
            part_count: parts.len() as u64,
            ..Request::default()
        })
    }

    #[cfg(test)]
    pub fn load(&mut self, pointer: Pointer, ty: &Type, align: u32) -> Result<Value> {
        self.load_with_signature(pointer, ty, align, None)
    }

    pub fn load_with_signature(
        &mut self,
        pointer: Pointer,
        ty: &Type,
        align: u32,
        signature: Option<&Arc<crate::FunctionSignature>>,
    ) -> Result<Value> {
        let mut inputs = Vec::new();
        if let Type::Tuple(fields) = ty {
            let offsets = layout(ty).field_offsets;
            inputs.push(self.part(&fields[0], None, offsets[0], None)?);
            inputs.push(self.part(&fields[1], None, offsets[1], None)?);
        } else {
            inputs.push(self.part(ty, None, 0, signature)?);
        }
        let mut output = [Part::default(); 2];
        let q = Request {
            op: 3,
            mode: 1,
            pointer: pointer.into(),
            size: layout(ty).size,
            align: align.into(),
            parts: inputs.as_ptr(),
            part_count: inputs.len() as u64,
            ..Request::default()
        };
        self.run(&q, &mut [], &mut output)?;
        if inputs.len() == 2 {
            return Ok(Value::Checked(
                scalar_from(&output[0])?,
                matches!(scalar_from(&output[1])?, ConstValue::Bool(true)),
            ));
        }
        let part = &output[0];
        if matches!(ty, Type::Ptr(_)) {
            return Ok(Value::Pointer(if part.authority.kind == 1 {
                part.authority.pointer.decode()?
            } else {
                Pointer::raw(part.low)
            }));
        }
        if let Type::FnPtr(sig) = ty {
            let signature = signature.cloned().unwrap_or_else(|| Arc::new((**sig).clone()));
            let target = if part.low == 0 {
                FunctionTarget::Null
            } else if part.authority.kind == 2 {
                match part.authority.target_kind {
                    0 => FunctionTarget::Null,
                    1 => FunctionTarget::Defined(crate::FunctionId(
                        part.authority.target.try_into().map_err(|_| backend())?,
                    )),
                    2 => FunctionTarget::InvalidBits(part.authority.target),
                    _ => return Err(backend()),
                }
            } else {
                FunctionTarget::InvalidBits(part.low)
            };
            return Ok(Value::FunctionPointer(FunctionPointer { signature, target }));
        }
        Ok(Value::Scalar(scalar_from(part)?))
    }

    #[cfg(test)]
    pub(super) fn test_read(&mut self, id: usize, read: bool) {
        self.raw[id].read = u64::from(read);
    }

    #[cfg(test)]
    pub(super) fn test_generation(&mut self, id: usize, delta: i64) {
        self.raw[id].generation = self.raw[id].generation.wrapping_add_signed(delta);
    }
}
