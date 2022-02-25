// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[test_only]
module Sui::TestScenario {
    use Sui::ID::{Self, VersionedID, IDBytes};
    use Sui::Transfer;
    use Sui::TxContext::{Self, TxContext};
    use Std::Vector;

    /// Attempted an operation that required a concluded transaction, but there are none
    const ENO_CONCLUDED_TRANSACTIONS: u64 = 0;

    /// Requested a transfer or user-defined event on an invalid transaction index
    const EINVALID_TX_INDEX: u64 = 1;

    /// Attempted to return an object to the inventory that was not previously removed from the
    /// inventory during the current transaction. Can happen if the user attempts to call
    /// `return_object` on a locally constructed object rather than one returned from a `TestScenario`
    /// function such as `remove_object`.
    const ECANT_RETURN_OBJECT: u64 = 2;

    /// Attempted to retrieve an object of a particular type from the inventory, but it is empty.
    /// Can happen if the user already transferred the object or a previous transaction failed to
    /// transfer the object to the user.
    const EEMPTY_INVENTORY: u64 = 3;

    /// Expected 1 object of this type in the tx sender's inventory, but found >1. 
    /// Consider using TestScenario::remove_object_by_id to select a specific object
    const EINVENTORY_AMBIGUITY: u64 = 4;

    /// The inventory previously contained an object of this type, but it was removed during the current
    /// transaction.
    const EALREADY_REMOVED_OBJECT: u64 = 5;

    /// Utility for mocking a multi-transaction Sui execution in a single Move procedure.
    /// A `Scenario` maintains a view of the global object pool built up by the execution. 
    /// These objects can be accessed via functions like `remove_object`, which gives the
    /// transaction sender access to (only) objects in their inventory.
    /// Example usage:
    /// ```
    /// let addr1: address = 0;
    /// let addr2: address = 1;
    /// // begin a test scenario in a context where addr1 is the sender
    /// let scenario = &mut TestScenario::begin(&addr1);
    /// // addr1 sends an object to addr2
    /// {
    ///     let some_object: SomeObject = ... // construct an object
    ///     Transfer::transfer(some_object, copy addr2)
    /// };
    /// // end the first transaction and begin a new one where addr2 is the sender
    /// TestScenario::next_tx(scenario, &addr2)        
    /// {
    ///     // remove the SomeObject value from addr2's inventory
    ///     let obj = TestScenario::remove_object<SomeObject>(scenario);
    ///     // use it to test some function that needs this value
    ///     SomeObject::some_function(obj)         
    /// }
    /// ... // more txes
    /// ```
    struct Scenario has drop {
        ctx: TxContext,
        /// Object ID's that have been removed during the current transaction. Needed to prevent
        /// double removals
        removed: vector<IDBytes>,
        /// The `i`th entry in this vector is the start index for events emitted by the `i`th transaction.
        /// This information allows us to partition events emitted by distinct transactions
        event_start_indexes: vector<u64>,
    }

    /// Begin a new multi-transaction test scenario in a context where `sender` is the tx sender
    public fun begin(sender: &address): Scenario {
        Scenario { 
            ctx: TxContext::new_from_address(*sender, 0),
            removed: Vector::empty(),
            event_start_indexes: vector[0],
        }
    }

    /// Advance the scenario to a new transaction where `sender` is the transaction sender
    public fun next_tx(scenario: &mut Scenario, sender: &address) {
        let last_tx_start_index = last_tx_start_index(scenario);
        let old_total_events = last_tx_start_index;

        // emit dummy Wrapped events for every removed object wrapped during the current tx.
        // we know an object was wrapped if:
        // - it was removed and not returned
        // - it does not appear in a transfer event
        // - its ID does not appear in a delete id event
        let transferred_ids = transferred_object_ids(last_tx_start_index);
        let deleted_ids = deleted_object_ids(last_tx_start_index);
        let i = 0;
        let removed = &scenario.removed;
        let num_removed = Vector::length(removed);
        while (i < num_removed) {
            let removed_id = ID::get_bytes_as_vec(Vector::borrow(removed, i));
            if (!Vector::contains(&transferred_ids, &removed_id) && !Vector::contains(&deleted_ids, &removed_id)) {
                // removed_id was wrapped by this transaction. emit a wrapped event
                emit_wrapped_object_event(removed_id)
            };
            i = i + 1
        };
        // reset `removed` for the next tx
        scenario.removed = Vector::empty();
       
        // start index for the next tx is the end index for the current one
        let new_total_events = num_events();
        let tx_event_count = new_total_events - old_total_events;
        let event_end_index = last_tx_start_index + tx_event_count;
        Vector::push_back(&mut scenario.event_start_indexes, event_end_index);

        // create a seed for new transaction digest to ensure that this tx has a different
        // digest (and consequently, different object ID's) than the previous tx
        let new_tx_digest_seed = (Vector::length(&scenario.event_start_indexes) as u8);
        scenario.ctx = TxContext::new_from_address(*sender, new_tx_digest_seed);
    }

    /// Remove the object of type `T` from the inventory of the current tx sender in `scenario`.
    /// An object is in the sender's inventory if:
    /// - The object is in the global event log
    /// - The sender owns the object, or the object is immutable
    /// - If the object was previously removed, it was subsequently replaced via a call to `return_object`.
    /// Aborts if there is no object of type `T` in the inventory of the tx sender
    /// Aborts if there is >1 object of type `T` in the inventory of the tx sender--this function
    /// only succeeds when the object to choose is unambiguous. In cases where there are multiple `T`'s, 
    /// the caller should resolve the ambiguity by using `remove_object_by_id`.
    public fun remove_object<T: key>(scenario: &mut Scenario): T {
        let sender = get_signer_address(scenario);
        remove_unique_object(scenario, sender)
    }

    /// Remove and return the child object of type `T2` owned by `parent_obj`.
    /// Aborts if there is no object of type `T2` owned by `parent_obj`
    /// Aborts if there is >1 object of type `T2` owned by `parent_obj`--this function
    /// only succeeds when the object to choose is unambiguous. In cases where there are are multiple `T`'s
    /// owned by `parent_obj`, the caller should resolve the ambiguity using `remove_nested_object_by_id`.
    public fun remove_nested_object<T1: key, T2: key>(
        scenario: &mut Scenario, parent_obj: &T1
    ): T2 {
        remove_unique_object(scenario, *ID::get_bytes(ID::get_id_bytes(parent_obj)))        
    }

    /// Same as `remove_object`, but returns the object of type `T` with object ID `id`.
    /// Should only be used in cases where current tx sender has more than one object of
    /// type `T` in their inventory.
    public fun remove_object_by_id<T: key>(_scenario: &mut Scenario, _id: IDBytes): T {
        // TODO: implement me
        abort(100)
    }

    /// Same as `remove_nested_object`, but returns the child object of type `T` with object ID `id`.
    /// Should only be used in cases where the parent object has more than one child of type `T`.
    public fun remove_nested_object_by_id<T1: key, T2: key>(
        _scenario: &mut Scenario, _parent_obj: &T1, _child_id: IDBytes
    ): T2 {  
        // TODO: implement me
        abort(200)
    }

    /// Return `t` to the global object pool maintained by `scenario`.
    /// Subsequent calls to `remove_object<T>` will succeed if the object is in the inventory of the current
    /// transaction sender.
    /// Aborts if `t` was not previously removed from the inventory via a call to `remove_object` or similar.
    public fun return_object<T: key>(scenario: &mut Scenario, t: T) {
        let id = ID::get_id_bytes(&t);
        let removed = &mut scenario.removed;
        // TODO: add Vector::remove_element to Std that does this 3-liner
        let (is_mem, idx) = Vector::index_of(removed, id);
        // can't return an object we haven't removed
        assert!(is_mem, ECANT_RETURN_OBJECT);
        Vector::remove(removed, idx);

        // Model object return as a self transfer. Because the events are the source of truth for all object values 
        // in the inventory, we must put any state change future txes want to see in an event. It would not be safe
        // to do (e.g.) `delete_object_for_testing(t)` instead.
        // TODO: do this with a special test-only event to enable writing tests that look directly at system events
        // like transfers. the current scheme will perturb the count of transfer events.
        Transfer::transfer(t, get_signer_address(scenario))
    }

    /// Return `true` if a call to `remove_object<T>(scenario)` will succeed
    public fun can_remove_object<T: key>(scenario: &Scenario): bool {
        let objects: vector<T> = get_inventory<T>(
            get_signer_address(scenario),
            last_tx_start_index(scenario)
        );
        let res = !Vector::is_empty(&objects);
        delete_object_for_testing(objects);
        res
    }

    /// Return the `TxContext` asociated with this `scenario`
    public fun ctx(scenario: &mut Scenario): &mut TxContext {
        &mut scenario.ctx
    }

    /// Generate a fresh ID for the current tx associated with this `scenario`
    public fun new_id(scenario: &mut Scenario): VersionedID {
        TxContext::new_id(&mut scenario.ctx)
    }

    /// Return the sender of the current tx in this `scenario`
    public fun get_signer_address(scenario: &Scenario): address {
        TxContext::get_signer_address(&scenario.ctx)
    }

    /// Return the number of concluded transactions in this scenario.
    /// This does not include the current transaction--e.g., this will return 0 if `next_tx` has never been called
    public fun num_concluded_txes(scenario: &Scenario): u64 {
        Vector::length(&scenario.event_start_indexes) - 1
    }

    /// Return the index in the global transaction log where the events emitted by the `tx_idx`th transaction begin
    fun tx_start_index(scenario: &Scenario, tx_idx: u64): u64 {
        let idxs = &scenario.event_start_indexes;
        let len = Vector::length(idxs);
        assert!(tx_idx < len, EINVALID_TX_INDEX);
        *Vector::borrow(idxs, tx_idx)
    }

    /// Return the tx start index of the current transaction. This is an index into the global event log
    /// such that all events emitted by the current transaction occur at or after this index
    fun last_tx_start_index(scenario: &Scenario): u64 {
        let idxs = &scenario.event_start_indexes;
        // Safe because because `event_start_indexes` is always non-empty
        *Vector::borrow(idxs, Vector::length(idxs) - 1)
    }

    /// Remove and return the unique object of type `T` that can be accessed by `signer_address`
    /// Aborts if there are no objects of type `T` that can be be accessed by `signer_address`
    /// Aborts if there is >1 object of type `T` that can be accessed by `signer_address`
    fun remove_unique_object<T: key>(scenario: &mut Scenario, signer_address: address): T {
        let num_concluded_txes = num_concluded_txes(scenario);
        // Can't remove objects transferred by previous transactions if there are none
        assert!(num_concluded_txes != 0, ENO_CONCLUDED_TRANSACTIONS);

        let objects: vector<T> = get_inventory<T>(
            signer_address,
            last_tx_start_index(scenario)
        );
        let objects_len = Vector::length(&objects);
        if (objects_len == 1) {
            // found a unique object. ensure that it hasn't already been removed, then return it
            let t = Vector::pop_back(&mut objects);
            let id = *ID::get_id_bytes(&t);
            Vector::destroy_empty(objects);

            assert!(!Vector::contains(&scenario.removed, &id), EALREADY_REMOVED_OBJECT);
            Vector::push_back(&mut scenario.removed, id);
            t
        } else if (objects_len == 0) {
            abort(EEMPTY_INVENTORY)
        } else { // objects_len > 1
            abort(EINVENTORY_AMBIGUITY)
        }
    }

    // TODO: Add API's for inspecting user events, printing the user's inventory, ...

    // ---Natives---

    /// Return all live objects of type `T` that can be accessed by `signer_address` in the current transaction
    /// Events at or beyond `tx_end_index` in the log should not be processed to build this inventory
    native fun get_inventory<T: key>(signer_address: address, tx_end_index: u64): vector<T>;

    /// Test-only function for deleting an arbitrary object. Useful for eliminating objects without the `drop` ability.
    native fun delete_object_for_testing<T>(t: T); 

    /// Return the total number of events emitted by all txes in the current VM execution, including both user-defined events and system events
    native fun num_events(): u64;

    /// Return the ID's of objects transferred since the `tx_begin_idx`th event in the global event log
    /// Does not include objects that were transferred, then subsequently deleted
    native fun transferred_object_ids(tx_begin_idx: u64): vector<vector<u8>>;

    /// Return the ID's of objects deleted since the `tx_begin_idx`th event in the global event log
    native fun deleted_object_ids(tx_begin_idx: u64): vector<vector<u8>>;

    /// Emit a special, test-only event recording that `object_id` was wrapped
    native fun emit_wrapped_object_event(object_id: vector<u8>);
}