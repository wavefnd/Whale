use crate::core::reloc::ObjectRelocation;

use crate::core::section::{Section, SectionKind};

use crate::core::symbol::ObjectSymbol;

use std::collections::HashSet;

pub use target_spec::{Endian, Machine, ObjectFormat, ObjectTarget, Target};

#[derive(Clone)]
pub struct ObjectFile {
    pub target: ObjectTarget,
    pub sections: Vec<Section>,
    pub symbols: Vec<ObjectSymbol>,
    pub relocations: Vec<ObjectRelocation>,
}

impl ObjectFile {
    /// Construct an AMD64 little-endian object in the requested format.
    pub fn new(format: ObjectFormat) -> Self {
        Self::with_target(ObjectTarget { format, ..Target::X86_64WhaleLinux.object_target() })
    }

    /// Retain explicit input identity; writers and link consumers validate support.
    pub fn with_target(target: ObjectTarget) -> Self {
        Self { target, sections: Vec::new(), symbols: Vec::new(), relocations: Vec::new() }
    }

    pub fn add_section(&mut self, name: &str, kind: SectionKind, align: u64) -> usize {
        self.sections.push(Section {
            name: name.to_string(),
            kind,
            data: Vec::new(),
            zero_fill: 0,
            align,
        });
        self.sections.len() - 1
    }

    /// Validate in-memory structure before writing or consuming this object.
    /// This does not allocate payloads, serialize, or apply an output-size budget.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_diagnostic().map_err(|e| e.to_string())
    }

    /// Typed structural validation; legacy `validate` renders this same cause.
    pub fn validate_diagnostic(&self) -> Result<(), crate::ObjectError> {
        use crate::{ObjectError as E, ObjectErrorKind as K};
        let error = |kind, message: String| E::new("validate", kind, message);
        self.target.validate().map_err(|e| error(K::InvalidTarget, e.to_string()))?;
        for (index, section) in self.sections.iter().enumerate() {
            if section.kind != SectionKind::Bss && section.zero_fill != 0 {
                return Err(error(
                    K::InvalidSection,
                    format!("section {:?}: zero_fill is only valid for BSS", section.name),
                )
                .section(index));
            }
            if section.kind == SectionKind::Bss && section.data.iter().any(|b| *b != 0) {
                return Err(error(
                    K::InvalidSection,
                    format!("BSS section {:?} has a nonzero initializer", section.name),
                )
                .section(index));
            }
            (section.data.len() as u64).checked_add(section.zero_fill).ok_or_else(|| {
                error(K::Overflow, format!("section {:?}: memory size overflow", section.name))
                    .section(index)
            })?;
            if section.name.contains('\0') {
                return Err(error(
                    K::InvalidName,
                    format!("section name {:?} contains embedded NUL byte", section.name),
                )
                .section(index));
            }
            if section.align != 0 && !section.align.is_power_of_two() {
                return Err(error(
                    K::InvalidAlignment,
                    format!(
                        "section {:?} alignment {} must be zero or a power of two",
                        section.name, section.align
                    ),
                )
                .section(index));
            }
        }

        let mut names = HashSet::new();
        for symbol in &self.symbols {
            if symbol.name.contains('\0') {
                return Err(error(
                    K::InvalidName,
                    format!("symbol name {:?} contains embedded NUL byte", symbol.name),
                )
                .symbol(&symbol.name));
            }
            if !names.insert(&symbol.name) {
                return Err(error(
                    K::DuplicateSymbol,
                    format!("duplicate symbol name {:?}", symbol.name),
                )
                .symbol(&symbol.name));
            }
            if let Some(index) = symbol.section_index {
                let section = self.sections.get(index).ok_or_else(|| {
                    error(
                        K::InvalidSection,
                        format!("symbol {:?} references invalid section {index}", symbol.name),
                    )
                    .section(index)
                    .symbol(&symbol.name)
                })?;
                let end = symbol.value.checked_add(symbol.size).ok_or_else(|| {
                    error(K::Overflow, format!("symbol range overflow for {:?}", symbol.name))
                        .symbol(&symbol.name)
                })?;
                if end > section.memory_size().expect("validated section size") {
                    return Err(error(
                        K::OutOfBounds,
                        format!(
                            "symbol range for {:?} exceeds section {:?} size {}",
                            symbol.name,
                            section.name,
                            section.memory_size().unwrap()
                        ),
                    )
                    .section(index)
                    .symbol(&symbol.name));
                }
            }
        }
        for relocation in &self.relocations {
            if relocation.symbol.contains('\0') {
                return Err(error(
                    K::InvalidName,
                    format!(
                        "relocation reference {:?} contains embedded NUL byte",
                        relocation.symbol
                    ),
                )
                .symbol(&relocation.symbol));
            }
            let section = self.sections.get(relocation.section_index).ok_or_else(|| {
                error(
                    K::InvalidSection,
                    format!(
                        "relocation for {:?} references invalid section {}",
                        relocation.symbol, relocation.section_index
                    ),
                )
                .section(relocation.section_index)
            })?;
            if section.kind == SectionKind::Bss {
                return Err(error(
                    K::InvalidRelocation,
                    format!(
                        "relocations targeting BSS section {:?} are not supported",
                        section.name
                    ),
                )
                .section(relocation.section_index));
            }
            let (_, width) = relocation
                .kind
                .encoding(self.target)
                .map_err(|e| error(K::InvalidRelocation, e).section(relocation.section_index))?;
            let end = relocation.offset.checked_add(width).ok_or_else(|| {
                error(
                    K::Overflow,
                    format!(
                        "relocation range overflow in section {:?} at offset {}",
                        section.name, relocation.offset
                    ),
                )
                .section(relocation.section_index)
            })?;
            if end > section.data.len() {
                return Err(error(
                    K::OutOfBounds,
                    format!(
                        "relocation range {}..{} exceeds section {:?} size {}",
                        relocation.offset,
                        end,
                        section.name,
                        section.data.len()
                    ),
                )
                .section(relocation.section_index));
            }
        }
        Ok(())
    }

    pub fn read(bytes: &[u8]) -> Result<Self, crate::ObjectReadError> {
        Self::read_with_limits(bytes, crate::ObjectReadLimits::default())
    }

    pub fn read_with_limits(
        bytes: &[u8],
        limits: crate::ObjectReadLimits,
    ) -> Result<Self, crate::ObjectReadError> {
        crate::formats::elf_read::read_elf(bytes, limits)
    }

    /// Decode with actual successful resource charges for a shared input budget.
    /// Auxiliary ELF sections/symbols and repeated name scans remain accounted.
    pub fn read_with_usage(
        bytes: &[u8],
        limits: crate::ObjectReadLimits,
    ) -> Result<(Self, crate::ObjectReadUsage), crate::ObjectReadError> {
        crate::formats::elf_read::read_elf_with_usage(bytes, limits)
    }

    /// Typed counterpart of `write`, including output budgets and allocation causes.
    pub fn write_diagnostic(&self) -> Result<Vec<u8>, crate::ObjectError> {
        self.write_diagnostic_with_limit(crate::formats::elf::DEFAULT_MAX_OUTPUT_SIZE)
    }

    pub fn write_diagnostic_with_limit(
        &self,
        max_output_size: u64,
    ) -> Result<Vec<u8>, crate::ObjectError> {
        crate::formats::elf::write_elf_checked(self, max_output_size)
    }

    pub fn write_with_limit(&self, max_output_size: u64) -> Result<Vec<u8>, String> {
        match self.target.format {
            ObjectFormat::ELF64 => crate::formats::elf::write_elf_with_limit(self, max_output_size),
        }
    }

    pub fn write(&self) -> Result<Vec<u8>, String> {
        match self.target.format {
            ObjectFormat::ELF64 => crate::formats::elf::write_elf(self),
        }
    }
}
