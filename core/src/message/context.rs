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

use crate::{
    ids::{MessageId, ProgramId},
    message::{
        Dispatch, HandleMessage, HandlePacket, IncomingMessage, InitMessage, InitPacket, Payload,
        ReplyMessage, ReplyPacket,
    },
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use codec::{Decode, Encode};
use gear_core_errors::MessageError as Error;
use scale_info::TypeInfo;

pub const OUTGOING_LIMIT: u32 = 1024;

/// Context settings.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Decode, Encode, TypeInfo)]
pub struct ContextSettings {
    /// Amount of gas needed for send a message.
    sending_fee: u64,
    /// Limit of outgoung messages that we can send before LimitExceeded.
    outgoing_limit: u32,
}

impl ContextSettings {
    /// Create new ContextSettings.
    pub fn new(sending_fee: u64, outgoing_limit: u32) -> Self {
        Self {
            sending_fee,
            outgoing_limit,
        }
    }
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self::new(0, OUTGOING_LIMIT)
    }
}

/// Context outcome.
///
/// Contains all sendings and wakes that should be done after execution.
#[derive(Default, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Decode, Encode, TypeInfo)]
pub struct ContextOutcome {
    init: Vec<InitMessage>,
    handle: Vec<HandleMessage>,
    reply: Option<ReplyMessage>,
    awakening: Vec<MessageId>,
    // Additional information section.
    program_id: ProgramId,
    source: ProgramId,
    origin_msg_id: MessageId,
}

impl ContextOutcome {
    /// Create new ContextOutcome.
    fn new(program_id: ProgramId, source: ProgramId, origin_msg_id: MessageId) -> Self {
        Self {
            program_id,
            source,
            origin_msg_id,
            ..Default::default()
        }
    }

    /// Destructs outcome after execution and returns provided dispatches and awaken message ids.
    pub fn drain(self) -> (Vec<Dispatch>, Vec<MessageId>) {
        let mut dispatches = Vec::new();

        for msg in self.init.into_iter() {
            dispatches.push(msg.into_dispatch(self.program_id));
        }

        for msg in self.handle.into_iter() {
            dispatches.push(msg.into_dispatch(self.program_id));
        }

        if let Some(msg) = self.reply {
            dispatches.push(msg.into_dispatch(self.program_id, self.source, self.origin_msg_id));
        };

        (dispatches, self.awakening)
    }
}

/// Store of previous message execution context.
#[derive(Clone, Default, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Decode, Encode, TypeInfo)]
pub struct ContextStore {
    outgoing: BTreeMap<u32, Option<Payload>>,
    reply: Option<Payload>,
    initialized: BTreeSet<ProgramId>,
    awaken: BTreeSet<MessageId>,
    reply_sent: bool,
}

/// Context of currently processing incoming message.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Decode, Encode, TypeInfo)]
pub struct MessageContext {
    current: IncomingMessage,
    outcome: ContextOutcome,
    store: ContextStore,
    settings: ContextSettings,
}

impl MessageContext {
    /// Create new MessageContext with default ContextSettings.
    pub fn new(
        message: IncomingMessage,
        program_id: ProgramId,
        store: Option<ContextStore>,
    ) -> Self {
        Self::new_with_settings(message, program_id, store, Default::default())
    }

    /// Create new MessageContext with given ContextSettings.
    pub fn new_with_settings(
        message: IncomingMessage,
        program_id: ProgramId,
        store: Option<ContextStore>,
        settings: ContextSettings,
    ) -> Self {
        Self {
            outcome: ContextOutcome::new(program_id, message.source(), message.id()),
            current: message,
            store: store.unwrap_or_default(),
            settings,
        }
    }

    /// Send a new program initialization message.
    ///
    /// Generates a new message from provided data packet.
    /// Returns message id and generated program id.
    pub fn init_program(&mut self, packet: InitPacket) -> Result<(ProgramId, MessageId), Error> {
        let program_id = packet.destination();

        if self.store.initialized.contains(&program_id) {
            return Err(Error::DuplicateInit);
        }

        let last = self.store.outgoing.len() as u32;

        if last >= self.settings.outgoing_limit {
            return Err(Error::LimitExceeded);
        }

        let message_id = MessageId::generate_outgoing(self.current.id(), last);
        let message = InitMessage::from_packet(message_id, packet);

        self.store.outgoing.insert(last, None);
        self.store.initialized.insert(program_id);
        self.outcome.init.push(message);

        Ok((program_id, message_id))
    }

    /// Send a new program initialization message.
    ///
    /// Generates message from provided data packet and stored by handle payload.
    /// Returns message id.
    pub fn send_commit(&mut self, handle: u32, packet: HandlePacket) -> Result<MessageId, Error> {
        if let Some(payload) = self.store.outgoing.get_mut(&handle) {
            if let Some(data) = payload.take() {
                let packet = {
                    let mut packet = packet;
                    packet.prepend(data);
                    packet
                };

                let message_id = MessageId::generate_outgoing(self.current.id(), handle);
                let message = HandleMessage::from_packet(message_id, packet);

                self.outcome.handle.push(message);

                Ok(message_id)
            } else {
                Err(Error::LateAccess)
            }
        } else {
            Err(Error::OutOfBounds)
        }
    }

    /// Provide space for storing payload for future message creation.
    ///
    /// Returns it's handle.
    pub fn send_init(&mut self) -> Result<u32, Error> {
        let last = self.store.outgoing.len() as u32;

        if last < self.settings.outgoing_limit {
            self.store.outgoing.insert(last, Some(Default::default()));

            Ok(last)
        } else {
            Err(Error::LimitExceeded)
        }
    }

    /// Pushes payload into stored payload by handle.
    pub fn send_push(&mut self, handle: u32, buffer: &[u8]) -> Result<(), Error> {
        match self.store.outgoing.get_mut(&handle) {
            Some(Some(data)) => {
                data.extend_from_slice(buffer);
                Ok(())
            }
            Some(None) => Err(Error::LateAccess),
            None => Err(Error::OutOfBounds),
        }
    }

    /// Send reply message.
    ///
    /// Generates reply from provided data packet and stored reply payload.
    /// Returns message id.
    pub fn reply_commit(&mut self, packet: ReplyPacket) -> Result<MessageId, Error> {
        if !self.store.reply_sent {
            let data = self.store.reply.take().unwrap_or_default();

            let packet = {
                let mut packet = packet;
                packet.prepend(data);
                packet
            };

            let message_id = MessageId::generate_reply(self.current.id(), packet.exit_code());
            let message = ReplyMessage::from_packet(message_id, packet);

            self.outcome.reply = Some(message);
            self.store.reply_sent = true;

            Ok(message_id)
        } else {
            Err(Error::DuplicateReply)
        }
    }

    /// Pushes payload into stored reply payload.
    pub fn reply_push(&mut self, buffer: &[u8]) -> Result<(), Error> {
        if !self.store.reply_sent {
            let data = self.store.reply.get_or_insert_with(Default::default);
            data.extend_from_slice(buffer);

            Ok(())
        } else {
            Err(Error::LateAccess)
        }
    }

    /// Wake message by it's message id.
    pub fn wake(&mut self, waker_id: MessageId) -> Result<(), Error> {
        if self.store.awaken.insert(waker_id) {
            self.outcome.awakening.push(waker_id);

            Ok(())
        } else {
            Err(Error::DuplicateWaking)
        }
    }

    /// Current processing incoming message.
    pub fn current(&self) -> &IncomingMessage {
        &self.current
    }

    /// Current program's id.
    pub fn program_id(&self) -> ProgramId {
        self.outcome.program_id
    }

    /// Destructs context after execution and returns provided outcome and store.
    pub fn drain(self) -> (ContextOutcome, ContextStore) {
        let Self { outcome, store, .. } = self;

        (outcome, store)
    }
}

#[cfg(test)]
mod tests {
    use crate::ids;

    use super::*;
    use alloc::vec;

    #[test]
    fn duplicated_init() {
        let mut message_context =
            MessageContext::new(Default::default(), Default::default(), Default::default());

        assert_eq!(
            message_context.settings.outgoing_limit,
            OUTGOING_LIMIT
        );

        let result = message_context.init_program(Default::default());

        assert!(result.is_ok());

        let duplicated_init = message_context.init_program(Default::default());

        assert_eq!(duplicated_init, Err(Error::DuplicateInit));
    }

    #[test]
    fn outgoing_limit_exceeded() {
        let settings = ContextSettings::new(0, 0);

        let mut message_context = MessageContext::new_with_settings(
            Default::default(),
            Default::default(),
            Default::default(),
            settings,
        );

        let limit_exceeded = message_context.init_program(Default::default());

        assert_eq!(limit_exceeded, Err(Error::LimitExceeded));
    }

    #[test]
    fn commit_out_of_bounds() {
        let mut message_context =
            MessageContext::new(Default::default(), Default::default(), Default::default());

        let out_of_bounds = message_context.send_commit(0, Default::default());

        assert_eq!(out_of_bounds, Err(Error::OutOfBounds));
    }

    #[test]
    fn successful_commit() {
        let mut message_context =
            MessageContext::new(Default::default(), Default::default(), Default::default());

        let result = message_context.init_program(Default::default());
        assert!(result.is_ok());

        let result = message_context.send_init();
        assert!(result.is_ok());

        let handle = result.unwrap();

        let result = message_context.send_commit(handle, Default::default());
        assert!(result.is_ok());
    }

    #[test]
    fn double_reply() {
        let mut message_context =
            MessageContext::new(Default::default(), Default::default(), Default::default());

        let result = message_context.init_program(Default::default());
        assert!(result.is_ok());

        let result = message_context.send_init();
        assert!(result.is_ok());

        let handle = result.unwrap();

        let result = message_context.send_commit(handle, Default::default());
        assert!(result.is_ok());

        let result = message_context.reply_commit(Default::default());
        assert!(result.is_ok());

        let result = message_context.reply_commit(Default::default());
        assert!(matches!(result, Err(Error::DuplicateReply)));
    }

    // Set of constants for clarity of a part of the test
    const INCOMING_MESSAGE_ID: u64 = 3;
    const INCOMING_MESSAGE_SOURCE: u64 = 4;

    #[test]
    /// Test that covers full api of `MessageContext`
    fn message_context_api() {
        // Creating an incoming message around which the runner builds the `MessageContext`
        let incoming_message = IncomingMessage::new(
            MessageId::from(INCOMING_MESSAGE_ID),
            ProgramId::from(INCOMING_MESSAGE_SOURCE),
            vec![1, 2].into(),
            0,
            0,
            None,
        );

        // Creating a message context
        let mut context = MessageContext::new(
            incoming_message,
            ids::ProgramId::from(INCOMING_MESSAGE_ID),
            None,
        );

        // Checking that the initial parameters of the context match the passed constants
        assert_eq!(context.current().id(), MessageId::from(INCOMING_MESSAGE_ID));
        assert!(context.store.reply.is_none());
        assert!(context.outcome.reply.is_none());

        // Creating a reply packet
        let reply_packet = ReplyPacket::new(vec![0, 0], 0);

        // Checking that we are able to initialize reply
        assert!(context.reply_push(&[1, 2, 3]).is_ok());

        // Setting reply message and making sure the operation was successful
        assert!(context.reply_commit(reply_packet.clone()).is_ok());

        // Checking that the `ReplyMessage` matches the passed one
        assert_eq!(
            context.outcome.reply.as_ref().unwrap().payload().to_vec(),
            vec![1, 2, 3, 0, 0],
        );

        // Checking that repeated call `reply_push(...)` returns error and does not do anything
        assert!(context.reply_push(&[1]).is_err());
        assert_eq!(
            context.outcome.reply.as_ref().unwrap().payload().to_vec(),
            vec![1, 2, 3, 0, 0],
        );

        // Checking that repeated call `reply_commit(...)` returns error and does not
        assert!(context.reply_commit(reply_packet.clone()).is_err());

        // Checking that at this point vector of outgoing messages is empty
        assert!(context.outcome.handle.is_empty());

        // Creating an expected handle for a future initialized message
        let expected_handle = 0;

        // Initializing message and compare its handle with expected one
        assert_eq!(
            context.send_init().expect("Error initializing new message"),
            expected_handle
        );

        // And checking that it is not formed
        assert!(context
            .store
            .outgoing
            .get(&expected_handle)
            .expect("This key should be")
            .is_some());

        // Checking that we are able to push payload for the
        // message that we have not committed yet
        assert!(context.send_push(expected_handle, &[5, 7]).is_ok());
        assert!(context.send_push(expected_handle, &[9]).is_ok());

        // Creating an outgoing packet to commit sending by parts
        let commit_packet = HandlePacket::default();

        // Checking if commit is successful
        assert!(context.send_commit(expected_handle, commit_packet).is_ok());

        // Checking that we are **NOT** able to push payload for the message or
        // commit it if we already committed it or directly pushed before
        assert!(context.send_push(0, &[5, 7]).is_err());
        assert!(context.send_push(expected_handle, &[5, 7]).is_err());
        assert!(context.send_commit(0, HandlePacket::default()).is_err());
        assert!(context
            .send_commit(expected_handle, HandlePacket::default())
            .is_err());

        // Checking that we also get an error when trying
        // to commit or send a non-existent message
        assert!(context.send_push(15, &[0]).is_err());
        assert!(context.send_commit(15, HandlePacket::default()).is_err());

        // Creating a handle to init and do not commit later
        // to show that the message will not be sent
        let expected_handle = 1;

        assert_eq!(
            context.send_init().expect("Error initializing new message"),
            expected_handle
        );
        assert!(context.send_push(expected_handle, &[2, 2]).is_ok());

        // Checking that reply message not lost and matches our initial
        assert!(context.outcome.reply.is_some());
        assert_eq!(
            context.outcome.reply.as_ref().unwrap().payload(),
            vec![1, 2, 3, 0, 0]
        );

        // Checking that on drain we get only messages that were fully formed (directly sent or committed)
        let (expected_result, _) = context.drain();
        assert_eq!(expected_result.handle.len(), 1);
        assert_eq!(expected_result.handle[0].payload(), vec![5, 7, 9]);
    }
}
