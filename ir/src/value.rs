// SPDX-License-Identifier: MPL-2.0

use core::fmt;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%v{}", self.0)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%b{}", self.0)
    }
}
