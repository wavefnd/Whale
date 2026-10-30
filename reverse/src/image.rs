//! Read-only AMD64 code ranges. File, section and virtual offsets stay distinct.
use object::{
    formats::elf_read::{ElfView, ObjectReadLimits},
    ObjectFile, RelocKind, SectionKind,
};

#[derive(Debug)]
pub struct CodeRegion<'a> {
    pub id: usize,
    pub section: Option<usize>,
    pub name: String,
    pub file_offset: u64,
    pub address: u64,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone)]
pub struct ImageSymbol {
    pub name: String,
    pub region: Option<usize>,
    pub offset: u64,
    pub address: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Annotation {
    pub region: usize,
    pub offset: usize,
    pub symbol: String,
    pub addend: i64,
    pub kind: RelocKind,
    pub width: usize,
}

#[derive(Debug)]
pub struct Image<'a> {
    pub kind: &'static str,
    pub entry: Option<u64>,
    pub regions: Vec<CodeRegion<'a>>,
    pub symbols: Vec<ImageSymbol>,
    pub relocations: Vec<Annotation>,
}

impl<'a> Image<'a> {
    pub fn raw(bytes: &'a [u8], base: u64) -> Result<Self, String> {
        base.checked_add(bytes.len() as u64).ok_or("raw address range overflow")?;
        Ok(Self {
            kind: "raw-amd64",
            entry: Some(base),
            regions: vec![CodeRegion {
                id: 0,
                section: None,
                name: "raw".into(),
                file_offset: 0,
                address: base,
                bytes,
            }],
            symbols: Vec::new(),
            relocations: Vec::new(),
        })
    }

    pub fn elf(bytes: &'a [u8], limits: ObjectReadLimits) -> Result<Self, String> {
        let view = ElfView::parse(bytes, limits).map_err(|e| e.to_string())?;
        match view.header.kind {
            1 => Self::relocatable(view, limits),
            2 => Self::executable(view, limits),
            _ => Err("reverse supports ELF64 AMD64 ET_REL and static ET_EXEC only".into()),
        }
    }

    fn relocatable(view: ElfView<'a>, limits: ObjectReadLimits) -> Result<Self, String> {
        let (object, usage) =
            ObjectFile::read_with_usage(view.bytes(), limits).map_err(|e| e.to_string())?;
        // Reader and image projection consume one allowance. Successful reads
        // report actual charges so string-heavy objects can use their budget
        // without reserving half for the much smaller image metadata pass.
        let mut work =
            limits.max_work.checked_sub(usage.work).ok_or("ELF inspection work limit exceeded")?;
        let mut image = Self {
            kind: "elf64-amd64-relocatable",
            entry: None,
            regions: Vec::new(),
            symbols: Vec::new(),
            relocations: Vec::new(),
        };
        let mut object_index = 0;
        charge(&mut work, object.sections.len())?;
        let mut region_map = vec![None; object.sections.len()];
        for index in 1..view.header.section_count {
            charge(&mut work, 1)?;
            let s = view.section(index).map_err(|e| e.to_string())?;
            if matches!(s.kind, 1 | 8) {
                let section = &object.sections[object_index];
                if section.kind == SectionKind::Text {
                    let bytes = view.range(s.offset, s.size).map_err(|e| e.to_string())?;
                    region_map[object_index] = Some(image.regions.len());
                    image.regions.push(CodeRegion {
                        id: image.regions.len(),
                        section: Some(object_index),
                        name: section.name.clone(),
                        file_offset: s.offset,
                        address: 0,
                        bytes,
                    });
                }
                object_index += 1;
            }
        }
        for symbol in object.symbols {
            charge(&mut work, 1)?;
            let region = symbol.section_index.and_then(|index| region_map[index]);
            image.symbols.push(ImageSymbol {
                name: symbol.name,
                region,
                offset: symbol.value,
                address: None,
            });
        }
        for relocation in object.relocations {
            charge(&mut work, 1)?;
            if let Some(region_id) = region_map[relocation.section_index] {
                image.relocations.push(Annotation {
                    region: region_id,
                    offset: relocation.offset,
                    symbol: relocation.symbol,
                    addend: relocation.addend,
                    kind: relocation.kind,
                    width: match relocation.kind {
                        RelocKind::Absolute64 => 8,
                        RelocKind::Relative8 => 1,
                        _ => 4,
                    },
                });
            }
        }
        let comparisons = image
            .relocations
            .len()
            .saturating_mul((usize::BITS - image.relocations.len().leading_zeros()) as usize);
        charge(&mut work, comparisons)?;
        image.relocations.sort_by_key(|r| (r.region, r.offset));
        Ok(image)
    }

    fn executable(view: ElfView<'a>, limits: ObjectReadLimits) -> Result<Self, String> {
        let mut image = Self {
            kind: "elf64-amd64-executable",
            entry: Some(view.header.entry),
            regions: Vec::new(),
            symbols: Vec::new(),
            relocations: Vec::new(),
        };
        let mut mappings = Vec::new();
        let mut work = limits.max_work;
        for index in 0..view.header.program_count {
            charge(&mut work, 1)?;
            let p = view.program(index).map_err(|e| e.to_string())?;
            if matches!(p.kind, 2 | 3) {
                return Err("dynamic/interpretive ELF program headers are unsupported".into());
            }
            if p.kind != 1 {
                continue;
            }
            if p.file_size > p.memory_size
                || p.flags & !7 != 0
                || (p.align > 1
                    && (!p.align.is_power_of_two()
                        || p.offset % p.align != p.virtual_address % p.align))
            {
                return Err("invalid ELF PT_LOAD sizes, permissions or alignment".into());
            }
            let end = p
                .virtual_address
                .checked_add(p.memory_size)
                .ok_or("PT_LOAD virtual address overflow")?;
            // A pure BSS segment has only a conceptual file offset. It must
            // participate in virtual validation without requiring file bytes.
            let bytes = if p.file_size == 0 {
                &[]
            } else {
                view.range(p.offset, p.file_size).map_err(|e| e.to_string())?
            };
            if p.memory_size != 0 {
                charge(&mut work, mappings.len())?;
                if mappings.iter().any(|&(start, stop)| p.virtual_address < stop && start < end) {
                    return Err("overlapping PT_LOAD virtual ranges are unsupported".into());
                }
                mappings.push((p.virtual_address, end));
            }
            if p.flags & 1 != 0 && !bytes.is_empty() {
                image.regions.push(CodeRegion {
                    id: image.regions.len(),
                    section: None,
                    name: format!("PT_LOAD[{index}]"),
                    file_offset: p.offset,
                    address: p.virtual_address,
                    bytes,
                });
            }
        }
        charge(&mut work, image.regions.len())?;
        if !image.regions.iter().any(|r| {
            view.header.entry >= r.address && view.header.entry - r.address < (r.bytes.len() as u64)
        }) {
            return Err("ELF entry is outside executable file-backed bytes".into());
        }
        if view.header.section_count > 0
            && view
                .range(view.header.section_offset, 64)
                .map_err(|e| e.to_string())?
                .iter()
                .any(|b| *b != 0)
        {
            return Err("executable section zero is not null".into());
        }
        let section_names = if view.header.string_section == 0 {
            None
        } else {
            let table = view.section(view.header.string_section).map_err(|e| e.to_string())?;
            if table.kind != 3 {
                return Err("section names require a string table".into());
            }
            let names = view.range(table.offset, table.size).map_err(|e| e.to_string())?;
            if !valid_string_table(names) {
                return Err("invalid section-name string table".into());
            }
            Some(names)
        };
        let mut string_work = 0usize;
        let mut symbol_count = 0usize;
        for index in 1..view.header.section_count {
            charge(&mut work, 1)?;
            let s = view.section(index).map_err(|e| e.to_string())?;
            if s.kind == 0 {
                continue;
            }
            if s.name != 0 {
                let names = section_names.ok_or("section name without string table")?;
                let tail =
                    names.get(s.name as usize..).ok_or("section name offset outside table")?;
                let remaining = limits.max_string_bytes.saturating_sub(string_work).min(work / 2);
                let end = tail
                    .iter()
                    .take(remaining.saturating_add(1))
                    .position(|b| *b == 0)
                    .ok_or("section-name budget or termination failure")?;
                charge(&mut work, (end + 1).checked_mul(2).ok_or("section-name work overflow")?)?;
                string_work = string_work
                    .checked_add(end + 1)
                    .filter(|n| *n <= limits.max_string_bytes)
                    .ok_or("section-name string budget exceeded")?;
                std::str::from_utf8(&tail[..end]).map_err(|_| "section name is not UTF-8")?;
            }

            if s.align != 0 && !s.align.is_power_of_two() {
                return Err("invalid executable section alignment".into());
            }
            s.address.checked_add(s.size).ok_or("executable section address overflow")?;
            if s.kind != 8 {
                view.range(s.offset, s.size).map_err(|e| e.to_string())?;
            }
            if s.kind != 2 {
                continue;
            }
            if s.entry_size != 24
                || s.size % 24 != 0
                || s.size == 0
                || s.size / 24 > limits.max_symbols as u64
                || s.info == 0
                || u64::from(s.info) > s.size / 24
                || s.link as usize >= view.header.section_count
            {
                return Err("invalid executable symbol table".into());
            }
            symbol_count = symbol_count
                .checked_add((s.size / 24) as usize)
                .filter(|n| *n <= limits.max_symbols)
                .ok_or("executable symbol count exceeded")?;
            let strings = view.section(s.link as usize).map_err(|e| e.to_string())?;
            if strings.kind != 3 {
                return Err("symbol names require a string table".into());
            }
            let names = view.range(strings.offset, strings.size).map_err(|e| e.to_string())?;
            if !valid_string_table(names) {
                return Err("invalid symbol-name string table".into());
            }
            if view.range(s.offset, 24).map_err(|e| e.to_string())?.iter().any(|byte| *byte != 0) {
                return Err("executable symbol zero is not null".into());
            }
            for n in 1..s.size / 24 {
                charge(&mut work, 1)?;
                let at = s.offset + n * 24;
                let binding = view.range(at + 4, 1).map_err(|e| e.to_string())?[0] >> 4;
                if (n < u64::from(s.info)) != (binding == 0) {
                    return Err("executable local/global symbol order contradicts sh_info".into());
                }
                let name_offset = view.u32(at).map_err(|e| e.to_string())? as usize;
                let remaining = limits.max_string_bytes.saturating_sub(string_work);
                let tail = names.get(name_offset..).ok_or("symbol name offset outside table")?;
                let end = tail
                    .iter()
                    .take(remaining.min(work / 3).saturating_add(1))
                    .position(|b| *b == 0)
                    .ok_or("unterminated symbol name or string budget exceeded")?;
                charge(&mut work, (end + 1).checked_mul(3).ok_or("symbol work overflow")?)?;
                string_work = string_work
                    .checked_add(end + 1)
                    .filter(|n| *n <= limits.max_string_bytes)
                    .ok_or("symbol string budget exceeded")?;
                let name =
                    std::str::from_utf8(&tail[..end]).map_err(|_| "symbol name is not UTF-8")?;
                let section = view.u16(at + 6).map_err(|e| e.to_string())?;
                if section == 0 || name.is_empty() {
                    continue;
                }
                if section as usize >= view.header.section_count && section < 0xff00 {
                    return Err("symbol section index outside table".into());
                }
                let symbol_kind = view.range(at + 4, 1).map_err(|e| e.to_string())?[0] & 15;
                // STT_TLS values are offsets within a TLS block, not process
                // addresses. TLS metadata is outside this static code model.
                if symbol_kind == 6 {
                    continue;
                }
                let value = view.u64(at + 8).map_err(|e| e.to_string())?;
                let size = view.u64(at + 16).map_err(|e| e.to_string())?;
                let symbol_end = value.checked_add(size).ok_or("symbol address range overflow")?;
                let region = if section == 0xfff1 {
                    None
                } else {
                    if section >= 0xff00 {
                        return Err("unsupported executable symbol section class".into());
                    }
                    let owner = view.section(section as usize).map_err(|e| e.to_string())?;
                    let owner_end = owner
                        .address
                        .checked_add(owner.size)
                        .ok_or("symbol defining section overflow")?;
                    let outside_owner = value < owner.address || symbol_end > owner_end;
                    // Linkers emit zero-sized NOTYPE boundary markers beyond
                    // their nominal owner. Retain their addresses as metadata
                    // without assigning an unrelated executable region.
                    if outside_owner && !(symbol_kind == 0 && size == 0) {
                        return Err("symbol range exceeds its defining section".into());
                    }
                    if !outside_owner
                        && owner.kind == 1
                        && owner.flags & 4 != 0
                        && value < owner_end
                    {
                        charge(&mut work, image.regions.len())?;
                        let found = image.regions.iter().find(|r| {
                            value >= r.address
                                && symbol_end <= r.address + r.bytes.len() as u64
                                && value - r.address < r.bytes.len() as u64
                        });
                        let r = found.ok_or(
                            "executable symbol is not in a file-backed executable segment",
                        )?;
                        let symbol_file = owner
                            .offset
                            .checked_add(value - owner.address)
                            .ok_or("symbol file offset overflow")?;
                        if r.file_offset.checked_add(value - r.address) != Some(symbol_file) {
                            return Err("symbol section and segment file mapping disagree".into());
                        }
                        Some(r)
                    } else {
                        None
                    }
                };
                image.symbols.push(ImageSymbol {
                    name: name.into(),
                    region: region.map(|r| r.id),
                    offset: region.map_or(0, |r| value - r.address),
                    address: Some(value),
                });
            }
        }
        Ok(image)
    }

    pub fn entry_offset(&self, region: &CodeRegion<'_>) -> usize {
        self.entry
            .and_then(|entry| entry.checked_sub(region.address))
            .filter(|n| *n < region.bytes.len() as u64)
            .map_or(0, |n| n as usize)
    }

    pub fn annotations(&self, region: usize, offset: usize, length: usize) -> Vec<&Annotation> {
        let start = self
            .relocations
            .partition_point(|r| (r.region, r.offset) < (region, offset.saturating_sub(7)));
        let end = offset.saturating_add(length);
        self.relocations[start..]
            .iter()
            .take_while(|r| r.region == region && r.offset < end)
            .filter(|r| r.offset.saturating_add(r.width) > offset)
            .collect()
    }
}

fn charge(work: &mut usize, count: usize) -> Result<(), String> {
    *work = work.checked_sub(count).ok_or("ELF inspection work limit exceeded")?;
    Ok(())
}

fn valid_string_table(bytes: &[u8]) -> bool {
    bytes.first() == Some(&0) && bytes.last() == Some(&0)
}
