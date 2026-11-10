// SPDX-License-Identifier: MPL-2.0
//! Source-level extension boundary: each implementation owns its parser and AST.
use crate::{AsmError, AssemblerOutput, AssemblyLimits};

/// Unlike the optional shared token/AST utilities, this trait imposes no syntax.
/// Limits must be enforced before allocating output, not just after emission.
pub trait Assembler: Send + Sync {
    fn id(&self) -> &str;

    /// Native object identity, if this implementation emits native machine code.
    /// Custom bytecode assemblers can keep the default and select their own writer.
    fn object_target(&self) -> Option<target_spec::ObjectTarget> {
        None
    }

    fn assemble(
        &self,
        source_name: &str,
        source: &str,
        limits: &AssemblyLimits,
    ) -> Result<AssemblerOutput, AsmError>;
}
