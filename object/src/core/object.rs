use crate::core::section::{Section, SectionKind};
use crate::core::symbol::ObjectSymbol;
use crate::core::reloc::ObjectRelocation;

pub enum ObjectFormat {
    ELF64,
}

pub struct ObjectFile {
    pub format: ObjectFormat,
    pub sections: Vec<Section>,
    pub symbols: Vec<ObjectSymbol>,
    pub relocations: Vec<ObjectRelocation>,
}

impl ObjectFile {
    pub fn new(format: ObjectFormat) -> Self {
        Self {
            format,
            sections: Vec::new(),
            symbols: Vec::new(),
            relocations: Vec::new(),
        }
    }

    pub fn add_section(&mut self, name: &str, kind: SectionKind, align: u64) -> usize {
        self.sections.push(Section {
            name: name.to_string(),
            kind,
            data: Vec::new(),
            align,
        });
        self.sections.len() - 1
    }

    pub fn write(&self) -> Result<Vec<u8>, String> {
        match self.format {
            ObjectFormat::ELF64 => crate::formats::elf::write_elf(self),
        }
    }
}
