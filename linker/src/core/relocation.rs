//! Checked, transactional application of the static AMD64 relocation subset.
use super::{layout::Layout, symbol_table::SymbolTable};

use object::{ObjectFile, RelocKind};

use std::{collections::BTreeMap, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocationErrorKind {
    InvalidInput,
    Resolution,
    Layout,
    Unsupported,
    Range,
    Overlap,
    Limit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelocationError {
    pub kind: RelocationErrorKind,
    pub object: Option<usize>,
    pub section: Option<usize>,
    pub offset: Option<usize>,
    pub message: String,
}

impl fmt::Display for RelocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "relocation {:?}", self.kind)?;
        if let Some(object) = self.object {
            write!(f, " in object {object}")?;
        }
        if let Some(section) = self.section {
            write!(f, " section {section}")?;
        }
        if let Some(offset) = self.offset {
            write!(f, " at offset {offset}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for RelocationError {}

fn error(kind: RelocationErrorKind, message: impl Into<String>) -> RelocationError {
    RelocationError { kind, object: None, section: None, offset: None, message: message.into() }
}

/// Return patched, independently owned section payloads. No caller input is mutated,
/// and no partial output is returned on any failure. PLT32 resolves directly to a
/// statically selected definition; GOT/dynamic relocation machinery is unsupported.
pub fn apply_relocations(
    objects: &[ObjectFile],
    layout: &Layout,
    max_bytes: u64,
) -> Result<Vec<Vec<Vec<u8>>>, RelocationError> {
    let mut symbols = SymbolTable::new();
    symbols.resolve(objects).map_err(|e| error(RelocationErrorKind::Resolution, e))?;

    let mut total = 0u64;
    for (oi, object) in objects.iter().enumerate() {
        for (si, section) in object.sections.iter().enumerate() {
            let address =
                layout.section_offsets.get(oi).and_then(|x| x.get(si)).ok_or_else(|| {
                    error(RelocationErrorKind::Layout, "missing section placement")
                })?;
            address
                .checked_add(
                    section
                        .memory_size()
                        .map_err(|e| error(RelocationErrorKind::InvalidInput, e))?,
                )
                .ok_or_else(|| error(RelocationErrorKind::Layout, "section address overflow"))?;
            total = total
                .checked_add(section.data.len() as u64)
                .filter(|n| *n <= max_bytes)
                .ok_or_else(|| {
                    error(RelocationErrorKind::Limit, "section payload budget exceeded")
                })?;
        }
    }

    let mut patches = Vec::new();
    let mut occupied: BTreeMap<(usize, usize, usize), usize> = BTreeMap::new();
    for (relocation, reference) in
        objects.iter().flat_map(|o| o.relocations.iter()).zip(&symbols.references)
    {
        let oi = reference.object_index;
        let si = reference.section_index;
        let object = &objects[oi];
        let result = (|| {
            let symbol = &symbols.symbols[&reference.target];
            let target = match (symbol.object_index, symbol.section_index) {
                (Some(owner), Some(section)) => layout
                    .section_offsets
                    .get(owner)
                    .and_then(|x| x.get(section))
                    .and_then(|base| base.checked_add(symbol.value))
                    .ok_or_else(|| {
                        error(
                            RelocationErrorKind::Layout,
                            "symbol address overflow or missing placement",
                        )
                    })?,
                (None, None) if reference.weak_undefined => 0,
                _ => {
                    return Err(error(
                        RelocationErrorKind::Unsupported,
                        "unsupported symbol address class",
                    ))
                },
            };
            let place = layout.section_offsets[oi][si]
                .checked_add(relocation.offset as u64)
                .ok_or_else(|| error(RelocationErrorKind::Layout, "relocation place overflow"))?;

            let absolute = i128::from(target) + i128::from(relocation.addend);
            let (value, width) = match relocation.kind {
                RelocKind::Absolute64 => (absolute, 8),
                RelocKind::Absolute32 | RelocKind::SignedAbsolute32 => (absolute, 4),
                RelocKind::Relative32 | RelocKind::PLT32 => (absolute - i128::from(place), 4),
                RelocKind::Relative8 => (absolute - i128::from(place), 1),
                RelocKind::GOTPCREL | RelocKind::TargetSpecific { .. } => {
                    return Err(error(
                        RelocationErrorKind::Unsupported,
                        "GOTPCREL requires unsupported dynamic/GOT structures",
                    ))
                },
            };

            let fits = match relocation.kind {
                RelocKind::Absolute64 => u64::try_from(value).is_ok(),
                RelocKind::Absolute32 => u32::try_from(value).is_ok(),
                RelocKind::SignedAbsolute32 | RelocKind::Relative32 | RelocKind::PLT32 => {
                    i32::try_from(value).is_ok()
                },
                RelocKind::Relative8 => i8::try_from(value).is_ok(),
                RelocKind::GOTPCREL | RelocKind::TargetSpecific { .. } => false,
            };

            if !fits {
                return Err(error(
                    RelocationErrorKind::Range,
                    format!("{:?} value {value} is out of range", relocation.kind),
                ));
            }

            let end = relocation
                .offset
                .checked_add(width)
                .filter(|end| *end <= object.sections[si].data.len())
                .ok_or_else(|| {
                    error(RelocationErrorKind::Range, "patch exceeds section payload")
                })?;

            let key = (oi, si, relocation.offset);
            if occupied
                .range(..=key)
                .next_back()
                .is_some_and(|(&(o, s, _), &e)| o == oi && s == si && e > relocation.offset)
                || occupied
                    .range(key..)
                    .next()
                    .is_some_and(|(&(o, s, start), _)| o == oi && s == si && start < end)
            {
                return Err(error(RelocationErrorKind::Overlap, "overlapping relocation patches"));
            }
            occupied.insert(key, end);
            Ok((oi, si, relocation.offset, width, (value as u64).to_le_bytes()))
        })();

        patches.push(result.map_err(|mut e: RelocationError| {
            e.object = Some(oi);
            e.section = Some(si);
            e.offset = Some(reference.offset);
            e
        })?);
    }

    let mut payloads: Vec<Vec<Vec<u8>>> =
        objects.iter().map(|o| o.sections.iter().map(|s| s.data.clone()).collect()).collect();
    for (oi, si, offset, width, bytes) in patches {
        payloads[oi][si][offset..offset + width].copy_from_slice(&bytes[..width]);
    }

    Ok(payloads)
}

impl diagnostic::ToDiagnostic for RelocationError {
    fn diagnostic(&self) -> diagnostic::Diagnostic {
        use diagnostic::{Context, Diagnostic};
        let code = match self.kind {
            RelocationErrorKind::InvalidInput => "link.relocation.input",
            RelocationErrorKind::Resolution => "link.relocation.symbol",
            RelocationErrorKind::Layout => "link.relocation.layout",
            RelocationErrorKind::Unsupported => "link.relocation.unsupported",
            RelocationErrorKind::Range => "link.relocation.range",
            RelocationErrorKind::Overlap => "link.relocation.overlap",
            RelocationErrorKind::Limit => "link.relocation.limit",
        };
        let mut d = Diagnostic::new("linker", "relocation", code, self.message.clone());
        if let Some(o) = self.object {
            d.contexts.push(Context::Input(o));
        }
        if let Some(s) = self.section {
            d.contexts.push(Context::Section { index: s, name: None });
        }
        if let Some(at) = self.offset {
            d.contexts.push(Context::SectionOffset(at as u64));
        }
        d
    }
}
