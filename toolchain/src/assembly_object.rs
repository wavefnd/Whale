// SPDX-License-Identifier: MPL-2.0
//! Host-side data transport for the complete Wave `assembler/asmobj` module.
//! No relocation selection or symbol-resolution policy belongs in this file.
use assembler::AssemblerOutput;

use object::{ObjectFile, ObjectTarget};

/// Whether this product links the Wave assembler-to-object module.
pub const fn available() -> bool {
    cfg!(whale_asmobj)
}

pub fn write(input: &AssemblerOutput, target: ObjectTarget) -> Result<Vec<u8>, String> {
    to_object(input, target)?.write()
}

pub fn to_object(input: &AssemblerOutput, target: ObjectTarget) -> Result<ObjectFile, String> {
    #[cfg(whale_asmobj)]
    {
        native::convert(input, target)
    }
    #[cfg(not(whale_asmobj))]
    {
        let _ = (input, target);
        Err("asmobj module is unavailable; build with Meson or set WHALE_ASMOBJ_DIR to its built archive".into())
    }
}

#[cfg(whale_asmobj)]
mod native {
    use super::*;

    use assembler::assembler::RelocKind as AReloc;

    use assembler::{AsmSectionKind, SymbolKind, SymbolVisibility};

    use object::{
        Machine, ObjectFormat, ObjectRelocation, ObjectSymbol, RelocKind, Section, SectionKind,
        SymbolBinding,
    };

    use std::collections::HashMap;

    #[repr(C)]
    struct Target {
        format: u64,
        machine: u64,
        address_bits: u64,
        endian: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SectionRow {
        name: *const u8,
        name_len: u64,
        kind: u64,
        align: u64,
        data: *const u8,
        data_len: u64,
        zero_fill: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SymbolRow {
        name: *const u8,
        name_len: u64,
        kind: u64,
        section: u64,
        offset: u64,
        size: u64,
        binding: u64,
        visibility: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RelocRow {
        section: u64,
        offset: u64,
        name: *const u8,
        name_len: u64,
        kind: u64,
        namespace: *const u8,
        namespace_len: u64,
        code: u64,
        addend: i64,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ResultRow {
        status: u64,
        index: u64,
        symbol_count: u64,
    }

    unsafe extern "C" {
        fn whale_asmobj(
            target: *const Target,
            sections: *const SectionRow,
            section_count: u64,
            symbols: *const SymbolRow,
            symbol_count: u64,
            relocations: *const RelocRow,
            relocation_count: u64,
            out_sections: *mut SectionRow,
            section_capacity: u64,
            out_symbols: *mut SymbolRow,
            symbol_capacity: u64,
            out_relocations: *mut RelocRow,
            relocation_capacity: u64,
            scratch: *mut u64,
            scratch_words: u64,
            result: *mut ResultRow,
        ) -> i32;
    }

    fn invalid() -> String {
        "invalid output from asmobj module".into()
    }

    fn allocate<T: Clone>(length: usize, value: T) -> Result<Vec<T>, String> {
        let mut items = Vec::new();
        items.try_reserve_exact(length).map_err(|_| "asmobj allocation failed")?;
        items.resize(length, value);
        Ok(items)
    }

    fn host(value: u64) -> Result<usize, String> {
        usize::try_from(value).map_err(|_| invalid())
    }

    fn name<'a>(
        table: &'a HashMap<(*const u8, u64), &str>,
        pointer: *const u8,
        length: u64,
    ) -> Result<&'a str, String> {
        table.get(&(pointer, length)).copied().ok_or_else(invalid)
    }

    pub(super) fn convert(
        input: &AssemblerOutput,
        target: ObjectTarget,
    ) -> Result<ObjectFile, String> {
        let ns = input.sections.len();
        let ny = input.symbols.len();
        let nr = input
            .sections
            .iter()
            .try_fold(0usize, |n, s| n.checked_add(s.relocs.len()))
            .ok_or("asmobj input limit exceeded")?;
        let total = ns
            .checked_add(ny)
            .and_then(|n| n.checked_add(nr))
            .ok_or("asmobj input limit exceeded")?;
        if total > 262_144 {
            return Err("asmobj input limit exceeded".into());
        }
        let symbols_capacity = ny + nr;
        let scratch_words =
            if symbols_capacity == 0 { 0 } else { symbols_capacity.next_power_of_two() * 8 };
        let mut names = HashMap::new();
        names.try_reserve(total).map_err(|_| "asmobj allocation failed")?;
        let mut sections = Vec::new();
        sections.try_reserve_exact(ns).map_err(|_| "asmobj allocation failed")?;
        let mut symbols = Vec::new();
        symbols.try_reserve_exact(ny).map_err(|_| "asmobj allocation failed")?;
        let mut relocs = Vec::new();
        relocs.try_reserve_exact(nr).map_err(|_| "asmobj allocation failed")?;
        for section in &input.sections {
            names.insert((section.name.as_ptr(), section.name.len() as u64), section.name.as_str());
            sections.push(SectionRow {
                name: section.name.as_ptr(),
                name_len: section.name.len() as u64,
                kind: match section.kind {
                    AsmSectionKind::Text => 0,
                    AsmSectionKind::Data => 1,
                    AsmSectionKind::ReadOnlyData => 2,
                    AsmSectionKind::Bss => 3,
                },
                align: section.align,
                data: section.data.as_ptr(),
                data_len: section.data.len() as u64,
                zero_fill: section.zero_fill,
            });
        }
        for symbol in &input.symbols {
            names.insert((symbol.name.as_ptr(), symbol.name.len() as u64), symbol.name.as_str());
            symbols.push(SymbolRow {
                name: symbol.name.as_ptr(),
                name_len: symbol.name.len() as u64,
                kind: match symbol.kind {
                    SymbolKind::NoType => 0,
                    SymbolKind::Function => 1,
                    SymbolKind::Object => 2,
                },
                section: symbol.section_index.map_or(u64::MAX, |i| i as u64),
                offset: symbol.offset,
                size: symbol.size,
                binding: u64::from(symbol.is_global),
                visibility: match symbol.visibility {
                    SymbolVisibility::Default => 0,
                    SymbolVisibility::Hidden => 1,
                },
            });
        }
        for (index, section) in input.sections.iter().enumerate() {
            for reloc in &section.relocs {
                names.insert(
                    (reloc.symbol.as_ptr(), reloc.symbol.len() as u64),
                    reloc.symbol.as_str(),
                );
                let (kind, namespace, code) = match &reloc.kind {
                    AReloc::Absolute64 => (0, "", 0),
                    AReloc::Absolute32 => (1, "", 0),
                    AReloc::SignedAbsolute32 => (2, "", 0),
                    AReloc::Relative32 => (3, "", 0),
                    AReloc::Branch32 => (4, "", 0),
                    AReloc::Relative8 => (5, "", 0),
                    AReloc::TargetSpecific { namespace, code } => {
                        (6, namespace.as_str(), u64::from(*code))
                    },
                };
                relocs.push(RelocRow {
                    section: index as u64,
                    offset: reloc.offset as u64,
                    name: reloc.symbol.as_ptr(),
                    name_len: reloc.symbol.len() as u64,
                    kind,
                    namespace: namespace.as_ptr(),
                    namespace_len: namespace.len() as u64,
                    code,
                    addend: reloc.addend,
                });
            }
        }
        let blank_section = SectionRow {
            name: std::ptr::null(),
            name_len: 0,
            kind: 0,
            align: 0,
            data: std::ptr::null(),
            data_len: 0,
            zero_fill: 0,
        };
        let blank_symbol = SymbolRow {
            name: std::ptr::null(),
            name_len: 0,
            kind: 0,
            section: 0,
            offset: 0,
            size: 0,
            binding: 0,
            visibility: 0,
        };
        let blank_reloc = RelocRow {
            section: 0,
            offset: 0,
            name: std::ptr::null(),
            name_len: 0,
            kind: 0,
            namespace: std::ptr::null(),
            namespace_len: 0,
            code: 0,
            addend: 0,
        };
        let mut out_sections = allocate(ns, blank_section)?;
        let mut out_symbols = allocate(symbols_capacity, blank_symbol)?;
        let mut out_relocs = allocate(nr, blank_reloc)?;
        let mut scratch = allocate(scratch_words, 0u64)?;
        let mut result = ResultRow::default();
        let raw_target = Target {
            format: match target.format {
                ObjectFormat::ELF64 => 0,
                ObjectFormat::COFF => 1,
                ObjectFormat::MachO64 => 2,
            },
            machine: match target.machine {
                Machine::X86_64 => 0,
                Machine::AArch64 => 1,
                Machine::RiscV64 => 2,
                Machine::LoongArch64 => 3,
            },
            address_bits: u64::from(target.address_bits),
            endian: match target.endian {
                object::Endian::Little => 0,
                object::Endian::Big => 1,
            },
        };
        // All rows borrow live immutable input. Output arrays/scratch have exact
        // advertised initialized lengths. The private Wave call retains nothing.
        let status = unsafe {
            whale_asmobj(
                &raw_target,
                sections.as_ptr(),
                ns as u64,
                symbols.as_ptr(),
                ny as u64,
                relocs.as_ptr(),
                nr as u64,
                out_sections.as_mut_ptr(),
                ns as u64,
                out_symbols.as_mut_ptr(),
                symbols_capacity as u64,
                out_relocs.as_mut_ptr(),
                nr as u64,
                scratch.as_mut_ptr(),
                scratch_words as u64,
                &mut result,
            )
        };
        if i64::from(status) != result.status as i64 {
            return Err(invalid());
        }
        if status != 0 {
            return Err(match status {
                1 => "unsupported object target".into(),
                2 => "symbol section index is invalid".into(),
                3 => "unsupported relocation namespace".into(),
                4 => "relocation namespace does not match object target".into(),
                5 => "unsupported AArch64 COFF instruction relocation".into(),
                6 => "assembler relocation namespace does not support this format".into(),
                7 => "asmobj output capacity exceeded".into(),
                8 => "invalid asmobj input".into(),
                9 => "asmobj work limit exceeded".into(),
                _ => invalid(),
            });
        }
        let count = host(result.symbol_count)?;
        if count > out_symbols.len() {
            return Err(invalid());
        }
        let mut object = ObjectFile::with_target(target);
        for (index, row) in out_sections.iter().enumerate() {
            let source = &input.sections[index];
            if row.data != source.data.as_ptr() || row.data_len != source.data.len() as u64 {
                return Err(invalid());
            }
            object.sections.push(Section {
                name: name(&names, row.name, row.name_len)?.into(),
                kind: match row.kind {
                    0 => SectionKind::Text,
                    1 => SectionKind::Data,
                    2 => SectionKind::Bss,
                    3 => SectionKind::ReadOnlyData,
                    _ => return Err(invalid()),
                },
                align: row.align,
                data: source.data.clone(),
                zero_fill: row.zero_fill,
            });
        }
        for row in &out_symbols[..count] {
            object.symbols.push(ObjectSymbol {
                name: name(&names, row.name, row.name_len)?.into(),
                kind: match row.kind {
                    0 => object::SymbolKind::NoType,
                    1 => object::SymbolKind::Function,
                    2 => object::SymbolKind::Object,
                    _ => return Err(invalid()),
                },
                section_index: if row.section == u64::MAX {
                    None
                } else {
                    Some(host(row.section)?)
                },
                value: row.offset,
                size: row.size,
                binding: match row.binding {
                    0 => SymbolBinding::Local,
                    1 => SymbolBinding::Global,
                    _ => return Err(invalid()),
                },
                visibility: match row.visibility {
                    0 => object::SymbolVisibility::Default,
                    1 => object::SymbolVisibility::Hidden,
                    _ => return Err(invalid()),
                },
            });
        }
        for row in &out_relocs {
            let kind = match row.kind {
                0 => RelocKind::Absolute64,
                1 => RelocKind::Absolute32,
                2 => RelocKind::SignedAbsolute32,
                3 => RelocKind::Relative32,
                4 => RelocKind::PLT32,
                5 => RelocKind::Relative8,
                6 => RelocKind::TargetSpecific {
                    machine: target.machine,
                    code: u32::try_from(row.code).map_err(|_| invalid())?,
                },
                7 => RelocKind::CoffSpecific {
                    machine: target.machine,
                    code: u16::try_from(row.code).map_err(|_| invalid())?,
                },
                8 => RelocKind::MachSpecific {
                    machine: target.machine,
                    code: u8::try_from(row.code).map_err(|_| invalid())?,
                },
                _ => return Err(invalid()),
            };
            object.relocations.push(ObjectRelocation {
                section_index: host(row.section)?,
                offset: host(row.offset)?,
                symbol: name(&names, row.name, row.name_len)?.into(),
                addend: row.addend,
                kind,
            });
        }
        object.validate()?;
        Ok(object)
    }
}
