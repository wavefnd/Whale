#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Text,
    Data,
    Bss,
    ReadOnlyData,
}

pub struct Section {
    pub name: String,
    pub kind: SectionKind,
    pub data: Vec<u8>,
    pub align: u64,
}
