use amd64::AMD64;

use assembler::{assemble, SymbolKind, SymbolVisibility};

#[test]
fn metadata_supports_forward_declarations_equ_sizes_and_local_labels() {
    let source="LEN equ 3\nsymbol entry, function, LEN, hidden\nglobal entry\nentry: nop\n.local: nop\nret\nsymbol .local, notype, 1, default\nsection .data\npayload: dd 1\nsymbol payload, object, 4, hidden\nextern imported\nsymbol imported, function, 0, default\n";
    let out = assemble(source, &AMD64).unwrap();
    let entry = out.symbols.iter().find(|s| s.name == "entry").unwrap();
    assert_eq!(
        (entry.kind, entry.size, entry.visibility, entry.is_global),
        (SymbolKind::Function, 3, SymbolVisibility::Hidden, true)
    );
    let local = out.symbols.iter().find(|s| s.name == "entry.local").unwrap();
    assert_eq!((local.kind, local.offset, local.size), (SymbolKind::NoType, 1, 1));
    let payload = out.symbols.iter().find(|s| s.name == "payload").unwrap();
    assert_eq!(
        (payload.kind, payload.size, payload.visibility),
        (SymbolKind::Object, 4, SymbolVisibility::Hidden)
    );
    let imported = out.symbols.iter().find(|s| s.name == "imported").unwrap();
    assert_eq!((imported.kind, imported.section_index), (SymbolKind::Function, None));
}

#[test]
fn invalid_metadata_cannot_silently_change_symbols() {
    for declaration in [
        "symbol x, invalid, 1, default",
        "symbol x, function, 1, invalid",
        "symbol x, function, -1, default",
        "symbol missing, function, 0, default",
        "symbol x, object, 2, default",
        "symbol x, object, missing, default",
        "symbol x function, 1, default",
        "symbol x, function, 1, default extra",
        "symbol x, function, 1, default\nsymbol x, function, 1, default",
    ] {
        assert!(assemble(&format!("x: ret\n{declaration}\n"), &AMD64).is_err(), "{declaration}");
    }
    let out = assemble("plain: ret\n", &AMD64).unwrap();
    assert_eq!(
        (out.symbols[0].kind, out.symbols[0].size, out.symbols[0].visibility),
        (SymbolKind::NoType, 0, SymbolVisibility::Default)
    );
}
