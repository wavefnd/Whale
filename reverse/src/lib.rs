// SPDX-License-Identifier: MPL-2.0
//! Bounded ELF/raw image inspection and typed reverse-analysis records.
//! Decoding and CFG analysis currently return explicit capability errors.
mod cfg;

mod model;

pub use cfg::*;

pub use model::*;

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    CoreUnavailable,
    Invalid { offset: usize },
    Truncated { offset: usize },
    Unsupported { offset: usize },
    Overlong { offset: usize },
    Capacity,
    AbiMismatch { expected: u64, actual: u64 },
    InvalidOutput(&'static str),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoreUnavailable => f.write_str("reverse decoder is unavailable in this build"),
            Self::Invalid { offset } => {
                write!(f, "invalid AMD64 encoding or input range at byte {offset}")
            },
            Self::Truncated { offset } => write!(f, "truncated AMD64 instruction at byte {offset}"),
            Self::Unsupported { offset } => {
                write!(f, "unsupported AMD64 encoding at byte {offset}")
            },
            Self::Overlong { offset } => {
                write!(f, "AMD64 instruction exceeds 15 bytes at byte {offset}")
            },
            Self::Capacity => f.write_str("reverse output capacity exhausted"),
            Self::AbiMismatch { expected, actual } => {
                write!(f, "reverse ABI mismatch: expected {expected}, got {actual}")
            },
            Self::InvalidOutput(reason) => write!(f, "invalid reverse result: {reason}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Whether instruction decoding and control-flow analysis are available.
pub const fn backend_available() -> bool {
    false
}

pub const fn backend_name() -> &'static str {
    "unavailable"
}

/// Report the missing decoding capability without interpreting input bytes.
pub fn decode(_input: &[u8], _offset: usize) -> Result<Instruction, DecodeError> {
    Err(DecodeError::CoreUnavailable)
}

pub mod format;

pub mod image;
