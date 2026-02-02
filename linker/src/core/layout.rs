pub struct Layout {
    pub section_offsets: Vec<Vec<u64>>, // [object_idx][section_idx]
    pub section_sizes: Vec<u64>,
    pub base_address: u64,
}

impl Layout {
    pub fn compute(objects: &[object::ObjectFile], base_address: u64) -> Self {
        let mut section_offsets = vec![];
        let mut current_addr = base_address;
        
        // Simple linear layout for now
        for obj in objects {
            let mut obj_offsets = vec![];
            for sec in &obj.sections {
                // Align current_addr
                current_addr = (current_addr + sec.align - 1) & !(sec.align - 1);
                obj_offsets.push(current_addr);
                current_addr += sec.data.len() as u64;
            }
            section_offsets.push(obj_offsets);
        }

        Self {
            section_offsets,
            section_sizes: vec![], // simplified
            base_address,
        }
    }
}
