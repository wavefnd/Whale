// SPDX-License-Identifier: MPL-2.0
#[cfg(test)]
pub(crate) mod branch;

#[cfg(any(test, not(whale_wave_riscv64)))]
#[path = "../encoding_reference.rs"]
pub(crate) mod reference;
