// SPDX-License-Identifier: MPL-2.0

use crate::{ConstValue, DataLayout, Function, Type};

#[derive(Clone, Debug)]
pub struct Global {
    pub name: String,
    pub ty: Type,
    pub init: ConstValue,
    pub align: u32,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub target: String,
    pub datalayout: DataLayout,
    pub globals: Vec<Global>,
    pub functions: Vec<Function>,
}

impl Module {
    pub fn new(target: impl Into<String>, datalayout: DataLayout) -> Self {
        Self {
            target: target.into(),
            datalayout,
            globals: Vec::new(),
            functions: Vec::new(),
        }
    }
}
