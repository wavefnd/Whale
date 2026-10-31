// SPDX-License-Identifier: MPL-2.0
//! Explicit Rust conformance implementation.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum StoredPointer {
    Data(Pointer),
    Function(FunctionPointer),
}

impl StoredPointer {
    fn bits(&self) -> u64 {
        match self {
            Self::Data(p) => p.address,
            Self::Function(p) => p.bits(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fragment {
    pointer: StoredPointer,
    byte: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AllocationKind {
    Stack,
    Global,
}

pub(super) struct Allocation {
    kind: AllocationKind,
    address: u64,
    pub(super) generation: u64,
    alive: bool,
    pub(super) read: bool,
    write: bool,
    pub(super) bytes: Vec<u8>,
    initialized: Vec<u8>,
    fragments: HashMap<usize, Fragment>,
}

pub(crate) struct Memory {
    arena: u64,
    pub(super) allocations: Vec<Allocation>,
    pub(super) limits: MemoryLimits,
    bytes: u64,
    fragments: usize,
    pub(super) work: u64,
    next_address: u64,
}

impl Memory {
    #[cfg(test)]
    pub(super) fn test_snapshot(&self, id: usize) -> (Vec<u8>, Vec<u8>, String) {
        let a = &self.allocations[id];
        (a.bytes.clone(), a.initialized.clone(), format!("{:?}", a.fragments))
    }

    #[cfg(test)]
    pub(super) fn test_alive(&self, id: usize) -> bool {
        self.allocations[id].alive
    }

    #[cfg(test)]
    pub(super) fn test_read(&mut self, id: usize, read: bool) {
        self.allocations[id].read = read;
    }

    #[cfg(test)]
    pub(super) fn test_generation(&mut self, id: usize, delta: i64) {
        self.allocations[id].generation =
            self.allocations[id].generation.wrapping_add_signed(delta);
    }

    pub fn new(limits: MemoryLimits) -> Self {
        Self {
            arena: NEXT_ARENA.fetch_add(1, Ordering::Relaxed),
            allocations: Vec::new(),
            limits,
            bytes: 0,
            fragments: 0,
            work: 0,
            next_address: 0x1000,
        }
    }

    fn limit(resource: MemoryResource, limit: u64) -> Fault {
        Fault::Limit { resource, limit }
    }

    fn charge(&mut self, work: u64) -> Result<()> {
        self.work = self
            .work
            .checked_add(work)
            .filter(|n| *n <= self.limits.max_work)
            .ok_or_else(|| Self::limit(MemoryResource::Work, self.limits.max_work))?;
        Ok(())
    }

    pub fn allocate(&mut self, size: u64, natural: u32, align: u32) -> Result<Pointer> {
        if self.allocations.len() >= self.limits.max_allocations {
            return Err(Self::limit(
                MemoryResource::Allocations,
                self.limits.max_allocations as u64,
            ));
        }
        let bytes = self
            .bytes
            .checked_add(size)
            .filter(|n| *n <= self.limits.max_bytes)
            .ok_or_else(|| Self::limit(MemoryResource::Bytes, self.limits.max_bytes))?;
        let length =
            usize::try_from(size).map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        let alignment = u64::from(align.max(natural));
        let address = self
            .next_address
            .checked_add(alignment - 1)
            .map(|a| a & !(alignment - 1))
            .ok_or(MemoryTrap::AddressOverflow)?;
        let next = address
            .checked_add(size.max(1))
            .and_then(|a| a.checked_add(1))
            .ok_or(MemoryTrap::AddressOverflow)?;
        self.charge(
            size.checked_mul(2)
                .and_then(|n| n.checked_add(1))
                .ok_or(MemoryTrap::AddressOverflow)?,
        )?;
        let mut data = Vec::new();
        let mut initialized = Vec::new();
        data.try_reserve_exact(length)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        initialized
            .try_reserve_exact(length)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        self.allocations
            .try_reserve(1)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        // Physical zero bytes are not semantic initialization.
        data.resize(length, 0);
        initialized.resize(length, 0);
        let capability = Capability {
            arena: self.arena,
            allocation: self.allocations.len(),
            generation: 0,
            lower: 0,
            upper: size,
            offset: 0,
            read: true,
            write: true,
        };
        self.allocations.push(Allocation {
            kind: AllocationKind::Stack,
            address,
            generation: 0,
            alive: true,
            read: true,
            write: true,
            bytes: data,
            initialized,
            fragments: HashMap::new(),
        });
        self.bytes = bytes;
        self.next_address = next;
        Ok(Pointer { address, capability: Some(capability) })
    }

    pub fn allocate_global(&mut self, size: u64, natural: u32, align: u32) -> Result<Pointer> {
        let pointer = self.allocate(size, natural, align)?;
        self.allocations[pointer.capability.unwrap().allocation].kind = AllocationKind::Global;
        Ok(pointer)
    }

    pub fn seal_global(&mut self, mut pointer: Pointer, readonly: bool) -> Result<Pointer> {
        let (id, _) = self.identity(pointer)?;
        if self.allocations[id].kind != AllocationKind::Global {
            return Err(MemoryTrap::InvalidLifetimeEnd.into());
        }
        self.allocations[id].write = !readonly;
        pointer.capability.as_mut().unwrap().write = !readonly;
        Ok(pointer)
    }

    pub fn retire_stack(&mut self, pointer: Pointer, owner_start: usize) -> Result<()> {
        let (id, capability) = self.identity(pointer)?;
        if id < owner_start
            || self.allocations[id].kind != AllocationKind::Stack
            || capability.offset != 0
            || capability.lower != 0
            || capability.upper != self.allocations[id].bytes.len() as u64
        {
            return Err(MemoryTrap::InvalidLifetimeEnd.into());
        }
        self.charge(1 + self.allocations[id].fragments.len() as u64)?;
        self.retire_allocation(id);
        Ok(())
    }

    fn retire_allocation(&mut self, id: usize) {
        let allocation = &mut self.allocations[id];
        if allocation.alive {
            allocation.alive = false;
            self.fragments -= allocation.fragments.len();
            allocation.fragments.clear();
            allocation.bytes.clear();
            allocation.initialized.clear();
        }
    }

    pub fn difference(&self, lhs: Pointer, rhs: Pointer) -> Result<i128> {
        let (a, left) = self.identity(lhs)?;
        let (b, right) = self.identity(rhs)?;
        if a != b || left.generation != right.generation {
            return Err(MemoryTrap::DifferentAllocation.into());
        }
        Ok(i128::from(left.offset) - i128::from(right.offset))
    }

    pub fn compare_order(&self, lhs: Pointer, rhs: Pointer) -> Result<std::cmp::Ordering> {
        Ok(self.difference(lhs, rhs)?.cmp(&0))
    }

    fn identity(&self, pointer: Pointer) -> Result<(usize, Capability)> {
        if pointer.address == 0 {
            return Err(MemoryTrap::NullPointer.into());
        }
        let cap = pointer.capability.ok_or(MemoryTrap::MissingProvenance)?;
        if cap.arena != self.arena {
            return Err(MemoryTrap::ExpiredAllocation.into());
        }
        let allocation =
            self.allocations.get(cap.allocation).ok_or(MemoryTrap::ExpiredAllocation)?;
        if !allocation.alive || allocation.generation != cap.generation {
            return Err(MemoryTrap::ExpiredAllocation.into());
        }
        if cap.lower > cap.offset
            || cap.offset > cap.upper
            || cap.upper > allocation.bytes.len() as u64
            || allocation.address.checked_add(cap.offset) != Some(pointer.address)
        {
            return Err(MemoryTrap::OutOfBounds.into());
        }
        Ok((cap.allocation, cap))
    }

    fn access(
        &self,
        pointer: Pointer,
        n: u64,
        align: u32,
        write: bool,
    ) -> Result<(usize, usize, usize)> {
        let (id, cap) = self.identity(pointer)?;
        let end = cap.offset.checked_add(n).ok_or(MemoryTrap::AddressOverflow)?;
        if end > cap.upper {
            return Err(MemoryTrap::OutOfBounds.into());
        }
        let allocation = &self.allocations[id];
        let permitted =
            if write { cap.write && allocation.write } else { cap.read && allocation.read };
        if !permitted {
            return Err(MemoryTrap::PermissionDenied.into());
        }
        if pointer.address % u64::from(align) != 0 {
            return Err(
                MemoryTrap::Misaligned { address: pointer.address, alignment: align }.into()
            );
        }
        Ok((id, cap.offset as usize, end as usize))
    }

    pub fn gep(
        &self,
        pointer: Pointer,
        plan: &[GepStep],
        indices: &[ConstValue],
    ) -> Result<Pointer> {
        if indices.is_empty() {
            return Ok(pointer);
        }
        let (_, mut cap) = self.identity(pointer)?;
        let base = i128::from(pointer.address) - i128::from(cap.offset);
        let mut offset = i128::from(cap.offset);
        for (step, index) in plan.iter().zip(indices) {
            let delta = match step {
                GepStep::Field(offset) => i128::from(*offset),
                GepStep::Stride(stride) => {
                    let index = match index {
                        ConstValue::I(v) => *v,
                        ConstValue::U(v) => {
                            i128::try_from(*v).map_err(|_| MemoryTrap::AddressOverflow)?
                        },
                        _ => unreachable!("verified integer GEP index"),
                    };
                    index.checked_mul(i128::from(*stride)).ok_or(MemoryTrap::AddressOverflow)?
                },
            };
            offset = offset.checked_add(delta).ok_or(MemoryTrap::AddressOverflow)?;
            let address = base.checked_add(offset).ok_or(MemoryTrap::AddressOverflow)?;
            if !(0..=i128::from(u64::MAX)).contains(&address) {
                return Err(MemoryTrap::AddressOverflow.into());
            }
        }
        let offset = u64::try_from(offset).map_err(|_| MemoryTrap::OutOfBounds)?;
        if offset < cap.lower || offset > cap.upper {
            return Err(MemoryTrap::OutOfBounds.into());
        }
        let address = u64::try_from(base + i128::from(offset)).unwrap();
        cap.offset = offset;
        Ok(Pointer { address, capability: Some(cap) })
    }

    pub fn equal(&self, lhs: Pointer, rhs: Pointer) -> Result<bool> {
        if lhs.address == 0 && rhs.address == 0 {
            return Ok(true);
        }
        if lhs.address == 0 {
            self.identity(rhs)?;
            return Ok(false);
        }
        if rhs.address == 0 {
            self.identity(lhs)?;
            return Ok(false);
        }
        let (_, a) = self.identity(lhs)?;
        let (_, b) = self.identity(rhs)?;
        Ok((a.allocation, a.generation, a.offset) == (b.allocation, b.generation, b.offset))
    }

    fn replace_fragments(
        &mut self,
        id: usize,
        start: usize,
        end: usize,
        new: &[(usize, Fragment)],
    ) -> Result<()> {
        let allocation = &self.allocations[id];
        self.charge(3 * allocation.fragments.capacity() as u64 + new.len() as u64)?;
        let removed =
            self.allocations[id].fragments.keys().filter(|i| **i >= start && **i < end).count();
        let count = self.fragments - removed + new.len();
        if count > self.limits.max_pointer_fragments {
            return Err(Self::limit(
                MemoryResource::PointerFragments,
                self.limits.max_pointer_fragments as u64,
            ));
        }
        self.allocations[id]
            .fragments
            .try_reserve(new.len())
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, new.len() as u64))?;
        self.allocations[id].fragments.retain(|i, _| *i < start || *i >= end);
        self.allocations[id].fragments.extend(new.iter().cloned());
        self.fragments = count;
        Ok(())
    }

    pub fn uninit(&mut self, pointer: Pointer, size: u64, align: u32) -> Result<()> {
        let (id, start, end) = self.access(pointer, size, align, true)?;
        self.charge((end - start) as u64)?;
        self.replace_fragments(id, start, end, &[])?;
        self.allocations[id].initialized[start..end].fill(0);
        Ok(())
    }

    pub fn copy(&mut self, dst: Pointer, src: Pointer, size: u64, align: u32) -> Result<()> {
        // No access occurs for an empty operation, even for null/raw pointers.
        if size == 0 {
            return Ok(());
        }
        let (source, ss, se) = self.access(src, size, align, false)?;
        let (destination, ds, de) = self.access(dst, size, align, true)?;
        if source == destination && ss < de && ds < se {
            return Err(MemoryTrap::OverlappingCopy.into());
        }
        self.charge(
            size.checked_mul(4)
                .ok_or(MemoryTrap::AddressOverflow)?
                .checked_add(self.allocations[source].fragments.capacity() as u64)
                .ok_or(MemoryTrap::AddressOverflow)?,
        )?;
        let source = &self.allocations[source];
        let mut data = Vec::new();
        let mut initialized = Vec::new();
        let mut fragments = Vec::new();
        let count = source.fragments.len().min(se - ss);
        data.try_reserve_exact(se - ss)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        initialized
            .try_reserve_exact(se - ss)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        fragments
            .try_reserve_exact(count)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, count as u64))?;
        data.extend_from_slice(&source.bytes[ss..se]);
        initialized.extend_from_slice(&source.initialized[ss..se]);
        fragments.extend(
            source
                .fragments
                .iter()
                .filter(|(at, _)| **at >= ss && **at < se)
                .map(|(at, tag)| (ds + (*at - ss), tag.clone())),
        );
        // Reserve/check metadata before any destination bytes are changed.
        self.replace_fragments(destination, ds, de, &fragments)?;
        self.allocations[destination].bytes[ds..de].copy_from_slice(&data);
        self.allocations[destination].initialized[ds..de].copy_from_slice(&initialized);
        Ok(())
    }

    pub fn set(&mut self, dst: Pointer, byte: u8, size: u64, align: u32) -> Result<()> {
        if size == 0 {
            return Ok(());
        }
        let (id, start, end) = self.access(dst, size, align, true)?;
        self.charge(size.checked_mul(2).ok_or(MemoryTrap::AddressOverflow)?)?;
        self.replace_fragments(id, start, end, &[])?;
        self.allocations[id].bytes[start..end].fill(byte);
        self.allocations[id].initialized[start..end].fill(1);
        Ok(())
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
        let (id, start, end) = self.access(pointer, layout(ty).size, align, false)?;
        self.charge((end - start) as u64)?;
        if let Type::Tuple(fields) = ty {
            let offsets = layout(ty).field_offsets;
            let a = self.load_scalar(id, start + offsets[0] as usize, &fields[0], None)?;
            let b = self.load_scalar(id, start + offsets[1] as usize, &fields[1], None)?;
            return Ok(Value::Checked(a.scalar().clone(), b.boolean()));
        }
        self.load_scalar(id, start, ty, signature)
    }

    fn load_scalar(
        &self,
        id: usize,
        start: usize,
        ty: &Type,
        callable_signature: Option<&Arc<crate::FunctionSignature>>,
    ) -> Result<Value> {
        let allocation = &self.allocations[id];
        let n = layout(ty).size as usize;
        for offset in start..start + n {
            if allocation.initialized[offset] == 0 {
                return Err(MemoryTrap::Uninitialized { offset: offset as u64 }.into());
            }
        }
        let bytes = &allocation.bytes[start..start + n];
        if *ty == Type::Bool {
            return match bytes[0] {
                0 => Ok(Value::Scalar(ConstValue::Bool(false))),
                1 => Ok(Value::Scalar(ConstValue::Bool(true))),
                byte => Err(MemoryTrap::InvalidBool { byte }.into()),
            };
        }
        if matches!(ty, Type::Ptr(_) | Type::FnPtr(_)) {
            let address = u64::from_le_bytes(bytes.try_into().unwrap());
            let complete = allocation.fragments.get(&start).filter(|fragment| {
                fragment.byte == 0
                    && fragment.pointer.bits() == address
                    && (0..8).all(|i| {
                        allocation.fragments.get(&(start + i))
                            == Some(&Fragment { pointer: fragment.pointer.clone(), byte: i as u8 })
                    })
            });
            return Ok(match ty {
                Type::Ptr(_) => Value::Pointer(if address == 0 {
                    Pointer::raw(0)
                } else {
                    match complete.map(|f| &f.pointer) {
                        Some(StoredPointer::Data(pointer)) => *pointer,
                        _ => Pointer::raw(address),
                    }
                }),
                Type::FnPtr(signature) => {
                    let signature = callable_signature
                        .cloned()
                        .unwrap_or_else(|| Arc::new((**signature).clone()));
                    let target = if address == 0 {
                        FunctionTarget::Null
                    } else {
                        match complete.map(|f| &f.pointer) {
                            Some(StoredPointer::Function(pointer))
                                if pointer.signature == signature =>
                            {
                                return Ok(Value::FunctionPointer(pointer.clone()))
                            },
                            _ => FunctionTarget::InvalidBits(address),
                        }
                    };
                    Value::FunctionPointer(FunctionPointer { signature, target })
                },
                _ => unreachable!(),
            });
        }
        let mut bits = [0u8; 16];
        bits[..n].copy_from_slice(bytes);
        Ok(Value::Scalar(scalar::pack(ty, u128::from_le_bytes(bits))))
    }

    pub fn store(&mut self, pointer: Pointer, ty: &Type, value: &Value, align: u32) -> Result<()> {
        let (id, start, end) = self.access(pointer, layout(ty).size, align, true)?;
        let mut bytes = Vec::new();
        let mut fragments = Vec::new();
        if let (Type::Tuple(fields), Value::Checked(v, flag)) = (ty, value) {
            let offsets = layout(ty).field_offsets;
            Self::encode(
                &fields[0],
                &Value::Scalar(v.clone()),
                start + offsets[0] as usize,
                &mut bytes,
                &mut fragments,
            );
            Self::encode(
                &fields[1],
                &Value::Scalar(ConstValue::Bool(*flag)),
                start + offsets[1] as usize,
                &mut bytes,
                &mut fragments,
            );
        } else {
            Self::encode(ty, value, start, &mut bytes, &mut fragments);
        }
        self.charge((end - start) as u64)?;
        self.replace_fragments(id, start, end, &fragments)?;
        for (at, byte) in bytes {
            self.allocations[id].bytes[at] = byte;
            self.allocations[id].initialized[at] = 1;
        }
        Ok(())
    }

    fn encode(
        ty: &Type,
        value: &Value,
        start: usize,
        bytes: &mut Vec<(usize, u8)>,
        fragments: &mut Vec<(usize, Fragment)>,
    ) {
        let n = layout(ty).size as usize;
        let data = match value {
            Value::Pointer(p) => {
                for i in 0..8 {
                    fragments.push((
                        start + i,
                        Fragment { pointer: StoredPointer::Data(*p), byte: i as u8 },
                    ));
                }
                u128::from(p.address).to_le_bytes()
            },
            Value::FunctionPointer(p) => {
                for i in 0..8 {
                    fragments.push((
                        start + i,
                        Fragment { pointer: StoredPointer::Function(p.clone()), byte: i as u8 },
                    ));
                }
                u128::from(p.bits()).to_le_bytes()
            },
            Value::Scalar(v) => scalar::raw(ty, v).to_le_bytes(),
            _ => unreachable!("verified scalar or checked storage"),
        };
        bytes.extend(data[..n].iter().enumerate().map(|(i, b)| (start + i, *b)));
    }

    pub fn allocation_count(&self) -> usize {
        self.allocations.len()
    }

    pub fn retire_from(&mut self, start: usize) -> Result<()> {
        self.charge((self.allocations.len() - start) as u64)?;
        for id in start..self.allocations.len() {
            if self.allocations[id].kind == AllocationKind::Stack {
                self.retire_allocation(id);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn retire_all(&mut self) {
        self.retire_from(0).unwrap();
    }
}
