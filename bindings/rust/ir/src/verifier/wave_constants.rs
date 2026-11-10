// SPDX-License-Identifier: MPL-2.0
//! Dependency packet transport; scheduling and stored-result equality run in Wave.
use super::{lookup, Definition, Values, VerifyError};

use crate::constant_wave::{Node, Wire};

use crate::verifier::wave_semantics::Node as TypeNode;

use crate::{ConstEvalError, ConstRef, IrLimits, LimitError, Resource};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawDefinition {
    kind: u64,
    id: u64,
    ty: u64,
    root: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Key {
    kind: u64,
    id: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Slot {
    kind: u64,
    id: u64,
    definition: u64,
    present: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Dependency {
    user: u64,
    next: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Pending {
    remaining: u64,
    head: u64,
    tail: u64,
    mark: u64,
    completed: u64,
}

#[repr(C)]
struct Context {
    definitions: *const RawDefinition,
    definition_count: u64,
    nodes: *const Node,
    node_count: u64,
    types: *const TypeNode,
    type_count: u64,
    type_edges: *const u64,
    type_edge_count: u64,
    base: *const Key,
    base_count: u64,
    imported: *const Slot,
    imported_capacity: u64,
    slots: *mut Slot,
    capacity: u64,
    pending: *mut Pending,
    pending_count: u64,
    dependencies: *mut Dependency,
    dependency_capacity: u64,
    stack: *mut u64,
    stack_capacity: u64,
    order: *mut u64,
    order_capacity: u64,
    max_work: u64,
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    definition: u64,
    reference_kind: u64,
    reference: u64,
    count: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_const_schedule(context: *mut Context, result: *mut ResultRecord) -> i32;

    fn whale_ir_const_value_equal(
        left_kind: u64,
        left_low: u64,
        left_high: u64,
        right_kind: u64,
        right_low: u64,
        right_high: u64,
    ) -> i32;
}

#[derive(Default)]
pub(super) struct Index {
    slots: Vec<Slot>,
}

fn resource(limits: IrLimits) -> ConstEvalError {
    ConstEvalError::ResourceLimit(LimitError {
        resource: Resource::AnalysisWork,
        limit: limits.max_analysis_work,
    })
}

fn backend() -> ConstEvalError {
    ConstEvalError::Backend("constant dependency transport")
}

fn allocate<T: Clone + Default>(count: usize, limits: IrLimits) -> Result<Vec<T>, ConstEvalError> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| resource(limits))?;
    values.resize(count, T::default());
    Ok(values)
}

fn key(reference: ConstRef) -> Key {
    match reference {
        ConstRef::Global(id) => Key { kind: 1, id: id.0.into() },
        ConstRef::Local(id) => Key { kind: 2, id: id.0.into() },
    }
}

pub(super) fn evaluate(
    scope: &str,
    definitions: Vec<Definition<'_>>,
    mut values: Values,
    globals: &Values,
    limits: IrLimits,
) -> Result<Values, VerifyError> {
    if definitions.is_empty() {
        return Ok(values);
    }
    let error = |at: usize, reason| VerifyError::InvalidConstExpression {
        scope: scope.into(),
        declaration: definitions[at].id,
        reason,
    };
    let mut wire = Wire::new(limits);
    let mut budget = crate::limits::Budget::new(limits)
        .map_err(|e| error(0, ConstEvalError::ResourceLimit(e)))?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(definitions.len()).map_err(|_| error(0, resource(limits)))?;
    for definition in &definitions {
        budget
            .ty(definition.ty)
            .and_then(|_| budget.expression(definition.expression))
            .map_err(|e| error(0, ConstEvalError::ResourceLimit(e)))?;
        let root = wire.expression(definition.expression).map_err(|e| error(0, e))?;
        let k = key(definition.id);
        raw.push(RawDefinition { kind: k.kind, id: k.id, ty: wire.types.ty(definition.ty), root });
    }
    let mut base = Vec::new();
    base.try_reserve_exact(values.len()).map_err(|_| error(0, resource(limits)))?;
    base.extend(values.keys().copied().map(key));
    base.sort_unstable_by_key(|key| (key.kind, key.id));
    let capacity = raw
        .len()
        .checked_add(base.len())
        .and_then(|n| n.max(1).checked_mul(2))
        .and_then(usize::checked_next_power_of_two)
        .ok_or_else(|| error(0, resource(limits)))?;
    if capacity > limits.max_analysis_work {
        return Err(error(0, resource(limits)));
    }
    let mut slots = allocate::<Slot>(capacity, limits).map_err(|e| error(0, e))?;
    let mut pending = allocate::<Pending>(raw.len(), limits).map_err(|e| error(0, e))?;
    let mut dependencies =
        allocate::<Dependency>(wire.nodes.len(), limits).map_err(|e| error(0, e))?;
    let mut stack = allocate::<u64>(limits.max_const_depth.saturating_add(2), limits)
        .map_err(|e| error(0, e))?;
    let mut order = allocate::<u64>(raw.len(), limits).map_err(|e| error(0, e))?;
    let imported = match &globals.index {
        Some(index) => index.slots.as_slice(),
        None if globals.is_empty() => &[],
        None => return Err(error(0, backend())),
    };
    let mut context = Context {
        definitions: raw.as_ptr(),
        definition_count: raw.len() as u64,
        nodes: wire.nodes.as_ptr(),
        node_count: wire.nodes.len() as u64,
        types: wire.types.nodes.as_ptr(),
        type_count: wire.types.nodes.len() as u64,
        type_edges: wire.types.edges.as_ptr(),
        type_edge_count: wire.types.edges.len() as u64,
        base: base.as_ptr(),
        base_count: base.len() as u64,
        imported: imported.as_ptr(),
        imported_capacity: imported.len() as u64,
        slots: slots.as_mut_ptr(),
        capacity: slots.len() as u64,
        pending: pending.as_mut_ptr(),
        pending_count: pending.len() as u64,
        dependencies: dependencies.as_mut_ptr(),
        dependency_capacity: dependencies.len() as u64,
        stack: stack.as_mut_ptr(),
        stack_capacity: stack.len() as u64,
        order: order.as_mut_ptr(),
        order_capacity: order.len() as u64,
        max_work: limits.max_analysis_work as u64,
    };
    let mut result = ResultRecord::default();
    let status = unsafe { whale_ir_const_schedule(&mut context, &mut result) };
    if i32::try_from(result.status).ok() != Some(status)
        || result.work > limits.max_analysis_work as u64
        || result.count > raw.len() as u64
    {
        return Err(error(0, backend()));
    }
    let at = usize::try_from(result.definition)
        .ok()
        .filter(|at| *at < definitions.len())
        .ok_or_else(|| error(0, backend()))?;
    match status {
        0 | 4 => {},
        1 => return Err(error(at, ConstEvalError::DuplicateDefinition(definitions[at].id))),
        2 => return Err(error(at, ConstEvalError::TypeMismatch)),
        3 => {
            let id = u32::try_from(result.reference).map_err(|_| error(at, backend()))?;
            let reference = match result.reference_kind {
                1 => ConstRef::Global(crate::GlobalId(id)),
                2 => ConstRef::Local(crate::ValueId(id)),
                _ => return Err(error(at, backend())),
            };
            return Err(error(at, ConstEvalError::UnknownReference(reference)));
        },
        7 => return Err(error(at, resource(limits))),
        _ => return Err(error(at, backend())),
    }
    if status == 0 && result.count != raw.len() as u64 {
        return Err(error(0, backend()));
    }
    order.truncate(result.count as usize);
    let mut seen = allocate::<bool>(raw.len(), limits).map_err(|e| error(0, e))?;
    for id in order {
        let i = usize::try_from(id)
            .ok()
            .filter(|i| *i < raw.len())
            .ok_or_else(|| error(0, backend()))?;
        if seen[i] {
            return Err(error(i, backend()));
        }
        seen[i] = true;
        let definition = &definitions[i];
        let computed = definition
            .expression
            .evaluate_with_limits(&|reference| lookup(&values, globals, reference).cloned(), limits)
            .map_err(|reason| error(i, reason))?;
        let (left_kind, left_low, left_high) = crate::constant_wave::payload(&computed);
        let (right_kind, right_low, right_high) = crate::constant_wave::payload(definition.result);
        match unsafe {
            whale_ir_const_value_equal(
                left_kind, left_low, left_high, right_kind, right_low, right_high,
            )
        } {
            1 => {},
            0 => return Err(error(i, ConstEvalError::ResultMismatch)),
            _ => return Err(error(i, backend())),
        }
        values.insert(definition.id, (definition.ty.clone(), computed));
    }
    if status == 4 {
        return Err(error(at, ConstEvalError::CyclicReference));
    }
    values.index = Some(Index { slots });
    Ok(values)
}
