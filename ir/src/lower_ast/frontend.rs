// SPDX-License-Identifier: MPL-2.0

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct Program {
    pub declarations: Vec<FunctionDeclaration>,
    pub globals: Vec<GlobalConst>,
    pub functions: Vec<Function>,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct GlobalConst {
    pub name: String,
    pub ty: TypeRef,
    pub init: Expr, // const expression (Lit/Binary/Cmp/Var(=other const))
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct Function {
    pub convention: crate::CallingConvention,
    pub linkage: crate::Linkage,
    pub link_name: Option<String>,
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: TypeRef,
    pub body: Vec<Stmt>,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub ty: TypeRef,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub enum TypeRef {
    Void,
    Bool,
    Int { bits: u16, signed: bool }, // i/u
    Float { bits: u16 },             // f16/f32/f64...
    Ptr(Box<TypeRef>),
    FnPtr(Box<SignatureRef>),
    Array { elem: Box<TypeRef>, len: u64 },
    Opaque(String),
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub enum Stmt {
    Return(Option<Expr>),

    ConstDecl {
        name: String,
        ty: TypeRef,
        init: Expr, // const expression only
    },

    VarDecl {
        name: String,
        ty: TypeRef,
        init: Option<Expr>,
    },
    Assign {
        name: String,
        value: Expr,
    },
    ExprStmt(Expr),
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Break,
    Continue,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub enum Expr {
    FunctionRef(String),
    NullFunction(SignatureRef),
    Call {
        callee: CalleeRef,
        args: Vec<Expr>,
    },
    Var(String),
    Lit(Lit),
    Binary {
        left: Box<Expr>,
        op: BinOpRef,
        right: Box<Expr>,
    },
    Cmp {
        left: Box<Expr>,
        op: CmpOpRef,
        right: Box<Expr>,
    },
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug, Copy)]
pub enum CmpOpRef {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub enum Lit {
    Bool(bool),
    Int {
        bits: u16,
        signed: bool,
        value: String,
    },
    Float {
        bits: u16,
        value: String,
    },
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug, Copy)]
pub enum BinOpRef {
    Add,
    Sub,
    Mul,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct SignatureRef {
    pub params: Vec<TypeRef>,
    pub ret: TypeRef,
    pub convention: crate::CallingConvention,
    pub variadic: bool,
}
#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct FunctionDeclaration {
    pub name: String,
    pub signature: SignatureRef,
    pub linkage: crate::Linkage,
    pub link_name: Option<String>,
}
#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub enum CalleeRef {
    Direct(String),
    Indirect(Box<Expr>),
}
