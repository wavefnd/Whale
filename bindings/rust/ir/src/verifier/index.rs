use crate::{Function, Type, ValueId};

use std::{cell::Cell, collections::HashMap, ops::Deref};

/// A borrowed view keeps stored SSA order/identities unchanged. The first
/// annotation wins for lookup; duplicate annotations retain their diagnostic.
pub(super) struct IndexedFunction<'a> {
    function: &'a Function,
    types: HashMap<ValueId, &'a Type>,
    constants: HashMap<ValueId, &'a crate::ConstValue>,
    pub(super) queries: Cell<usize>,
}

impl<'a> IndexedFunction<'a> {
    pub(super) fn new(function: &'a Function) -> Self {
        let mut types = HashMap::with_capacity(function.value_types.len());
        for (value, ty) in &function.value_types {
            types.entry(*value).or_insert(ty);
        }
        let mut constants = HashMap::new();
        for ins in function.blocks.iter().flat_map(|b| &b.instructions) {
            if let crate::Instruction::Const { dst, value, .. }
            | crate::Instruction::ConstDecl { dst, value, .. } = ins
            {
                constants.entry(*dst).or_insert(value);
            }
        }
        Self { function, types, constants, queries: Cell::new(0) }
    }

    pub(super) fn constant(&self, value: ValueId) -> Option<&'a crate::ConstValue> {
        self.constants.get(&value).copied()
    }

    pub(super) fn value_type(&self, value: ValueId) -> Option<&'a Type> {
        self.queries.set(self.queries.get() + 1);
        self.types.get(&value).copied()
    }
}

impl Deref for IndexedFunction<'_> {
    type Target = Function;

    fn deref(&self) -> &Function {
        self.function
    }
}
