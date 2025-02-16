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

//! Manager which handles results of message processing.
//!
//! Should be mentioned, that if message contains value we have a guarantee that it will be sent further in case of successful execution,
//! or sent back in case execution ends up with an error. This guarantee is reached by the following conditions:
//! 1. **Reserve/unreserve model for transferring values**.
//! Ownership over message value is moved not by simple transfer operation, which decreases **free** balance of sender. That is done by
//! reserving value before message is executed and repatriating reserved in favor of beneficiary in case of successful execution, or unreserving
//! in case of execution resulting in a trap. So, it gives us a guarantee that regardless of the result of message execution, there is **always some
//! value** to perform asset management, i.e move tokens further to the recipient or give back to sender. The guarantee is implemented by using
//! corresponding `pallet_balances` functions (`reserve`, `repatriate_reserved`, `unreserve` along with `transfer`) in `pallet_gear` extrinsics,
//! [`JournalHandler::send_dispatch`] and [`JournalHandler::send_value`] procedures.
//!
//! 2. **Balance sufficiency before adding message with value to the queue**.
//! Before message is added to the queue, sender's balance is checked for having adequate amount of assets to send desired value. For actors, who
//! can sign transactions, these checks are done in extrinsic calls. For programs these checks are done on core backend level during execution. In details,
//! when a message is executed, it has some context, which is set from the pallet level, and a part of the context data is program's actual balance (current balance +
//! value sent within the executing message). So if during execution of the original message some other messages were sent, message send call is followed
//! by program's balance checks. The check gives guarantee that value reservation call in [`JournalHandler::send_dispatch`] for program's messages won't fail,
//! because there is always a sufficient balance for the call.
//!
//! 3. **Messages's value management considers existential deposit rule**.
//! It means that before message with value is added to the queue, value is checked to be in the valid range - `{0} ∪ [existential_deposit; +inf)`. This is
//! crucial for programs. The check gives guarantee that if funds were moved to the program, the program will definitely have an account in `pallet_balances`
//! registry and will be able then to manage these funds. Without this check, program could receive funds, but won't be able to use them.
//!
//! Due to these 3 conditions implemented in `pallet_gear`, we have a guarantee that value management calls, performed by user or program, won't fail.

mod journal;
mod task;

pub use journal::*;
pub use task::*;

use crate::{Authorship, Config, CostsPerBlockOf, GasHandlerOf, GearProgramPallet, WaitlistOf};
use codec::{Decode, Encode};
use common::{
    event::*, scheduler::*, storage::*, ActiveProgram, CodeStorage, GasPrice, GasTree, Origin,
    ProgramState,
};
use core_processor::common::{Actor, ExecutableActorData};
use frame_support::traits::{BalanceStatus, Currency, ReservableCurrency};
use gear_core::{
    ids::{CodeId, MessageId, ProgramId},
    message::{ExitCode, StoredDispatch},
    program::Program as NativeProgram,
};
use sp_runtime::traits::{SaturatedConversion, UniqueSaturatedInto};
use sp_std::{
    collections::{btree_map::BTreeMap, btree_set::BTreeSet},
    convert::TryInto,
    marker::PhantomData,
    prelude::*,
};

// Tolerance towards rounding error when converting gas to balance etc.
pub(crate) const TOL: u128 = 10;

#[derive(Clone, Decode, Encode)]
pub enum HandleKind {
    Init(Vec<u8>),
    Handle(ProgramId),
    Reply(MessageId, ExitCode),
}

/// Journal handler implementation for `pallet_gear`.
pub struct ExtManager<T: Config> {
    /// Ids checked that they are users.
    users: BTreeSet<ProgramId>,
    /// Ids checked that they are programs.
    programs: BTreeSet<ProgramId>,
    /// Messages dispatches.
    dispatch_statuses: BTreeMap<MessageId, DispatchStatus>,
    /// Programs, which state changed.
    state_changes: BTreeSet<ProgramId>,
    /// Phantom data for generic usage.
    _phantom: PhantomData<T>,
}

/// Data need for depositing event about queue processing result.
pub struct QueuePostProcessingData {
    /// Message dispatches results.
    pub dispatch_statuses: BTreeMap<MessageId, DispatchStatus>,
    /// Programs, which state changed.
    pub state_changes: BTreeSet<ProgramId>,
}

impl<T: Config> From<ExtManager<T>> for QueuePostProcessingData {
    fn from(ext_manager: ExtManager<T>) -> Self {
        Self {
            dispatch_statuses: ext_manager.dispatch_statuses,
            state_changes: ext_manager.state_changes,
        }
    }
}

impl<T: Config> Default for ExtManager<T>
where
    T::AccountId: Origin,
{
    fn default() -> Self {
        ExtManager {
            _phantom: PhantomData,
            users: Default::default(),
            programs: Default::default(),
            dispatch_statuses: Default::default(),
            state_changes: Default::default(),
        }
    }
}

impl<T: Config> ExtManager<T>
where
    T::AccountId: Origin,
{
    /// Check if id is program and save result.
    pub fn check_program_id(&mut self, id: &ProgramId) -> bool {
        // TODO: research how much need to charge for `program_exists` query.
        if self.programs.contains(id) {
            true
        } else if self.users.contains(id) {
            false
        } else if GearProgramPallet::<T>::program_exists(*id) {
            self.programs.insert(*id);
            true
        } else {
            self.users.insert(*id);
            false
        }
    }

    /// Check if id is user and save result.
    pub fn check_user_id(&mut self, id: &ProgramId) -> bool {
        !self.check_program_id(id)
    }

    /// NOTE: By calling this function we can't differ whether `None` returned, because
    /// program with `id` doesn't exist or it's terminated
    pub fn get_actor(&self, id: ProgramId, with_pages: bool) -> Option<Actor> {
        let active: ActiveProgram = common::get_program(id.into_origin())?.try_into().ok()?;
        let program = {
            let code_id = CodeId::from_origin(active.code_hash);
            let code = T::CodeStorage::get_code(code_id)?;
            NativeProgram::from_parts(
                id,
                code,
                active.allocations,
                matches!(active.state, ProgramState::Initialized),
            )
        };

        let balance = <T as Config>::Currency::free_balance(
            &<T::AccountId as Origin>::from_origin(id.into_origin()),
        )
        .unique_saturated_into();
        let pages_data = if with_pages {
            common::get_program_data_for_pages(id.into_origin(), active.pages_with_data.iter())
                .ok()?
        } else {
            Default::default()
        };

        Some(Actor {
            balance,
            destination_program: id,
            executable_data: Some(ExecutableActorData {
                program,
                pages_data,
            }),
        })
    }

    pub fn set_program(&self, program_id: ProgramId, code_id: CodeId, message_id: MessageId) {
        // Program can be added to the storage only with code, which is done in `submit_program` extrinsic.
        // Code can exist without program, but the latter can't exist without code.
        assert!(
            T::CodeStorage::exists(code_id),
            "Program set must be called only when code exists",
        );

        // An empty program has been just constructed: it contains no mem allocations.
        let program = common::ActiveProgram {
            allocations: Default::default(),
            pages_with_data: Default::default(),
            code_hash: code_id.into_origin(),
            state: common::ProgramState::Uninitialized { message_id },
        };

        common::set_program(program_id.into_origin(), program);
    }

    pub fn charge_for_wake(
        &self,
        message_id: MessageId,
        bn: <T as frame_system::Config>::BlockNumber,
    ) {
        let duration = <frame_system::Pallet<T>>::block_number()
            .saturated_into::<u32>()
            .saturating_sub(bn.saturated_into::<u32>());

        let holding_cost = (duration as u64).saturating_mul(CostsPerBlockOf::<T>::waitlist());

        match GasHandlerOf::<T>::spend(message_id, holding_cost) {
            Ok(_) => {
                match GasHandlerOf::<T>::get_external(message_id) {
                    Ok(maybe_origin) => {
                        if let Some(origin) = maybe_origin {
                            let charge = T::GasPrice::gas_price(holding_cost);
                            if let Some(author) = Authorship::<T>::author() {
                                match <T as Config>::Currency::repatriate_reserved(
                                    &origin,
                                    &author,
                                    charge,
                                    BalanceStatus::Free,
                                ) {
                                    Ok(leftover) => {
                                        if leftover > TOL.unique_saturated_into() {
                                            log::debug!(
                                                target: "essential",
                                                "Reserved funds not fully repatriated from {:?} to 0x{:?}: amount = {:?}, leftover = {:?}",
                                                origin,
                                                author,
                                                charge,
                                                leftover,
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        log::debug!(
                                            target: "essential",
                                            "Failure to repatriate reserves of {:?} from {:?} to 0x{:?}: {:?}",
                                            charge,
                                            origin,
                                            author,
                                            e,
                                        );
                                    }
                                }
                            }
                        } else {
                            log::debug!(
                                target: "essential",
                                "Failed to get origin of {:?}",
                                message_id,
                            );
                        }
                    }
                    Err(_err) => {
                        // We only can get an error here if the gas tree is invalidated
                        // TODO: handle appropriately
                        unreachable!("Can never happen unless gas tree corrupted");
                    }
                }
            }
            Err(err) => {
                log::debug!(
                    target: "essential",
                    "Error charging {:?} of gas rent for awakening message {:?}: {:?}",
                    holding_cost,
                    message_id,
                    err,
                );
            }
        }
    }

    pub fn wake_message_impl(
        &self,
        program_id: ProgramId,
        message_id: MessageId,
    ) -> Option<StoredDispatch> {
        let (waitlisted, bn) = WaitlistOf::<T>::remove(program_id, message_id).ok()?;

        self.charge_for_wake(waitlisted.id(), bn);

        Some(waitlisted)
    }
}
