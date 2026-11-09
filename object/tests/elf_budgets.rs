use object::*;

fn object() -> ObjectFile {
    let mut obj = ObjectFile::new(ObjectFormat::ELF64);
    obj.add_section(".text", SectionKind::Text, 1);
    obj.sections[0].data = vec![0; 8];
    obj
}

fn sym(name: String) -> ObjectSymbol {
    ObjectSymbol {
        kind: object::SymbolKind::NoType,
        name,
        section_index: Some(0),
        value: 0,
        size: 8,
        binding: SymbolBinding::Global,
        visibility: SymbolVisibility::Default,
    }
}

#[test]
fn complete_metadata_and_alignment_have_exact_output_budget_boundaries() {
    let mut obj = object();
    obj.symbols.push(sym("entry".into()));
    let bss = obj.add_section(".bss", SectionKind::Bss, 4096);
    obj.sections[bss].zero_fill = 1 << 32;
    let data = obj.add_section(".data", SectionKind::Data, 256);
    obj.sections[data].data = vec![1, 2, 3];
    obj.relocations.push(ObjectRelocation {
        section_index: 0,
        offset: 0,
        symbol: "synthesized".into(),
        addend: 0,
        kind: RelocKind::Absolute64,
    });
    let bytes = obj.write().unwrap();
    assert_eq!(obj.write_with_limit(bytes.len() as u64).unwrap(), bytes);
    assert!(obj.write_with_limit(bytes.len() as u64 - 1).unwrap_err().contains("size limit"));
    assert!(bytes.len() < 4096, "BSS memory remains sparse");
}

#[test]
fn each_name_source_is_preflighted_before_proportional_copies() {
    for source in ["symbol", "section", "undefined"] {
        let mut obj = object();
        let name = "n".repeat(4096);
        match source {
            "symbol" => obj.symbols.push(sym(name)),
            "section" => obj.sections[0].name = name,
            "undefined" => obj.relocations.push(ObjectRelocation {
                section_index: 0,
                offset: 0,
                symbol: name,
                addend: 0,
                kind: RelocKind::Absolute64,
            }),
            _ => unreachable!(),
        }
        assert!(obj.write_with_limit(512).unwrap_err().contains("size limit"));
        assert!(obj.write().is_ok());
    }
}

#[test]
fn huge_name_budget_child() {
    if std::env::var_os("WHALE_ELF_BUDGET_CHILD").is_none() {
        return;
    }
    let mut obj = object();
    obj.symbols.push(sym("a".repeat(28 * 1024 * 1024)));
    assert!(obj.write_with_limit(512).unwrap_err().contains("size limit"));
}

#[cfg(target_os = "linux")]
#[test]
fn huge_names_reject_before_copying_in_a_capped_child_process() {
    let output = std::process::Command::new("sh")
        .env_clear()
        .arg("-c")
        .arg("ulimit -c 0; ulimit -v 51200; exec \"$@\"")
        .arg("elf-test")
        .arg(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("huge_name_budget_child")
        .env("MALLOC_ARENA_MAX", "1")
        .env("WHALE_ELF_BUDGET_CHILD", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
