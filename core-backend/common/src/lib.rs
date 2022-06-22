// This file is part of Gear.

// Copyright (C) 2021-2022 Gear Technologies Inc.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Crate provides support for wasm runtime.

#![no_std]

extern crate alloc;

pub mod error_processor;
pub mod funcs;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};
use core::fmt;
use gear_core::{
    env::Ext,
    gas::GasAmount,
    ids::{CodeId, MessageId, ProgramId},
    memory::{Memory, PageBuf, PageNumber, WasmPageNumber},
    message::{ContextStore, Dispatch, DispatchKind},
};
use gear_core_errors::{ExtError, MemoryError};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TerminationReason {
    Exit(ProgramId),
    Leave,
    Success,
    Trap(TrapExplanation),
    Wait,
    GasAllowanceExceeded,
}

#[derive(Debug, Clone, derive_more::Display, Eq, PartialEq)]
pub enum TrapExplanation {
    #[display(fmt = "{}", _0)]
    Core(ExtError),
    #[display(fmt = "{}", _0)]
    Other(String),
    #[display(fmt = "Unable to call a forbidden function")]
    ForbiddenFunction,
    #[display(fmt = "Reason is unknown. Possibly `unreachable` instruction is occurred")]
    Unknown,
}

pub struct ExtInfo {
    pub gas_amount: GasAmount,
    pub allocations: BTreeSet<WasmPageNumber>,
    pub pages_data: BTreeMap<PageNumber, PageBuf>,
    pub generated_dispatches: Vec<Dispatch>,
    pub awakening: Vec<MessageId>,
    pub program_candidates_data: BTreeMap<CodeId, Vec<(ProgramId, MessageId)>>,
    pub context_store: ContextStore,
}

pub trait IntoExtInfo {
    fn into_ext_info(
        self,
        memory: &dyn Memory,
    ) -> Result<(ExtInfo, Option<TrapExplanation>), (MemoryError, GasAmount)>;

    fn into_gas_amount(self) -> GasAmount;

    fn last_error(&self) -> Option<&ExtError>;
}

pub struct BackendReport {
    pub termination: TerminationReason,
    pub info: ExtInfo,
}

#[derive(Debug, derive_more::Display)]
#[display(fmt = "{}", reason)]
pub struct BackendError<T> {
    pub gas_amount: GasAmount,
    pub reason: T,
}

pub trait Environment<E: Ext + IntoExtInfo + 'static>: Sized {
    /// An error issues in environment
    type Error: fmt::Display;

    /// Creates new external environment to execute wasm binary:
    /// 1) instatiates wasm binary.
    /// 2) creates wasm memory with filled data (execption if lazy pages enabled).
    /// 3) instatiate external funcs for wasm module.
    fn new(
        ext: E,
        binary: &[u8],
        entries: BTreeSet<DispatchKind>,
        mem_size: WasmPageNumber,
    ) -> Result<Self, BackendError<Self::Error>>;

    /// Returns addr to the stack end if it can be identified
    fn get_stack_mem_end(&mut self) -> Option<WasmPageNumber>;

    /// Get ref to mem wrapper
    fn get_mem(&self) -> &dyn Memory;

    /// Get mut ref to mem wrapper
    fn get_mem_mut(&mut self) -> &mut dyn Memory;

    /// Run instance setup starting at `entry_point` - wasm export function name.
    /// Also runs `post_execution_handler` after running instance at provided entry point.
    fn execute<F, T>(
        self,
        entry_point: &DispatchKind,
        post_execution_handler: F,
    ) -> Result<BackendReport, BackendError<Self::Error>>
    where
        F: FnOnce(&dyn Memory) -> Result<(), T>,
        T: fmt::Display;

    /// Consumes environment and returns gas state.
    fn into_gas_amount(self) -> GasAmount;
}

pub trait AsTerminationReason {
    fn as_termination_reason(&self) -> Option<&TerminationReason>;
}
