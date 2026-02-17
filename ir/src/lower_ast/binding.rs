// SPDX-License-Identifier: MPL-2.0

use crate::{ConstValue, Type, ValueId};

#[derive(Clone, Debug)]
pub(crate) enum Binding {
    Addr { ptr: ValueId, ty: Type }, // ptr<ty>
    Const { ty: Type, value: ConstValue },
}