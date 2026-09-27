// SPDX-License-Identifier: MPL-2.0

pub const AST_FORMAT_VERSION: u32 = 2;
/// Compatibility alias for the explicitly versioned AST format.
pub const SOCKET_VERSION: u32 = AST_FORMAT_VERSION;
pub mod interchange;

pub mod frontend;

mod binding;
mod error;
mod o0;
mod support;

pub use error::LowerError;
pub use o0::lower_o0;
