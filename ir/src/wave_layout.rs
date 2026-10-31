// SPDX-License-Identifier: MPL-2.0
//! Internal borrowed data bridge for the Wave IR layout kernel.

use crate::{LayoutError, Type, TypeLayout};

use target_spec::StorageLayout;

const MAX_NODES: usize = 1_000_000;

const MAX_EDGES: usize = 1_000_000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Node {
    kind: u64,
    child: u64,
    first: u64,
    count: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Storage {
    ptr_bits: u64,
    pointer_align: u64,
    scalar_align1: u64,
    scalar_align2: u64,
    scalar_align4: u64,
    scalar_align8: u64,
    scalar_align16: u64,
    large_array_threshold: u64,
    large_array_align: u64,
    has_large_array: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Layout {
    size: u64,
    align: u64,
    stride: u64,
    allocation_align: u64,
    depth: u64,
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    index: u64,
}

unsafe extern "C" {
    fn whale_ir_layout(
        storage: *const Storage,
        nodes: *const Node,
        node_count: u64,
        edges: *const u64,
        edge_count: u64,
        root: u64,
        max_depth: u64,
        max_work: u64,
        scratch: *mut Layout,
        scratch_count: u64,
        scratch_offsets: *mut u64,
        scratch_offset_count: u64,
        output: *mut Layout,
        output_count: u64,
        output_offsets: *mut u64,
        output_offset_count: u64,
        result: *mut ResultRecord,
    ) -> i32;
}

fn add_type(ty: &Type, nodes: &mut Vec<Node>, edges: &mut Vec<u64>) -> Result<u64, LayoutError> {
    enum Task<'a> {
        Visit(&'a Type),
        Array(u64),
        Aggregate { kind: u64, count: usize },
    }
    let mut pending = vec![Task::Visit(ty)];
    let mut values = Vec::<u64>::new();
    while let Some(task) = pending.pop() {
        let node = match task {
            Task::Visit(Type::Array(child, count)) => {
                pending.try_reserve(2).map_err(|_| LayoutError::Overflow)?;
                pending.push(Task::Array(*count));
                pending.push(Task::Visit(child));
                continue;
            },
            Task::Visit(Type::Struct(fields)) => {
                if fields.len() > MAX_EDGES.saturating_sub(edges.len()) {
                    return Err(LayoutError::Overflow);
                }
                pending.try_reserve(fields.len() + 1).map_err(|_| LayoutError::Overflow)?;
                pending.push(Task::Aggregate { kind: 9, count: fields.len() });
                pending.extend(fields.iter().rev().map(Task::Visit));
                continue;
            },
            Task::Visit(Type::Tuple(fields)) => {
                if fields.len() > MAX_EDGES.saturating_sub(edges.len()) {
                    return Err(LayoutError::Overflow);
                }
                pending.try_reserve(fields.len() + 1).map_err(|_| LayoutError::Overflow)?;
                pending.push(Task::Aggregate { kind: 10, count: fields.len() });
                pending.extend(fields.iter().rev().map(Task::Visit));
                continue;
            },
            Task::Visit(leaf) => {
                let kind = match leaf {
                    Type::Void => 0,
                    Type::Bool | Type::I1 | Type::U1 | Type::I8 | Type::U8 => 1,
                    Type::I16 | Type::U16 | Type::F16 => 2,
                    Type::I32 | Type::U32 | Type::F32 => 3,
                    Type::I64 | Type::U64 | Type::F64 => 4,
                    Type::I128 | Type::U128 => 5,
                    Type::Ptr(_) => 6,
                    Type::FnPtr(_) => 7,
                    Type::Array(..) | Type::Struct(_) | Type::Tuple(_) => unreachable!(),
                };
                Node { kind, ..Node::default() }
            },
            Task::Array(count) => Node {
                kind: 8,
                child: values.pop().expect("array child was traversed"),
                count,
                ..Node::default()
            },
            Task::Aggregate { kind, count } => {
                let first_value = values.len() - count;
                if count > MAX_EDGES.saturating_sub(edges.len()) {
                    return Err(LayoutError::Overflow);
                }
                edges.try_reserve(count).map_err(|_| LayoutError::Overflow)?;
                let first = u64::try_from(edges.len()).map_err(|_| LayoutError::Overflow)?;
                edges.extend_from_slice(&values[first_value..]);
                values.truncate(first_value);
                Node { kind, first, count: count as u64, ..Node::default() }
            },
        };
        if nodes.len() == MAX_NODES {
            return Err(LayoutError::Overflow);
        }
        nodes.try_reserve(1).map_err(|_| LayoutError::Overflow)?;
        values.try_reserve(1).map_err(|_| LayoutError::Overflow)?;
        let index = u64::try_from(nodes.len()).map_err(|_| LayoutError::Overflow)?;
        let mut node = node;
        if node.kind == 6 || node.kind == 7 {
            // Pointees and function signatures do not participate in storage layout.
            node.child = index;
        }
        nodes.push(node);
        values.push(index);
    }
    Ok(values.pop().expect("root type was traversed"))
}

pub(super) fn layout(
    ty: &Type,
    storage: &StorageLayout,
    max_depth: usize,
) -> Result<(TypeLayout, u32), LayoutError> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let depth = max_depth.min(crate::MAX_IR_NESTING);
    let root = add_type(ty, &mut nodes, &mut edges)?;
    let (threshold, large_align, has_large_array) = match storage.large_array {
        Some((threshold, align)) => (threshold, u64::from(align), 1),
        None => (0, 0, 0),
    };
    let input = Storage {
        ptr_bits: u64::from(storage.data.ptr_bits),
        pointer_align: u64::from(storage.pointer_align),
        scalar_align1: u64::from(storage.scalar_align[0]),
        scalar_align2: u64::from(storage.scalar_align[1]),
        scalar_align4: u64::from(storage.scalar_align[2]),
        scalar_align8: u64::from(storage.scalar_align[3]),
        scalar_align16: u64::from(storage.scalar_align[4]),
        large_array_threshold: threshold,
        large_array_align: large_align,
        has_large_array,
    };
    let mut scratch = vec![Layout::default(); nodes.len()];
    let mut output = vec![Layout::default(); nodes.len()];
    let mut scratch_offsets = vec![0; edges.len()];
    let mut output_offsets = vec![0; edges.len()];
    let mut result = ResultRecord::default();
    let node_count = u64::try_from(nodes.len()).map_err(|_| LayoutError::Overflow)?;
    let edge_count = u64::try_from(edges.len()).map_err(|_| LayoutError::Overflow)?;
    let work = node_count
        .checked_mul(3)
        .and_then(|n| edge_count.checked_mul(3).and_then(|e| n.checked_add(e)))
        .ok_or(LayoutError::Overflow)?;
    // The Wave kernel validates every span and writes only caller-owned scratch
    // until all type, target, depth, work and output checks have succeeded.
    let status = unsafe {
        whale_ir_layout(
            &input,
            nodes.as_ptr(),
            node_count,
            edges.as_ptr(),
            edge_count,
            root,
            u64::try_from(depth).map_err(|_| LayoutError::DepthLimit)?,
            work,
            scratch.as_mut_ptr(),
            node_count,
            scratch_offsets.as_mut_ptr(),
            edge_count,
            output.as_mut_ptr(),
            node_count,
            output_offsets.as_mut_ptr(),
            edge_count,
            &mut result,
        )
    };
    if status != 0 {
        return Err(match status {
            1 => LayoutError::Void,
            2 => LayoutError::DepthLimit,
            3 | 4 | 6 => LayoutError::Overflow,
            _ => LayoutError::InvalidTarget,
        });
    }
    let root = usize::try_from(root).map_err(|_| LayoutError::Overflow)?;
    let layout = output[root];
    let root_node = nodes[root];
    let field_offsets = if root_node.kind == 9 || root_node.kind == 10 {
        let first = usize::try_from(root_node.first).map_err(|_| LayoutError::Overflow)?;
        let count = usize::try_from(root_node.count).map_err(|_| LayoutError::Overflow)?;
        output_offsets[first..first + count].to_vec()
    } else {
        Vec::new()
    };
    let natural = TypeLayout {
        size: layout.size,
        align: u32::try_from(layout.align).map_err(|_| LayoutError::InvalidTarget)?,
        field_offsets,
        element_stride: (root_node.kind == 8).then_some(layout.stride),
    };
    let allocation_align =
        u32::try_from(layout.allocation_align).map_err(|_| LayoutError::InvalidTarget)?;
    Ok((natural, allocation_align))
}
