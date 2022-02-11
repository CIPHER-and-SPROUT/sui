// Copyright (c) Mysten Labs
// SPDX-License-Identifier: Apache-2.0

use crate::{base_types::ObjectID, event::Event, object::Object};

/// An abstraction of the (possibly distributed) store for objects, and (soon) events and transactions
pub trait Storage {
    fn reset(&mut self);

    fn read_object(&self, id: &ObjectID) -> Option<Object>;

    fn write_object(&mut self, object: Object);

    /// Record an event that happened during execution  
    fn log_event(&mut self, event: Event);

    fn delete_object(&mut self, id: &ObjectID);
}
