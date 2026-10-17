//! Minimal static Linux AMD64 ET_EXEC emission. No loader, libc or startup is added.
use super::{
    layout::{Layout, SectionPlacement},
    relocation::apply_relocations,
    symbol_table::{SymbolKey, SymbolTable},
};

use object::{ObjectFile, SectionKind};

const BASE: u64 = 0x400000;

const PAGE: u64 = 4096;

// Portable Linux AMD64 lower 48-bit user range, excluding its final guard page.
// Do not rely on optional LA57 hardware/kernel support for emitted executables.
const USER_END: u64 = (1 << 47) - PAGE;

#[derive(Debug, Clone)]
pub struct Segment {
    pub flags: u32,
    pub offset: u64,
    pub address: u64,
    pub file_size: u64,
    pub memory_size: u64,
}

fn align(value: u64, alignment: u64) -> Result<u64, String> {
    value
        .checked_add(value.wrapping_neg() & (alignment - 1))
        .ok_or_else(|| "executable alignment overflow".into())
}

/// The payload Layout API remains useful independently. Executable placement also
/// accounts for headers and page permissions, and puts NOBITS last in the RW load.
pub fn executable_layout(
    objects: &[ObjectFile],
    max_bytes: u64,
) -> Result<(Layout, Vec<Segment>), String> {
    for object in objects {
        object.target.validate_amd64().map_err(|e| e.to_string())?;
        object.validate()?;
    }
    let has_ro =
        objects.iter().any(|o| o.sections.iter().any(|s| s.kind == SectionKind::ReadOnlyData));
    let has_rw = objects
        .iter()
        .any(|o| o.sections.iter().any(|s| matches!(s.kind, SectionKind::Data | SectionKind::Bss)));
    let header_size = 64 + 56 * (2 + u64::from(has_ro) + u64::from(has_rw)); // PT_LOADs plus GNU_STACK
    let mut layout = Layout {
        section_offsets: objects.iter().map(|o| vec![0; o.sections.len()]).collect(),
        section_sizes: Vec::new(),
        sections: Vec::new(),
        base_address: BASE,
        file_size: header_size,
        memory_size: header_size,
    };
    let mut segments = Vec::new();
    for (kinds, flags) in [
        (&[SectionKind::Text][..], 5),
        (&[SectionKind::ReadOnlyData][..], 4),
        (&[SectionKind::Data, SectionKind::Bss][..], 6),
    ] {
        let exists = flags == 5
            || objects.iter().any(|o| o.sections.iter().any(|s| kinds.contains(&s.kind)));
        if !exists {
            continue;
        }

        let start = if flags == 5 { 0 } else { align(layout.file_size, PAGE)? };
        let mut cursor = if flags == 5 { header_size } else { start };
        let mut file_end = cursor;

        // BSS follows every initialized RW section, including sections in later inputs.
        for kind in kinds {
            for (oi, object) in objects.iter().enumerate() {
                for (si, section) in object.sections.iter().enumerate() {
                    if section.kind != *kind {
                        continue;
                    }
                    let va = align(
                        BASE.checked_add(cursor).ok_or("executable address overflow")?,
                        section.align.max(1),
                    )?;
                    cursor = va - BASE;
                    let size = section.memory_size()?;
                    layout.section_offsets[oi][si] = va;
                    layout.sections.push(SectionPlacement {
                        object_index: oi,
                        section_index: si,
                        kind: *kind,
                        align: section.align.max(1),
                        file_offset: cursor,
                        memory_address: va,
                        file_size: section.file_size(),
                        memory_size: size,
                    });
                    cursor = cursor.checked_add(size).ok_or("executable memory size overflow")?;
                    if *kind != SectionKind::Bss {
                        file_end = cursor;
                    }
                }
            }
        }

        if file_end > max_bytes {
            return Err("executable output byte limit exceeded".into());
        }

        let memory_end = BASE.checked_add(cursor).ok_or("executable virtual address overflow")?;
        if memory_end > USER_END {
            return Err("executable exceeds portable lower 48-bit user address space".into());
        }

        segments.push(Segment {
            flags,
            offset: start,
            address: BASE + start,
            file_size: file_end - start,
            memory_size: cursor - start,
        });
        layout.file_size = file_end;
        layout.memory_size = cursor;
    }

    layout.section_sizes = objects
        .iter()
        .flat_map(|o| o.sections.iter())
        .map(|s| s.memory_size())
        .collect::<Result<_, _>>()?;
    Ok((layout, segments))
}

pub fn write_executable(
    objects: &[ObjectFile],
    entry: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    write_executable_checked(objects, entry, max_bytes).map_err(|e| e.to_string())
}

pub fn write_executable_checked(
    objects: &[ObjectFile],
    entry: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, crate::LinkError> {
    use crate::{LinkError, LinkErrorKind as K};
    if entry.is_empty() {
        return Err(LinkError::new(K::Entry, "entry symbol must not be empty"));
    }
    let mut symbols = SymbolTable::new();
    symbols.resolve(objects).map_err(|e| LinkError::new(K::Symbols, e))?;
    let selected = symbols.symbols.get(&SymbolKey::Global(entry.into())).ok_or_else(|| {
        LinkError::new(
            K::Entry,
            format!("entry symbol {entry:?} is missing or not globally defined"),
        )
    })?;
    let (owner, section) = selected
        .object_index
        .zip(selected.section_index)
        .ok_or_else(|| LinkError::new(K::Entry, "entry symbol is undefined"))?;
    let source = &objects[owner].sections[section];
    if source.kind != SectionKind::Text || selected.value >= source.data.len() as u64 {
        return Err(LinkError::new(K::Entry, "entry must point inside an executable section"));
    }
    let (layout, segments) =
        executable_layout(objects, max_bytes).map_err(|e| LinkError::new(K::Layout, e))?;
    let address = layout.section_offsets[owner][section]
        .checked_add(selected.value)
        .ok_or_else(|| LinkError::new(K::Entry, "entry address overflow"))?;
    let payloads = apply_relocations(objects, &layout, max_bytes).map_err(|e| {
        let mut result = LinkError::new(K::Relocation, e.to_string());
        result.contexts = diagnostic::ToDiagnostic::diagnostic(&e).contexts;
        result.relocation_cause = Some(Box::new(e));
        result
    })?;
    let size = usize::try_from(layout.file_size)
        .map_err(|_| LinkError::new(K::Emission, "executable exceeds host capacity"))?;
    let mut out = vec![0; size];
    out[..16].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    put16(&mut out, 16, 2);
    put16(&mut out, 18, 62);
    put32(&mut out, 20, 1);
    put64(&mut out, 24, address);
    put64(&mut out, 32, 64);
    put16(&mut out, 52, 64);
    put16(&mut out, 54, 56);
    put16(&mut out, 56, (segments.len() + 1) as u16);
    for (i, s) in segments.iter().enumerate() {
        let at = 64 + i * 56;
        put32(&mut out, at, 1);
        put32(&mut out, at + 4, s.flags);
        put64(&mut out, at + 8, s.offset);
        put64(&mut out, at + 16, s.address);
        put64(&mut out, at + 24, s.address);
        put64(&mut out, at + 32, s.file_size);
        put64(&mut out, at + 40, s.memory_size);
        put64(&mut out, at + 48, PAGE);
    }
    let stack = 64 + segments.len() * 56;
    put32(&mut out, stack, 0x6474e551);
    put32(&mut out, stack + 4, 6);
    put64(&mut out, stack + 48, 16);
    for p in &layout.sections {
        if p.kind == SectionKind::Bss {
            continue;
        }
        let start = usize::try_from(p.file_offset)
            .map_err(|_| LinkError::new(K::Emission, "section offset exceeds host capacity"))?;
        let payload = &payloads[p.object_index][p.section_index];
        out[start..start + payload.len()].copy_from_slice(payload);
    }
    Ok(out)
}

fn put16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(out: &mut [u8], at: usize, value: u64) {
    out[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
