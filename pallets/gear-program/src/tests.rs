// This file is part of Gear.

// Copyright (C) 2022 Gear Technologies Inc.
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

use super::*;
use crate::mock::*;
use common::{storage::*, ActiveProgram, CodeMetadata, CodeStorage, Origin as _, ProgramState};
use frame_support::{assert_noop, assert_ok};
use gear_core::{
    code::{Code, CodeAndId},
    ids::{CodeId, MessageId, ProgramId},
    memory::{PageBuf, PageNumber, WasmPageNumber},
    message::{DispatchKind, StoredDispatch, StoredMessage},
};
use hex_literal::hex;
use sp_std::collections::btree_map::BTreeMap;
use utils::CreateProgramResult;
use wasm_instrument::gas_metering::ConstantCostRules;

#[test]
fn pause_program_works() {
    new_test_ext().execute_with(|| {
        let raw_code = hex!("0061736d01000000010401600000020f0103656e76066d656d6f727902000103020100070a010668616e646c6500000a040102000b0019046e616d650203010000060d01000a656e762e6d656d6f7279").to_vec();
        let code = Code::try_new(raw_code, 1, |_| ConstantCostRules::default())
            .expect("Error creating Code");

        let code_and_id = CodeAndId::new(code);
        let code_id = code_and_id.code_id();
        let code_hash = code_id.into_origin();

        Pallet::<Test>::add_code(code_and_id, CodeMetadata::new([0; 32].into(), 1)).unwrap();

        let wasm_static_pages = WasmPageNumber(16);
        let memory_pages = {
            let mut pages = BTreeMap::new();
            for page in wasm_static_pages.to_gear_pages_iter() {
                pages.insert(page, PageBuf::new_zeroed());
            }
            for page in (wasm_static_pages + 2.into()).to_gear_pages_iter() {
                pages.insert(page, PageBuf::new_zeroed());
            }
            for i in 0..wasm_static_pages.to_gear_page().0 {
                pages.insert(i.into(), PageBuf::new_zeroed());
            }

            pages
        };
        let allocations = memory_pages.iter().map(|(p, _)| p.to_wasm_page()).collect();
        let pages_with_data = memory_pages.keys().copied().collect();

        let program_id: ProgramId = 1.into();

        common::set_program_and_pages_data(
            program_id.into_origin(),
            ActiveProgram {
                allocations,
                pages_with_data,
                code_hash,
                state: ProgramState::Initialized,
            },
            memory_pages.clone(),
        )
        .expect("memory pages are not valid");

        let msg_id_1: MessageId = 1.into();
        WaitlistOf::<Test>::insert(StoredDispatch::new(
            DispatchKind::Handle,
            StoredMessage::new(msg_id_1, 3.into(), program_id, Default::default(), 0, None),
            None,
        ))
        .expect("Duplicate message is wl");

        let msg_id_2: MessageId = 2.into();
        WaitlistOf::<Test>::insert(StoredDispatch::new(
            DispatchKind::Handle,
            StoredMessage::new(msg_id_2, 4.into(), program_id, Default::default(), 0, None),
            None,
        ))
        .expect("Duplicate message is wl");

        run_to_block(2, None);

        assert_ok!(GearProgram::pause_program(program_id));

        assert!(GearProgram::program_paused(program_id));

        assert!(Pallet::<Test>::get_code(code_id).is_some());

        // although the memory pages should be removed
        assert!(
            common::get_program_data_for_pages(program_id.into_origin(), memory_pages.keys())
                .unwrap()
                .is_empty()
        );

        assert!(WaitlistOf::<Test>::remove(program_id, msg_id_1).is_err());
        assert!(WaitlistOf::<Test>::remove(program_id, msg_id_2).is_err());
    });
}

#[test]
fn pause_program_twice_fails() {
    new_test_ext().execute_with(|| {
        let raw_code = hex!("0061736d01000000010401600000020f0103656e76066d656d6f727902000103020100070a010668616e646c6500000a040102000b0019046e616d650203010000060d01000a656e762e6d656d6f7279").to_vec();
        let code = Code::try_new(raw_code, 1, |_| ConstantCostRules::default())
            .expect("Error creating Code");

        let code_and_id = CodeAndId::new(code);
        let code_hash = code_and_id.code_id().into_origin();

        Pallet::<Test>::add_code(code_and_id, CodeMetadata::new([0; 32].into(), 1)).unwrap();

        let program_id: ProgramId = 1.into();
        common::set_program(
            program_id.into_origin(),
            ActiveProgram {
                allocations: Default::default(),
                pages_with_data: Default::default(),
                code_hash,
                state: ProgramState::Initialized,
            },
        );

        run_to_block(2, None);

        assert_ok!(GearProgram::pause_program(program_id));
        assert_noop!(
            GearProgram::pause_program(program_id),
            PauseError::ProgramNotFound
        );
    });
}

#[test]
fn pause_terminated_program_fails() {
    new_test_ext().execute_with(|| {
        let raw_code = hex!("0061736d01000000010401600000020f0103656e76066d656d6f727902000103020100070a010668616e646c6500000a040102000b0019046e616d650203010000060d01000a656e762e6d656d6f7279").to_vec();
        let code = Code::try_new(raw_code, 1, |_| ConstantCostRules::default())
            .expect("Error creating Code");

        let code_and_id = CodeAndId::new(code);
        let code_hash = code_and_id.code_id().into_origin();

        Pallet::<Test>::add_code(code_and_id, CodeMetadata::new([0; 32].into(), 1)).unwrap();

        let program_id: ProgramId = 1.into();
        common::set_program(
            program_id.into_origin(),
            ActiveProgram {
                allocations: Default::default(),
                pages_with_data: Default::default(),
                code_hash,
                state: ProgramState::Initialized,
            },
        );

        run_to_block(2, None);

        assert_ok!(common::set_program_terminated_status(
            program_id.into_origin()
        ));

        assert_noop!(
            GearProgram::pause_program(program_id),
            PauseError::ProgramTerminated
        );
    });
}

#[test]
fn pause_uninitialized_program_works() {
    new_test_ext().execute_with(|| {
        let static_pages = WasmPageNumber(16);
        let CreateProgramResult {
            program_id,
            code_id,
            init_msg,
            msg_1,
            msg_2,
            memory_pages,
        } = utils::create_uninitialized_program_messages(static_pages);

        run_to_block(2, None);

        assert_ok!(GearProgram::pause_program(program_id));

        assert!(GearProgram::program_paused(program_id));
        assert!(common::get_program(program_id.into_origin()).is_none());

        assert!(Pallet::<Test>::get_code(code_id).is_some());

        // although the memory pages should be removed
        assert!(
            common::get_program_data_for_pages(program_id.into_origin(), memory_pages.keys())
                .unwrap()
                .is_empty()
        );

        assert!(WaitlistOf::<Test>::remove(program_id, msg_1.id()).is_err());
        assert!(WaitlistOf::<Test>::remove(program_id, msg_2.id()).is_err());
        assert!(WaitlistOf::<Test>::remove(program_id, init_msg.id()).is_err());

        assert!(common::waiting_init_take_messages(program_id).is_empty());
    });
}

#[test]
fn resume_uninitialized_program_works() {
    let _ = env_logger::Builder::from_default_env()
        .format_module_path(false)
        .format_level(true)
        .try_init();
    new_test_ext().execute_with(|| {
        let static_pages = WasmPageNumber(16);
        let CreateProgramResult {
            program_id,
            init_msg,
            msg_1,
            msg_2,
            memory_pages,
            ..
        } = utils::create_uninitialized_program_messages(static_pages);

        run_to_block(2, None);

        assert_ok!(GearProgram::pause_program(program_id));

        let wait_list = IntoIterator::into_iter([&init_msg, &msg_1, &msg_2])
            .map(|d| (d.id(), d.clone()))
            .collect::<BTreeMap<_, _>>();

        run_to_block(100, None);
        assert_ok!(GearProgram::resume_program_impl(
            program_id,
            memory_pages.clone(),
            wait_list,
        ));
        assert!(!GearProgram::program_paused(program_id));

        let new_memory_pages =
            common::get_program_data_for_pages(program_id.into_origin(), memory_pages.keys())
                .unwrap();
        assert_eq!(memory_pages, new_memory_pages);

        let waiting_init = common::waiting_init_take_messages(program_id);
        assert_eq!(waiting_init.len(), 2);
        assert!(waiting_init.contains(&msg_1.id()));
        assert!(waiting_init.contains(&msg_2.id()));

        assert_eq!(
            WaitlistOf::<Test>::remove(program_id, init_msg.id())
                .map(|(_, bn)| bn)
                .unwrap(),
            100
        );
        assert_eq!(
            WaitlistOf::<Test>::remove(program_id, msg_1.id())
                .map(|(_, bn)| bn)
                .unwrap(),
            100
        );
        assert_eq!(
            WaitlistOf::<Test>::remove(program_id, msg_2.id())
                .map(|(_, bn)| bn)
                .unwrap(),
            100
        );
    });
}

#[test]
fn resume_program_twice_fails() {
    new_test_ext().execute_with(|| {
        let static_pages = WasmPageNumber(16);
        let CreateProgramResult {
            program_id,
            memory_pages,
            init_msg,
            msg_1,
            msg_2,
            ..
        } = utils::create_uninitialized_program_messages(static_pages);

        run_to_block(2, None);

        assert_ok!(GearProgram::pause_program(program_id));

        let wait_list = IntoIterator::into_iter([init_msg, msg_1, msg_2])
            .map(|d| (d.id(), d))
            .collect::<BTreeMap<_, _>>();

        run_to_block(100, None);

        assert_ok!(GearProgram::resume_program_impl(
            program_id,
            memory_pages.clone(),
            wait_list.clone(),
        ));
        assert_noop!(
            GearProgram::resume_program_impl(program_id, memory_pages, wait_list),
            Error::<Test>::PausedProgramNotFound
        );
    });
}

#[test]
fn resume_program_wrong_memory_fails() {
    new_test_ext().execute_with(|| {
        let static_pages = WasmPageNumber(16);
        let CreateProgramResult {
            program_id,
            mut memory_pages,
            init_msg,
            msg_1,
            msg_2,
            ..
        } = utils::create_uninitialized_program_messages(static_pages);

        run_to_block(2, None);

        assert_ok!(GearProgram::pause_program(program_id));

        run_to_block(100, None);
        memory_pages.remove(&0.into());
        assert_noop!(
            GearProgram::resume_program_impl(
                program_id,
                memory_pages,
                IntoIterator::into_iter([init_msg, msg_1, msg_2])
                    .map(|d| (d.id(), d))
                    .collect()
            ),
            Error::<Test>::WrongMemoryPages
        );
    });
}

#[test]
fn resume_program_wrong_list_fails() {
    new_test_ext().execute_with(|| {
        let static_pages = WasmPageNumber(16);
        let CreateProgramResult {
            program_id,
            memory_pages,
            init_msg,
            msg_1,
            msg_2,
            ..
        } = utils::create_uninitialized_program_messages(static_pages);

        run_to_block(2, None);

        assert_ok!(GearProgram::pause_program(program_id));

        run_to_block(100, None);

        let (kind, message, opt_context) = msg_2.into_parts();

        let msg_2 = StoredDispatch::new(
            kind,
            StoredMessage::new(
                message.id(),
                message.source(),
                message.destination(),
                vec![0, 1, 2, 3, 4, 5],
                message.value(),
                message.reply(),
            ),
            opt_context,
        );

        assert_noop!(
            GearProgram::resume_program_impl(
                program_id,
                memory_pages,
                IntoIterator::into_iter([init_msg, msg_1, msg_2])
                    .map(|d| (d.id(), d))
                    .collect()
            ),
            Error::<Test>::WrongWaitList
        );
    });
}

mod utils {
    use gear_core::memory::PageBuf;

    use super::*;

    pub struct CreateProgramResult {
        pub program_id: ProgramId,
        pub code_id: CodeId,
        pub init_msg: StoredDispatch,
        pub msg_1: StoredDispatch,
        pub msg_2: StoredDispatch,
        pub memory_pages: BTreeMap<PageNumber, PageBuf>,
    }

    pub fn create_uninitialized_program_messages(
        wasm_static_pages: WasmPageNumber,
    ) -> CreateProgramResult {
        let raw_code = hex!("0061736d01000000010401600000020f0103656e76066d656d6f727902000103020100070a010668616e646c6500000a040102000b0019046e616d650203010000060d01000a656e762e6d656d6f7279").to_vec();
        let code = Code::try_new(raw_code, 1, |_| ConstantCostRules::default())
            .expect("Error creating Code");

        let code_and_id = CodeAndId::new(code);
        let code_id = code_and_id.code_id();

        Pallet::<Test>::add_code(code_and_id, CodeMetadata::new([0; 32].into(), 1)).unwrap();

        let memory_pages = {
            let mut pages = BTreeMap::new();
            for page in wasm_static_pages.to_gear_pages_iter() {
                pages.insert(page, PageBuf::new_zeroed());
            }
            for page in (wasm_static_pages + 2.into()).to_gear_pages_iter() {
                pages.insert(page, PageBuf::new_zeroed());
            }
            for i in 0..wasm_static_pages.to_gear_page().0 {
                pages.insert(i.into(), PageBuf::new_zeroed());
            }

            pages
        };
        let allocations = memory_pages.iter().map(|(p, _)| p.to_wasm_page()).collect();
        let pages_with_data = memory_pages.keys().copied().collect();

        let init_msg_id: MessageId = 3.into();
        let program_id: ProgramId = 1.into();
        common::set_program_and_pages_data(
            program_id.into_origin(),
            ActiveProgram {
                allocations,
                pages_with_data,
                code_hash: code_id.into_origin(),
                state: ProgramState::Uninitialized {
                    message_id: init_msg_id,
                },
            },
            memory_pages.clone(),
        )
        .expect("memory_pages has invalid pages number");

        // init message
        let init_msg = StoredDispatch::new(
            DispatchKind::Handle,
            StoredMessage::new(
                init_msg_id,
                3.into(),
                program_id,
                Default::default(),
                0,
                None,
            ),
            None,
        );
        WaitlistOf::<Test>::insert(init_msg.clone()).expect("Duplicate message is wl");

        let msg_id_1: MessageId = 1.into();
        let msg_1 = StoredDispatch::new(
            DispatchKind::Handle,
            StoredMessage::new(msg_id_1, 3.into(), program_id, Default::default(), 0, None),
            None,
        );
        WaitlistOf::<Test>::insert(msg_1.clone()).expect("Duplicate message is wl");
        common::waiting_init_append_message_id(program_id, msg_id_1);

        let msg_id_2 = 2.into();
        let msg_2 = StoredDispatch::new(
            DispatchKind::Handle,
            StoredMessage::new(msg_id_2, 4.into(), program_id, Default::default(), 0, None),
            None,
        );
        WaitlistOf::<Test>::insert(msg_2.clone()).expect("Duplicate message is wl");
        common::waiting_init_append_message_id(program_id, msg_id_2);

        CreateProgramResult {
            program_id,
            code_id,
            init_msg,
            msg_1,
            msg_2,
            memory_pages,
        }
    }
}
