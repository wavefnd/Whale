use object::*;

use reverse::image::Image;

fn executable() -> Vec<u8> {
    let mut b = vec![0; 121];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    fn u16w(b: &mut [u8], at: usize, v: u16) {
        b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn u32w(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn u64w(b: &mut [u8], at: usize, v: u64) {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    u16w(&mut b, 16, 2);
    u16w(&mut b, 18, 62);
    u32w(&mut b, 20, 1);
    u64w(&mut b, 24, 0x400078);
    u64w(&mut b, 32, 64);
    u16w(&mut b, 52, 64);
    u16w(&mut b, 54, 56);
    u16w(&mut b, 56, 1);
    u32w(&mut b, 64, 1);
    u32w(&mut b, 68, 5);
    u64w(&mut b, 80, 0x400000);
    u64w(&mut b, 96, 121);
    u64w(&mut b, 104, 1 << 30);
    u64w(&mut b, 112, 4096);
    b[120] = 0xc3;
    b
}

#[test]
fn executable_without_section_table_preserves_file_and_virtual_offsets() {
    let bytes = executable();
    let image = Image::elf(&bytes, ObjectReadLimits::default()).unwrap();
    assert_eq!(image.entry, Some(0x400078));
    assert_eq!(image.regions.len(), 1);
    let r = &image.regions[0];
    assert_eq!(r.address, 0x400000);
    assert_eq!(r.file_offset, 0);
    assert_eq!(r.bytes.len(), 121);
    assert_eq!(image.entry_offset(r), 120);
    assert_eq!(r.bytes[120], 0xc3);
}

#[test]
fn malformed_executable_ranges_sizes_and_entry_are_rejected() {
    let bytes = executable();
    for end in 0..bytes.len() {
        assert!(Image::elf(&bytes[..end], ObjectReadLimits::default()).is_err());
    }
    for (at, value) in
        [(24, 0x500000u64), (72, u64::MAX), (80, u64::MAX - 1), (96, 122), (104, 120), (112, 3)]
    {
        let mut bad = bytes.clone();
        bad[at..at + 8].copy_from_slice(&value.to_le_bytes());
        assert!(Image::elf(&bad, ObjectReadLimits::default()).is_err(), "at {at}");
    }
    let mut bad = bytes.clone();
    bad[16] = 3;
    assert!(Image::elf(&bad, ObjectReadLimits::default()).is_err());
}

#[test]
fn relocatable_code_regions_and_symbolic_call_annotations_preserve_identity() {
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    o.add_section(".text", SectionKind::Text, 1);
    o.sections[0].data = vec![0xe8, 0, 0, 0, 0, 0xc3];
    o.symbols.push(ObjectSymbol {
        name: "external".into(),
        section_index: None,
        value: 0,
        size: 0,
        binding: SymbolBinding::Global,
        kind: SymbolKind::Function,
        visibility: SymbolVisibility::Default,
    });
    o.relocations.push(ObjectRelocation {
        section_index: 0,
        offset: 1,
        symbol: "external".into(),
        kind: RelocKind::PLT32,
        addend: -4,
    });
    let bytes = o.write().unwrap();
    let image = Image::elf(&bytes, ObjectReadLimits::default()).unwrap();
    assert_eq!(image.kind, "elf64-amd64-relocatable");
    assert_eq!(image.regions[0].section, Some(0));
    assert_eq!(image.regions[0].bytes, [0xe8, 0, 0, 0, 0, 0xc3]);
    let a = image.annotations(0, 0, 5);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].symbol, "external");
    assert_eq!(a[0].offset, 1);
    assert_eq!(a[0].width, 4);
    assert!(image.annotations(0, 5, 1).is_empty());
    if reverse::backend_available() {
        let inst = reverse::decode(image.regions[0].bytes, 0).unwrap();
        let structured = reverse::format::instruction_json(&inst, 0, &a);
        assert!(structured["target"].is_null());
        let text = reverse::format::instruction_text(&inst, 0, &a);
        assert!(text.contains("reloc(external-4)"));
        assert!(!text.contains("call 0x5"));
    }
}

#[test]
fn raw_address_overflow_is_rejected() {
    assert!(Image::raw(&[0, 1], u64::MAX).is_err());
    let image = Image::raw(&[], 42).unwrap();
    assert_eq!(image.regions[0].address, 42);
}

fn symbol_executable(owner: u16) -> Vec<u8> {
    fn w16(b: &mut [u8], at: usize, v: u16) {
        b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn w32(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn w64(b: &mut [u8], at: usize, v: u64) {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    let mut b = vec![0; 0x1043];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    w16(&mut b, 16, 2);
    w16(&mut b, 18, 62);
    w32(&mut b, 20, 1);
    w64(&mut b, 24, 0x401000);
    w64(&mut b, 32, 64);
    w64(&mut b, 40, 128);
    w16(&mut b, 52, 64);
    w16(&mut b, 54, 56);
    w16(&mut b, 56, 1);
    w16(&mut b, 58, 64);
    w16(&mut b, 60, 4);
    w32(&mut b, 64, 1);
    w32(&mut b, 68, 5);
    w64(&mut b, 72, 0x1000);
    w64(&mut b, 80, 0x401000);
    w64(&mut b, 96, 1);
    w64(&mut b, 104, 1);
    w64(&mut b, 112, 4096);
    for (index, kind, flags, address, offset, size, link, info, ent) in [
        (1usize, 1, 6, 0x401000, 0x1000, 1, 0, 0, 0),
        (2, 2, 0, 0, 0x1010, 48, 3, 2, 24),
        (3, 3, 0, 0, 0x1040, 3, 0, 0, 0),
    ] {
        let at = 128 + index * 64;
        w32(&mut b, at + 4, kind);
        w64(&mut b, at + 8, flags);
        w64(&mut b, at + 16, address);
        w64(&mut b, at + 24, offset);
        w64(&mut b, at + 32, size);
        w32(&mut b, at + 40, link);
        w32(&mut b, at + 44, info);
        w64(&mut b, at + 48, 1);
        w64(&mut b, at + 56, ent);
    }
    b[0x1000] = 0xc3;
    b[0x1040..].copy_from_slice(b"\0x\0");
    w32(&mut b, 0x1028, 1);
    b[0x102c] = 2;
    w16(&mut b, 0x102e, owner);
    w64(&mut b, 0x1030, 0x401000);
    w64(&mut b, 0x1038, 1);
    b
}

#[test]
fn executable_symbol_ownership_and_work_budgets_are_validated() {
    let good = symbol_executable(1);
    let image = Image::elf(&good, ObjectReadLimits::default()).unwrap();
    assert_eq!(image.symbols[0].region, Some(0));
    let bad = symbol_executable(2);
    assert!(Image::elf(&bad, ObjectReadLimits::default())
        .unwrap_err()
        .contains("defining section"));
    let limits = ObjectReadLimits { max_work: 0, ..Default::default() };
    assert!(Image::elf(&good, limits).unwrap_err().contains("work limit"));
}

#[test]
fn high_half_symbol_file_mapping_uses_checked_relative_offsets() {
    let mut bytes = symbol_executable(1);
    let address = u64::MAX - 1;
    for at in [24usize, 80, 88, 208, 0x1030] {
        bytes[at..at + 8].copy_from_slice(&address.to_le_bytes());
    }
    bytes[112..120].copy_from_slice(&1u64.to_le_bytes());
    let image = Image::elf(&bytes, ObjectReadLimits::default()).unwrap();
    assert_eq!(image.symbols[0].region, Some(0));
    assert_eq!(image.symbols[0].offset, 0);
}

#[test]
fn inactive_executable_sections_ignore_undefined_header_fields() {
    let mut bytes = executable();
    bytes.resize(256, 0);
    write64(&mut bytes, 40, 128);
    bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
    bytes[60..62].copy_from_slice(&2u16.to_le_bytes());
    assert!(Image::elf(&bytes, ObjectReadLimits::default()).is_ok());
    bytes[192..196].copy_from_slice(&u32::MAX.to_le_bytes());
    write64(&mut bytes, 192 + 24, u64::MAX);
    write64(&mut bytes, 192 + 48, 3);
    assert!(Image::elf(&bytes, ObjectReadLimits::default()).is_ok());
    bytes[128] = 1;
    assert!(Image::elf(&bytes, ObjectReadLimits::default()).unwrap_err().contains("section zero"));

    let mut inactive_owner = symbol_executable(1);
    inactive_owner[128 + 64 + 4..128 + 64 + 8].copy_from_slice(&0u32.to_le_bytes());
    inactive_owner[128 + 64 + 48..128 + 64 + 56].copy_from_slice(&3u64.to_le_bytes());
    let image = Image::elf(&inactive_owner, ObjectReadLimits::default()).unwrap();
    assert_eq!(image.symbols[0].region, None);
}

#[test]
fn executable_string_tables_require_nul_boundaries_even_for_unused_bytes() {
    let original = symbol_executable(1);
    assert!(Image::elf(&original, ObjectReadLimits::default()).is_ok());
    let mut prefix = original.clone();
    prefix[0x1040] = b'X';
    assert!(Image::elf(&prefix, ObjectReadLimits::default())
        .unwrap_err()
        .contains("symbol-name string table"));
    let mut tail = original.clone();
    tail.push(b'X');
    write64(&mut tail, 128 + 3 * 64 + 32, 4);
    assert!(Image::elf(&tail, ObjectReadLimits::default()).is_err());
    *tail.last_mut().unwrap() = 0;
    assert!(Image::elf(&tail, ObjectReadLimits::default()).is_ok());
    write64(&mut tail, 128 + 3 * 64 + 32, 0);
    assert!(Image::elf(&tail, ObjectReadLimits::default()).is_err());

    let mut named = original;
    named[62..64].copy_from_slice(&3u16.to_le_bytes());
    assert!(Image::elf(&named, ObjectReadLimits::default()).is_ok());
    named[0x1040] = b'X';
    assert!(Image::elf(&named, ObjectReadLimits::default())
        .unwrap_err()
        .contains("section-name string table"));
}

#[test]
fn executable_symtab_checks_null_record_and_local_global_boundary() {
    let original = symbol_executable(1);
    assert_eq!(Image::elf(&original, ObjectReadLimits::default()).unwrap().symbols.len(), 1);
    let mut null = original.clone();
    null[0x1018] = 123;
    assert!(Image::elf(&null, ObjectReadLimits::default()).unwrap_err().contains("symbol zero"));
    for info in [0, 3, u32::MAX] {
        let mut invalid = original.clone();
        invalid[128 + 2 * 64 + 44..128 + 2 * 64 + 48].copy_from_slice(&info.to_le_bytes());
        assert!(Image::elf(&invalid, ObjectReadLimits::default()).is_err(), "info {info}");
    }
    let mut wrong_order = original.clone();
    wrong_order[128 + 2 * 64 + 44..128 + 2 * 64 + 48].copy_from_slice(&1u32.to_le_bytes());
    assert!(Image::elf(&wrong_order, ObjectReadLimits::default())
        .unwrap_err()
        .contains("local/global"));
    for binding in [1u8, 2] {
        wrong_order[0x102c] = (binding << 4) | 2;
        assert!(Image::elf(&wrong_order, ObjectReadLimits::default()).is_ok());
    }
}

#[test]
fn empty_load_ranges_do_not_overlap_real_mappings() {
    let mut bytes = executable();
    bytes.resize(177, 0);
    bytes[176] = 0xc3;
    bytes[56..58].copy_from_slice(&2u16.to_le_bytes());
    write64(&mut bytes, 24, 0x4000b0);
    write64(&mut bytes, 96, 177);
    write64(&mut bytes, 104, 177);
    bytes[120..124].copy_from_slice(&1u32.to_le_bytes());
    bytes[124..128].copy_from_slice(&4u32.to_le_bytes());
    write64(&mut bytes, 128, 0);
    write64(&mut bytes, 136, 0x400001);
    write64(&mut bytes, 144, 0x400001);
    write64(&mut bytes, 152, 0);
    write64(&mut bytes, 160, 0);
    write64(&mut bytes, 168, 1);
    for address in [0x400001, 0x4000b1] {
        write64(&mut bytes, 136, address);
        let image = Image::elf(&bytes, ObjectReadLimits::default()).unwrap();
        assert_eq!(image.regions.len(), 1);
    }
    write64(&mut bytes, 136, 0x400001);
    write64(&mut bytes, 160, 1);
    assert!(Image::elf(&bytes, ObjectReadLimits::default())
        .unwrap_err()
        .contains("overlapping PT_LOAD"));
    write64(&mut bytes, 160, 0);
    write64(&mut bytes, 152, 1);
    assert!(Image::elf(&bytes, ObjectReadLimits::default()).is_err());
    write64(&mut bytes, 152, 0);
    write64(&mut bytes, 168, 3);
    assert!(Image::elf(&bytes, ObjectReadLimits::default()).is_err());
}

#[test]
fn relocatable_inspection_shares_actual_reader_work_instead_of_reserving_half() {
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    object.add_section(".text", SectionKind::Text, 1);
    object.sections[0].data = vec![0xe8, 0, 0, 0, 0, 0xc3];
    let name = format!("external_{}", "x".repeat(4096));
    object.symbols.push(ObjectSymbol {
        name: name.clone(),
        section_index: None,
        value: 0,
        size: 0,
        binding: SymbolBinding::Global,
        kind: SymbolKind::Function,
        visibility: SymbolVisibility::Default,
    });
    object.relocations.push(ObjectRelocation {
        section_index: 0,
        offset: 1,
        symbol: name.clone(),
        kind: RelocKind::PLT32,
        addend: -4,
    });
    let bytes = object.write().unwrap();
    let (_, usage) = ObjectFile::read_with_usage(&bytes, ObjectReadLimits::default()).unwrap();
    // Reading alone fits, but image construction still needs a share of the
    // same allowance. It must not silently start a fresh work budget.
    let reader_only = ObjectReadLimits { max_work: usage.work, ..Default::default() };
    ObjectFile::read_with_usage(&bytes, reader_only).unwrap();
    assert!(Image::elf(&bytes, reader_only).unwrap_err().contains("inspection work limit"));
    // A small metadata allowance is enough even when name inspection dominates
    // reader work. Splitting the total in half would reject this valid object.
    let shared = ObjectReadLimits { max_work: usage.work + 128, ..Default::default() };
    assert!(usage.work > shared.max_work / 2);
    let image = Image::elf(&bytes, shared).unwrap();
    assert_eq!(image.regions[0].bytes, [0xe8, 0, 0, 0, 0, 0xc3]);
    assert_eq!(image.symbols[0].name, name);
    assert_eq!(image.relocations[0].symbol, name);
    assert_eq!(image.relocations[0].offset, 1);
    assert_eq!(image.relocations[0].width, 4);
}

fn write64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn metadata_executable(kind: u8, value: u64, size: u64) -> Vec<u8> {
    let mut bytes = symbol_executable(1);
    bytes.resize(0x1191, 0);
    bytes[60..62].copy_from_slice(&5u16.to_le_bytes());
    // Retain the code function and add a separate data/TLS owner plus metadata.
    let owner = 128 + 4 * 64;
    bytes[owner + 4..owner + 8].copy_from_slice(&1u32.to_le_bytes());
    for (offset, value) in [(8, 0x403), (16, 0x402000), (24, 0x1008), (32, 16), (48, 8)] {
        write64(&mut bytes, owner + offset, value);
    }
    let symtab = 128 + 2 * 64;
    write64(&mut bytes, symtab + 24, 0x1100);
    write64(&mut bytes, symtab + 32, 72);
    bytes[symtab + 44..symtab + 48].copy_from_slice(&1u32.to_le_bytes());
    let strings = 128 + 3 * 64;
    write64(&mut bytes, strings + 24, 0x1180);
    write64(&mut bytes, strings + 32, 17);
    bytes[0x1180..].copy_from_slice(b"\0start\0metadata\0\0");
    for (at, name, info, section, address, length) in
        [(0x1118, 1u32, 0x12u8, 1u16, 0x401000u64, 1u64), (0x1130, 7, 0x10 | kind, 4, value, size)]
    {
        bytes[at..at + 4].copy_from_slice(&name.to_le_bytes());
        bytes[at + 4] = info;
        bytes[at + 6..at + 8].copy_from_slice(&section.to_le_bytes());
        write64(&mut bytes, at + 8, address);
        write64(&mut bytes, at + 16, length);
    }
    bytes
}

#[test]
fn zero_sized_notype_boundary_markers_remain_non_code_metadata() {
    for address in [0x403000, 0x401000] {
        let bytes = metadata_executable(0, address, 0);
        let image = Image::elf(&bytes, ObjectReadLimits::default()).unwrap();
        assert_eq!(image.symbols[0].name, "start");
        assert_eq!(image.symbols[0].region, Some(0));
        let marker = &image.symbols[1];
        assert_eq!(marker.name, "metadata");
        assert_eq!(marker.address, Some(address));
        assert_eq!(marker.region, None);
        assert_eq!(image.entry_offset(&image.regions[0]), 0);
    }
    for (kind, size) in [(2, 0), (0, 1), (1, 0)] {
        assert!(Image::elf(
            &metadata_executable(kind, 0x403000, size),
            ObjectReadLimits::default()
        )
        .unwrap_err()
        .contains("defining section"));
    }
}

#[test]
fn tls_offsets_are_omitted_without_losing_static_code_or_claiming_process_addresses() {
    for offset in [0, 8] {
        let bytes = metadata_executable(6, offset, 8);
        let image = Image::elf(&bytes, ObjectReadLimits::default()).unwrap();
        assert_eq!(image.symbols.len(), 1);
        assert_eq!(image.symbols[0].name, "start");
        assert_eq!(image.symbols[0].address, Some(0x401000));
        assert_eq!(image.symbols[0].region, Some(0));
        assert_eq!(image.regions[0].bytes, [0xc3]);
    }
    // Non-TLS symbols with the same invalid VA still fail owner validation.
    assert!(Image::elf(&metadata_executable(1, 0, 8), ObjectReadLimits::default()).is_err());
}

#[test]
fn pure_bss_load_segments_need_no_file_range_but_preserve_virtual_validation() {
    let mut bytes = executable();
    bytes.resize(177, 0);
    bytes[176] = 0xc3;
    bytes[56..58].copy_from_slice(&2u16.to_le_bytes());
    write64(&mut bytes, 24, 0x4000b0);
    write64(&mut bytes, 96, 177);
    write64(&mut bytes, 104, 177);
    bytes[120..124].copy_from_slice(&1u32.to_le_bytes());
    bytes[124..128].copy_from_slice(&6u32.to_le_bytes());
    write64(&mut bytes, 128, 0x100000);
    write64(&mut bytes, 136, 0x500000);
    write64(&mut bytes, 144, 0x500000);
    write64(&mut bytes, 152, 0);
    write64(&mut bytes, 160, 1 << 40);
    write64(&mut bytes, 168, 4096);
    for offset in [0, 0x100000] {
        write64(&mut bytes, 128, offset);
        let image = Image::elf(&bytes, ObjectReadLimits::default()).unwrap();
        assert_eq!(image.regions.len(), 1);
        assert_eq!(image.regions[0].bytes.len(), 177);
        assert_eq!(image.entry_offset(&image.regions[0]), 176);
    }
    for (at, value) in [(152, 1), (128, 0x100001), (136, 0x400000), (136, u64::MAX - 1)] {
        let mut bad = bytes.clone();
        write64(&mut bad, at, value);
        assert!(Image::elf(&bad, ObjectReadLimits::default()).is_err(), "at {at}");
    }
}
