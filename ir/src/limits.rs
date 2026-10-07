// SPDX-License-Identifier: MPL-2.0
//! Bounded traversal before recursive type cloning, comparison or diagnostics.
use crate::{ConstExpr, ConstExprKind, FunctionSignature, Instruction, Module, Terminator, Type};
use std::fmt;

/// Recursive downstream type helpers and text parsing use this safety ceiling.
/// Callers can lower depth budgets, or raise them up to this bound.
pub const MAX_IR_NESTING: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrLimits {
    pub max_type_depth: usize,
    pub max_const_depth: usize,
    /// Total traversal nodes, including annotated types and expression nodes.
    pub max_nodes: usize,
    pub max_input_bytes: usize,
    pub max_tokens: usize,
}
impl Default for IrLimits {
    fn default() -> Self {
        Self {
            max_type_depth: 128,
            max_const_depth: 128,
            max_nodes: 1_000_000,
            max_input_bytes: 8 * 1024 * 1024,
            max_tokens: 1_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    TypeDepth,
    ConstantDepth,
    Nodes,
    InputBytes,
    Tokens,
    Configuration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitError {
    pub resource: Resource,
    pub limit: usize,
}
impl fmt::Display for LimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IR {:?} limit exceeded (limit {})",
            self.resource, self.limit
        )
    }
}
impl std::error::Error for LimitError {}
impl IrLimits {
    pub fn validate(self) -> Result<(), LimitError> {
        if self.max_type_depth > MAX_IR_NESTING || self.max_const_depth > MAX_IR_NESTING {
            return Err(LimitError {
                resource: Resource::Configuration,
                limit: MAX_IR_NESTING,
            });
        }
        Ok(())
    }
}

pub(crate) struct Budget {
    pub limits: IrLimits,
    remaining: usize,
}
impl Budget {
    pub fn new(limits: IrLimits) -> Result<Self, LimitError> {
        limits.validate()?;
        Ok(Self {
            limits,
            remaining: limits.max_nodes,
        })
    }
    pub fn nodes(&mut self, n: usize) -> Result<(), LimitError> {
        self.remaining = self.remaining.checked_sub(n).ok_or(LimitError {
            resource: Resource::Nodes,
            limit: self.limits.max_nodes,
        })?;
        Ok(())
    }
    pub fn ty(&mut self, root: &Type) -> Result<(), LimitError> {
        let mut pending = vec![(root, 0)];
        while let Some((ty, depth)) = pending.pop() {
            self.nodes(1)?;
            if depth > self.limits.max_type_depth {
                return Err(LimitError {
                    resource: Resource::TypeDepth,
                    limit: self.limits.max_type_depth,
                });
            }
            let mut push = |ty| pending.push((ty, depth + 1));
            match ty {
                Type::Ptr(t) | Type::Array(t, _) => push(t),
                Type::Struct(ts) | Type::Tuple(ts) => {
                    // Do not allocate a worklist for an oversized wide node.
                    if ts.len() > self.remaining.saturating_sub(pending.len()) {
                        return Err(LimitError {
                            resource: Resource::Nodes,
                            limit: self.limits.max_nodes,
                        });
                    }
                    pending.extend(ts.iter().rev().map(|t| (t, depth + 1)));
                }
                Type::FnPtr(sig) => {
                    if sig.params.len() >= self.remaining.saturating_sub(pending.len()) {
                        return Err(LimitError {
                            resource: Resource::Nodes,
                            limit: self.limits.max_nodes,
                        });
                    }
                    pending.push((&sig.ret, depth + 1));
                    pending.extend(sig.params.iter().rev().map(|t| (t, depth + 1)));
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub fn signature(&mut self, sig: &FunctionSignature) -> Result<(), LimitError> {
        self.nodes(1)?;
        self.ty(&sig.ret)?;
        for t in &sig.params {
            self.ty(t)?;
        }
        Ok(())
    }
    pub fn expression(&mut self, root: &ConstExpr) -> Result<(), LimitError> {
        let mut pending = vec![(root, 0)];
        while let Some((e, depth)) = pending.pop() {
            self.nodes(1)?;
            if depth > self.limits.max_const_depth {
                return Err(LimitError {
                    resource: Resource::ConstantDepth,
                    limit: self.limits.max_const_depth,
                });
            }
            self.ty(&e.ty)?;
            match &e.kind {
                ConstExprKind::Binary { left, right, .. }
                | ConstExprKind::Compare { left, right, .. } => {
                    pending.push((right, depth + 1));
                    pending.push((left, depth + 1));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub(crate) fn check_module(m: &Module, limits: IrLimits) -> Result<(), LimitError> {
    let mut b = Budget::new(limits)?;
    for g in &m.globals {
        b.nodes(1)?;
        b.ty(&g.ty)?;
        b.expression(&g.init_expr)?;
    }
    for d in &m.declarations {
        b.nodes(1)?;
        b.signature(&d.signature)?;
    }
    for f in &m.functions {
        b.nodes(1)?;
        b.ty(&f.ret_ty)?;
        for p in &f.params {
            b.nodes(1)?;
            b.ty(&p.ty)?;
        }
        for (_, ty) in &f.value_types {
            b.nodes(1)?;
            b.ty(ty)?;
        }
        for block in &f.blocks {
            b.nodes(1)?;
            for ins in &block.instructions {
                b.nodes(1)?;
                use Instruction::*;
                match ins {
                    ConstDecl { expression, .. } => b.expression(expression)?,
                    Const { ty, .. }
                    | Undef { ty, .. }
                    | Mov { ty, .. }
                    | Bin { ty, .. }
                    | Not { ty, .. }
                    | Cmp { ty, .. }
                    | ICmp { ty, .. }
                    | FCmp { ty, .. }
                    | Select { ty, .. }
                    | Checked { ty, .. }
                    | Alloca { ty, .. }
                    | Load { ty, .. }
                    | Store { ty, .. }
                    | Uninit { ty, .. } => b.ty(ty)?,
                    Phi { ty, incomings, .. } => {
                        b.ty(ty)?;
                        b.nodes(incomings.len())?;
                    }
                    Cast { src_ty, dst_ty, .. } => {
                        b.ty(src_ty)?;
                        b.ty(dst_ty)?;
                    }
                    Extract { dst_ty, .. } => b.ty(dst_ty)?,
                    Gep {
                        dst_ty, indices, ..
                    } => {
                        b.ty(dst_ty)?;
                        b.nodes(indices.len())?;
                    }
                    NullFunction { signature, .. } | FunctionAddr { signature, .. } => {
                        b.signature(signature)?
                    }
                    Call { ret_ty, args, .. } => {
                        b.ty(ret_ty)?;
                        b.nodes(args.len())?;
                    }
                    _ => {}
                }
            }
            if let Some(t) = &block.terminator {
                b.nodes(1)?;
                match t {
                    Terminator::Ret { ty, .. } => b.ty(ty)?,
                    Terminator::Switch { ty, cases, .. } => {
                        b.ty(ty)?;
                        b.nodes(cases.len())?;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

/// Dispose of a rejected owned type without recursively dropping its tree.
pub(crate) fn discard_type(root: Type) {
    let mut pending = vec![root];
    while let Some(ty) = pending.pop() {
        match ty {
            Type::Ptr(t) | Type::Array(t, _) => pending.push(*t),
            Type::Struct(ts) | Type::Tuple(ts) => pending.extend(ts),
            Type::FnPtr(sig) => {
                pending.push(sig.ret);
                pending.extend(sig.params);
            }
            _ => {}
        }
    }
}
pub(crate) fn discard_signature(sig: FunctionSignature) {
    discard_type(sig.ret);
    for t in sig.params {
        discard_type(t);
    }
}
