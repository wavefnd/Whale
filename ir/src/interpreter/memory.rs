// SPDX-License-Identifier: MPL-2.0
//! Synthetic 64-bit addresses, byte initialization and separately copied capabilities.
#[cfg(not(whale_wave_ir))]
use super::scalar;

use super::{FunctionPointer, FunctionTarget, Value};

use crate::{ConstValue, Target, Type, TypeLayout};

#[cfg(not(whale_wave_ir))]
use std::collections::HashMap;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

static NEXT_ARENA: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
pub struct MemoryLimits {
    pub max_bytes: u64,
    pub max_allocations: usize,
    pub max_pointer_fragments: usize,
    /// Bytes processed plus metadata entries examined, independently of IR steps.
    pub max_work: u64,
}

impl Default for MemoryLimits {
    fn default() -> Self {
        Self {
            max_bytes: 64 * 1024 * 1024,
            max_allocations: 16_384,
            max_pointer_fragments: 262_144,
            max_work: 256 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryResource {
    Bytes,
    Allocations,
    PointerFragments,
    Work,
    HostAllocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryTrap {
    NullPointer,
    MissingProvenance,
    ExpiredAllocation,
    OutOfBounds,
    AddressOverflow,
    PermissionDenied,
    Misaligned { address: u64, alignment: u32 },
    Uninitialized { offset: u64 },
    InvalidBool { byte: u8 },
    OverlappingCopy,
    DifferentAllocation,
    InvalidLifetimeEnd,
}

impl std::fmt::Display for MemoryTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NullPointer => f.write_str("null pointer access"),
            Self::MissingProvenance => f.write_str("pointer has no tracked allocation provenance"),
            Self::ExpiredAllocation => f.write_str("allocation lifetime has ended"),
            Self::OutOfBounds => f.write_str("pointer range is outside its allocation"),
            Self::AddressOverflow => f.write_str("64-bit address arithmetic overflow"),
            Self::PermissionDenied => f.write_str("pointer access permission denied"),
            Self::Misaligned { address, alignment } => {
                write!(f, "address {address:#x} does not satisfy alignment {alignment}")
            },
            Self::Uninitialized { offset } => {
                write!(f, "uninitialized byte at allocation offset {offset}")
            },
            Self::InvalidBool { byte } => write!(f, "invalid Bool storage byte {byte:#x}"),
            Self::OverlappingCopy => f.write_str("overlapping memcpy ranges"),
            Self::DifferentAllocation => {
                f.write_str("pointer operands belong to different allocations")
            },
            Self::InvalidLifetimeEnd => f.write_str(
                "lifetime_end requires the current frame\'s exact stack allocation base",
            ),
        }
    }
}

#[derive(Debug)]
pub(super) enum Fault {
    Trap(MemoryTrap),
    Limit {
        resource: MemoryResource,
        limit: u64,
    },
    #[cfg_attr(not(whale_wave_ir), allow(dead_code))]
    Backend(&'static str),
}

impl From<MemoryTrap> for Fault {
    fn from(t: MemoryTrap) -> Self {
        Self::Trap(t)
    }
}

type Result<T> = std::result::Result<T, Fault>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Capability {
    pub(super) arena: u64,
    pub(super) allocation: usize,
    pub(super) generation: u64,
    pub(super) lower: u64,
    pub(super) upper: u64,
    pub(super) offset: u64,
    pub(super) read: bool,
    pub(super) write: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Pointer {
    pub address: u64,
    pub(super) capability: Option<Capability>,
}

impl Pointer {
    pub fn raw(address: u64) -> Self {
        Self { address, capability: None }
    }
}

#[derive(Clone, Debug)]
pub(super) enum GepStep {
    Stride(u64),
    Field(u64),
}

fn layout(ty: &Type) -> TypeLayout {
    crate::layout_of_with_limit(ty, Target::X86_64WhaleLinux, crate::MAX_IR_NESTING)
        .expect("bounded layout checked before execution")
}

#[cfg(not(whale_wave_ir))]
#[path = "memory_reference.rs"]
mod reference;

#[cfg(not(whale_wave_ir))]
pub(super) use reference::Memory;

#[cfg(whale_wave_ir)]
#[path = "memory_wave.rs"]
mod wave;

#[cfg(whale_wave_ir)]
pub(super) use wave::Memory;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_binary16_pattern_survives_typed_storage_and_copy() {
        let mut memory = Memory::new(MemoryLimits::default());
        let source = memory.allocate(2, 2, 2).unwrap();
        let dest = memory.allocate(2, 2, 2).unwrap();
        for bits in 0..=u16::MAX {
            let value = Value::Scalar(ConstValue::F(crate::FloatBits::F16(bits)));
            memory.store(source, &Type::F16, &value, 2).unwrap();
            memory.copy(dest, source, 2, 2).unwrap();
            assert_eq!(memory.load(dest, &Type::F16, 2).unwrap().scalar(), value.scalar());
            assert_eq!(
                memory.load(dest, &Type::U16, 2).unwrap().scalar(),
                &ConstValue::U(bits.into())
            );
        }
    }

    fn trapped<T>(result: Result<T>, expected: MemoryTrap) {
        assert!(matches!(result, Err(Fault::Trap(t)) if t == expected));
    }

    fn advance(m: &Memory, p: Pointer, n: i128) -> Pointer {
        m.gep(p, &[GepStep::Stride(1)], &[ConstValue::I(n)]).unwrap()
    }

    #[test]
    fn permissions_lifetime_generation_and_restricted_ranges_are_checked() {
        let mut m = Memory::new(MemoryLimits::default());
        let p = m.allocate(8, 8, 8).unwrap();
        m.set(p, 0, 8, 8).unwrap();
        let mut readonly = p;
        readonly.capability.as_mut().unwrap().write = false;
        assert!(m.load(readonly, &Type::U64, 8).is_ok());
        trapped(
            m.store(readonly, &Type::U64, &Value::Scalar(ConstValue::U(0)), 8),
            MemoryTrap::PermissionDenied,
        );
        m.test_read(0, false);
        trapped(m.load(p, &Type::U8, 1), MemoryTrap::PermissionDenied);
        m.test_read(0, true);
        let mut narrow = p;
        narrow.capability.as_mut().unwrap().upper = 4;
        trapped(m.load(narrow, &Type::U64, 1), MemoryTrap::OutOfBounds);
        m.test_generation(0, 1);
        trapped(m.load(p, &Type::U8, 1), MemoryTrap::ExpiredAllocation);
        m.test_generation(0, -1);
        m.retire_all();
        trapped(m.load(p, &Type::U8, 1), MemoryTrap::ExpiredAllocation);
        // Empty copy/set do not access even an expired capability.
        m.copy(p, p, 0, 8).unwrap();
        m.set(p, 1, 0, 8).unwrap();
    }

    #[test]
    fn split_pointer_copies_reassemble_only_matching_byte_fragments() {
        let mut m = Memory::new(MemoryLimits::default());
        let object = m.allocate(4, 4, 4).unwrap();
        let source = m.allocate(8, 8, 8).unwrap();
        let dest = m.allocate(8, 8, 8).unwrap();
        let ty = Type::ptr_to(Type::U32);
        m.store(source, &ty, &Value::Pointer(object), 8).unwrap();
        m.copy(dest, source, 4, 1).unwrap();
        trapped(m.load(dest, &ty, 8), MemoryTrap::Uninitialized { offset: 4 });
        m.copy(advance(&m, dest, 4), advance(&m, source, 4), 4, 1).unwrap();
        assert_eq!(m.load(dest, &ty, 8).unwrap().pointer(), object);
        let p = advance(&m, dest, 7);
        // Replacing even identical bytes destroys that fragment's authority.
        m.store(p, &Type::U8, &Value::Scalar(ConstValue::U(0)), 1).unwrap();
        let raw = m.load(dest, &ty, 8).unwrap().pointer();
        assert_eq!(raw.address, object.address);
        trapped(m.load(raw, &Type::U32, 4), MemoryTrap::MissingProvenance);
    }

    #[test]
    fn failed_copy_limits_and_invalid_ranges_do_not_partially_write() {
        let mut m =
            Memory::new(MemoryLimits { max_pointer_fragments: 8, ..MemoryLimits::default() });
        let object = m.allocate(1, 1, 1).unwrap();
        let source = m.allocate(8, 8, 8).unwrap();
        let dest = m.allocate(8, 8, 8).unwrap();
        m.store(source, &Type::ptr_to(Type::U8), &Value::Pointer(object), 8).unwrap();
        m.set(dest, 255, 8, 8).unwrap();
        assert!(matches!(
            m.copy(dest, source, 8, 8),
            Err(Fault::Limit { resource: MemoryResource::PointerFragments, .. })
        ));
        assert_eq!(m.load(dest, &Type::U64, 8).unwrap().scalar(), &ConstValue::U(u64::MAX.into()));
        trapped(m.copy(dest, source, 9, 1), MemoryTrap::OutOfBounds);
        assert_eq!(m.load(dest, &Type::U64, 8).unwrap().scalar(), &ConstValue::U(u64::MAX.into()));
        let before = m.allocations[2].bytes.clone();
        m.limits.max_work = m.work;
        assert!(matches!(
            m.set(dest, 0, 8, 8),
            Err(Fault::Limit { resource: MemoryResource::Work, .. })
        ));
        assert_eq!(before, m.allocations[2].bytes);
    }

    #[test]
    fn negative_gep_can_return_from_one_past_and_uninit_keeps_physical_bytes() {
        let mut m = Memory::new(MemoryLimits::default());
        let p = m.allocate(8, 8, 8).unwrap();
        let end = advance(&m, p, 8);
        assert_eq!(advance(&m, end, -8), p);
        m.set(p, 42, 8, 1).unwrap();
        m.uninit(p, 8, 1).unwrap();
        assert_eq!(m.allocations[0].bytes, vec![42; 8]);
        trapped(m.load(p, &Type::U8, 1), MemoryTrap::Uninitialized { offset: 0 });
    }
}

#[cfg(test)]
mod generated;
