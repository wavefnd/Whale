// SPDX-License-Identifier: MPL-2.0
//! Borrowed source metadata transport; Wave validates spans and identities.
use crate::{Module, SourceMetadata, SourceMetadataError, SourceTarget};

#[repr(C)]
struct File {
    id: u64,
    bytes: *const u8,
    length: u64,
}

#[repr(C)]
#[derive(Default)]
struct Original {
    id: u64,
    has_span: u64,
    file: u64,
    line: u64,
    column: u64,
    end_line: u64,
    end_column: u64,
}

#[repr(C)]
struct Block {
    function: u64,
    block: u64,
    instructions: u64,
    terminated: u64,
}

#[repr(C)]
struct Mapping {
    kind: u64,
    function: u64,
    block: u64,
    index: u64,
    original: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Slot {
    kind: u64,
    a: u64,
    b: u64,
    c: u64,
    value: u64,
    extra: u64,
}

#[repr(C)]
#[derive(Default)]
struct RawResult {
    status: u64,
    index: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_verify_sources(
        files: *const File,
        file_count: u64,
        originals: *const Original,
        original_count: u64,
        blocks: *const Block,
        block_count: u64,
        globals: *const u64,
        global_count: u64,
        mappings: *const Mapping,
        mapping_count: u64,
        table: *mut Slot,
        capacity: u64,
        max_work: u64,
        result: *mut RawResult,
    ) -> i32;
}

pub(crate) enum ValidationError {
    Invalid(SourceMetadataError),
    Work,
}

pub(crate) fn validate(
    metadata: &SourceMetadata,
    module: &Module,
    max_work: usize,
) -> Result<usize, ValidationError> {
    if metadata.files.is_empty() && metadata.originals.is_empty() && metadata.mappings.is_empty() {
        return Ok(0);
    }
    let block_count = module
        .functions
        .iter()
        .try_fold(0usize, |n, f| n.checked_add(f.blocks.len()))
        .ok_or(ValidationError::Work)?;
    let count = [
        metadata.files.len(),
        metadata.originals.len(),
        metadata.mappings.len(),
        block_count,
        module.globals.len(),
        module.global_storage.len(),
    ]
    .into_iter()
    .try_fold(0usize, usize::checked_add)
    .ok_or(ValidationError::Work)?;
    let capacity = count
        .max(1)
        .checked_mul(2)
        .and_then(usize::checked_next_power_of_two)
        .ok_or(ValidationError::Work)?;
    // The Wave initialization and every subsequent probe consume this budget.
    if capacity > max_work {
        return Err(ValidationError::Work);
    }
    let mut files = Vec::new();
    files.try_reserve_exact(metadata.files.len()).map_err(|_| ValidationError::Work)?;
    for file in &metadata.files {
        files.push(File {
            id: file.id.into(),
            bytes: file.name.as_ptr(),
            length: file.name.len() as u64,
        });
    }
    let mut originals = Vec::new();
    originals.try_reserve_exact(metadata.originals.len()).map_err(|_| ValidationError::Work)?;
    for node in &metadata.originals {
        let mut raw = Original { id: node.id, ..Original::default() };
        if let Some(span) = &node.span {
            raw.has_span = 1;
            raw.file = span.file.into();
            raw.line = span.line.into();
            raw.column = span.column.into();
            raw.end_line = span.end_line.into();
            raw.end_column = span.end_column.into();
        }
        originals.push(raw);
    }
    let mut blocks = Vec::new();
    blocks.try_reserve_exact(block_count).map_err(|_| ValidationError::Work)?;
    for f in &module.functions {
        for b in &f.blocks {
            blocks.push(Block {
                function: f.id.0.into(),
                block: b.id.0.into(),
                instructions: b.instructions.len() as u64,
                terminated: u64::from(b.terminator.is_some()),
            });
        }
    }
    let mut globals = Vec::new();
    globals
        .try_reserve_exact(module.globals.len() + module.global_storage.len())
        .map_err(|_| ValidationError::Work)?;
    globals.extend(module.globals.iter().map(|g| u64::from(g.id.0)));
    globals.extend(module.global_storage.iter().map(|g| u64::from(g.id.0)));
    let mut mappings = Vec::new();
    mappings.try_reserve_exact(metadata.mappings.len()).map_err(|_| ValidationError::Work)?;
    for mapping in &metadata.mappings {
        let (kind, function, block, index) = match mapping.target {
            SourceTarget::Instruction { function, block, index } => {
                (0, u64::from(function.0), u64::from(block.0), index as u64)
            },
            SourceTarget::Terminator { function, block } => {
                (1, u64::from(function.0), u64::from(block.0), 0)
            },
            SourceTarget::Global(id) => (2, u64::from(id.0), 0, 0),
        };
        mappings.push(Mapping { kind, function, block, index, original: mapping.original });
    }
    let mut table = Vec::new();
    table.try_reserve_exact(capacity).map_err(|_| ValidationError::Work)?;
    table.resize(capacity, Slot::default());
    let mut result = RawResult::default();
    let status = unsafe {
        whale_ir_verify_sources(
            files.as_ptr(),
            files.len() as u64,
            originals.as_ptr(),
            originals.len() as u64,
            blocks.as_ptr(),
            blocks.len() as u64,
            globals.as_ptr(),
            globals.len() as u64,
            mappings.as_ptr(),
            mappings.len() as u64,
            table.as_mut_ptr(),
            capacity as u64,
            max_work as u64,
            &mut result,
        )
    };
    let invalid = |s| ValidationError::Invalid(SourceMetadataError(s));
    if status != result.status as i32 || result.work > max_work as u64 {
        return Err(invalid("invalid Wave source metadata result"));
    }
    match status {
        0 => Ok(result.work as usize),
        1 => Err(invalid("invalid or duplicate source file")),
        2 => Err(invalid("duplicate original node identity")),
        3 => Err(invalid("invalid original source span")),
        4 => Err(invalid("unknown original node or duplicate source mapping")),
        5 => Err(invalid("source mapping targets a missing IR operation")),
        7 => Err(ValidationError::Work),
        _ => Err(invalid("invalid Wave source metadata result")),
    }
}
