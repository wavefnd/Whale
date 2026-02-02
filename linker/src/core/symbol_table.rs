use object::{ObjectFile, SymbolBinding};
use std::collections::HashMap;

pub struct SymbolTable {
    pub symbols: HashMap<String, ResolvedSymbol>,
}

pub struct ResolvedSymbol {
    pub name: String,
    pub object_index: Option<usize>,
    pub section_index: Option<usize>,
    pub value: u64,
    pub size: u64,
    pub binding: SymbolBinding,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self { symbols: HashMap::new() }
    }

    pub fn resolve(&mut self, objects: &[ObjectFile]) -> Result<(), String> {
        for (obj_idx, obj) in objects.iter().enumerate() {
            for sym in &obj.symbols {
                if sym.section_index.is_some() {
                    // Definition
                    if let Some(existing) = self.symbols.get(&sym.name) {
                        if existing.section_index.is_some() && sym.binding == SymbolBinding::Global {
                            return Err(format!("Duplicate global symbol: {}", sym.name));
                        }
                    }
                    self.symbols.insert(sym.name.clone(), ResolvedSymbol {
                        name: sym.name.clone(),
                        object_index: Some(obj_idx),
                        section_index: sym.section_index,
                        value: sym.value,
                        size: sym.size,
                        binding: sym.binding,
                    });
                }
            }
        }
        Ok(())
    }
}
