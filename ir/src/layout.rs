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
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
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
pub fn layout_of_with_limit(
    ty: &Type,
    target: Target,
    max_depth: usize,
) -> Result<TypeLayout, LayoutError> {
    let scalar = |size, align| TypeLayout {
        size,
        align,
        field_offsets: Vec::new(),
        element_stride: None,
    };
    // A new target must select its own rules rather than inheriting host layout.
    match target {
        Target::X86_64WhaleLinux => {}
    }
    Ok(match ty {
        Type::Void => return Err(LayoutError::Void),
        Type::Bool | Type::I1 | Type::U1 | Type::I8 | Type::U8 => scalar(1, 1),
        Type::I16 | Type::U16 | Type::F16 => scalar(2, 2),
        Type::I32 | Type::U32 | Type::F32 => scalar(4, 4),
        Type::I64 | Type::U64 | Type::F64 => scalar(8, 8),
        Type::I128 | Type::U128 => scalar(16, 16),
        Type::Ptr(_) | Type::FnPtr(_) => {
            let bytes = target.data_layout().ptr_bits / 8;
            scalar(u64::from(bytes), bytes)
        }
        Type::Array(element, count) => {
            let remaining = max_depth.checked_sub(1).ok_or(LayoutError::DepthLimit)?;
            let element = layout_of_with_limit(element, target, remaining)?;
            TypeLayout {
                size: element
                    .size
                    .checked_mul(*count)
                    .ok_or(LayoutError::Overflow)?,
                align: element.align,
                field_offsets: Vec::new(),
                element_stride: Some(element.size),
            }
        }
        Type::Struct(fields) | Type::Tuple(fields) => {
            let remaining = max_depth.checked_sub(1).ok_or(LayoutError::DepthLimit)?;
            let mut result = scalar(0, 1);
            for field in fields {
                let field = layout_of_with_limit(field, target, remaining)?;
                result.align = result.align.max(field.align);
                result.size = align_up(result.size, field.align)?;
                result.field_offsets.push(result.size);
                result.size = result
                    .size
                    .checked_add(field.size)
                    .ok_or(LayoutError::Overflow)?;
            }
            result.size = align_up(result.size, result.align)?;
            result
        }
    })
}

fn align_up(size: u64, align: u32) -> Result<u64, LayoutError> {
    let mask = u64::from(align) - 1;
    let padding = size.wrapping_neg() & mask;
    size.checked_add(padding).ok_or(LayoutError::Overflow)
}

/// Alignment for a standalone local/global object, not a field or array element.
/// SysV AMD64 gives arrays of at least 16 bytes at least 16-byte placement alignment.
pub fn allocation_align(ty: &Type, target: Target) -> Result<u32, LayoutError> {
    let layout = layout_of(ty, target)?;
    Ok(if matches!(ty, Type::Array(..)) && layout.size >= 16 {
        layout.align.max(16)
    } else {
        layout.align
    })
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
