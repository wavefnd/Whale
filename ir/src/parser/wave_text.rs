// SPDX-License-Identifier: MPL-2.0
//! Borrowed token transport shared by the Wave grammar parsers.

use super::lexer::{Kind, Token};

#[repr(C)]
pub(super) struct RawToken {
    pub data: *const u8,
    pub length: u64,
    pub kind: u64,
}

impl From<&Token<'_>> for RawToken {
    fn from(token: &Token<'_>) -> Self {
        // Grammar parsing only inspects the kind of decoded string tokens.
        // Do not borrow their allocation: the compatibility parser can move
        // strings out of its tokens while these descriptors remain alive.
        let (bytes, kind): (&[u8], u64) = match &token.kind {
            Kind::Word(s) => (s.as_bytes(), 0),
            Kind::String(_) => (&[], 1),
            Kind::Mark(s) if *s == "<eof>" => (s.as_bytes(), 3),
            Kind::Mark(s) => (s.as_bytes(), 2),
        };
        Self { data: bytes.as_ptr(), length: bytes.len() as u64, kind }
    }
}
