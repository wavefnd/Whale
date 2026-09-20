use std::collections::HashMap;

use crate::core::object::ObjectFile;
use crate::core::reloc::RelocKind;
use crate::core::section::SectionKind;
use crate::core::symbol::{SymbolBinding, SymbolVisibility};

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
    fn to_bytes(&self) -> [u8; 64] {
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
    fn to_bytes(&self) -> [u8; 24] {
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
    fn to_bytes(&self) -> [u8; 24] {
        let mut out = [0u8; 24];
        out[0..8].copy_from_slice(&self.offset.to_le_bytes());
        out[8..16].copy_from_slice(&self.info.to_le_bytes());
        out[16..24].copy_from_slice(&self.addend.to_le_bytes());
        out
    }
}

#[derive(Clone)]
struct SymBuild {
    name: String,
    section_index: Option<usize>,
    value: u64,
    size: u64,
    binding: SymbolBinding,
    visibility: SymbolVisibility,
}

pub fn write_elf(obj: &ObjectFile) -> Result<Vec<u8>, String> {
    for section in &obj.sections {
        if section.name.contains('\0') {
            return Err(format!(
                "section name {:?} contains embedded NUL byte",
                section.name
            ));
        }
    }

    for sym in &obj.symbols {
        if sym.name.contains('\0') {
            return Err(format!(
                "symbol name {:?} contains embedded NUL byte",
                sym.name
            ));
        }
    }

    for reloc in &obj.relocations {
        if reloc.symbol.contains('\0') {
            return Err(format!(
                "relocation reference {:?} contains embedded NUL byte",
                reloc.symbol
            ));
        }
    }

    let mut shstrtab = vec![0u8];
    let mut strtab = vec![0u8];

    let mut shdrs = vec![Elf64Shdr {
        name: 0,
        type_: SHT_NULL,
        ..Default::default()
    }];
    let mut payloads: Vec<Option<Vec<u8>>> = vec![None];

    let mut section_to_shdr_idx = vec![0usize; obj.sections.len()];
    for (sec_idx, section) in obj.sections.iter().enumerate() {
        let name_idx = push_name(&mut shstrtab, &section.name);
        let (type_, flags) = section_type_and_flags(section.kind);
        let data_size = section.data.len() as u64;

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
            name: s.name.clone(),
            section_index: s.section_index,
            value: s.value,
            size: s.size,
            binding: s.binding,
            visibility: s.visibility,
        })
        .collect();

    for reloc in &obj.relocations {
        if symbols.iter().all(|s| s.name != reloc.symbol) {
            symbols.push(SymBuild {
                name: reloc.symbol.clone(),
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

    let first_global_index = 1 + ordered_syms
        .iter()
        .take_while(|s| s.binding == SymbolBinding::Local)
        .count();

    let mut elf_syms = Vec::with_capacity(1 + ordered_syms.len());
    elf_syms.push(Elf64Sym::default()); // STN_UNDEF

    let mut symbol_index_by_name: HashMap<String, usize> = HashMap::new();
    for sym in ordered_syms {
        let name_idx = if sym.name.is_empty() {
            0
        } else {
            push_name(&mut strtab, &sym.name)
        };

        let shndx = sym
            .section_index
            .and_then(|idx| section_to_shdr_idx.get(idx).copied())
            .unwrap_or(0) as u16;

        let elf_sym = Elf64Sym {
            name: name_idx,
            info: (binding_to_stb(sym.binding) << 4),
            other: visibility_to_stv(sym.visibility),
            shndx,
            value: sym.value,
            size: sym.size,
        };

        let index = elf_syms.len();
        symbol_index_by_name.entry(sym.name).or_insert(index);
        elf_syms.push(elf_sym);
    }

    let mut rela_shdr_indices = Vec::new();
    for (sec_idx, _) in obj.sections.iter().enumerate() {
        let relocs: Vec<_> = obj
            .relocations
            .iter()
            .filter(|r| r.section_index == sec_idx)
            .collect();
        if relocs.is_empty() {
            continue;
        }

        let rela_name = format!(".rela{}", obj.sections[sec_idx].name);
        let name_idx = push_name(&mut shstrtab, &rela_name);

        let mut rela_data = Vec::with_capacity(relocs.len() * ELF_RELA_SIZE as usize);
        for reloc in relocs {
            let sym_idx = symbol_index_by_name
                .get(&reloc.symbol)
                .copied()
                .unwrap_or(0) as u64;
            let rtype = reloc_type(reloc.kind) as u64;

            let rela = Elf64Rela {
                offset: reloc.offset as u64,
                info: (sym_idx << 32) | rtype,
                addend: reloc.addend,
            };
            rela_data.extend_from_slice(&rela.to_bytes());
        }

        let target_sec_idx = section_to_shdr_idx[sec_idx] as u32;
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

    let symtab_name = push_name(&mut shstrtab, ".symtab");
    let symtab_idx = shdrs.len();
    let mut symtab_data = Vec::with_capacity(elf_syms.len() * ELF_SYM_SIZE as usize);
    for sym in &elf_syms {
        symtab_data.extend_from_slice(&sym.to_bytes());
    }
    shdrs.push(Elf64Shdr {
        name: symtab_name,
        type_: SHT_SYMTAB,
        info: first_global_index as u32,
        addralign: 8,
        entsize: ELF_SYM_SIZE,
        size: symtab_data.len() as u64,
        ..Default::default()
    });
    payloads.push(Some(symtab_data));

    let strtab_name = push_name(&mut shstrtab, ".strtab");
    let strtab_idx = shdrs.len();
    shdrs.push(Elf64Shdr {
        name: strtab_name,
        type_: SHT_STRTAB,
        addralign: 1,
        size: strtab.len() as u64,
        ..Default::default()
    });
    payloads.push(Some(strtab.clone()));

    shdrs[symtab_idx].link = strtab_idx as u32;
    for idx in rela_shdr_indices {
        shdrs[idx].link = symtab_idx as u32;
    }

    let shstrtab_name = push_name(&mut shstrtab, ".shstrtab");
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
        current_offset = align_up(current_offset, align);
        shdrs[idx].offset = current_offset;

        if shdrs[idx].type_ != SHT_NOBITS {
            let size = payloads[idx].as_ref().map(|p| p.len()).unwrap_or(0) as u64;
            shdrs[idx].size = size;
            current_offset += size;
        }
    }

    let shoff = align_up(current_offset, 8);
    let hdr = Elf64Header {
        ident: [0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        type_: 1,    // ET_REL
        machine: 62, // EM_X86_64
        version: 1,
        shoff,
        ehsize: ELF_HDR_SIZE as u16,
        shentsize: ELF_SHDR_SIZE,
        shnum: shdrs.len() as u16,
        shstrndx: shstrtab_idx as u16,
        ..Default::default()
    };

    let mut out = Vec::new();
    out.extend_from_slice(&hdr.to_bytes());

    for idx in 1..shdrs.len() {
        if shdrs[idx].type_ == SHT_NOBITS {
            continue;
        }
        let offset = shdrs[idx].offset as usize;
        if out.len() < offset {
            out.resize(offset, 0);
        }
        if let Some(data) = &payloads[idx] {
            out.extend_from_slice(data);
        }
    }

    if out.len() < shoff as usize {
        out.resize(shoff as usize, 0);
    }

    for shdr in &shdrs {
        out.extend_from_slice(&shdr.to_bytes());
    }

    Ok(out)
}

fn align_up(value: u64, align: u64) -> u64 {
    if align <= 1 {
        value
    } else {
        (value + align - 1) & !(align - 1)
    }
}

fn push_name(table: &mut Vec<u8>, name: &str) -> u32 {
    debug_assert!(
        !name.contains('\0'),
        "push_name received embedded NUL in {:?}",
        name
    );
    let idx = table.len() as u32;
    table.extend_from_slice(name.as_bytes());
    table.push(0);
    idx
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

fn reloc_type(kind: RelocKind) -> u32 {
    match kind {
        RelocKind::Absolute64 => 1, // R_X86_64_64
        RelocKind::Absolute32 => 10, // R_X86_64_32
        RelocKind::Relative32 => 2, // R_X86_64_PC32
        RelocKind::Relative8 => 15, // R_X86_64_PC8
        RelocKind::GOTPCREL => 9,   // R_X86_64_GOTPCREL
        RelocKind::PLT32 => 4,      // R_X86_64_PLT32
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
        let fixtures = [
            ("\0text", "beginning"),
            (".te\0xt", "middle"),
            (".text\0", "end"),
        ];

        for (bad_name, position) in fixtures {
            let mut obj = ObjectFile::new(ObjectFormat::ELF64);
            obj.sections.push(Section {
                name: bad_name.to_string(),
                kind: SectionKind::Text,
                data: vec![0x90],
                align: 16,
            });

            let err = write_elf(&obj).expect_err(&format!("expected error for NUL at {position}"));
            assert!(
                err.contains("section name"),
                "expected context 'section name', got: {err}"
            );
            assert!(
                err.contains("embedded NUL"),
                "expected 'embedded NUL' in error, got: {err}"
            );
            let escaped = format!("{:?}", bad_name);
            assert!(
                err.contains(&escaped),
                "expected escaped name {escaped} in error, got: {err}"
            );
        }
    }

    #[test]
    fn test_reject_symbol_name_embedded_nul() {
        let fixtures = [
            ("\0sym", "beginning"),
            ("my\0sym", "middle"),
            ("sym\0", "end"),
        ];

        for (bad_name, position) in fixtures {
            let mut obj = ObjectFile::new(ObjectFormat::ELF64);
            obj.sections.push(Section {
                name: ".text".to_string(),
                kind: SectionKind::Text,
                data: vec![0x90],
                align: 16,
            });
            obj.symbols.push(ObjectSymbol {
                name: bad_name.to_string(),
                section_index: Some(0),
                value: 0,
                size: 1,
                binding: SymbolBinding::Global,
                visibility: SymbolVisibility::Default,
            });

            let err = write_elf(&obj).expect_err(&format!("expected error for NUL at {position}"));
            assert!(
                err.contains("symbol name"),
                "expected context 'symbol name', got: {err}"
            );
            assert!(
                err.contains("embedded NUL"),
                "expected 'embedded NUL' in error, got: {err}"
            );
            let escaped = format!("{:?}", bad_name);
            assert!(
                err.contains(&escaped),
                "expected escaped name {escaped} in error, got: {err}"
            );
        }
    }

    #[test]
    fn test_reject_relocation_reference_embedded_nul() {
        let fixtures = [
            ("\0reloc", "beginning"),
            ("rel\0oc", "middle"),
            ("reloc\0", "end"),
        ];

        for (bad_name, position) in fixtures {
            let mut obj = ObjectFile::new(ObjectFormat::ELF64);
            obj.sections.push(Section {
                name: ".text".to_string(),
                kind: SectionKind::Text,
                data: vec![0x90; 8],
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
            assert!(
                err.contains("embedded NUL"),
                "expected 'embedded NUL' in error, got: {err}"
            );
            let escaped = format!("{:?}", bad_name);
            assert!(
                err.contains(&escaped),
                "expected escaped name {escaped} in error, got: {err}"
            );
        }
    }

    #[test]
    fn test_valid_names_utf8_and_empty() {
        let mut obj = ObjectFile::new(ObjectFormat::ELF64);
        let sec_idx = obj.sections.len();
        obj.sections.push(Section {
            name: ".текст_café_日本語".to_string(),
            kind: SectionKind::Text,
            data: vec![0x90],
            align: 16,
        });
        // Empty symbol name (mandatory STN_UNDEF entry)
        obj.symbols.push(ObjectSymbol {
            name: "".to_string(),
            section_index: None,
            value: 0,
            size: 0,
            binding: SymbolBinding::Local,
            visibility: SymbolVisibility::Default,
        });
        // Non-ASCII UTF-8 symbol
        obj.symbols.push(ObjectSymbol {
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
