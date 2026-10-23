use crate::{ObjectError, ObjectErrorKind as ErrorKind, ObjectResource};

use std::collections::{HashMap, HashSet};

fn write_error(kind: ErrorKind, message: impl Into<String>) -> ObjectError {
    ObjectError::new("write", kind, message)
}

fn output_limit() -> ObjectError {
    let mut e = write_error(ErrorKind::ResourceLimit, "ELF output exceeds configured size limit");
    e.resource = Some(ObjectResource::OutputBytes);
    e
}

use crate::core::object::ObjectFile;

use crate::core::section::SectionKind;

use crate::core::symbol::{SymbolBinding, SymbolKind, SymbolVisibility};

const ELF_HDR_SIZE: u64 = 64;

const ELF_SHDR_SIZE: u16 = 64;

const ELF_SYM_SIZE: u64 = 24;

const ELF_RELA_SIZE: u64 = 24;

const SHT_NULL: u32 = 0;

const SHT_PROGBITS: u32 = 1;

const SHT_SYMTAB: u32 = 2;

const SHT_STRTAB: u32 = 3;

const SHT_RELA: u32 = 4;

const SHT_NOBITS: u32 = 8;

const SHF_WRITE: u64 = 0x1;

const SHF_ALLOC: u64 = 0x2;

const SHF_EXECINSTR: u64 = 0x4;

const SHF_INFO_LINK: u64 = 0x40;

#[repr(C)]
#[derive(Default)]
struct Elf64Header {
    ident: [u8; 16],
    type_: u16,
    machine: u16,
    version: u32,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

impl Elf64Header {
    fn to_bytes(&self) -> [u8; 64] {
        let mut out = [0u8; 64];
        out[0..16].copy_from_slice(&self.ident);
        out[16..18].copy_from_slice(&self.type_.to_le_bytes());
        out[18..20].copy_from_slice(&self.machine.to_le_bytes());
        out[20..24].copy_from_slice(&self.version.to_le_bytes());
        out[24..32].copy_from_slice(&self.entry.to_le_bytes());
        out[32..40].copy_from_slice(&self.phoff.to_le_bytes());
        out[40..48].copy_from_slice(&self.shoff.to_le_bytes());
        out[48..52].copy_from_slice(&self.flags.to_le_bytes());
        out[52..54].copy_from_slice(&self.ehsize.to_le_bytes());
        out[54..56].copy_from_slice(&self.phentsize.to_le_bytes());
        out[56..58].copy_from_slice(&self.phnum.to_le_bytes());
        out[58..60].copy_from_slice(&self.shentsize.to_le_bytes());
        out[60..62].copy_from_slice(&self.shnum.to_le_bytes());
        out[62..64].copy_from_slice(&self.shstrndx.to_le_bytes());
        out
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Elf64Shdr {
    name: u32,
    type_: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    addralign: u64,
    entsize: u64,
}

impl Elf64Shdr {
    fn to_bytes(self) -> [u8; 64] {
        let mut out = [0u8; 64];
        out[0..4].copy_from_slice(&self.name.to_le_bytes());
        out[4..8].copy_from_slice(&self.type_.to_le_bytes());
        out[8..16].copy_from_slice(&self.flags.to_le_bytes());
        out[16..24].copy_from_slice(&self.addr.to_le_bytes());
        out[24..32].copy_from_slice(&self.offset.to_le_bytes());
        out[32..40].copy_from_slice(&self.size.to_le_bytes());
        out[40..44].copy_from_slice(&self.link.to_le_bytes());
        out[44..48].copy_from_slice(&self.info.to_le_bytes());
        out[48..56].copy_from_slice(&self.addralign.to_le_bytes());
        out[56..64].copy_from_slice(&self.entsize.to_le_bytes());
        out
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Elf64Sym {
    name: u32,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
}

impl Elf64Sym {
    fn to_bytes(self) -> [u8; 24] {
        let mut out = [0u8; 24];
        out[0..4].copy_from_slice(&self.name.to_le_bytes());
        out[4] = self.info;
        out[5] = self.other;
        out[6..8].copy_from_slice(&self.shndx.to_le_bytes());
        out[8..16].copy_from_slice(&self.value.to_le_bytes());
        out[16..24].copy_from_slice(&self.size.to_le_bytes());
        out
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Elf64Rela {
    offset: u64,
    info: u64,
    addend: i64,
}

impl Elf64Rela {
    fn to_bytes(self) -> [u8; 24] {
        let mut out = [0u8; 24];
        out[0..8].copy_from_slice(&self.offset.to_le_bytes());
        out[8..16].copy_from_slice(&self.info.to_le_bytes());
        out[16..24].copy_from_slice(&self.addend.to_le_bytes());
        out
    }
}

#[derive(Clone)]
struct SymBuild<'a> {
    kind: SymbolKind,
    name: &'a str,
    section_index: Option<usize>,
    value: u64,
    size: u64,
    binding: SymbolBinding,
    visibility: SymbolVisibility,
}

/// Default serialized-output budget. Call `write_elf_with_limit` to override it.
pub const DEFAULT_MAX_OUTPUT_SIZE: u64 = 256 * 1024 * 1024;

pub fn write_elf(obj: &ObjectFile) -> Result<Vec<u8>, String> {
    write_elf_with_limit(obj, DEFAULT_MAX_OUTPUT_SIZE)
}

pub fn write_elf_with_limit(obj: &ObjectFile, max_output_size: u64) -> Result<Vec<u8>, String> {
    write_elf_checked(obj, max_output_size).map_err(|e| e.to_string())
}

/// Serialize using typed validation, budget and allocation causes.
pub fn write_elf_checked(obj: &ObjectFile, max_output_size: u64) -> Result<Vec<u8>, ObjectError> {
    let machine = obj
        .target
        .elf_machine()
        .map_err(|e| write_error(ErrorKind::InvalidTarget, e.to_string()))?;
    let relocation_sections: HashSet<_> = obj.relocations.iter().map(|r| r.section_index).collect();
    checked_section_count(obj.sections.len(), relocation_sections.len())?;
    preflight(obj, max_output_size)?;
    obj.validate_diagnostic()?;
    let mut symbol_names: HashSet<_> = obj.symbols.iter().map(|s| &s.name).collect();

    let mut shstrtab = vec![0u8];
    let mut strtab = vec![0u8];

    let mut shdrs = vec![Elf64Shdr { name: 0, type_: SHT_NULL, ..Default::default() }];
    let mut payloads: Vec<Option<Vec<u8>>> = vec![None];

    let mut section_to_shdr_idx = vec![0usize; obj.sections.len()];
    for (sec_idx, section) in obj.sections.iter().enumerate() {
        let name_idx = push_name(&mut shstrtab, &section.name)?;
        let (type_, flags) = section_type_and_flags(section.kind);
        let data_size = section.memory_size().expect("validated section extent");

        section_to_shdr_idx[sec_idx] = shdrs.len();
        shdrs.push(Elf64Shdr {
            name: name_idx,
            type_,
            flags,
            size: data_size,
            addralign: section.align.max(1),
            ..Default::default()
        });

        if type_ == SHT_NOBITS {
            payloads.push(None);
        } else {
            payloads.push(Some(section.data.clone()));
        }
    }

    let mut symbols: Vec<SymBuild> = obj
        .symbols
        .iter()
        .map(|s| SymBuild {
            kind: s.kind,
            name: &s.name,
            section_index: s.section_index,
            value: s.value,
            size: s.size,
            binding: s.binding,
            visibility: s.visibility,
        })
        .collect();

    for reloc in &obj.relocations {
        if symbol_names.insert(&reloc.symbol) {
            symbols.push(SymBuild {
                kind: SymbolKind::NoType,
                name: &reloc.symbol,
                section_index: None,
                value: 0,
                size: 0,
                binding: SymbolBinding::Global,
                visibility: SymbolVisibility::Default,
            });
        }
    }

    let mut local_syms = Vec::new();
    let mut non_local_syms = Vec::new();
    for sym in symbols {
        if sym.binding == SymbolBinding::Local {
            local_syms.push(sym);
        } else {
            non_local_syms.push(sym);
        }
    }
    local_syms.extend(non_local_syms);
    let ordered_syms = local_syms;

    let first_global_index =
        1 + ordered_syms.iter().take_while(|s| s.binding == SymbolBinding::Local).count();

    let symbol_count = ordered_syms
        .len()
        .checked_add(1)
        .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF symbol count overflow"))?;
    checked_u32(symbol_count, "symbol count")?;
    checked_table_bytes(symbol_count, ELF_SYM_SIZE as usize, max_output_size)?;
    let mut elf_syms = Vec::new();
    elf_syms
        .try_reserve_exact(symbol_count)
        .map_err(|_| write_error(ErrorKind::HostAllocation, "ELF symbol allocation failed"))?;
    elf_syms.push(Elf64Sym::default()); // STN_UNDEF

    let mut symbol_index_by_name: HashMap<&str, usize> = HashMap::new();
    for sym in ordered_syms {
        let name_idx = if sym.name.is_empty() { 0 } else { push_name(&mut strtab, sym.name)? };

        let shndx = u16::try_from(
            sym.section_index.and_then(|idx| section_to_shdr_idx.get(idx).copied()).unwrap_or(0),
        )
        .map_err(|_| write_error(ErrorKind::Overflow, "ELF symbol section index overflow"))?;

        let elf_sym = Elf64Sym {
            name: name_idx,
            info: (binding_to_stb(sym.binding) << 4)
                | match sym.kind {
                    SymbolKind::NoType => 0,
                    SymbolKind::Object => 1,
                    SymbolKind::Function => 2,
                },
            other: visibility_to_stv(sym.visibility),
            shndx,
            value: sym.value,
            size: sym.size,
        };

        let index = elf_syms.len();
        symbol_index_by_name.entry(sym.name).or_insert(index);
        elf_syms.push(elf_sym);
    }

    // Validation above guarantees valid section references. Preserve input
    // order within each section while scanning relocation metadata only once.
    let mut grouped_relocations = vec![Vec::new(); obj.sections.len()];
    for relocation in &obj.relocations {
        grouped_relocations[relocation.section_index].push(relocation);
    }
    let mut rela_shdr_indices = Vec::new();
    for (sec_idx, relocs) in grouped_relocations.into_iter().enumerate() {
        if relocs.is_empty() {
            continue;
        }

        let rela_name = format!(".rela{}", obj.sections[sec_idx].name);
        let name_idx = push_name(&mut shstrtab, &rela_name)?;

        let capacity = checked_table_bytes(relocs.len(), ELF_RELA_SIZE as usize, max_output_size)?;
        let mut rela_data = Vec::new();
        rela_data.try_reserve_exact(capacity).map_err(|_| {
            write_error(ErrorKind::HostAllocation, "ELF relocation allocation failed")
        })?;
        for reloc in relocs {
            let sym_idx = u64::from(checked_u32(
                *symbol_index_by_name.get(reloc.symbol.as_str()).ok_or_else(|| {
                    write_error(ErrorKind::InvalidRelocation, "ELF relocation symbol missing")
                })?,
                "relocation symbol index",
            )?);
            let rtype = reloc
                .kind
                .encoding(obj.target)
                .map_err(|e| write_error(ErrorKind::InvalidRelocation, e))?
                .0 as u64;

            let rela = Elf64Rela {
                offset: reloc.offset as u64,
                info: (sym_idx << 32) | rtype,
                addend: reloc.addend,
            };
            rela_data.extend_from_slice(&rela.to_bytes());
        }

        let target_sec_idx = checked_u32(section_to_shdr_idx[sec_idx], "relocation section index")?;
        let idx = shdrs.len();
        shdrs.push(Elf64Shdr {
            name: name_idx,
            type_: SHT_RELA,
            flags: SHF_INFO_LINK,
            info: target_sec_idx,
            addralign: 8,
            entsize: ELF_RELA_SIZE,
            size: rela_data.len() as u64,
            ..Default::default()
        });
        payloads.push(Some(rela_data));
        rela_shdr_indices.push(idx);
    }

    let symtab_name = push_name(&mut shstrtab, ".symtab")?;
    let symtab_idx = shdrs.len();
    let capacity = checked_table_bytes(elf_syms.len(), ELF_SYM_SIZE as usize, max_output_size)?;
    let mut symtab_data = Vec::new();
    symtab_data.try_reserve_exact(capacity).map_err(|_| {
        write_error(ErrorKind::HostAllocation, "ELF symbol table allocation failed")
    })?;
    for sym in &elf_syms {
        symtab_data.extend_from_slice(&sym.to_bytes());
    }
    shdrs.push(Elf64Shdr {
        name: symtab_name,
        type_: SHT_SYMTAB,
        info: checked_u32(first_global_index, "first global symbol index")?,
        addralign: 8,
        entsize: ELF_SYM_SIZE,
        size: symtab_data.len() as u64,
        ..Default::default()
    });
    payloads.push(Some(symtab_data));

    let strtab_name = push_name(&mut shstrtab, ".strtab")?;
    let strtab_idx = shdrs.len();
    shdrs.push(Elf64Shdr {
        name: strtab_name,
        type_: SHT_STRTAB,
        addralign: 1,
        size: strtab.len() as u64,
        ..Default::default()
    });
    payloads.push(Some(strtab.clone()));

    shdrs[symtab_idx].link = checked_u32(strtab_idx, "string table index")?;
    for idx in rela_shdr_indices {
        shdrs[idx].link = checked_u32(symtab_idx, "symbol table index")?;
    }

    let shstrtab_name = push_name(&mut shstrtab, ".shstrtab")?;
    let shstrtab_idx = shdrs.len();
    shdrs.push(Elf64Shdr {
        name: shstrtab_name,
        type_: SHT_STRTAB,
        addralign: 1,
        size: shstrtab.len() as u64,
        ..Default::default()
    });
    payloads.push(Some(shstrtab.clone()));

    let mut current_offset = ELF_HDR_SIZE;
    for idx in 1..shdrs.len() {
        let align = shdrs[idx].addralign.max(1);
        let offset = align_up(current_offset, align)?;
        shdrs[idx].offset = offset;

        if shdrs[idx].type_ != SHT_NOBITS {
            let size = payloads[idx].as_ref().map(|p| p.len()).unwrap_or(0) as u64;
            shdrs[idx].size = size;
            current_offset = offset
                .checked_add(size)
                .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF file layout overflow"))?;
        }
    }

    let shoff = align_up(current_offset, 8)?;
    let section_bytes = checked_table_bytes(shdrs.len(), ELF_SHDR_SIZE as usize, max_output_size)?;
    let total_size = shoff
        .checked_add(section_bytes as u64)
        .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF header layout overflow"))?;
    let output_capacity = checked_output_size(total_size, max_output_size)?;
    let hdr = Elf64Header {
        ident: [0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        type_: 1, // ET_REL
        machine,
        flags: obj.target.elf_flags(),
        version: 1,
        shoff,
        ehsize: ELF_HDR_SIZE as u16,
        shentsize: ELF_SHDR_SIZE,
        shnum: checked_section_count(obj.sections.len(), relocation_sections.len())?,
        shstrndx: u16::try_from(shstrtab_idx).map_err(|_| {
            write_error(ErrorKind::Overflow, "ELF section-name table index overflow")
        })?,
        ..Default::default()
    };

    let mut out = Vec::new();
    out.try_reserve_exact(output_capacity)
        .map_err(|_| write_error(ErrorKind::HostAllocation, "ELF output allocation failed"))?;
    out.extend_from_slice(&hdr.to_bytes());

    for idx in 1..shdrs.len() {
        if shdrs[idx].type_ == SHT_NOBITS {
            continue;
        }
        let offset = usize::try_from(shdrs[idx].offset)
            .map_err(|_| write_error(ErrorKind::Overflow, "ELF offset exceeds host capacity"))?;
        if out.len() < offset {
            out.resize(offset, 0);
        }
        if let Some(data) = &payloads[idx] {
            out.extend_from_slice(data);
        }
    }

    let shoff = usize::try_from(shoff)
        .map_err(|_| write_error(ErrorKind::Overflow, "ELF header offset exceeds host capacity"))?;
    if out.len() < shoff {
        out.resize(shoff, 0);
    }

    for shdr in &shdrs {
        out.extend_from_slice(&shdr.to_bytes());
    }

    Ok(out)
}

/// Account for the complete output before cloning any name or payload. Borrowed
/// name sets and counts are the only working metadata built during this pass.
fn preflight(obj: &ObjectFile, limit: u64) -> Result<(), ObjectError> {
    let mut relocation_counts = HashMap::<usize, usize>::new();
    for reloc in &obj.relocations {
        if reloc.section_index >= obj.sections.len() {
            return Err(write_error(
                ErrorKind::InvalidSection,
                format!("relocation references invalid section {}", reloc.section_index),
            )
            .section(reloc.section_index));
        }
        let count = relocation_counts.entry(reloc.section_index).or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF relocation count overflow"))?;
    }
    let section_count = checked_section_count(obj.sections.len(), relocation_counts.len())?;
    let mut shstr_size = 1usize;
    let mut str_size = 1usize;
    let mut offset = ELF_HDR_SIZE;
    for (index, section) in obj.sections.iter().enumerate() {
        shstr_size = checked_name_end(shstr_size, section.name.len())?;
        checked_output_size(shstr_size as u64, limit)?;
        if section.align != 0 && !section.align.is_power_of_two() {
            return Err(write_error(
                ErrorKind::InvalidAlignment,
                format!(
                    "section {:?} alignment {} must be zero or a power of two",
                    section.name, section.align
                ),
            )
            .section(index));
        }
        let aligned = align_up(offset, section.align.max(1))?;
        if section.kind != SectionKind::Bss {
            offset = aligned
                .checked_add(section.file_size())
                .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF file layout overflow"))?;
        }
        if relocation_counts.contains_key(&index) {
            let name_len = section
                .name
                .len()
                .checked_add(5)
                .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF string table overflow"))?;
            shstr_size = checked_name_end(shstr_size, name_len)?;
            checked_output_size(shstr_size as u64, limit)?;
        }
        checked_output_size(offset, limit)?;
    }
    let mut names = HashSet::<&str>::new();
    let mut symbols = 1usize; // STN_UNDEF
    for name in obj
        .symbols
        .iter()
        .map(|s| s.name.as_str())
        .chain(obj.relocations.iter().map(|r| r.symbol.as_str()))
    {
        // Reject a huge name before hashing it or copying string-table data.
        checked_output_size(name.len() as u64, limit)?;
        if names.insert(name) {
            symbols = symbols
                .checked_add(1)
                .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF symbol count overflow"))?;
            if !name.is_empty() {
                str_size = checked_name_end(str_size, name.len())?;
                checked_output_size(str_size as u64, limit)?;
            }
        }
    }
    checked_u32(symbols, "symbol count")?;
    for index in 0..obj.sections.len() {
        if let Some(count) = relocation_counts.get(&index) {
            let bytes = checked_table_bytes(*count, ELF_RELA_SIZE as usize, limit)?;
            offset = align_up(offset, 8)?
                .checked_add(bytes as u64)
                .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF file layout overflow"))?;
        }
    }
    let sym_bytes = checked_table_bytes(symbols, ELF_SYM_SIZE as usize, limit)?;
    offset = align_up(offset, 8)?
        .checked_add(sym_bytes as u64)
        .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF file layout overflow"))?;
    for name in [".symtab", ".strtab", ".shstrtab"] {
        shstr_size = checked_name_end(shstr_size, name.len())?;
    }
    offset = offset
        .checked_add(str_size as u64)
        .and_then(|n| n.checked_add(shstr_size as u64))
        .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF file layout overflow"))?;
    let headers = checked_table_bytes(usize::from(section_count), ELF_SHDR_SIZE as usize, limit)?;
    let total = align_up(offset, 8)?
        .checked_add(headers as u64)
        .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF header layout overflow"))?;
    checked_output_size(total, limit)?;
    // Also check host addressability of proportional metadata before allocation.
    for (count, stride) in [
        (usize::from(section_count), std::mem::size_of::<Elf64Shdr>()),
        (symbols, std::mem::size_of::<SymBuild<'_>>()),
        (symbols, std::mem::size_of::<Elf64Sym>()),
    ] {
        checked_table_bytes(count, stride, u64::MAX)?;
    }
    Ok(())
}

fn align_up(value: u64, align: u64) -> Result<u64, ObjectError> {
    if !align.is_power_of_two() {
        return Err(write_error(
            ErrorKind::InvalidAlignment,
            "ELF alignment must be a power of two",
        ));
    }
    value
        .checked_add(value.wrapping_neg() & (align - 1))
        .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF alignment overflow"))
}

fn checked_u32(value: usize, field: &str) -> Result<u32, ObjectError> {
    u32::try_from(value)
        .map_err(|_| write_error(ErrorKind::Overflow, format!("ELF {field} exceeds 32-bit range")))
}

fn checked_section_count(input: usize, relocation_sections: usize) -> Result<u16, ObjectError> {
    let count = input
        .checked_add(relocation_sections)
        .and_then(|n| n.checked_add(4))
        .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF section count overflow"))?;
    // ELF reserves indices >= SHN_LORESERVE. Extended numbering is not supported.
    if count >= 0xff00 {
        return Err(write_error(
            ErrorKind::Unsupported,
            "ELF extended section numbering is not supported",
        ));
    }
    Ok(count as u16)
}

fn checked_output_size(size: u64, limit: u64) -> Result<usize, ObjectError> {
    if size > limit {
        return Err(output_limit());
    }
    let size = usize::try_from(size)
        .map_err(|_| write_error(ErrorKind::Overflow, "ELF output exceeds host capacity"))?;
    if size > isize::MAX as usize {
        return Err(write_error(ErrorKind::Overflow, "ELF output exceeds host capacity"));
    }
    Ok(size)
}

fn checked_table_bytes(count: usize, stride: usize, limit: u64) -> Result<usize, ObjectError> {
    let size = count
        .checked_mul(stride)
        .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF table size overflow"))?;
    checked_output_size(size as u64, limit)
}

fn checked_name_end(current: usize, name_len: usize) -> Result<usize, ObjectError> {
    let end = current
        .checked_add(name_len)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| write_error(ErrorKind::Overflow, "ELF string table overflow"))?;
    checked_u32(end, "string table size")?;
    Ok(end)
}

fn push_name(table: &mut Vec<u8>, name: &str) -> Result<u32, ObjectError> {
    let end = checked_name_end(table.len(), name.len())?;
    let index = checked_u32(table.len(), "string offset")?;
    table
        .try_reserve(end - table.len())
        .map_err(|_| write_error(ErrorKind::HostAllocation, "ELF string allocation failed"))?;
    table.extend_from_slice(name.as_bytes());
    table.push(0);
    Ok(index)
}

fn section_type_and_flags(kind: SectionKind) -> (u32, u64) {
    match kind {
        SectionKind::Text => (SHT_PROGBITS, SHF_ALLOC | SHF_EXECINSTR),
        SectionKind::Data => (SHT_PROGBITS, SHF_ALLOC | SHF_WRITE),
        SectionKind::ReadOnlyData => (SHT_PROGBITS, SHF_ALLOC),
        SectionKind::Bss => (SHT_NOBITS, SHF_ALLOC | SHF_WRITE),
    }
}

fn binding_to_stb(binding: SymbolBinding) -> u8 {
    match binding {
        SymbolBinding::Local => 0,
        SymbolBinding::Global => 1,
        SymbolBinding::Weak => 2,
    }
}

fn visibility_to_stv(vis: SymbolVisibility) -> u8 {
    match vis {
        SymbolVisibility::Default => 0,
        SymbolVisibility::Hidden => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::object::{ObjectFile, ObjectFormat};

    use crate::core::reloc::{ObjectRelocation, RelocKind};

    use crate::core::section::{Section, SectionKind};

    use crate::core::symbol::{ObjectSymbol, SymbolBinding, SymbolVisibility};

    #[test]
    fn test_reject_section_name_embedded_nul() {
        let fixtures = [("\0text", "beginning"), (".te\0xt", "middle"), (".text\0", "end")];

        for (bad_name, position) in fixtures {
            let mut obj = ObjectFile::new(ObjectFormat::ELF64);
            obj.sections.push(Section {
                name: bad_name.to_string(),
                kind: SectionKind::Text,
                data: vec![0x90],
                zero_fill: 0,
                align: 16,
            });

            let err = write_elf(&obj).expect_err(&format!("expected error for NUL at {position}"));
            assert!(err.contains("section name"), "expected context 'section name', got: {err}");
            assert!(err.contains("embedded NUL"), "expected 'embedded NUL' in error, got: {err}");
            let escaped = format!("{:?}", bad_name);
            assert!(err.contains(&escaped), "expected escaped name {escaped} in error, got: {err}");
        }
    }

    #[test]
    fn test_reject_symbol_name_embedded_nul() {
        let fixtures = [("\0sym", "beginning"), ("my\0sym", "middle"), ("sym\0", "end")];

        for (bad_name, position) in fixtures {
            let mut obj = ObjectFile::new(ObjectFormat::ELF64);
            obj.sections.push(Section {
                name: ".text".to_string(),
                kind: SectionKind::Text,
                data: vec![0x90],
                zero_fill: 0,
                align: 16,
            });
            obj.symbols.push(ObjectSymbol {
                kind: crate::SymbolKind::NoType,
                name: bad_name.to_string(),
                section_index: Some(0),
                value: 0,
                size: 1,
                binding: SymbolBinding::Global,
                visibility: SymbolVisibility::Default,
            });

            let err = write_elf(&obj).expect_err(&format!("expected error for NUL at {position}"));
            assert!(err.contains("symbol name"), "expected context 'symbol name', got: {err}");
            assert!(err.contains("embedded NUL"), "expected 'embedded NUL' in error, got: {err}");
            let escaped = format!("{:?}", bad_name);
            assert!(err.contains(&escaped), "expected escaped name {escaped} in error, got: {err}");
        }
    }

    #[test]
    fn test_reject_relocation_reference_embedded_nul() {
        let fixtures = [("\0reloc", "beginning"), ("rel\0oc", "middle"), ("reloc\0", "end")];

        for (bad_name, position) in fixtures {
            let mut obj = ObjectFile::new(ObjectFormat::ELF64);
            obj.sections.push(Section {
                name: ".text".to_string(),
                kind: SectionKind::Text,
                data: vec![0x90; 8],
                zero_fill: 0,
                align: 16,
            });
            obj.relocations.push(ObjectRelocation {
                section_index: 0,
                offset: 0,
                symbol: bad_name.to_string(),
                kind: RelocKind::Relative32,
                addend: -4,
            });

            let err = write_elf(&obj).expect_err(&format!("expected error for NUL at {position}"));
            assert!(
                err.contains("relocation reference"),
                "expected context 'relocation reference', got: {err}"
            );
            assert!(err.contains("embedded NUL"), "expected 'embedded NUL' in error, got: {err}");
            let escaped = format!("{:?}", bad_name);
            assert!(err.contains(&escaped), "expected escaped name {escaped} in error, got: {err}");
        }
    }

    #[test]
    fn test_valid_names_utf8_and_empty() {
        let mut obj = ObjectFile::new(ObjectFormat::ELF64);
        let sec_idx = obj.sections.len();
        obj.sections.push(Section {
            name: ".текст_café_日本語".to_string(),
            kind: SectionKind::Text,
            data: vec![0x90; 4],
            zero_fill: 0,
            align: 16,
        });
        // Empty symbol name (mandatory STN_UNDEF entry)
        obj.symbols.push(ObjectSymbol {
            kind: crate::SymbolKind::NoType,
            name: "".to_string(),
            section_index: None,
            value: 0,
            size: 0,
            binding: SymbolBinding::Local,
            visibility: SymbolVisibility::Default,
        });
        // Non-ASCII UTF-8 symbol
        obj.symbols.push(ObjectSymbol {
            kind: crate::SymbolKind::NoType,
            name: "функция_café".to_string(),
            section_index: Some(sec_idx),
            value: 0,
            size: 1,
            binding: SymbolBinding::Global,
            visibility: SymbolVisibility::Default,
        });
        // Relocation referencing valid non-ASCII symbol
        obj.relocations.push(ObjectRelocation {
            section_index: sec_idx,
            offset: 0,
            symbol: "внешний_sym".to_string(),
            kind: RelocKind::Relative32,
            addend: -4,
        });

        let elf_bytes = write_elf(&obj).expect("writing ELF with valid UTF-8 names must succeed");
        assert_eq!(&elf_bytes[0..4], &[0x7f, b'E', b'L', b'F']);

        let contains_nul_terminated = |bytes: &[u8], name: &str| {
            let mut pattern = name.as_bytes().to_vec();
            pattern.push(0);
            bytes.windows(pattern.len()).any(|w| w == pattern.as_slice())
        };

        assert!(contains_nul_terminated(&elf_bytes, ".текст_café_日本語"));
        assert!(contains_nul_terminated(&elf_bytes, "функция_café"));
        assert!(contains_nul_terminated(&elf_bytes, "внешний_sym"));
    }
}

#[cfg(test)]
mod layout_boundary_tests {
    use super::*;

    #[test]
    fn synthetic_metadata_checks_reserved_indices_and_integer_widths() {
        assert_eq!(checked_section_count(0xff00 - 5, 0).unwrap(), 0xfeff);
        assert!(checked_section_count(0xff00 - 5, 1).is_err());
        assert!(checked_section_count(usize::MAX, 1).is_err());
        assert_eq!(checked_u32(u32::MAX as usize, "index").unwrap(), u32::MAX);
        if let Ok(too_large) = usize::try_from(u64::from(u32::MAX) + 1) {
            assert!(checked_u32(too_large, "index").is_err());
        }
        assert_eq!(checked_name_end(1, 3).unwrap(), 5);
        assert!(checked_name_end(u32::MAX as usize, 1).is_err());
        assert!(checked_name_end(usize::MAX, 1).is_err());
        assert!(checked_table_bytes(usize::MAX, 24, u64::MAX).is_err());
    }

    #[test]
    fn checked_alignment_handles_aligned_maximum_and_overflow() {
        assert_eq!(align_up(u64::MAX, 1).unwrap(), u64::MAX);
        assert_eq!(align_up(u64::MAX - 7, 8).unwrap(), u64::MAX - 7);
        assert!(align_up(u64::MAX, 8).is_err());
        assert!(align_up(1, 3).is_err());
        assert!(align_up(1, 0).is_err());
    }
}
