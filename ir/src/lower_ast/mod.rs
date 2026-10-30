// SPDX-License-Identifier: MPL-2.0

pub const AST_FORMAT_VERSION: u32 = 5;

/// Compatibility alias for the explicitly versioned AST format.
pub const SOCKET_VERSION: u32 = AST_FORMAT_VERSION;

pub mod interchange;

pub mod frontend;

pub mod wave_hir;

mod binding;

mod error;

pub mod limits;

#[cfg(whale_wave_ir)]
mod wave_limits;

mod o0;

#[cfg(whale_wave_ir)]
mod wave_lower;

mod support;

pub use error::LowerError;

pub use limits::{AstLimitError, AstLimits, MAX_AST_DEPTH};

#[cfg(not(whale_wave_ir))]
pub use o0::{lower_o0, lower_o0_with_limits};

#[cfg(whale_wave_ir)]
pub fn lower_o0(
    program: &frontend::Program,
    target: &str,
    datalayout: crate::DataLayout,
) -> Result<crate::Module, LowerError> {
    lower_o0_with_limits(program, target, datalayout, AstLimits::default())
}

#[cfg(whale_wave_ir)]
pub fn lower_o0_with_limits(
    program: &frontend::Program,
    target: &str,
    datalayout: crate::DataLayout,
    limits: AstLimits,
) -> Result<crate::Module, LowerError> {
    wave_lower::lower(program, target, datalayout, limits)
}

// Explicit conformance oracle; production Wave lowering never falls back here.
#[doc(hidden)]
pub use o0::{
    lower_o0 as lower_o0_reference, lower_o0_with_limits as lower_o0_reference_with_limits,
};
