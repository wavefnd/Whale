// SPDX-License-Identifier: MPL-2.0

use crate::Type;

#[derive(Debug)]
pub enum LowerError {
    UnsupportedType(String),
    UnknownVariable(String),
    TypeMismatch { expected: Type, got: Type },
    MissingInit(String),
    UnsupportedStmt,
    UnsupportedExpr,
    BreakOutsideLoop,
    ContinueOutsideLoop,
    DuplicateGlobal(String),
    AssignToConst(String),
    NonConstExpr,
}
