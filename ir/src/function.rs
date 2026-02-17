// SPDX-License-Identifier: MPL-2.0

use crate::{BasicBlock, BlockId, Type, ValueId};

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub id: ValueId,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_ty: Type,
    pub blocks: Vec<BasicBlock>,
    pub entry: BlockId,

    pub value_types: Vec<(ValueId, Type)>,
}

impl Function {
    pub fn value_type(&self, id: ValueId) -> Option<&Type> {
        self.value_types
            .iter()
            .find(|(vid, _)| *vid == id)
            .map(|(_, t)| t)
    }
}
