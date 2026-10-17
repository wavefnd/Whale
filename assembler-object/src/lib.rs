// SPDX-License-Identifier: MPL-2.0
//! Bridge from common assembler output to checked, target-identified ELF objects.
use assembler::AssemblerOutput;

use std::collections::{HashMap, HashSet};

use object::{
    ObjectFile, ObjectRelocation, ObjectSymbol, ObjectTarget, RelocKind as ObjectRelocKind,
    SectionKind, SymbolBinding, SymbolVisibility,
};

pub fn write(out: &AssemblerOutput, target: ObjectTarget) -> Result<Vec<u8>, String> {
    to_object(out, target)?.write()
}

pub fn to_object(out: &AssemblerOutput, target: ObjectTarget) -> Result<ObjectFile, String> {
    target.validate().map_err(|e| e.to_string())?;
    let mut obj = ObjectFile::with_target(target);
    let mut section_map = Vec::with_capacity(out.sections.len());

    for sec in &out.sections {
        let kind = match sec.kind {
            assembler::AsmSectionKind::Text => SectionKind::Text,
            assembler::AsmSectionKind::Data => SectionKind::Data,
            assembler::AsmSectionKind::ReadOnlyData => SectionKind::ReadOnlyData,
            assembler::AsmSectionKind::Bss => SectionKind::Bss,
        };
        let align = sec.align;
        let idx = obj.add_section(&sec.name, kind, align);
        obj.sections[idx].data = sec.data.clone();
        obj.sections[idx].zero_fill = sec.zero_fill;
        section_map.push(idx);
    }

    for sym in &out.symbols {
        let section_index = sym
            .section_index
            .map(|idx| section_map.get(idx).copied().ok_or("symbol section index is invalid"))
            .transpose()?;

        obj.symbols.push(ObjectSymbol {
            kind: match sym.kind {
                assembler::SymbolKind::NoType => object::SymbolKind::NoType,
                assembler::SymbolKind::Function => object::SymbolKind::Function,
                assembler::SymbolKind::Object => object::SymbolKind::Object,
            },
            name: sym.name.clone(),
            section_index,
            value: sym.offset,
            size: sym.size,
            binding: if sym.is_global { SymbolBinding::Global } else { SymbolBinding::Local },
            visibility: match sym.visibility {
                assembler::SymbolVisibility::Default => SymbolVisibility::Default,
                assembler::SymbolVisibility::Hidden => SymbolVisibility::Hidden,
            },
        });
    }

    let undefined: HashMap<&str, bool> =
        out.symbols.iter().map(|s| (s.name.as_str(), s.section_index.is_none())).collect();
    let mut known: HashSet<&str> = out.symbols.iter().map(|s| s.name.as_str()).collect();
    for (asm_sec_idx, sec) in out.sections.iter().enumerate() {
        let Some(&obj_sec_idx) = section_map.get(asm_sec_idx) else {
            continue;
        };

        for reloc in &sec.relocs {
            let is_undefined_symbol = undefined.get(reloc.symbol.as_str()).copied().unwrap_or(true);

            let kind = match reloc.kind {
                assembler::assembler::RelocKind::TargetSpecific { ref namespace, code } => {
                    let machine = match namespace.as_str() {
                        "aarch64" => object::Machine::AArch64,
                        "riscv64" => object::Machine::RiscV64,
                        "loongarch64" => object::Machine::LoongArch64,
                        _ => return Err(format!("unsupported relocation namespace {namespace:?}")),
                    };
                    if machine != target.machine {
                        return Err("relocation namespace does not match object target".into());
                    }
                    ObjectRelocKind::TargetSpecific { machine, code }
                },
                assembler::assembler::RelocKind::Absolute64 => ObjectRelocKind::Absolute64,
                assembler::assembler::RelocKind::Absolute32 => ObjectRelocKind::Absolute32,
                assembler::assembler::RelocKind::SignedAbsolute32 => {
                    ObjectRelocKind::SignedAbsolute32
                },
                assembler::assembler::RelocKind::Relative32 => ObjectRelocKind::Relative32,
                assembler::assembler::RelocKind::Branch32 => {
                    if is_undefined_symbol {
                        ObjectRelocKind::PLT32
                    } else {
                        ObjectRelocKind::Relative32
                    }
                },
                assembler::assembler::RelocKind::Relative8 => ObjectRelocKind::Relative8,
            };

            obj.relocations.push(ObjectRelocation {
                section_index: obj_sec_idx,
                offset: reloc.offset,
                symbol: reloc.symbol.clone(),
                addend: reloc.addend,
                kind,
            });

            if known.insert(reloc.symbol.as_str()) {
                obj.symbols.push(ObjectSymbol {
                    kind: object::SymbolKind::NoType,
                    name: reloc.symbol.clone(),
                    section_index: None,
                    value: 0,
                    size: 0,
                    binding: SymbolBinding::Global,
                    visibility: SymbolVisibility::Default,
                });
            }
        }
    }

    obj.validate()?;
    Ok(obj)
}
