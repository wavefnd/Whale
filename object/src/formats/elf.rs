use crate::core::object::ObjectFile;
use crate::core::section::SectionKind;
use crate::core::symbol::SymbolBinding;
use crate::core::reloc::RelocKind;

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

#[repr(C)]
#[derive(Default)]
struct Elf64Sym {
    name: u32,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
}

#[repr(C)]
#[derive(Default)]
struct Elf64Rela {
    offset: u64,
    info: u64,
    addend: i64,
}

pub fn write_elf(obj: &ObjectFile) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();

    // 1. String tables
    let mut shstrtab = Vec::new();
    shstrtab.push(0);
    
    let mut strtab = Vec::new();
    strtab.push(0);

    // 2. Build sections info
    let mut elf_sections = Vec::new();
    elf_sections.push(Elf64Shdr::default()); // Null section

    let mut current_offset = 64; // Header size

    // Section headers mapping
    let mut section_to_shdr_idx = Vec::new();
    
    // Create section headers for user sections
    for section in &obj.sections {
        let name_idx = shstrtab.len() as u32;
        shstrtab.extend_from_slice(section.name.as_bytes());
        shstrtab.push(0);

        let type_ = match section.kind {
            SectionKind::Text | SectionKind::Data | SectionKind::ReadOnlyData => 1, // SHT_PROGBITS
            SectionKind::Bss => 8, // SHT_NOBITS
        };

        let flags = match section.kind {
            SectionKind::Text => 6, // SHF_ALLOC | SHF_EXECINSTR
            SectionKind::Data => 3, // SHF_WRITE | SHF_ALLOC
            SectionKind::ReadOnlyData => 2, // SHF_ALLOC
            SectionKind::Bss => 3, // SHF_WRITE | SHF_ALLOC
        };

        let shdr = Elf64Shdr {
            name: name_idx,
            type_,
            flags,
            offset: current_offset,
            size: section.data.len() as u64,
            addralign: section.align,
            ..Default::default()
        };

        if section.kind != SectionKind::Bss {
            current_offset += section.data.len() as u64;
        }
        
        section_to_shdr_idx.push(elf_sections.len());
        elf_sections.push(shdr);
    }

    // Symbol table section
    let symtab_shdr_idx = elf_sections.len();
    let symtab_name = shstrtab.len() as u32;
    shstrtab.extend_from_slice(b".symtab\0");
    elf_sections.push(Elf64Shdr {
        name: symtab_name,
        type_: 2, // SHT_SYMTAB
        link: symtab_shdr_idx as u32 + 1, // Next is strtab
        entsize: 24,
        addralign: 8,
        ..Default::default()
    });

    // String table section
    let strtab_name = shstrtab.len() as u32;
    shstrtab.extend_from_slice(b".strtab\0");
    elf_sections.push(Elf64Shdr {
        name: strtab_name,
        type_: 3, // SHT_STRTAB
        addralign: 1,
        ..Default::default()
    });

    // Relocation sections
    let mut rela_sections = Vec::new();
    for (sec_idx, _) in obj.sections.iter().enumerate() {
        let has_relocs = obj.relocations.iter().any(|r| r.section_index == sec_idx);
        if !has_relocs { continue; }

        let name_idx = shstrtab.len() as u32;
        let rela_name = format!(".rela{}", obj.sections[sec_idx].name);
        shstrtab.extend_from_slice(rela_name.as_bytes());
        shstrtab.push(0);

        rela_sections.push((sec_idx, elf_sections.len()));
        elf_sections.push(Elf64Shdr {
            name: name_idx,
            type_: 4, // SHT_RELA
            flags: 0x40, // SHF_INFO_LINK
            link: symtab_shdr_idx as u32,
            info: section_to_shdr_idx[sec_idx] as u32,
            addralign: 8,
            entsize: 24,
            ..Default::default()
        });
    }

    // Section string table
    let shstrtab_name = shstrtab.len() as u32;
    shstrtab.extend_from_slice(b".shstrtab\0");
    let shstrtab_idx = elf_sections.len();
    elf_sections.push(Elf64Shdr {
        name: shstrtab_name,
        type_: 3, // SHT_STRTAB
        addralign: 1,
        ..Default::default()
    });

    // 3. Build Symbols
    let mut elf_syms = Vec::new();
    elf_syms.push(Elf64Sym::default()); // Null

    for s in &obj.symbols {
        let name_idx = strtab.len() as u32;
        strtab.extend_from_slice(s.name.as_bytes());
        strtab.push(0);

        let bind = match s.binding {
            SymbolBinding::Local => 0,
            SymbolBinding::Global => 1,
            SymbolBinding::Weak => 2,
        };

        let shndx = s.section_index.map(|i| section_to_shdr_idx[i] as u16).unwrap_or(0);

        elf_syms.push(Elf64Sym {
            name: name_idx,
            info: (bind << 4),
            shndx,
            value: s.value,
            size: s.size,
            ..Default::default()
        });
    }

    // 4. Finalize offsets and build final buffer
    // Set offsets for Symtab, Strtab, etc.
    elf_sections[symtab_shdr_idx].offset = current_offset;
    elf_sections[symtab_shdr_idx].size = (elf_syms.len() * 24) as u64;
    current_offset += elf_sections[symtab_shdr_idx].size;

    elf_sections[symtab_shdr_idx + 1].offset = current_offset;
    elf_sections[symtab_shdr_idx + 1].size = strtab.len() as u64;
    current_offset += elf_sections[symtab_shdr_idx + 1].size;

    let mut elf_relas_groups = Vec::new();
    for (sec_idx, shdr_idx) in &rela_sections {
        let mut group = Vec::new();
        for r in obj.relocations.iter().filter(|r| r.section_index == *sec_idx) {
            let sym_idx = obj.symbols.iter().position(|s| s.name == r.symbol).map(|i| i + 1).unwrap_or(0);
            let type_ = match r.kind {
                RelocKind::Absolute64 => 1,
                RelocKind::Relative32 => 2,
                RelocKind::GOTPCREL => 9,
                RelocKind::PLT32 => 4,
            };
            group.push(Elf64Rela {
                offset: r.offset as u64,
                info: ((sym_idx as u64) << 32) | (type_ as u64),
                addend: r.addend,
            });
        }
        elf_sections[*shdr_idx].offset = current_offset;
        elf_sections[*shdr_idx].size = (group.len() * 24) as u64;
        current_offset += elf_sections[*shdr_idx].size;
        elf_relas_groups.push(group);
    }

    elf_sections[shstrtab_idx].offset = current_offset;
    elf_sections[shstrtab_idx].size = shstrtab.len() as u64;
    current_offset += elf_sections[shstrtab_idx].size;

    // Header
    let hdr = Elf64Header {
        ident: [0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        type_: 1, machine: 0x3e, version: 1,
        shoff: current_offset, shentsize: 64, shnum: elf_sections.len() as u16, shstrndx: shstrtab_idx as u16,
        ehsize: 64, ..Default::default()
    };

    // 5. Write to buffer
    unsafe {
        let ptr = &hdr as *const Elf64Header as *const u8;
        out.extend_from_slice(std::slice::from_raw_parts(ptr, 64));
    }
    for sec in &obj.sections {
        if sec.kind != SectionKind::Bss {
            out.extend_from_slice(&sec.data);
        }
    }
    for sym in &elf_syms {
        unsafe {
            let ptr = sym as *const Elf64Sym as *const u8;
            out.extend_from_slice(std::slice::from_raw_parts(ptr, 24));
        }
    }
    out.extend_from_slice(&strtab);
    for group in elf_relas_groups {
        for rela in group {
            unsafe {
                let ptr = &rela as *const Elf64Rela as *const u8;
                out.extend_from_slice(std::slice::from_raw_parts(ptr, 24));
            }
        }
    }
    out.extend_from_slice(&shstrtab);
    for shdr in &elf_sections {
        unsafe {
            let ptr = shdr as *const Elf64Shdr as *const u8;
            out.extend_from_slice(std::slice::from_raw_parts(ptr, 64));
        }
    }

    Ok(out)
}
