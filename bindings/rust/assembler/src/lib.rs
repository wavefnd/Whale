pub mod assembler;

pub mod ast;

pub mod error;

pub mod tokens;

pub mod traits;

pub use assembler::*;

pub use ast::MemoryAddressMode;

pub use error::{AsmError, SourceSpan};

mod diagnostic;

pub mod extension;

pub mod fixed;

pub use target_spec::{Machine, ObjectTarget};

/// The implementation actually selected by this owning module.
pub fn module_identity() -> &'static str {
    #[cfg(whale_wave_assembler)]
    {
        unsafe extern "C" {
            fn whale_assembler_module_identity() -> u64;
        }
        // This no-argument stamp is exported by this module's guarded archive.
        if unsafe { whale_assembler_module_identity() } == 6289348675458126593 {
            "wave"
        } else {
            "invalid-wave"
        }
    }
    #[cfg(not(whale_wave_assembler))]
    {
        "rust-reference"
    }
}

#[cfg(test)]
mod implementation_identity_tests {
    #[test]
    fn selected_implementation_has_its_own_compiled_identity() {
        assert_eq!(
            super::module_identity(),
            if cfg!(whale_wave_assembler) { "wave" } else { "rust-reference" }
        );
    }
}
