// SPDX-License-Identifier: MPL-2.0

//! Storage layout for an explicit output target; no host `size_of`/`align_of`.
use crate::{Target, Type};

use std::fmt;

/// Natural type layout in bytes. Tail padding is included in `size`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeLayout {
    pub size: u64,
    pub align: u32,
    /// Struct/tuple field offsets in declaration order; empty for other types.
    pub field_offsets: Vec<u64>,
    /// Distance between array elements, including each element's tail padding.
    pub element_stride: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    Void,
    Overflow,
    DepthLimit,
    ZeroSizedPointerArithmetic,
    InvalidTarget,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTarget => "invalid target storage layout",
            Self::Void => "void has no storage layout",
            Self::Overflow => "type layout exceeds the target address space",
            Self::DepthLimit => "type layout nesting limit exceeded",
            Self::ZeroSizedPointerArithmetic => "zero-sized pointee arithmetic is unsupported",
        })
    }
}

impl std::error::Error for LayoutError {}

/// Compute natural storage layout, with at most 128 nested aggregate levels.
pub fn layout_of(ty: &Type, target: Target) -> Result<TypeLayout, LayoutError> {
    layout_of_with_limit(ty, target, 128)
}

/// Override the aggregate nesting budget. Pointers do not lay out their pointee.
/// Requests above `MAX_IR_NESTING` are clamped to that stack-safe ceiling.
pub fn layout_of_with_limit(
    ty: &Type,
    target: Target,
    max_depth: usize,
) -> Result<TypeLayout, LayoutError> {
    layout_for_target(ty, &target.specification(), max_depth)
}

/// Layout for an explicitly supplied target, including out-of-tree targets.
pub fn layout_for_target(
    ty: &Type,
    target: &target_spec::TargetSpec,
    max_depth: usize,
) -> Result<TypeLayout, LayoutError> {
    target.validate().map_err(|_| LayoutError::InvalidTarget)?;
    #[cfg(whale_wave_ir)]
    return crate::wave_layout::layout(ty, &target.storage, max_depth).map(|(layout, _)| layout);
    #[cfg(not(whale_wave_ir))]
    layout_storage(ty, &target.storage, max_depth.min(crate::MAX_IR_NESTING))
}

#[cfg(any(not(whale_wave_ir), test))]
fn layout_storage(
    ty: &Type,
    storage: &target_spec::StorageLayout,
    max_depth: usize,
) -> Result<TypeLayout, LayoutError> {
    let scalar =
        |size, align| TypeLayout { size, align, field_offsets: Vec::new(), element_stride: None };
    let scalar_for = |size: u64, index: usize| scalar(size, storage.scalar_align[index]);
    let result = match ty {
        Type::Void => return Err(LayoutError::Void),
        Type::Bool | Type::I1 | Type::U1 | Type::I8 | Type::U8 => scalar_for(1, 0),
        Type::I16 | Type::U16 | Type::F16 => scalar_for(2, 1),
        Type::I32 | Type::U32 | Type::F32 => scalar_for(4, 2),
        Type::I64 | Type::U64 | Type::F64 => scalar_for(8, 3),
        Type::I128 | Type::U128 => scalar_for(16, 4),
        Type::Ptr(_) | Type::FnPtr(_) => {
            let bytes = storage.data.ptr_bits / 8;
            scalar(u64::from(bytes), storage.pointer_align)
        },
        Type::Array(element, count) => {
            let remaining = max_depth.checked_sub(1).ok_or(LayoutError::DepthLimit)?;
            let element = layout_storage(element, storage, remaining)?;
            TypeLayout {
                size: element.size.checked_mul(*count).ok_or(LayoutError::Overflow)?,
                align: element.align,
                field_offsets: Vec::new(),
                element_stride: Some(element.size),
            }
        },
        Type::Struct(fields) | Type::Tuple(fields) => {
            let remaining = max_depth.checked_sub(1).ok_or(LayoutError::DepthLimit)?;
            let mut result = scalar(0, 1);
            for field in fields {
                let field = layout_storage(field, storage, remaining)?;
                result.align = result.align.max(field.align);
                result.size = align_up(result.size, field.align)?;
                result.field_offsets.push(result.size);
                result.size = result.size.checked_add(field.size).ok_or(LayoutError::Overflow)?;
            }
            result.size = align_up(result.size, result.align)?;
            result
        },
    };
    let max_size = if storage.data.ptr_bits == 32 { u64::from(u32::MAX) } else { u64::MAX };
    if result.size > max_size {
        return Err(LayoutError::Overflow);
    }
    Ok(result)
}

#[cfg(any(not(whale_wave_ir), test))]
fn align_up(size: u64, align: u32) -> Result<u64, LayoutError> {
    let mask = u64::from(align) - 1;
    let padding = size.wrapping_neg() & mask;
    size.checked_add(padding).ok_or(LayoutError::Overflow)
}

/// Alignment for a standalone local/global object, not a field or array element.
/// SysV AMD64 gives arrays of at least 16 bytes at least 16-byte placement alignment.
pub fn allocation_align(ty: &Type, target: Target) -> Result<u32, LayoutError> {
    allocation_align_for_target(ty, &target.specification())
}

pub fn allocation_align_for_target(
    ty: &Type,
    target: &target_spec::TargetSpec,
) -> Result<u32, LayoutError> {
    #[cfg(whale_wave_ir)]
    {
        target.validate().map_err(|_| LayoutError::InvalidTarget)?;
        crate::wave_layout::layout(ty, &target.storage, 128).map(|(_, align)| align)
    }
    #[cfg(not(whale_wave_ir))]
    {
        let layout = layout_for_target(ty, target, 128)?;
        Ok(match target.storage.large_array {
            Some((threshold, align))
                if matches!(ty, Type::Array(..)) && layout.size >= threshold =>
            {
                layout.align.max(align)
            },
            _ => layout.align,
        })
    }
}

/// Scale for the first native GEP index; a zero-sized pointee cannot be stepped.
pub fn pointer_stride(pointee: &Type, target: Target) -> Result<u64, LayoutError> {
    let size = layout_of(pointee, target)?.size;
    if size == 0 {
        Err(LayoutError::ZeroSizedPointerArithmetic)
    } else {
        Ok(size)
    }
}

#[cfg(all(test, whale_wave_ir))]
mod wave_reference_tests {
    use super::*;

    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn ty(state: &mut u64, depth: u8) -> Type {
        let choice = if depth == 0 { next(state) % 7 } else { next(state) % 12 };
        match choice {
            0 => Type::Void,
            1 => Type::U8,
            2 => Type::I16,
            3 => Type::F32,
            4 => Type::U64,
            5 => Type::I128,
            6 => Type::Ptr(Box::new(Type::Void)),
            7 => Type::Array(Box::new(ty(state, depth - 1)), next(state) % 8),
            8 => Type::Array(Box::new(ty(state, depth - 1)), u64::MAX),
            9 | 10 => {
                let count = next(state) % 4;
                let fields = (0..count).map(|_| ty(state, depth - 1)).collect();
                Type::Struct(fields)
            },
            _ => Type::Tuple(vec![ty(state, depth - 1), ty(state, depth - 1)]),
        }
    }

    #[test]
    fn wave_layout_agrees_with_reference_on_nested_types_and_limits() {
        let target = Target::X86_64WhaleLinux.specification();
        let mut state = 0x149_350_256_u64;
        for _ in 0..4000 {
            let ty = ty(&mut state, 5);
            let depth = (next(&mut state) % 8) as usize;
            let reference = layout_storage(&ty, &target.storage, depth);
            let wave = layout_for_target(&ty, &target, depth);
            assert_eq!(wave, reference, "type {ty:?}, depth {depth}");
        }
        let mut deep = Type::U8;
        for _ in 0..20_000 {
            deep = Type::Array(Box::new(deep), 1);
        }
        let first_error = Box::leak(Box::new(Type::Struct(vec![Type::Void, deep])));
        assert_eq!(layout_for_target(first_error, &target, 256), Err(LayoutError::Void));
    }
}
