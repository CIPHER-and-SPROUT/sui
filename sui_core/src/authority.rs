// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use move_binary_format::CompiledModule;
use move_bytecode_utils::module_cache::ModuleCache;
use move_core_types::{
    language_storage::{ModuleId, StructTag},
    resolver::{ModuleResolver, ResourceResolver},
};
use move_vm_runtime::native_functions::NativeFunctionTable;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    pin::Pin,
    sync::Arc,
};
use sui_adapter::adapter;
use sui_types::{
    base_types::*,
    committee::Committee,
    crypto::AuthoritySignature,
    error::{SuiError, SuiResult},
    fp_bail, fp_ensure, gas,
    messages::*,
    object::{Data, Object, Owner},
    storage::Storage,
    MOVE_STDLIB_ADDRESS, SUI_FRAMEWORK_ADDRESS,
};

use crate::authority_batch::BatchSender;

#[cfg(test)]
#[path = "unit_tests/authority_tests.rs"]
pub mod authority_tests;

mod temporary_store;
use temporary_store::AuthorityTemporaryStore;

mod authority_store;
pub use authority_store::AuthorityStore;

// based on https://github.com/diem/move/blob/62d48ce0d8f439faa83d05a4f5cd568d4bfcb325/language/tools/move-cli/src/sandbox/utils/mod.rs#L50
const MAX_GAS_BUDGET: u64 = 18446744073709551615 / 1000 - 1;

/// a Trait object for `signature::Signer` that is:
/// - Pin, i.e. confined to one place in memory (we don't want to copy private keys).
/// - Sync, i.e. can be safely shared between threads.
///
/// Typically instantiated with Box::pin(keypair) where keypair is a `KeyPair`
///
pub type StableSyncAuthoritySigner =
    Pin<Arc<dyn signature::Signer<AuthoritySignature> + Send + Sync>>;

pub struct AuthorityState {
    // Fixed size, static, identity of the authority
    /// The name of this authority.
    pub name: AuthorityName,
    /// Committee of this Sui instance.
    pub committee: Committee,
    /// The signature key of the authority.
    pub secret: StableSyncAuthoritySigner,

    /// Move native functions that are available to invoke
    _native_functions: NativeFunctionTable,
    move_vm: Arc<adapter::MoveVM>,

    /// The database
    _database: Arc<AuthorityStore>,

    /// The sender to notify of new transactions
    /// and create batches for this authority.
    /// Keep as None if there is no need for this.
    batch_sender: Option<BatchSender>,
}

/// The authority state encapsulates all state, drives execution, and ensures safety.
///
/// Note the authority operations can be accessed through a read ref (&) and do not
/// require &mut. Internally a database is synchronized through a mutex lock.
///
/// Repeating valid commands should produce no changes and return no error.
impl AuthorityState {
    /// Set a listener for transaction certificate updates. Returns an
    /// error if a listener is already registered.
    pub fn set_batch_sender(&mut self, batch_sender: BatchSender) -> SuiResult {
        if self.batch_sender.is_some() {
            return Err(SuiError::AuthorityUpdateFailure);
        }
        self.batch_sender = Some(batch_sender);
        Ok(())
    }

    /// The logic to check one object against a reference, and return the object if all is well
    /// or an error if not.
    fn check_one_lock(
        &self,
        transaction: &Transaction,
        object_kind: InputObjectKind,
        object: &Object,
        mutable_object_addresses: &HashSet<SuiAddress>,
    ) -> SuiResult {
        match object_kind {
            InputObjectKind::MovePackage(package_id) => {
                fp_ensure!(
                    object.data.try_as_package().is_some(),
                    SuiError::MoveObjectAsPackage {
                        object_id: package_id
                    }
                );
            }
            InputObjectKind::OwnedMoveObject((object_id, sequence_number, object_digest)) => {
                fp_ensure!(
                    sequence_number <= SequenceNumber::MAX,
                    SuiError::InvalidSequenceNumber
                );

                // Check that the seq number is the same
                fp_ensure!(
                    object.version() == sequence_number,
                    SuiError::UnexpectedSequenceNumber {
                        object_id,
                        expected_sequence: object.version(),
                    }
                );

                // Check the digest matches
                fp_ensure!(
                    object.digest() == object_digest,
                    SuiError::InvalidObjectDigest {
                        object_id,
                        expected_digest: object_digest
                    }
                );

                // If the object is the gas object, check that it is an owned object
                // and there is enough gas.
                if object_kind.object_id() == transaction.gas_payment_object_ref().0 {
                    gas::check_gas_requirement(transaction, object)?;
                }

                // If the object is the transfer object, check that it is not shared object.
                if let TransactionKind::Transfer(t) = &transaction.data.kind {
                    if object_kind.object_id() == t.object_ref.0 {
                        fp_ensure!(
                            matches!(object.owner, Owner::AddressOwner(..)),
                            SuiError::TransferSharedError
                        );
                    }
                }

                match object.owner {
                    Owner::SharedImmutable => {
                        // Nothing else to check for SharedImmutable.
                    }
                    Owner::AddressOwner(owner) => {
                        // Check the owner is the transaction sender.
                        fp_ensure!(
                            transaction.sender_address() == owner,
                            SuiError::IncorrectSigner
                        );
                    }
                    Owner::ObjectOwner(owner) => {
                        // Check that the object owner is another mutable object in the input.
                        fp_ensure!(
                            mutable_object_addresses.contains(&owner),
                            SuiError::IncorrectSigner
                        );
                    }
                    Owner::SharedMutable => {
                        return Err(SuiError::UnsupportedSharedObjectError);
                    }
                };
            }
            InputObjectKind::SharedMoveObject(..) => (),
        };
        Ok(())
    }

    /// Check all the objects used in the transaction against the database, and ensure
    /// that they are all the correct version and number.
    async fn check_locks(
        &self,
        transaction: &Transaction,
    ) -> Result<Vec<(InputObjectKind, Object)>, SuiError> {
        let input_objects = transaction.input_objects();
        let mut all_objects = Vec::with_capacity(input_objects.len());

        let objects = self.fetch_objects(&input_objects).await?;
        let mutable_object_addresses: HashSet<_> = objects
            .iter()
            .flat_map(|opt_obj| match opt_obj {
                Some(obj) if !obj.is_read_only() => Some(obj.id().into()),
                _ => None,
            })
            .collect();
        let mut errors = Vec::new();
        for (object_kind, object) in input_objects.into_iter().zip(objects) {
            let object = match object {
                Some(object) => object,
                None => {
                    errors.push(object_kind.object_not_found_error());
                    continue;
                }
            };

            match self.check_one_lock(transaction, object_kind, &object, &mutable_object_addresses)
            {
                Ok(()) => all_objects.push((object_kind, object)),
                Err(e) => {
                    errors.push(e);
                }
            }
        }

        // If any errors with the locks were detected, we return all errors to give the client
        // a chance to update the authority if possible.
        if !errors.is_empty() {
            return Err(SuiError::LockErrors { errors });
        }

        fp_ensure!(!all_objects.is_empty(), SuiError::ObjectInputArityViolation);

        Ok(all_objects)
    }

    async fn fetch_objects(
        &self,
        input_objects: &[InputObjectKind],
    ) -> Result<Vec<Option<Object>>, SuiError> {
        // There is at least one input
        fp_ensure!(
            !input_objects.is_empty(),
            SuiError::ObjectInputArityViolation
        );
        // Ensure that there are no duplicate inputs
        let mut used = HashSet::new();
        if !input_objects.iter().all(|o| used.insert(o.object_id())) {
            return Err(SuiError::DuplicateObjectRefInput);
        }

        let ids: Vec<_> = input_objects.iter().map(|kind| kind.object_id()).collect();

        self.get_objects(&ids[..]).await
    }

    /// Initiate a new transaction.
    pub async fn handle_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<TransactionInfoResponse, SuiError> {
        // Check the sender's signature.
        transaction.check_signature()?;
        let transaction_digest = transaction.digest();

        let mutable_objects: Vec<_> = self
            .check_locks(&transaction)
            .await?
            .into_iter()
            .filter_map(|(object_kind, object)| match object_kind {
                InputObjectKind::MovePackage(_) => None,
                InputObjectKind::OwnedMoveObject(object_ref) => {
                    if object.is_read_only() {
                        None
                    } else {
                        Some(object_ref)
                    }
                }
                InputObjectKind::SharedMoveObject(..) => None,
            })
            .collect();

        let signed_transaction = SignedTransaction::new(transaction, self.name, &*self.secret);

        // Check and write locks, to signed transaction, into the database
        // The call to self.set_transaction_lock checks the lock is not conflicting,
        // and returns ConflictingTransaction error in case there is a lock on a different
        // existing transaction.
        self.set_transaction_lock(&mutable_objects, signed_transaction)
            .await?;

        // Return the signed Transaction or maybe a cert.
        self.make_transaction_info(&transaction_digest).await
    }

    /// Confirm a transfer.
    pub async fn handle_confirmation_transaction(
        &self,
        confirmation_transaction: ConfirmationTransaction,
    ) -> SuiResult<TransactionInfoResponse> {
        // Check the certificate and retrieve the transfer data.
        let certificate = &confirmation_transaction.certificate;
        certificate.check(&self.committee)?;

        // If the transaction contains shared objects, we need to ensure they have been scheduled
        // for processing by the consensus protocol.
        let transaction = &certificate.transaction;
        let transaction_digest = transaction.digest();
        if transaction.contains_shared_object() {
            let mut lock_errors = Vec::new();
            for object_id in transaction.shared_input_objects() {
                // Check whether the shared objects have already been assigned a sequence number by
                // the consensus. Bail if the transaction contains even one shared object that either:
                // (i) was not assigned a sequence number, or
                // (ii) has a different sequence number than the current one.
                //
                // Note that if the shared object is not in storage (it has been destroyed), we keep
                // processing the transaction to unlock all single-writer objects. The execution engine
                // will simply execute no-op.
                match self._database.sequenced(transaction_digest, *object_id)? {
                    Some(lock) => {
                        if let Some(object) = self._database.get_object(object_id)? {
                            if object.version() != lock {
                                lock_errors.push(SuiError::InvalidSequenceNumber);
                            }
                        }
                    }
                    None => lock_errors.push(SuiError::InvalidSequenceNumber),
                }
            }
            fp_ensure!(
                lock_errors.is_empty(),
                SuiError::LockErrors {
                    errors: lock_errors
                }
            );

            // Now let's process the certificate as usual: this executes the transaction and
            // unlock all single-writer objects. Since transactions with shared objects always
            // have at least one owner objects, it is not necessary to re-check the locks on
            // shared objects as we do with owned objects.
            let result = self
                .process_certificate(confirmation_transaction.clone())
                .await;

            // TODO [#676]: What if we crash right here? The cleanup should be done atomically
            // within `process_certificate`. It is not safety-critical but cleanup won't happen.

            // If the execution is successfully, we cleanup some data structures.
            if result.is_ok() {
                for object_id in transaction.shared_input_objects() {
                    self._database
                        .delete_sequence_lock(transaction_digest, *object_id)?;
                    if self._database.get_object(object_id)?.is_none() {
                        self._database.delete_schedule(object_id)?;
                    }
                }
            }
            result
        }
        // In case there are no shared objects, we simply process the certificate.
        else {
            // Ensure an idempotent answer
            let transaction_info = self.make_transaction_info(&transaction_digest).await?;
            if transaction_info.certified_transaction.is_some() {
                return Ok(transaction_info);
            }

            self.process_certificate(confirmation_transaction).await
        }
    }

    async fn process_certificate(
        &self,
        confirmation_transaction: ConfirmationTransaction,
    ) -> Result<TransactionInfoResponse, SuiError> {
        let certificate = confirmation_transaction.certificate;
        let transaction = certificate.transaction.clone();
        let transaction_digest = transaction.digest();

        let mut inputs: Vec<_> = self
            .check_locks(&transaction)
            .await?
            .into_iter()
            .map(|(_, object)| object)
            .collect();
        for object_id in transaction.shared_input_objects() {
            if let Some(object) = self._database.get_object(object_id)? {
                inputs.push(object);
            }
        }

        let mut transaction_dependencies: BTreeSet<_> = inputs
            .iter()
            .map(|object| object.previous_transaction)
            .collect();

        // Insert into the certificates map
        let mut tx_ctx = TxContext::new(&transaction.sender_address(), transaction_digest);

        let gas_object_id = transaction.gas_payment_object_ref().0;
        let (mut temporary_store, status) =
            self.execute_transaction(transaction, inputs, &mut tx_ctx)?;

        // Remove from dependencies the generic hash
        transaction_dependencies.remove(&TransactionDigest::genesis());

        let unwrapped_object_ids = self.get_unwrapped_object_ids(temporary_store.written())?;
        temporary_store.patch_unwrapped_object_version(unwrapped_object_ids);
        let to_signed_effects = temporary_store.to_signed_effects(
            &self.name,
            &*self.secret,
            &transaction_digest,
            transaction_dependencies.into_iter().collect(),
            status,
            &gas_object_id,
        );
        // Update the database in an atomic manner

        let (seq, resp) = self
            .update_state(temporary_store, certificate, to_signed_effects)
            .await?; // Returns the OrderInfoResponse

        // If there is a notifier registered, notify:
        if let Some(sender) = &self.batch_sender {
            sender.send_item(seq, transaction_digest).await?;
        }

        Ok(resp)
    }

    fn execute_transaction(
        &self,
        transaction: Transaction,
        mut inputs: Vec<Object>,
        tx_ctx: &mut TxContext,
    ) -> SuiResult<(AuthorityTemporaryStore, ExecutionStatus)> {
        let mut temporary_store = AuthorityTemporaryStore::new(self, &inputs, tx_ctx.digest());
        // unwraps here are safe because we built `inputs`
        let mut gas_object = inputs.pop().unwrap();

        let status = match transaction.data.kind {
            TransactionKind::Transfer(t) => AuthorityState::transfer(
                &mut temporary_store,
                inputs,
                t.recipient,
                gas_object.clone(),
            ),
            TransactionKind::Call(c) => {
                // unwraps here are safe because we built `inputs`
                let package = inputs.pop().unwrap();
                adapter::execute(
                    &self.move_vm,
                    &mut temporary_store,
                    self._native_functions.clone(),
                    package,
                    &c.module,
                    &c.function,
                    c.type_arguments,
                    inputs,
                    c.pure_arguments,
                    c.gas_budget,
                    gas_object.clone(),
                    tx_ctx,
                )
            }
            TransactionKind::Publish(m) => adapter::publish(
                &mut temporary_store,
                self._native_functions.clone(),
                m.modules,
                tx_ctx,
                m.gas_budget,
                gas_object.clone(),
            ),
        }?;
        if let ExecutionStatus::Failure { gas_used, .. } = &status {
            // Roll back the temporary store if execution failed.
            temporary_store.reset();
            // This gas deduction cannot fail.
            gas::deduct_gas(&mut gas_object, *gas_used);
            temporary_store.write_object(gas_object);
        }
        temporary_store.ensure_active_inputs_mutated();
        Ok((temporary_store, status))
    }

    /// Process certificates coming from the consensus. It is crucial that this function is only
    /// called by a single task (ie. the task handling consensus outputs).
    pub async fn handle_consensus_certificate(
        &self,
        certificate: &CertifiedTransaction,
    ) -> SuiResult<()> {
        // Ensure it is the first time we see this certificate.
        let transaction = &certificate.transaction;
        let transaction_digest = transaction.digest();
        for id in transaction.shared_input_objects() {
            if self._database.sequenced(transaction_digest, *id)?.is_some() {
                return Ok(());
            }
        }

        // Check the certificate.
        certificate.check(&self.committee)?;

        // Persist the certificate. We are about to lock one or more shared object.
        // We thus need to make sure someone (if not the client) can continue the protocol.
        // Also atomically lock the shared objects for this particular transaction.
        self._database.persist_certificate_and_lock_shared_objects(
            transaction_digest,
            transaction,
            certificate.clone(),
        )
    }

    fn transfer(
        temporary_store: &mut AuthorityTemporaryStore,
        mut inputs: Vec<Object>,
        recipient: SuiAddress,
        mut gas_object: Object,
    ) -> SuiResult<ExecutionStatus> {
        if !inputs.len() == 1 {
            return Ok(ExecutionStatus::Failure {
                gas_used: gas::MIN_OBJ_TRANSFER_GAS,
                error: Box::new(SuiError::ObjectInputArityViolation),
            });
        }

        // Safe to do pop due to check !is_empty()
        let mut output_object = inputs.pop().unwrap();

        let gas_used = gas::calculate_object_transfer_cost(&output_object);
        if let Err(err) = gas::try_deduct_gas(&mut gas_object, gas_used) {
            return Ok(ExecutionStatus::Failure {
                gas_used: gas::MIN_OBJ_TRANSFER_GAS,
                error: Box::new(err),
            });
        }
        temporary_store.write_object(gas_object);

        if let Err(err) = output_object.transfer(recipient) {
            return Ok(ExecutionStatus::Failure {
                gas_used: gas::MIN_OBJ_TRANSFER_GAS,
                error: Box::new(err),
            });
        }
        temporary_store.write_object(output_object);
        Ok(ExecutionStatus::Success { gas_used })
    }

    pub async fn handle_transaction_info_request(
        &self,
        request: TransactionInfoRequest,
    ) -> Result<TransactionInfoResponse, SuiError> {
        self.make_transaction_info(&request.transaction_digest)
            .await
    }

    pub async fn handle_account_info_request(
        &self,
        request: AccountInfoRequest,
    ) -> Result<AccountInfoResponse, SuiError> {
        self.make_account_info(request.account)
    }

    pub async fn handle_object_info_request(
        &self,
        request: ObjectInfoRequest,
    ) -> Result<ObjectInfoResponse, SuiError> {
        let ref_and_digest = match request.request_kind {
            ObjectInfoRequestKind::PastObjectInfo(seq) => {
                // Get the Transaction Digest that created the object
                let parent_iterator = self
                    .get_parent_iterator(request.object_id, Some(seq))
                    .await?;

                parent_iterator
                    .first()
                    .map(|(object_ref, tx_digest)| (*object_ref, *tx_digest))
            }
            ObjectInfoRequestKind::LatestObjectInfo(_) => {
                // Or get the latest object_reference and transaction entry.
                self.get_latest_parent_entry(request.object_id).await?
            }
        };

        let (requested_object_reference, parent_certificate) = match ref_and_digest {
            Some((object_ref, transaction_digest)) => (
                Some(object_ref),
                if transaction_digest == TransactionDigest::genesis() {
                    None
                } else {
                    // Get the cert from the transaction digest
                    Some(self.read_certificate(&transaction_digest).await?.ok_or(
                        SuiError::CertificateNotfound {
                            certificate_digest: transaction_digest,
                        },
                    )?)
                },
            ),
            None => (None, None),
        };

        // Return the latest version of the object and the current lock if any, if requested.
        let object_and_lock = match request.request_kind {
            ObjectInfoRequestKind::LatestObjectInfo(request_layout) => {
                match self.get_object(&request.object_id).await {
                    Ok(Some(object)) => {
                        let lock = if object.is_read_only() {
                            // Read only objects have no locks.
                            None
                        } else {
                            self.get_transaction_lock(&object.to_object_reference())
                                .await?
                        };
                        let layout = match request_layout {
                            Some(format) => {
                                let resolver = ModuleCache::new(&self);
                                object.get_layout(format, &resolver)?
                            }
                            None => None,
                        };

                        Some(ObjectResponse {
                            object,
                            lock,
                            layout,
                        })
                    }
                    Err(e) => return Err(e),
                    _ => None,
                }
            }
            ObjectInfoRequestKind::PastObjectInfo(_) => None,
        };

        Ok(ObjectInfoResponse {
            parent_certificate,
            requested_object_reference,
            object_and_lock,
        })
    }

    pub async fn new(
        committee: Committee,
        name: AuthorityName,
        secret: StableSyncAuthoritySigner,
        store: Arc<AuthorityStore>,
        genesis_packages: Vec<Vec<CompiledModule>>,
        genesis_ctx: &mut TxContext,
    ) -> Self {
        let native_functions =
            sui_framework::natives::all_natives(MOVE_STDLIB_ADDRESS, SUI_FRAMEWORK_ADDRESS);
        let state = AuthorityState {
            committee,
            name,
            secret,
            _native_functions: native_functions.clone(),
            move_vm: adapter::new_move_vm(native_functions)
                .expect("We defined natives to not fail here"),
            _database: store,
            batch_sender: None,
        };

        for genesis_modules in genesis_packages {
            state
                .store_package_and_init_modules_for_genesis(genesis_ctx, genesis_modules)
                .await
                .expect("We expect publishing the Genesis packages to not fail");
        }
        state
    }

    #[cfg(test)]
    pub fn db(&self) -> Arc<AuthorityStore> {
        self._database.clone()
    }

    async fn get_object(&self, object_id: &ObjectID) -> Result<Option<Object>, SuiError> {
        self._database.get_object(object_id)
    }

    pub async fn insert_object(&self, object: Object) {
        self._database
            .insert_object(object)
            .expect("TODO: propagate the error")
    }

    /// Persist the Genesis package to DB along with the side effects for module initialization
    async fn store_package_and_init_modules_for_genesis(
        &self,
        ctx: &mut TxContext,
        modules: Vec<CompiledModule>,
    ) -> SuiResult {
        debug_assert!(ctx.digest() == TransactionDigest::genesis());
        let inputs = Transaction::input_objects_in_compiled_modules(&modules);
        let input_objects = self
            .fetch_objects(&inputs)
            .await?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        let mut temporary_store = AuthorityTemporaryStore::new(self, &input_objects, ctx.digest());
        let package_id = ObjectID::from(*modules[0].self_id().address());
        let natives = self._native_functions.clone();
        let vm = adapter::verify_and_link(&temporary_store, &modules, package_id, natives)?;
        if let ExecutionStatus::Failure { error, .. } = adapter::store_package_and_init_modules(
            &mut temporary_store,
            &vm,
            modules,
            ctx,
            MAX_GAS_BUDGET,
        ) {
            return Err(*error);
        };
        self._database
            .update_objects_state_for_genesis(temporary_store, ctx.digest())
    }

    /// Make an information response for a transaction
    async fn make_transaction_info(
        &self,
        transaction_digest: &TransactionDigest,
    ) -> Result<TransactionInfoResponse, SuiError> {
        self._database.get_transaction_info(transaction_digest)
    }

    fn make_account_info(&self, account: SuiAddress) -> Result<AccountInfoResponse, SuiError> {
        self._database
            .get_account_objects(account)
            .map(|object_ids| AccountInfoResponse {
                object_ids,
                owner: account,
            })
    }

    // Helper function to manage transaction_locks

    /// Initialize a transaction lock for an object/sequence to None
    pub async fn init_transaction_lock(&self, object_ref: ObjectRef) {
        self._database
            .init_transaction_lock(object_ref)
            .expect("TODO: propagate the error")
    }

    /// Set the transaction lock to a specific transaction
    pub async fn set_transaction_lock(
        &self,
        mutable_input_objects: &[ObjectRef],
        signed_transaction: SignedTransaction,
    ) -> Result<(), SuiError> {
        self._database
            .set_transaction_lock(mutable_input_objects, signed_transaction)
    }

    async fn update_state(
        &self,
        temporary_store: AuthorityTemporaryStore,

        certificate: CertifiedTransaction,
        signed_effects: SignedTransactionEffects,
    ) -> Result<(u64, TransactionInfoResponse), SuiError> {
        self._database
            .update_state(temporary_store, certificate, signed_effects)
    }

    /// Get a read reference to an object/seq lock
    pub async fn get_transaction_lock(
        &self,
        object_ref: &ObjectRef,
    ) -> Result<Option<SignedTransaction>, SuiError> {
        self._database.get_transaction_lock(object_ref)
    }

    // Helper functions to manage certificates

    /// Read from the DB of certificates
    pub async fn read_certificate(
        &self,
        digest: &TransactionDigest,
    ) -> Result<Option<CertifiedTransaction>, SuiError> {
        self._database.read_certificate(digest)
    }

    pub async fn parent(&self, object_ref: &ObjectRef) -> Option<TransactionDigest> {
        self._database
            .parent(object_ref)
            .expect("TODO: propagate the error")
    }

    pub async fn get_objects(
        &self,
        _objects: &[ObjectID],
    ) -> Result<Vec<Option<Object>>, SuiError> {
        self._database.get_objects(_objects)
    }

    /// Returns all parents (object_ref and transaction digests) that match an object_id, at
    /// any object version, or optionally at a specific version.
    pub async fn get_parent_iterator(
        &self,
        object_id: ObjectID,
        seq: Option<SequenceNumber>,
    ) -> Result<Vec<(ObjectRef, TransactionDigest)>, SuiError> {
        {
            self._database.get_parent_iterator(object_id, seq)
        }
    }

    pub async fn get_latest_parent_entry(
        &self,
        object_id: ObjectID,
    ) -> Result<Option<(ObjectRef, TransactionDigest)>, SuiError> {
        self._database.get_latest_parent_entry(object_id)
    }

    /// Given all mutated objects during a transaction, return the list of objects
    /// that were unwrapped (i.e. re-appeared after being deleted).
    fn get_unwrapped_object_ids(
        &self,
        written: &BTreeMap<ObjectID, Object>,
    ) -> Result<Vec<ObjectID>, SuiError> {
        // For each mutated object, we first find out whether there was a transaction
        // that deleted this object in the past.
        let parents = self._database.multi_get_parents(
            &written
                .iter()
                .map(|(object_id, object)| (*object_id, object.version(), OBJECT_DIGEST_DELETED))
                .collect::<Vec<_>>(),
        )?;
        // Filter the list of mutated objects based on whether they were deleted in the past.
        // These objects are the unwrapped ones.
        let filtered = written
            .iter()
            .zip(parents.iter())
            .filter_map(|((object_id, _object), d)| d.map(|_| *object_id))
            .collect();
        Ok(filtered)
    }
}

impl ModuleResolver for AuthorityState {
    type Error = SuiError;

    fn get_module(&self, module_id: &ModuleId) -> Result<Option<Vec<u8>>, Self::Error> {
        self._database.get_module(module_id)
    }
}
