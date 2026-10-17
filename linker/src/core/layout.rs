use object::{ObjectFile, SectionKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionPlacement {
    pub object_index: usize,
    pub section_index: usize,
    pub kind: SectionKind,
    pub align: u64,
    pub file_offset: u64,
    pub memory_address: u64,
    pub file_size: u64,
    pub memory_size: u64,
}

#[derive(Debug)]
pub struct Layout {
    /// Memory addresses indexed by input object and section.
    pub section_offsets: Vec<Vec<u64>>,
    /// Logical sizes in input object/section order.
    pub section_sizes: Vec<u64>,
    pub sections: Vec<SectionPlacement>,
    pub base_address: u64,
    /// Payload region sizes, excluding executable headers and segment construction.
    pub file_size: u64,
    pub memory_size: u64,
}

impl Layout {
    /// Compute deterministic section placement. Zero object alignment means one.
    /// BSS consumes memory but never advances the file cursor.
    /// ELF program headers and load-segment permissions are a later writer step.
    pub fn compute(objects: &[ObjectFile], base_address: u64) -> Result<Self, String> {
        let mut layout = Self {
            section_offsets: Vec::new(),
            section_sizes: Vec::new(),
            sections: Vec::new(),
            base_address,
            file_size: 0,
            memory_size: 0,
        };

        let mut memory_end = base_address;
        for (object_index, obj) in objects.iter().enumerate() {
            obj.target.validate_amd64().map_err(|e| format!("link input {object_index}: {e}"))?;
            let mut offsets = Vec::new();
            for (section_index, sec) in obj.sections.iter().enumerate() {
                let context =
                    format!("input {object_index} section {section_index} {:?}", sec.name);

                let align = sec.align.max(1);
                if !align.is_power_of_two() {
                    return Err(format!("{context}: alignment must be zero or a power of two"));
                }

                let memory_size = sec.memory_size()?;
                let file_size = sec.file_size();
                let memory_address = align_up(memory_end, align)
                    .ok_or_else(|| format!("{context}: memory alignment overflow"))?;
                memory_end = memory_address
                    .checked_add(memory_size)
                    .ok_or_else(|| format!("{context}: memory size overflow"))?;
                let file_offset = align_up(layout.file_size, align)
                    .ok_or_else(|| format!("{context}: file alignment overflow"))?;

                if sec.kind != SectionKind::Bss {
                    layout.file_size = file_offset
                        .checked_add(file_size)
                        .ok_or_else(|| format!("{context}: file size overflow"))?;
                }
                offsets.push(memory_address);
                layout.section_sizes.push(memory_size);
                layout.sections.push(SectionPlacement {
                    object_index,
                    section_index,
                    kind: sec.kind,
                    align,
                    file_offset,
                    memory_address,
                    file_size,
                    memory_size,
                });
            }
            layout.section_offsets.push(offsets);
        }

        layout.memory_size = memory_end - base_address;
        Ok(layout)
    }
}

fn align_up(value: u64, align: u64) -> Option<u64> {
    value.checked_add(value.wrapping_neg() & (align - 1))
}
