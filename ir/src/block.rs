// SPDX-License-Identifier: MPL-2.0

use crate::{BlockId, Instruction, Terminator};

#[derive(Clone, Debug)]
pub struct BasicBlock {
    pub id: BlockId,
    pub name: String,
    pub instructions: Vec<Instruction>,
    pub terminator: Option<Terminator>,
}

impl BasicBlock {
    pub fn new(id: BlockId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            instructions: Vec::new(),
            terminator: None,
        }
    }

    pub fn is_terminated(&self) -> bool {
        self.terminator.is_some()
    }
}
