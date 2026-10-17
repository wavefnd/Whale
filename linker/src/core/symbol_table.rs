use object::{ObjectFile, SymbolBinding, SymbolKind, SymbolVisibility};

use std::collections::HashMap;

/// Local names belong to one input object; exported names share a link namespace.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum SymbolKey {
    Global(String),
    Local { object_index: usize, name: String },
}

#[derive(Default)]
pub struct SymbolTable {
    pub symbols: HashMap<SymbolKey, ResolvedSymbol>,
    /// One entry per consumed relocation, in input object/relocation order.
    pub references: Vec<ResolvedReference>,
}

pub struct ResolvedSymbol {
    pub name: String,
    /// `None` for the defined zero value of an unresolved weak reference.
    pub object_index: Option<usize>,
    pub section_index: Option<usize>,
    pub value: u64,
    pub size: u64,
    pub binding: SymbolBinding,
    pub kind: SymbolKind,
    pub visibility: SymbolVisibility,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedReference {
    pub object_index: usize,
    pub section_index: usize,
    pub offset: usize,
    pub target: SymbolKey,
    pub weak_undefined: bool,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Strong definitions win over weak definitions in either input order;
    /// multiple weak definitions select the first input. Only actual relocation
    /// references require resolution. An unsatisfied weak reference resolves to
    /// zero; an unreferenced undefined declaration imposes no requirement.
    pub fn resolve(&mut self, objects: &[ObjectFile]) -> Result<(), String> {
        self.symbols.clear();
        self.references.clear();

        for (index, obj) in objects.iter().enumerate() {
            obj.target.validate_amd64().map_err(|e| format!("link input {index}: {e}"))?;
            obj.validate().map_err(|e| format!("link input {index}: {e}"))?;
        }

        let mut symbols: HashMap<SymbolKey, ResolvedSymbol> = HashMap::new();
        for (obj_idx, obj) in objects.iter().enumerate() {
            for sym in obj.symbols.iter().filter(|s| s.section_index.is_some()) {
                let key = if sym.binding == SymbolBinding::Local {
                    SymbolKey::Local { object_index: obj_idx, name: sym.name.clone() }
                } else {
                    SymbolKey::Global(sym.name.clone())
                };

                if let Some(existing) = symbols.get(&key) {
                    if sym.binding == SymbolBinding::Local {
                        return Err(format!(
                            "Duplicate local symbol in object {obj_idx}: {}",
                            sym.name
                        ));
                    }

                    match (existing.binding, sym.binding) {
                        (SymbolBinding::Global, SymbolBinding::Global) => {
                            return Err(format!(
                                "Duplicate global symbol {:?}: objects {} and {obj_idx}",
                                sym.name,
                                existing.object_index.unwrap()
                            ))
                        },

                        (SymbolBinding::Global, SymbolBinding::Weak)
                        | (SymbolBinding::Weak, SymbolBinding::Weak) => continue,
                        _ => {},
                    }
                }

                symbols.insert(
                    key,
                    ResolvedSymbol {
                        name: sym.name.clone(),
                        object_index: Some(obj_idx),
                        section_index: sym.section_index,
                        value: sym.value,
                        size: sym.size,
                        binding: sym.binding,
                        kind: sym.kind,
                        visibility: sym.visibility,
                    },
                );
            }
        }

        let mut references = Vec::new();
        let mut weak_undefined = HashMap::new();
        for (obj_idx, obj) in objects.iter().enumerate() {
            let declarations: HashMap<_, _> =
                obj.symbols.iter().map(|s| (s.name.as_str(), s)).collect();
            for reloc in &obj.relocations {
                let declaration = declarations.get(reloc.symbol.as_str());
                let key = if declaration.is_some_and(|s| s.binding == SymbolBinding::Local) {
                    SymbolKey::Local { object_index: obj_idx, name: reloc.symbol.clone() }
                } else {
                    SymbolKey::Global(reloc.symbol.clone())
                };

                let missing = !symbols.contains_key(&key);
                let weak = declaration.is_some_and(|s| s.binding == SymbolBinding::Weak);
                if missing && !weak {
                    return Err(format!("Unresolved strong symbol {:?} referenced by object {obj_idx}, section {:?} ({}), offset {}",
                        reloc.symbol, obj.sections[reloc.section_index].name, reloc.section_index, reloc.offset));
                }

                if missing {
                    let declaration = declaration.expect("weak reference has a declaration");
                    weak_undefined.entry(key.clone()).or_insert_with(|| ResolvedSymbol {
                        name: reloc.symbol.clone(),
                        object_index: None,
                        section_index: None,
                        value: 0,
                        size: 0,
                        binding: SymbolBinding::Weak,
                        kind: declaration.kind,
                        visibility: declaration.visibility,
                    });
                }

                references.push(ResolvedReference {
                    object_index: obj_idx,
                    section_index: reloc.section_index,
                    offset: reloc.offset,
                    target: key,
                    weak_undefined: missing,
                });
            }
        }

        symbols.extend(weak_undefined);

        self.symbols = symbols;
        self.references = references;

        Ok(())
    }
}
