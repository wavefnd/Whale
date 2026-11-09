pub use crate::ast::AsmSectionKind;

use crate::error::AsmError;

use crate::tokens::tokenize;

use crate::traits::ISA;

pub struct AssemblerOutput {
    pub sections: Vec<AsmSection>,
    pub symbols: Vec<AsmSymbol>,
}

pub struct AsmSection {
    pub kind: AsmSectionKind,
    /// Maximum power-of-two alignment requested by this section.
    pub align: u64,
    pub name: String,
    pub data: Vec<u8>,
    /// Unbacked zero storage, used by .bss instead of allocating bytes.
    pub zero_fill: u64,
    pub relocs: Vec<Relocation>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    #[default]
    NoType,
    Function,
    Object,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SymbolVisibility {
    #[default]
    Default,
    Hidden,
}

pub struct AsmSymbol {
    pub kind: SymbolKind,
    pub size: u64,
    pub visibility: SymbolVisibility,
    pub name: String,
    pub section_index: Option<usize>,
    pub offset: u64,
    pub is_global: bool,
}

#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: usize,
    pub symbol: String,
    pub kind: RelocKind,
    pub addend: i64,
}

#[derive(Debug, Clone)]
pub enum RelocKind {
    /// Opaque relocation interpreted only by the owning target implementation.
    TargetSpecific {
        namespace: String,
        code: u32,
    },
    Absolute64,
    Absolute32,
    /// A sign-extended absolute disp32 address (ELF R_X86_64_32S).
    SignedAbsolute32,
    /// A PC-relative data/address reference, independent of symbol definition.
    Relative32,
    /// A near control transfer; an undefined target may require a PLT entry.
    Branch32,
    Relative8,
}

/// Limits for assembly implementations. Sparse BSS does not consume materialized output.
#[derive(Debug, Clone, Copy)]
pub struct AssemblyLimits {
    /// Total materialized bytes across every section, checked before allocation.
    pub max_output_bytes: usize,
    /// A changed label layout at this pass limit is an error, never an artifact.
    pub max_relax_iterations: usize,
}

impl Default for AssemblyLimits {
    fn default() -> Self {
        Self { max_output_bytes: 64 * 1024 * 1024, max_relax_iterations: 1024 }
    }
}

pub fn assemble(source: &str, isa: &(impl ISA + ?Sized)) -> Result<AssemblerOutput, AsmError> {
    assemble_with_limits(source, isa, &AssemblyLimits::default())
}

pub fn assemble_with_limits(
    source: &str,
    isa: &(impl ISA + ?Sized),
    limits: &AssemblyLimits,
) -> Result<AssemblerOutput, AsmError> {
    assemble_named_with_limits("<input>", source, isa, limits)
}

/// Assemble a named UTF-8 source with filename/range diagnostics.
pub fn assemble_named(
    source_name: &str,
    source: &str,
    isa: &(impl ISA + ?Sized),
) -> Result<AssemblerOutput, AsmError> {
    assemble_named_with_limits(source_name, source, isa, &AssemblyLimits::default())
}

pub fn assemble_named_with_limits(
    source_name: &str,
    source: &str,
    isa: &(impl ISA + ?Sized),
    limits: &AssemblyLimits,
) -> Result<AssemblerOutput, AsmError> {
    let tokens = tokenize(source).map_err(|error| error.with_source_name(source_name))?;
    let mut ast = isa.parse(&tokens).map_err(|error| error.with_source_name(source_name))?;
    ast.source_name = source_name.into();
    isa.encode_with_limits(&ast, limits).map_err(|error| ast.error_at(error, 0, None))
}
