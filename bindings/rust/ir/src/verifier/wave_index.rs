// SPDX-License-Identifier: MPL-2.0
//! Borrowed identity keys; hashing, collision checks and insertion run in Wave.

use super::{AnalysisBudget, VerifyError};

#[repr(C)]
#[derive(Clone, Copy)]
struct Key {
    kind: u64,
    number: u64,
    bytes: *const u8,
    count: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Slot {
    kind: u64,
    number: u64,
    bytes: *const u8,
    count: u64,
    occupied: u64,
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    index: u64,
    expected: u64,
    actual: u64,
    detail: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_verify_index_insert(
        table: *mut Slot,
        capacity: u64,
        key: *const Key,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;

    fn whale_ir_verify_index_contains(
        table: *mut Slot,
        capacity: u64,
        key: *const Key,
        max_work: u64,
        result: *mut ResultRecord,
    ) -> i32;
}

pub(super) struct Index<'a> {
    table: Vec<Slot>,
    borrowed: std::marker::PhantomData<&'a str>,
}

fn resource_error(budget: &AnalysisBudget) -> VerifyError {
    VerifyError::ResourceLimit(crate::LimitError {
        resource: crate::Resource::AnalysisWork,
        limit: budget.limit,
    })
}

impl<'a> Index<'a> {
    pub(super) fn new(count: usize, budget: &AnalysisBudget) -> Result<Self, VerifyError> {
        let capacity = count
            .max(1)
            .checked_mul(2)
            .and_then(usize::checked_next_power_of_two)
            .ok_or_else(|| resource_error(budget))?;
        if capacity > budget.limit.saturating_sub(budget.work.get()) {
            return Err(resource_error(budget));
        }
        budget.work.set(budget.work.get() + capacity);
        let table = vec![
            Slot { kind: 0, number: 0, bytes: std::ptr::null(), count: 0, occupied: 0 };
            capacity
        ];
        Ok(Self { table, borrowed: std::marker::PhantomData })
    }

    pub(super) fn name(
        &mut self,
        name: &'a str,
        budget: &AnalysisBudget,
    ) -> Result<bool, VerifyError> {
        self.run(
            Key { kind: 1, number: 0, bytes: name.as_ptr(), count: name.len() as u64 },
            true,
            budget,
        )
    }

    pub(super) fn id(&mut self, id: u32, budget: &AnalysisBudget) -> Result<bool, VerifyError> {
        self.run(
            Key { kind: 0, number: u64::from(id), bytes: std::ptr::null(), count: 0 },
            true,
            budget,
        )
    }

    pub(super) fn contains_id(
        &mut self,
        id: u32,
        budget: &AnalysisBudget,
    ) -> Result<bool, VerifyError> {
        self.run(
            Key { kind: 0, number: u64::from(id), bytes: std::ptr::null(), count: 0 },
            false,
            budget,
        )
    }

    pub(super) fn bits(
        &mut self,
        value: u128,
        budget: &AnalysisBudget,
    ) -> Result<bool, VerifyError> {
        self.run(
            Key {
                kind: 2,
                number: value as u64,
                bytes: std::ptr::null(),
                count: (value >> 64) as u64,
            },
            true,
            budget,
        )
    }

    fn run(
        &mut self,
        key: Key,
        insert: bool,
        budget: &AnalysisBudget,
    ) -> Result<bool, VerifyError> {
        let remaining = budget.limit.saturating_sub(budget.work.get()) as u64;
        let mut result = ResultRecord::default();
        let status = unsafe {
            if insert {
                whale_ir_verify_index_insert(
                    self.table.as_mut_ptr(),
                    self.table.len() as u64,
                    &key,
                    remaining,
                    &mut result,
                )
            } else {
                whale_ir_verify_index_contains(
                    self.table.as_mut_ptr(),
                    self.table.len() as u64,
                    &key,
                    remaining,
                    &mut result,
                )
            }
        };
        if status != result.status as i32 || result.work > remaining {
            return Err(resource_error(budget));
        }
        budget.work.set(budget.work.get() + result.work as usize);
        match status {
            0 => Ok(insert),
            1 => Ok(!insert),
            _ => Err(resource_error(budget)),
        }
    }
}
