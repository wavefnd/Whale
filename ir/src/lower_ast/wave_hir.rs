// SPDX-License-Identifier: MPL-2.0
//! Adapter for the pinned Wave frontend's post-import, post-monomorphization
//! TypedProgram facts. The bounded scalar test surface uses resolved types and
//! ExpressionId values; it does not depend on Wave backend-owned structures.
use super::{frontend as f, AstLimits, LowerError};

pub const WAVE_HIR_REVISION: &str = "e0952ed65d07e14c6ed0aa64f3c9a5532dd7533b";

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct Program {
    pub wave_revision: String,
    pub functions: Vec<Function>,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: ResolvedType,
    pub body: Vec<Statement>,
    pub is_async: bool,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub ty: ResolvedType,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub enum ResolvedType {
    Void,
    Bool,
    Integer { bits: u16, signed: bool },
    Unsupported(String),
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "socket", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct Expression {
    pub id: u64,
    /// Use NumericExpressionInfo.result_type for contextual integer literals;
    /// use HirExpressionType::Resolved for already-resolved named expressions.
    pub resolved_type: ResolvedType,
    pub kind: ExpressionKind,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub enum ExpressionKind {
    Integer(String),
    Bool(bool),
    Local(String),
    Binary { left: Box<Expression>, operator: Operator, right: Box<Expression> },
    Unsupported(Unsupported),
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug)]
#[repr(u64)]
pub enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    ShiftLeft,
    ShiftRight,
    LogicalAnd,
    LogicalOr,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug)]
#[repr(u64)]
pub enum Unsupported {
    Call = 1,
    Aggregate = 2,
    Async = 3,
    InlineAssembly = 4,
    Other = 5,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub enum Statement {
    Variable { name: String, ty: ResolvedType, init: Option<Expression> },
    Assign { name: String, value: Expression },
    If { condition: Expression, body: Vec<Statement>, else_body: Vec<Statement> },
    While { condition: Expression, body: Vec<Statement> },
    Return(Option<Expression>),
    Expression(Expression),
    Unsupported(Unsupported),
}

#[derive(Debug)]
pub enum AdapterError {
    Revision,
    Unavailable,
    Resource,
    InvalidTransport,
    ResolvedType { node: usize },
    Unsupported { node: usize, category: u64 },
    DuplicateExpression { id: u64 },
    UnknownBinding { node: usize },
    NumericLiteral { node: usize },
    DuplicateName { node: usize },
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wave typed-HIR adapter: {self:?}")
    }
}

impl std::error::Error for AdapterError {}

/// Validate the entire typed surface in Wave before publishing any frontend AST.
/// Unsupported calls, aggregate/async/asm nodes and inconsistent facts are errors.
pub fn adapt(program: &Program, limits: AstLimits) -> Result<f::Program, AdapterError> {
    if program.wave_revision != WAVE_HIR_REVISION {
        return Err(AdapterError::Revision);
    }
    #[cfg(whale_wave_ir)]
    {
        native::adapt(program, limits)
    }
    #[cfg(not(whale_wave_ir))]
    {
        let _ = limits;
        Err(AdapterError::Unavailable)
    }
}

/// Direct checked path from resolved Wave facts into the frontend-independent IR.
pub fn lower(
    program: &Program,
    target: &str,
    datalayout: crate::DataLayout,
    limits: AstLimits,
) -> Result<crate::Module, HirLowerError> {
    let program = adapt(program, limits).map_err(HirLowerError::Adapter)?;
    super::lower_o0_with_limits(&program, target, datalayout, limits).map_err(HirLowerError::Lower)
}

#[derive(Debug)]
pub enum HirLowerError {
    Adapter(AdapterError),
    Lower(LowerError),
}

impl std::fmt::Display for HirLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Adapter(error) => error.fmt(f),
            Self::Lower(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for HirLowerError {}

#[cfg(whale_wave_ir)]
mod native {
    use super::*;

    use std::collections::HashMap;

    const NONE: u64 = u64::MAX;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Node([u64; 8]);

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Output {
        ty: u64,
        operation: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Pair([u64; 2]);

    #[repr(C)]
    #[derive(Default)]
    struct ResultRecord {
        status: u64,
        node: u64,
        detail: u64,
        work: u64,
    }

    unsafe extern "C" {
        fn whale_ir_adapt_wave_hir(
            nodes: *const Node,
            node_count: u64,
            edges: *const u64,
            edge_count: u64,
            functions: u64,
            function_count: u64,
            output: *mut Output,
            bindings: *mut Pair,
            identities: *mut Pair,
            identity_capacity: u64,
            max_depth: u64,
            max_work: u64,
            result: *mut ResultRecord,
        ) -> i32;
    }

    macro_rules! node {
        ($this:ident,$tag:expr $(,$field:expr)* $(,)?) => {{
            let fields = [$($field),*];
            $this.node($tag, &fields)
        }};
    }

    struct Transport<'a> {
        nodes: Vec<Node>,
        edges: Vec<u64>,
        names: HashMap<&'a str, u64>,
        expressions: HashMap<usize, usize>,
        types: HashMap<usize, usize>,
        limits: AstLimits,
        strings: usize,
        depth: usize,
    }

    impl<'a> Transport<'a> {
        fn node(&mut self, tag: u64, fields: &[u64]) -> Result<u64, AdapterError> {
            if self.nodes.len() >= self.limits.max_nodes {
                return Err(AdapterError::Resource);
            }
            self.nodes.try_reserve(1).map_err(|_| AdapterError::Resource)?;
            let mut node = Node::default();
            node.0[0] = tag;
            node.0[1..1 + fields.len()].copy_from_slice(fields);
            let index = self.nodes.len() as u64;
            self.nodes.push(node);
            Ok(index)
        }

        fn string(&mut self, text: &str) -> Result<(), AdapterError> {
            self.strings = self
                .strings
                .checked_add(text.len())
                .filter(|n| *n <= self.limits.max_string_bytes)
                .ok_or(AdapterError::Resource)?;
            Ok(())
        }

        fn name(&mut self, text: &'a str) -> Result<u64, AdapterError> {
            self.string(text)?;
            let next = self.names.len() as u64;
            Ok(*self.names.entry(text).or_insert(next))
        }

        fn list<T>(
            &mut self,
            values: &'a [T],
            mut visit: impl FnMut(&mut Self, &'a T) -> Result<u64, AdapterError>,
        ) -> Result<(u64, u64), AdapterError> {
            if values.len() > self.limits.max_nodes.saturating_sub(self.nodes.len()) {
                return Err(AdapterError::Resource);
            }
            let mut list = Vec::new();
            list.try_reserve(values.len()).map_err(|_| AdapterError::Resource)?;
            for value in values {
                list.push(visit(self, value)?);
            }
            if list.len() > self.limits.max_nodes.saturating_sub(self.edges.len()) {
                return Err(AdapterError::Resource);
            }
            self.edges.try_reserve(list.len()).map_err(|_| AdapterError::Resource)?;
            let first = self.edges.len() as u64;
            self.edges.extend(list);
            Ok((first, values.len() as u64))
        }

        fn enter(&mut self) -> Result<(), AdapterError> {
            if self.depth >= self.limits.max_depth.min(super::super::MAX_AST_DEPTH) {
                return Err(AdapterError::Resource);
            }
            self.depth += 1;
            Ok(())
        }

        fn ty(&mut self, ty: &'a ResolvedType) -> Result<u64, AdapterError> {
            let id = match ty {
                ResolvedType::Void => node!(self, 2, 0)?,
                ResolvedType::Bool => node!(self, 2, 1)?,
                ResolvedType::Integer { bits, signed } => {
                    node!(self, 2, 2, u64::from(*bits), u64::from(*signed))?
                },
                ResolvedType::Unsupported(name) => {
                    self.string(name)?;
                    node!(self, 2, 3)?
                },
            };
            self.types.insert(ty as *const _ as usize, id as usize);
            Ok(id)
        }

        fn expression(&mut self, expression: &'a Expression) -> Result<u64, AdapterError> {
            self.enter()?;
            let ty = self.ty(&expression.resolved_type)?;
            let id = match &expression.kind {
                ExpressionKind::Integer(text) => {
                    self.string(text)?;
                    node!(self, 3, expression.id, ty, text.as_ptr() as u64, text.len() as u64)?
                },
                ExpressionKind::Bool(value) => {
                    node!(self, 4, expression.id, ty, u64::from(*value))?
                },
                ExpressionKind::Local(name) => node!(self, 5, expression.id, ty, self.name(name)?)?,
                ExpressionKind::Binary { left, operator, right } => node!(
                    self,
                    6,
                    expression.id,
                    ty,
                    *operator as u64,
                    self.expression(left)?,
                    self.expression(right)?
                )?,
                ExpressionKind::Unsupported(kind) => node!(self, 12, *kind as u64)?,
            };
            self.expressions.insert(expression as *const _ as usize, id as usize);
            self.depth -= 1;
            Ok(id)
        }

        fn statement(&mut self, statement: &'a Statement) -> Result<u64, AdapterError> {
            self.enter()?;
            let id = match statement {
                Statement::Variable { name, ty, init } => node!(
                    self,
                    7,
                    self.name(name)?,
                    self.ty(ty)?,
                    match init {
                        Some(expression) => self.expression(expression)?,
                        None => NONE,
                    }
                )?,
                Statement::Assign { name, value } => {
                    node!(self, 8, self.name(name)?, self.expression(value)?)?
                },
                Statement::If { condition, body, else_body } => {
                    let condition = self.expression(condition)?;
                    let (first, count) = self.list(body, Self::statement)?;
                    let (other, other_count) = self.list(else_body, Self::statement)?;
                    node!(self, 9, condition, first, count, other, other_count)?
                },
                Statement::While { condition, body } => {
                    let condition = self.expression(condition)?;
                    let (first, count) = self.list(body, Self::statement)?;
                    node!(self, 10, condition, first, count)?
                },
                Statement::Return(value) => node!(
                    self,
                    11,
                    match value {
                        Some(expression) => self.expression(expression)?,
                        None => NONE,
                    }
                )?,
                Statement::Expression(expression) => node!(self, 13, self.expression(expression)?)?,
                Statement::Unsupported(kind) => node!(self, 12, *kind as u64)?,
            };
            self.depth -= 1;
            Ok(id)
        }

        fn function(&mut self, function: &'a Function) -> Result<u64, AdapterError> {
            let (params, count) = self.list(&function.parameters, |this, param| {
                node!(this, 1, this.name(&param.name)?, this.ty(&param.ty)?)
            })?;
            let (body, body_count) = self.list(&function.body, Self::statement)?;
            node!(
                self,
                0,
                self.name(&function.name)?,
                params,
                count,
                self.ty(&function.return_type)?,
                body,
                body_count,
                u64::from(function.is_async)
            )
        }

        fn type_ref(
            &self,
            ty: &ResolvedType,
            output: &[Output],
        ) -> Result<f::TypeRef, AdapterError> {
            let index = *self
                .types
                .get(&(ty as *const _ as usize))
                .ok_or(AdapterError::InvalidTransport)?;
            match output[index].ty {
                0 => Ok(f::TypeRef::Void),
                1 => Ok(f::TypeRef::Bool),
                value @ 2..=13 => {
                    let (bits, signed) = match value {
                        2 => (1, true),
                        3 => (1, false),
                        4 => (8, true),
                        5 => (16, true),
                        6 => (32, true),
                        7 => (64, true),
                        8 => (128, true),
                        9 => (8, false),
                        10 => (16, false),
                        11 => (32, false),
                        12 => (64, false),
                        13 => (128, false),
                        _ => unreachable!(),
                    };
                    Ok(f::TypeRef::Int { bits, signed })
                },
                _ => Err(AdapterError::InvalidTransport),
            }
        }

        fn materialize_expr(
            &self,
            expression: &Expression,
            output: &[Output],
        ) -> Result<f::Expr, AdapterError> {
            let index = *self
                .expressions
                .get(&(expression as *const _ as usize))
                .ok_or(AdapterError::InvalidTransport)?;
            let mapped = match &expression.kind {
                ExpressionKind::Integer(value) => {
                    let f::TypeRef::Int { bits, signed } =
                        self.type_ref(&expression.resolved_type, output)?
                    else {
                        return Err(AdapterError::InvalidTransport);
                    };
                    f::Expr::Lit(f::Lit::Int { bits, signed, value: value.clone() })
                },
                ExpressionKind::Bool(value) => f::Expr::Lit(f::Lit::Bool(*value)),
                ExpressionKind::Local(name) => f::Expr::Var(name.clone()),
                ExpressionKind::Binary { left, right, .. } => {
                    let left = Box::new(self.materialize_expr(left, output)?);
                    let right = Box::new(self.materialize_expr(right, output)?);
                    let op = output[index].operation;
                    if op >= 32 {
                        let op = match op {
                            32 => f::CmpOpRef::Eq,
                            33 => f::CmpOpRef::Ne,
                            34 => f::CmpOpRef::Lt,
                            35 => f::CmpOpRef::Le,
                            36 => f::CmpOpRef::Gt,
                            37 => f::CmpOpRef::Ge,
                            _ => return Err(AdapterError::InvalidTransport),
                        };
                        f::Expr::Cmp { left, op, right }
                    } else {
                        let op = match op {
                            0 => f::BinOpRef::Add,
                            1 => f::BinOpRef::Sub,
                            2 => f::BinOpRef::Mul,
                            3 => f::BinOpRef::BitAnd,
                            4 => f::BinOpRef::BitOr,
                            5 => f::BinOpRef::BitXor,
                            6 => f::BinOpRef::Div,
                            7 => f::BinOpRef::Rem,
                            8 => f::BinOpRef::ShiftLeft,
                            9 => f::BinOpRef::ShiftRight,
                            10 => f::BinOpRef::LogicalAnd,
                            11 => f::BinOpRef::LogicalOr,
                            _ => return Err(AdapterError::InvalidTransport),
                        };
                        f::Expr::Binary { left, op, right }
                    }
                },
                ExpressionKind::Unsupported(_) => return Err(AdapterError::InvalidTransport),
            };
            Ok(f::Expr::Located {
                annotation: crate::SourceAnnotation { id: expression.id, source: None },
                expression: Box::new(mapped),
            })
        }

        fn materialize_statements(
            &self,
            statements: &[Statement],
            output: &[Output],
        ) -> Result<Vec<f::Stmt>, AdapterError> {
            statements
                .iter()
                .map(|statement| {
                    Ok(match statement {
                        Statement::Variable { name, ty, init } => f::Stmt::VarDecl {
                            name: name.clone(),
                            ty: self.type_ref(ty, output)?,
                            init: init
                                .as_ref()
                                .map(|expression| self.materialize_expr(expression, output))
                                .transpose()?,
                        },
                        Statement::Assign { name, value } => f::Stmt::Assign {
                            name: name.clone(),
                            value: self.materialize_expr(value, output)?,
                        },
                        Statement::If { condition, body, else_body } => f::Stmt::If {
                            cond: self.materialize_expr(condition, output)?,
                            then_body: self.materialize_statements(body, output)?,
                            else_body: self.materialize_statements(else_body, output)?,
                        },
                        Statement::While { condition, body } => f::Stmt::While {
                            cond: self.materialize_expr(condition, output)?,
                            body: self.materialize_statements(body, output)?,
                        },
                        Statement::Return(value) => f::Stmt::Return(
                            value
                                .as_ref()
                                .map(|expression| self.materialize_expr(expression, output))
                                .transpose()?,
                        ),
                        Statement::Expression(expression) => {
                            f::Stmt::ExprStmt(self.materialize_expr(expression, output)?)
                        },
                        Statement::Unsupported(_) => return Err(AdapterError::InvalidTransport),
                    })
                })
                .collect()
        }
    }

    pub(super) fn adapt(program: &Program, limits: AstLimits) -> Result<f::Program, AdapterError> {
        let mut model = Transport {
            nodes: Vec::new(),
            edges: Vec::new(),
            names: HashMap::new(),
            expressions: HashMap::new(),
            types: HashMap::new(),
            limits,
            strings: 0,
            depth: 0,
        };
        let (first, count) = model.list(&program.functions, Transport::function)?;
        let capacity = model
            .nodes
            .len()
            .checked_mul(2)
            .and_then(usize::checked_next_power_of_two)
            .ok_or(AdapterError::Resource)?
            .max(1);
        let mut output = vec![Output::default(); model.nodes.len()];
        let mut bindings = vec![Pair::default(); model.nodes.len()];
        let mut identities = vec![Pair::default(); capacity];
        let mut result = ResultRecord::default();
        let status = unsafe {
            whale_ir_adapt_wave_hir(
                model.nodes.as_ptr(),
                model.nodes.len() as u64,
                model.edges.as_ptr(),
                model.edges.len() as u64,
                first,
                count,
                output.as_mut_ptr(),
                bindings.as_mut_ptr(),
                identities.as_mut_ptr(),
                capacity as u64,
                limits.max_depth.min(super::super::MAX_AST_DEPTH) as u64,
                limits.max_source_work as u64,
                &mut result,
            )
        };
        if status != 0 || result.status != 0 {
            let node = result.node as usize;
            return Err(match status {
                1 | 2 => AdapterError::Resource,
                4 => AdapterError::ResolvedType { node },
                5 => AdapterError::Unsupported { node, category: result.detail },
                6 => AdapterError::DuplicateExpression { id: result.detail },
                7 => AdapterError::UnknownBinding { node },
                8 => AdapterError::NumericLiteral { node },
                9 => AdapterError::DuplicateName { node },
                _ => AdapterError::InvalidTransport,
            });
        }
        if result.work > limits.max_source_work as u64 {
            return Err(AdapterError::InvalidTransport);
        }
        let functions = program
            .functions
            .iter()
            .map(|function| {
                Ok(f::Function {
                    convention: crate::CallingConvention::Whale,
                    linkage: crate::Linkage::Internal,
                    link_name: None,
                    name: function.name.clone(),
                    parameters: function
                        .parameters
                        .iter()
                        .map(|parameter| {
                            Ok(f::Parameter {
                                name: parameter.name.clone(),
                                ty: model.type_ref(&parameter.ty, &output)?,
                            })
                        })
                        .collect::<Result<_, AdapterError>>()?,
                    return_type: model.type_ref(&function.return_type, &output)?,
                    body: model.materialize_statements(&function.body, &output)?,
                })
            })
            .collect::<Result<_, AdapterError>>()?;
        Ok(f::Program { declarations: vec![], globals: vec![], functions })
    }
}
